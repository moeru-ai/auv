//! Whole-scroll recipient lifetime, with application faults between samples.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use auv_driver_common::input_cancellation::{InputCancellation, with_input_cancellation};
use auv_driver_common::{
  ClickOptions, Driver, DriverError, InputActionProgress, InputPolicy, MotionTiming, Point, Scroll, ScrollMotion, ScrollOptions,
  ScrollStreamControl, ScrollStreamOptions, ScrollVelocity, TimingFunction, WindowInput,
};
use auv_driver_windows::WindowsDriver;
use serde_json::{Value, json};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::IsWindow;

use super::background::{discover_by_hwnd, point_in_window, run_id};
use super::receiver_process::{Receiver, ReceiverSpec, receiver_binary};
use super::records::Records;
use super::watch::DesktopWatch;

pub fn run(output: &Path) -> Result<bool> {
  ensure!(std::env::var("AUV_ISOLATED_DESKTOP_EVAL").as_deref() == Ok("1"), "requires an isolated desktop");
  ensure!(!output.exists(), "scroll batch evidence directory must be fresh");
  std::fs::create_dir_all(output)?;
  let session = WindowsDriver::new().open_local()?;
  let binary = receiver_binary()?;
  let watch = DesktopWatch::start()?;
  let original = watch.snapshot();
  let mut records = Records::create(&output.join("window-scroll-batch.jsonl"), &run_id(), "window-scroll-batch")?;
  let cases = [
    "motion_success",
    "motion_stay",
    "motion_aba",
    "motion_minimize",
    "motion_close",
    "stream_success",
    "stream_stay",
    "stream_aba",
    "stream_minimize",
    "stream_close",
    "stream_idle_loss",
  ];
  let mut all_passed = true;
  for case in cases {
    ensure!(watch.modifiers_released(), "modifier already held; no input allowed");
    let streaming = case.starts_with("stream_");
    let kind = case.split_once('_').unwrap().1;
    let idle = kind == "idle_loss";
    let fault_kind = if idle { "stay" } else { kind };
    let spec = |index, title: &str, color: &str| ReceiverSpec {
      index,
      title: title.into(),
      left: 40,
      top: 100,
      width: 430,
      height: 360,
      color: color.into(),
      sink: false,
    };
    let mut a = Receiver::spawn_detached_with(&binary, &spec(0, "AUV scroll recipient", "2E6F9E"), &["--foreground-input"])?;
    let mut b = Receiver::spawn_detached_with(&binary, &spec(1, "AUV scroll alternate", "9E412E"), &["--foreground-input"])?;
    a.request(json!({"cmd": "keep_foreground"}))?;
    b.request(json!({"cmd": "keep_foreground"}))?;
    let (window, _) = discover_by_hwnd(&session, a.hwnd())?;
    let (other, _) = discover_by_hwnd(&session, b.hwnd())?;
    session.window().activate(&window)?;
    let point = point_in_window(&window, a.layout.edit_center);
    let setup = session.window().click(
      &window,
      point,
      ClickOptions {
        policy: InputPolicy::ForegroundPreferred,
        ..Default::default()
      },
    )?;
    records.write("setup", json!({"case": case, "action": setup}))?;
    let before_a = a.request(json!({"cmd": "state"}))?;
    let before_b = b.request(json!({"cmd": "state"}))?;
    let before = watch.snapshot();
    ensure!(before.physical_keys == 0 && before.physical_mouse == 0, "physical input: case refused");
    let cancellation = Arc::new(InputCancellation::default());
    let mut fault = Value::Null;
    let mut callback_error = None;
    let mut sample_count = 0_u64;
    let mut notify = |delivered: Scroll| {
      sample_count += 1;
      if kind == "success" || !fault.is_null() || callback_error.is_some() || (!idle && delivered.delta_y == 0.0) {
        return;
      }
      // Pause the actor at its progress callback. The old per-sample guard has
      // already dropped here. Wait for the application's receipt before asking
      // its GUI thread to change foreground; no timer guesses input delivery.
      let applied = (|| -> Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let acknowledged = loop {
          let state = a.request(json!({"cmd": "state"}))?;
          let wheels = state["raw"]["edit_wheel"].as_u64().context("recipient omitted wheel count")?;
          if idle || wheels > 0 {
            break state;
          }
          ensure!(Instant::now() < deadline, "application never acknowledged the first wheel");
          std::thread::sleep(Duration::from_millis(10));
        };
        let reply = a.request(json!({"cmd": "apply_mouse_fault", "kind": fault_kind, "target": b.hwnd().to_string()}))?;
        ensure!(reply["applied"] == true, "application did not apply its batch fault");
        Ok(json!({"after_progress": delivered, "acknowledged": acknowledged, "receipt": reply["receipt"]}))
      })();
      match applied {
        Ok(receipt) => fault = receipt,
        Err(error) => {
          callback_error = Some(error.to_string());
          cancellation.cancel();
        }
      }
    };
    let started = Instant::now();
    let operation = with_input_cancellation(cancellation.clone(), || {
      let options = ScrollOptions {
        policy: InputPolicy::ForegroundPreferred,
        settle: Duration::from_millis(100),
        ..Default::default()
      };
      if streaming {
        let control = ScrollStreamControl::new();
        if !idle {
          control.set_velocity(ScrollVelocity::new(0.0, 400.0))?;
        }
        session
          .window()
          .scroll_stream(
            &window,
            point,
            &control,
            ScrollStreamOptions {
              sample_rate_hz: 4,
              max_acceleration: None,
              lease: Duration::from_secs(1),
            },
            options,
            &mut |progress| notify(progress.delivered),
          )
          .map(|result| json!(result))
      } else {
        session
          .window()
          .scroll_motion(
            &window,
            point,
            &ScrollMotion {
              total: Scroll::new(0.0, 400.0),
              timing: MotionTiming::FixedDuration {
                duration: Duration::from_secs(1),
                function: TimingFunction::Linear,
              },
              sample_rate_hz: 4,
            },
            options,
            &mut |progress| notify(progress.delivered),
          )
          .map(|result| json!(result))
      }
    });
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let terminal = if kind == "close" {
      a.closing_receipt()?
    } else {
      Value::Null
    };
    let after_a = if terminal.is_null() {
      a.request(json!({"cmd": "state"}))?
    } else {
      terminal["state"].clone()
    };
    let after_b = b.request(json!({"cmd": "state"}))?;
    let after = watch.snapshot();
    let (target_lost, progress) = match operation.as_ref().err() {
      Some(DriverError::InputAction { cause, progress }) => (matches!(cause.as_ref(), DriverError::TargetLost { .. }), Some(progress)),
      Some(DriverError::TargetLost { .. }) => (true, None),
      _ => (false, None),
    };
    let a_wheels = after_a["raw"]["edit_wheel"].as_u64().context("recipient omitted final wheels")?;
    let advanced = after_a["first_line"].as_i64().context("recipient omitted final line")?
      > before_a["first_line"].as_i64().context("recipient omitted initial line")?;
    let passed = if kind == "success" {
      operation.is_ok()
        && a_wheels > 0
        && advanced
        && if streaming {
          operation.as_ref().unwrap()["reason"] == "lease_expired"
        } else {
          operation.as_ref().unwrap()["delivered"]["delta_y"] == 400.0
        }
    } else {
      operation.is_err()
        && target_lost
        && fault["receipt"]["reached_target"] == true
        && if idle {
          matches!(progress, Some(InputActionProgress::NotDelivered))
            && a_wheels == 0
            && !advanced
            && after.injected_mouse == before.injected_mouse
            && after.cursor == before.cursor
        } else {
          matches!(progress, Some(InputActionProgress::Submitted { .. })) && a_wheels == 1 && advanced
        }
        && (kind != "aba" || fault["receipt"]["returned_owner"] == true)
        && (kind != "minimize" || after_a["minimized"] == true)
        && (kind != "close" || terminal["event"] == "closing")
    } && callback_error.is_none()
      && after_b["raw"]["edit_wheel"] == before_b["raw"]["edit_wheel"]
      && after_b["first_line"] == before_b["first_line"]
      && after_a["clicks"] == 0
      && after_b["clicks"] == 0
      && after.physical_keys == 0
      && after.physical_mouse == 0
      && watch.modifiers_released();
    let directory = output.join(case);
    std::fs::create_dir(&directory)?;
    for (name, target) in [("recipient", &window), ("alternate", &other)] {
      match session.window().capture(target) {
        Ok(capture) => capture.image.save(directory.join(format!("{name}.png")))?,
        Err(error) => records.write("capture_error", json!({"case": case, "receiver": name, "error": error.to_string()}))?,
      }
    }
    records.write("case", json!({"case": case, "passed": passed, "native_ok": operation.is_ok(), "result": operation.as_ref().ok(),
      "error": operation.as_ref().err().map(ToString::to_string), "input_progress": progress, "target_lost": target_lost,
      "before_a": before_a, "before_b": before_b, "after_a": after_a, "after_b": after_b,
      "fault": fault, "callback_error": callback_error, "terminal_receipt": terminal, "samples": sample_count, "elapsed_ms": elapsed_ms,
      "foreground_before": before.foreground, "foreground_after": after.foreground, "cursor_before": before.cursor, "cursor_after": after.cursor,
      "physical_keys": after.physical_keys, "physical_mouse": after.physical_mouse, "modifiers_released": watch.modifiers_released(),
      "injected_mouse": after.injected_mouse.saturating_sub(before.injected_mouse), "injected_keys": after.injected_keys.saturating_sub(before.injected_keys), "input_retries": 0}))?;
    all_passed &= passed;
    ensure!(after.physical_keys == 0 && after.physical_mouse == 0, "physical input: teardown input refused");
    if after.foreground == a.hwnd() {
      let _ = a.yield_foreground(original.foreground);
    }
    if after.foreground == b.hwnd() {
      let _ = b.yield_foreground(original.foreground);
    }
    session.input().move_to(Point::new(original.cursor.0.into(), original.cursor.1.into()))?;
    for receiver in [&mut a, &mut b] {
      let hwnd = receiver.hwnd();
      let exited = receiver.close().is_ok();
      // SAFETY: read-only query after the owned process exit receipt.
      let destroyed = unsafe { !IsWindow(HWND(hwnd as _)).as_bool() };
      records.write("cleanup", json!({"case": case, "window": hwnd, "process_exited": exited, "window_destroyed": destroyed}))?;
      ensure!(exited && destroyed, "owned scroll receiver cleanup incomplete");
    }
  }
  let final_state = watch.snapshot();
  let restored = final_state.foreground == original.foreground && final_state.cursor == original.cursor && watch.modifiers_released();
  records.write(
    "summary",
    json!({"passed": all_passed && restored, "cases": cases.len(), "foreground_restored": final_state.foreground == original.foreground,
    "pointer_restored": final_state.cursor == original.cursor, "modifiers_released": watch.modifiers_released(), "input_retries": 0}),
  )?;
  Ok(all_passed && restored)
}
