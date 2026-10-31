//! Target-recipient regression through real, overlapping Win32 receivers.
//! The application changes foreground only after processing its first click.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use auv_driver_common::input_cancellation::{InputCancellation, with_input_cancellation};
use auv_driver_common::{Click, ClickOptions, Driver, DriverError, InputPolicy, Point, WindowInput};
use auv_driver_windows::WindowsDriver;
use serde_json::json;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::IsWindow;

use super::background::{discover_by_hwnd, point_in_window, run_id};
use super::receiver_process::{Receiver, ReceiverSpec, receiver_binary};
use super::records::Records;
use super::watch::DesktopWatch;

pub fn run(output: &Path) -> Result<bool> {
  ensure!(std::env::var("AUV_ISOLATED_DESKTOP_EVAL").as_deref() == Ok("1"), "requires an isolated desktop");
  ensure!(!output.exists(), "mouse evidence directory must be fresh");
  std::fs::create_dir_all(output)?;
  let session = WindowsDriver::new().open_local()?;
  let binary = receiver_binary()?;
  let watch = DesktopWatch::start()?;
  let original = watch.snapshot();
  let cases = [
    "success",
    "stay",
    "aba",
    "minimize",
    "invalid_count",
    "invalid_point",
    "invalid_interval",
    "pre_cancel",
    "owner_mismatch",
  ];
  // TODO(windows-mouse-recipient): root-close receipts need the receiver's
  // terminal state before exit; add that owned terminal receipt before close cases.
  let mut records = Records::create(&output.join("window-mouse.jsonl"), &run_id(), "window-mouse")?;
  let mut all_passed = true;
  for case in cases {
    ensure!(watch.modifiers_released(), "modifier already held; no input allowed");
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
    let mut a = Receiver::spawn_detached_with(&binary, &spec(0, "AUV mouse recipient", "2E6F9E"), &["--foreground-input"])?;
    let mut b = Receiver::spawn_detached_with(&binary, &spec(1, "AUV mouse alternate", "9E412E"), &["--foreground-input"])?;
    a.request(json!({"cmd": "keep_foreground"}))?;
    b.request(json!({"cmd": "keep_foreground"}))?;
    let (window, _) = discover_by_hwnd(&session, a.hwnd())?;
    let (other, _) = discover_by_hwnd(&session, b.hwnd())?;
    let no_input = matches!(case, "invalid_count" | "invalid_point" | "invalid_interval" | "pre_cancel" | "owner_mismatch");
    session.window().activate(if no_input { &other } else { &window })?;
    let point = if case == "invalid_point" {
      auv_driver_common::geometry::WindowPoint::new(f64::NAN, 10.0)
    } else {
      point_in_window(&window, a.layout.button_center)
    };
    let mut requested = window.clone();
    if case == "owner_mismatch" {
      requested.process_id = Some(0);
    }
    if matches!(case, "stay" | "aba" | "minimize") {
      ensure!(
        a.request(json!({"cmd": "arm_mouse_fault", "kind": case, "target": b.hwnd().to_string()}))?["armed"] == true,
        "mouse fault was not armed"
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
    let operation = with_input_cancellation(cancellation, || {
      session.window().click(
        &requested,
        point,
        ClickOptions {
          policy: InputPolicy::ForegroundPreferred,
          click: Click::Repeated {
            count: if case == "invalid_count" { 0 } else { 3 },
            interval: if case == "invalid_interval" {
              Duration::MAX
            } else {
              Duration::from_millis(400)
            },
          },
          ..Default::default()
        },
      )
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let (after_a, after_b, fault) = loop {
      let a_state = a.request(json!({"cmd": "state"}))?;
      let b_state = b.request(json!({"cmd": "state"}))?;
      let receipt = a.request(json!({"cmd": "mouse_fault_receipt"}))?;
      let total = a_state["clicks"].as_u64().unwrap_or(0) + b_state["clicks"].as_u64().unwrap_or(0);
      let expected = if no_input {
        0
      } else if operation.is_ok() {
        3
      } else {
        1
      };
      if total >= expected && (no_input || case == "success" || !receipt["receipt"].is_null()) {
        break (a_state, b_state, receipt);
      }
      ensure!(Instant::now() < deadline, "application did not acknowledge the submitted click prefix");
      std::thread::sleep(Duration::from_millis(10));
    };
    let after = watch.snapshot();
    let (target_lost, progress) = operation.as_ref().err().map_or((false, None), |error| match error {
      DriverError::TargetLost { .. } => (true, None),
      DriverError::InputAction { cause, progress } => (matches!(cause.as_ref(), DriverError::TargetLost { .. }), Some(progress)),
      _ => (false, None),
    });
    let passed = if no_input {
      operation.is_err()
        && matches!(progress, Some(auv_driver_common::InputActionProgress::NotDelivered))
        && after_a["clicks"] == 0
        && after_b["clicks"] == 0
        && after.foreground == before.foreground
        && after.cursor == before.cursor
        && after.injected_mouse == before.injected_mouse
        && after.injected_keys == before.injected_keys
    } else if case == "success" {
      operation.is_ok() && after_a["clicks"] == 3 && after_b["clicks"] == 0
    } else {
      operation.is_err()
        && target_lost
        && matches!(progress, Some(auv_driver_common::InputActionProgress::Submitted { action })
          if action.mouse_disturbance == auv_driver_common::DisturbanceLevel::Temporary)
        && fault["receipt"]["reached_target"] == true
        && after_a["clicks"] == 1
        && after_b["clicks"] == 0
        && (case != "aba" || fault["receipt"]["returned_owner"] == true)
        && (case != "minimize" || after_a["minimized"] == true)
    } && after.physical_keys == 0
      && after.physical_mouse == 0
      && watch.modifiers_released();
    let directory = output.join(case);
    std::fs::create_dir(&directory)?;
    for (name, target) in [("recipient", &window), ("alternate", &other)] {
      match session.window().capture(target) {
        Ok(capture) => {
          capture.image.save(directory.join(format!("{name}.png")))?;
        }
        Err(error) => records.write("capture_error", json!({"case": case, "receiver": name, "error": error.to_string()}))?,
      }
    }
    records.write("case", json!({"case": case, "passed": passed, "before_a": before_a, "before_b": before_b,
      "after_a": after_a, "after_b": after_b, "fault": fault, "target_lost": target_lost, "input_progress": progress,
      "action": operation.as_ref().ok(), "error": operation.as_ref().err().map(ToString::to_string),
      "foreground_before": before.foreground, "foreground_after": after.foreground,
      "injected_mouse": after.injected_mouse.saturating_sub(before.injected_mouse), "injected_keys": after.injected_keys.saturating_sub(before.injected_keys),
      "physical_mouse": after.physical_mouse, "physical_keys": after.physical_keys, "modifiers_released": watch.modifiers_released(), "input_retries": 0}))?;
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
      // SAFETY: a read-only query after the owned process exit receipt.
      let destroyed = unsafe { !IsWindow(HWND(hwnd as _)).as_bool() };
      records.write("cleanup", json!({"case": case, "window": hwnd, "process_exited": exited, "window_destroyed": destroyed}))?;
      ensure!(exited && destroyed, "owned receiver cleanup incomplete");
    }
  }
  let final_state = watch.snapshot();
  let restored = final_state.cursor == original.cursor && final_state.foreground == original.foreground;
  records.write("summary", json!({"passed": all_passed && restored, "cases": cases.len(), "pointer_restored": final_state.cursor == original.cursor,
    "foreground_restored": final_state.foreground == original.foreground, "modifiers_released": watch.modifiers_released(), "input_retries": 0}))?;
  Ok(all_passed && restored)
}
