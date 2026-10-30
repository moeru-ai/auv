//! Independent application and desktop receipts for Windows keyboard recipient protection.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use auv_driver_common::input::InputPolicy;
use auv_driver_common::{Driver, DriverError, InputTarget, KeyboardInput, TypeTextOptions};
use auv_driver_windows::WindowsDriver;
use serde_json::{Value, json};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::IsWindow;

use super::background::{discover_by_hwnd, run_id};
use super::receiver_process::{Receiver, ReceiverSpec, receiver_binary};
use super::records::Records;
use super::watch::DesktopWatch;

pub fn run(output: &Path) -> Result<bool> {
  std::fs::create_dir_all(output)?;
  let mut records = Records::create(&output.join("window-keyboard.jsonl"), &run_id(), "window-keyboard")?;
  let session = WindowsDriver::new().open_local()?;
  let binary = receiver_binary()?;
  let watch = DesktopWatch::start()?;
  let original = watch.snapshot();
  let mut all_passed = true;
  for case in [
    "legacy_stay",
    "bound_success",
    "bound_stay",
    "bound_aba",
    "bound_minimize",
    "bound_close",
  ] {
    let spec = |index, title: &str, left| ReceiverSpec {
      index,
      title: title.into(),
      left,
      top: 100,
      width: 430,
      height: 360,
      color: "2E6F9E".into(),
      sink: false,
    };
    let mut a = Receiver::spawn_detached_with(&binary, &spec(0, "AUV keyboard recipient", 40), &["--foreground-input"])?;
    let mut b = Receiver::spawn_detached_with(&binary, &spec(1, "AUV keyboard alternate", 510), &["--foreground-input"])?;
    a.request(json!({"cmd": "keep_foreground"}))?;
    b.request(json!({"cmd": "keep_foreground"}))?;
    ensure!(b.request(json!({"cmd": "prepare_keyboard"}))?["focused"] == true, "alternate typing control not ready");
    let (window, _) = discover_by_hwnd(&session, a.hwnd())?;
    session.window().activate(&window)?;
    ensure!(a.request(json!({"cmd": "prepare_keyboard"}))?["focused"] == true, "recipient typing control not ready");
    let a_before = a.request(json!({"cmd": "state"}))?;
    let b_before = b.request(json!({"cmd": "state"}))?;
    ensure!(a_before["typing"]["text"] == "" && b_before["typing"]["text"] == "", "fixture text must start empty");
    let fault = match case {
      "legacy_stay" | "bound_stay" => Some("stay"),
      "bound_aba" => Some("aba"),
      "bound_minimize" => Some("minimize"),
      "bound_close" => Some("close"),
      _ => None,
    };
    if let Some(kind) = fault {
      ensure!(
        a.request(json!({"cmd": "arm_keyboard_fault", "kind": kind, "target": b.hwnd().to_string()}))?["armed"] == true,
        "keyboard fault was not armed"
      );
    }
    let before = watch.snapshot();
    ensure!(
      before.foreground == a.hwnd() && before.physical_keys == 0 && before.physical_mouse == 0,
      "stop before keyboard input: recipient or physical-input gate failed"
    );
    let target = if case == "legacy_stay" {
      InputTarget::Foreground
    } else {
      InputTarget::Window(window.clone())
    };
    let result = session.input().input_keyboard(
      &target,
      vec![KeyboardInput::TypeText {
        text: "abc".into(),
        options: TypeTextOptions {
          policy: InputPolicy::ForegroundPreferred,
          // The fixture's release-triggered fault is armed and acknowledged before delivery.
          // Its receipt and exact A/B text below prove that it fired between characters.
          inter_char_delay: Duration::from_millis(400),
          ..Default::default()
        },
      }],
      false,
    );
    let a_after = a.request(json!({"cmd": "state"})).ok();
    let b_after = b.request(json!({"cmd": "state"}))?;
    let fault_receipt = a.request(json!({"cmd": "keyboard_fault_receipt"})).ok();
    let after = watch.snapshot();
    let native_progress = result.as_ref().err().and_then(|error| match &error.cause {
      DriverError::InputAction { progress, .. } => Some(progress),
      _ => None,
    });
    let operation = match &result {
      Ok(actions) => json!({"status": "ok", "actions": actions}),
      Err(error) => json!({"status": "error", "cause": format!("{:?}", error.cause),
        "progress": error.progress, "native_progress": native_progress}),
    };
    let text_a = a_after.as_ref().and_then(|state| state["typing"]["text"].as_str());
    let text_b = b_after["typing"]["text"].as_str().context("alternate omitted text")?;
    let b_keys = b_after["typing"]["keys"].as_array().context("alternate omitted key log")?.len();
    let fired = fault_receipt.as_ref().is_some_and(|value| value["receipt"]["reached_target"] == true);
    // SAFETY: read-only liveness query of the HWND owned by fixture A.
    let closed = unsafe { !IsWindow(HWND(a.hwnd() as _)).as_bool() };
    let passed = match case {
      "legacy_stay" => result.is_ok() && fired && text_a == Some("a") && text_b == "bc",
      "bound_success" => result.is_ok() && text_a == Some("abc") && text_b.is_empty() && b_keys == 0,
      "bound_close" => {
        result.is_err()
          && closed
          && after.injected_keys > before.injected_keys
          && text_b.is_empty()
          && b_keys == 0
          && native_progress.is_some()
      }
      _ => {
        let fault_completed = match case {
          "bound_aba" => {
            fault_receipt.as_ref().is_some_and(|value| value["receipt"]["returned_owner"] == true) && after.foreground == a.hwnd()
          }
          "bound_minimize" => a_after.as_ref().is_some_and(|value| value["minimized"] == true),
          _ => true,
        };
        result.is_err() && fired && fault_completed && text_a == Some("a") && text_b.is_empty() && b_keys == 0 && native_progress.is_some()
      }
    };
    let events: Vec<Value> = watch
      .timeline(before.taken_ms, after.taken_ms)
      .foreground_events
      .iter()
      .map(|event| json!({"at": event.at, "named": event.window, "actual": event.actual}))
      .collect();
    records.write(
      "case",
      json!({"case": case, "passed": passed, "a": a.hwnd(), "b": b.hwnd(),
      "before_a": a_before, "before_b": b_before, "after_a": a_after, "after_b": b_after,
      "fault": fault_receipt, "operation": operation, "a_closed": closed, "events": events,
      "foreground_before": before.foreground, "foreground_after": after.foreground,
      "injected_keys": after.injected_keys.saturating_sub(before.injected_keys),
      "injected_mouse": after.injected_mouse.saturating_sub(before.injected_mouse),
      "physical_keys": after.physical_keys, "physical_mouse": after.physical_mouse,
      "pointer_unchanged": after.cursor == before.cursor, "input_retries": 0}),
    )?;
    all_passed &= passed;
    ensure!(
      after.physical_keys == 0
        && after.physical_mouse == 0
        && after.cursor == before.cursor
        && after.injected_mouse == before.injected_mouse,
      "desktop witness rejected keyboard case"
    );
    if after.foreground == b.hwnd() {
      let _ = b.yield_foreground(original.foreground);
    } else if after.foreground == a.hwnd() {
      let _ = a.yield_foreground(original.foreground);
    }
    for receiver in [&mut a, &mut b] {
      let hwnd = receiver.hwnd();
      let exited = receiver.close().is_ok();
      // SAFETY: read-only check after the owned process exit receipt.
      let destroyed = unsafe { !IsWindow(HWND(hwnd as _)).as_bool() };
      records.write("cleanup", json!({"case": case, "window": hwnd, "process_exited": exited, "window_destroyed": destroyed}))?;
      ensure!(exited && destroyed, "owned receiver cleanup failed");
    }
  }
  let finish = watch.snapshot();
  records.write(
    "summary",
    json!({"passed": all_passed, "cases": 6,
    "pointer_restored": finish.cursor == original.cursor, "foreground_restored": finish.foreground == original.foreground,
    "physical_keys": finish.physical_keys, "physical_mouse": finish.physical_mouse, "input_retries": 0}),
  )?;
  Ok(all_passed)
}
