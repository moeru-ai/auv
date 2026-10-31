//! Complete drags with faults after an application-acknowledged held move.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use auv_driver_common::input_cancellation::{InputCancellation, with_input_cancellation};
use auv_driver_common::{
  DisturbanceLevel, Driver, DriverError, InputActionProgress, InputPolicy, MouseButton, MouseCubicBezierSegment, MoveMouseRequest, Point,
  WindowInput,
};
use auv_driver_windows::WindowsDriver;
use serde_json::{Value, json};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::UI::WindowsAndMessaging::IsWindow;

use super::background::{discover_by_hwnd, run_id};
use super::receiver_process::{Receiver, ReceiverSpec, receiver_binary};
use super::records::Records;
use super::watch::DesktopWatch;

pub fn run(output: &Path) -> Result<bool> {
  ensure!(std::env::var("AUV_ISOLATED_DESKTOP_EVAL").as_deref() == Ok("1"), "requires an isolated desktop");
  ensure!(!output.exists(), "drag evidence directory must be fresh");
  std::fs::create_dir_all(output)?;
  let session = WindowsDriver::new().open_local()?;
  let binary = receiver_binary()?;
  let watch = DesktopWatch::start()?;
  let original = watch.snapshot();
  let mut records = Records::create(&output.join("window-drag.jsonl"), &run_id(), "window-drag")?;
  let cases = [
    "success",
    "stay",
    "aba",
    "minimize",
    "close",
    "cancel_after_move",
    "pre_cancel",
    "invalid_start",
    "invalid_curve",
    "invalid_timing",
    "invalid_duration",
    "invalid_mouse",
    "owner_mismatch",
  ];
  let mut all_passed = true;
  for case in cases {
    let no_input = matches!(
      case,
      "pre_cancel" | "invalid_start" | "invalid_curve" | "invalid_timing" | "invalid_duration" | "invalid_mouse" | "owner_mismatch"
    );
    // SAFETY: read-only high-bit query. Never start with a user's button held.
    ensure!(watch.modifiers_released() && unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON.0)) >= 0 }, "input already held");
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
    let mut a = Receiver::spawn_detached_with(&binary, &spec(0, "AUV drag recipient", "2E6F9E"), &["--foreground-input"])?;
    let mut b = Receiver::spawn_detached_with(&binary, &spec(1, "AUV drag alternate", "9E412E"), &["--foreground-input"])?;
    a.request(json!({"cmd": "keep_foreground"}))?;
    b.request(json!({"cmd": "keep_foreground"}))?;
    let (window, _) = discover_by_hwnd(&session, a.hwnd())?;
    let (other, _) = discover_by_hwnd(&session, b.hwnd())?;
    session.window().activate(&window)?;
    let layout = a.request(json!({"cmd": "layout"}))?;
    let pad: [i32; 4] = serde_json::from_value(layout["pad_rect"].clone())?;
    ensure!(pad[2] - pad[0] > 100 && pad[3] - pad[1] > 10, "application pad too small");
    let start = Point::new(f64::from(pad[0] + 20), f64::from((pad[1] + pad[3]) / 2));
    let end = Point::new(f64::from(pad[2] - 20), start.y);
    let mut movement = MoveMouseRequest::direct(start);
    movement.curve.segments.push(MouseCubicBezierSegment {
      control_1: Point::new((end.x - start.x) / 3.0, 0.0),
      control_2: Point::new((end.x - start.x) * 2.0 / 3.0, 0.0),
      end: Point::new(end.x - start.x, 0.0),
    });
    movement.options.duration = Duration::from_secs(1);
    movement.options.sample_rate_hz = 4;
    movement.options.curve_tolerance = 0.5;
    match case {
      "invalid_start" => movement.start = auv_driver_common::MouseStart::Screen(Point::new(f64::NAN, start.y)),
      "invalid_curve" => movement.curve.segments[0].end.x = f64::NAN,
      "invalid_timing" => movement.options.sample_rate_hz = 0,
      "invalid_duration" => movement.options.duration = Duration::MAX,
      "invalid_mouse" => movement.mouse = u64::MAX,
      _ => {}
    }
    let mut requested = window.clone();
    if case == "owner_mismatch" {
      requested.process_id = Some(0);
    }
    if no_input {
      session.window().activate(&other)?;
    }
    if matches!(case, "stay" | "aba" | "minimize" | "close") {
      ensure!(
        a.request(json!({"cmd": "arm_mouse_fault", "kind": case, "target": b.hwnd().to_string(), "trigger": "drag"}))?["armed"] == true,
        "drag fault not armed"
      );
    }
    let before_a = a.request(json!({"cmd": "state"}))?;
    let before_b = b.request(json!({"cmd": "state"}))?;
    let before = watch.snapshot();
    ensure!(before.physical_keys == 0 && before.physical_mouse == 0, "physical input: case refused");
    let cancellation = Arc::new(InputCancellation::default());
    if case == "pre_cancel" {
      cancellation.cancel();
    }
    let deliver = || {
      with_input_cancellation(cancellation.clone(), || {
        session.window().drag(&requested, movement, MouseButton::Left, InputPolicy::ForegroundPreferred)
      })
    };
    let started = Instant::now();
    let (operation, cancellation_receipt) = if case == "cancel_after_move" {
      std::thread::scope(|scope| -> Result<_> {
        let observer_cancellation = cancellation.clone();
        let receiver = &mut a;
        let observer = scope.spawn(move || {
          let receipt = (|| -> Result<Value> {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
              let state = receiver.request(json!({"cmd": "state"}))?;
              if state["pad"]["moves"].as_u64().unwrap_or(0) > 0 {
                return Ok(json!({"requested": true, "acknowledged": state}));
              }
              ensure!(Instant::now() < deadline, "application never acknowledged a held move");
              std::thread::sleep(Duration::from_millis(10));
            }
          })();
          observer_cancellation.cancel();
          receipt
        });
        let operation = deliver();
        Ok((operation, observer.join().map_err(|_| anyhow::anyhow!("drag cancellation observer panicked"))??))
      })?
    } else {
      (deliver(), Value::Null)
    };
    let elapsed_ms = started.elapsed().as_millis();
    // Give queued application messages a bounded drain. This is not the fault
    // trigger: the application itself triggered faults after a processed move.
    std::thread::sleep(Duration::from_millis(100));
    let terminal = if case == "close" {
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
    let fault = if terminal.is_null() {
      a.request(json!({"cmd": "mouse_fault_receipt"}))?["receipt"].clone()
    } else {
      terminal["fault"].clone()
    };
    let after = watch.snapshot();
    // Read before the controller's separate recovery; a recovery release must
    // never turn an actor failure into success.
    // SAFETY: read-only high-bit query, with no synthesized input.
    let button_released = unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON.0)) >= 0 };
    let (target_lost, cancelled, progress) = operation.as_ref().err().map_or((false, false, None), |error| match error {
      DriverError::InputAction { cause, progress } => (
        matches!(cause.as_ref(), DriverError::TargetLost { .. }),
        matches!(cause.as_ref(), DriverError::InvalidInput { message } if message.contains("cancelled")),
        Some(progress),
      ),
      DriverError::TargetLost { .. } => (true, false, None),
      _ => (false, false, None),
    });
    let submitted =
      matches!(progress, Some(InputActionProgress::Submitted { action }) if action.mouse_disturbance == DisturbanceLevel::Temporary);
    let no_wrong_move = after_b["raw"]["frame_down"] == before_b["raw"]["frame_down"]
      && after_b["raw"]["frame_move"] == before_b["raw"]["frame_move"]
      && after_b["pad"].is_null();
    let passed = if no_input {
      operation.is_err()
        && matches!(progress, Some(InputActionProgress::NotDelivered))
        && (case != "owner_mismatch" || target_lost)
        && (case != "pre_cancel" || cancelled)
        && after_a["pad"].is_null()
        && after_a["raw"] == before_a["raw"]
        && after_b["raw"] == before_b["raw"]
        && after.cursor == before.cursor
        && after.foreground == before.foreground
        && after.injected_mouse == before.injected_mouse
        && after.injected_keys == before.injected_keys
    } else if case == "success" {
      operation.is_ok()
        && after_a["pad"]["down"] == json!([start.x as i32, start.y as i32])
        && after_a["pad"]["up"] == json!([end.x as i32, end.y as i32])
        && after_a["pad"]["moves"].as_u64().unwrap_or(0) > 0
    } else if case == "cancel_after_move" {
      cancelled
        && submitted
        && !target_lost
        && cancellation_receipt["requested"] == true
        && after_a["pad"]["last"] == cancellation_receipt["acknowledged"]["pad"]["last"]
        && json!([after.cursor.0, after.cursor.1]) == cancellation_receipt["acknowledged"]["pad"]["last"]
    } else {
      target_lost
        && submitted
        && fault["reached_target"] == true
        && json!([after.cursor.0, after.cursor.1]) == fault["cursor_at_fault"]
        && after_a["pad"]["last"] == fault["pad_at_fault"]["last"]
        && (case != "aba" || fault["returned_owner"] == true)
        && (case != "minimize" || after_a["minimized"] == true)
        && (case != "close" || terminal["event"] == "closing")
    } && no_wrong_move
      && button_released
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
      "error": operation.as_ref().err().map(ToString::to_string), "input_progress": progress, "target_lost": target_lost, "cancelled": cancelled,
      "start": start, "end": end, "before_a": before_a, "before_b": before_b, "after_a": after_a, "after_b": after_b,
      "fault": fault, "terminal_receipt": terminal, "cancellation_receipt": cancellation_receipt, "elapsed_ms": elapsed_ms,
      "button_released_before_controller_cleanup": button_released, "foreground_before": before.foreground, "foreground_after": after.foreground,
      "cursor_before": before.cursor, "cursor_after": after.cursor, "physical_keys": after.physical_keys, "physical_mouse": after.physical_mouse,
      "modifiers_released": watch.modifiers_released(), "injected_mouse": after.injected_mouse.saturating_sub(before.injected_mouse),
      "injected_keys": after.injected_keys.saturating_sub(before.injected_keys), "input_retries": 0}))?;
    all_passed &= passed;
    ensure!(after.physical_keys == 0 && after.physical_mouse == 0, "physical input: teardown input refused");
    // Recover only the actor-owned logical button after cancellation TLS ends.
    // This also clears a retained uncertain hold on an unfixed Driver.
    let release = session.input().mouse_up(0);
    records.write(
      "controller_release",
      json!({"case": case, "ok": release.is_ok(), "action": release.as_ref().ok(),
      "error": release.as_ref().err().map(ToString::to_string)}),
    )?;
    ensure!(release.is_ok(), "owned drag recovery failed");
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
      // SAFETY: read-only query after this owned process's exit receipt.
      let destroyed = unsafe { !IsWindow(HWND(hwnd as _)).as_bool() };
      records.write("cleanup", json!({"case": case, "window": hwnd, "process_exited": exited, "window_destroyed": destroyed}))?;
      ensure!(exited && destroyed, "owned drag receiver cleanup incomplete");
    }
  }
  let final_state = watch.snapshot();
  let restored = final_state.cursor == original.cursor && final_state.foreground == original.foreground && watch.modifiers_released();
  records.write("summary", json!({"passed": all_passed && restored, "cases": cases.len(), "pointer_restored": final_state.cursor == original.cursor,
    "foreground_restored": final_state.foreground == original.foreground, "modifiers_released": watch.modifiers_released(), "input_retries": 0}))?;
  Ok(all_passed && restored)
}
