//! Independent application and desktop receipts for Windows keyboard recipient protection.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use auv_driver_common::input::InputPolicy;
use auv_driver_common::input_cancellation::{InputCancellation, with_input_cancellation};
use auv_driver_common::{
  Driver, DriverError, InputActionProgress, InputTarget, KeyboardInput, PasteTextOptions, PressKeysOptions, TypeTextOptions,
};
use auv_driver_windows::WindowsDriver;
use serde_json::{Value, json};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
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
  let cases = [
    "legacy_stay",
    "bound_success",
    "bound_stay",
    "bound_aba",
    "bound_minimize",
    "bound_close",
    "bound_press_success",
    "bound_press_stay",
    "bound_combo_stay",
    "bound_paste_success",
    "bound_paste_stay",
    "bound_cancel_after_char",
    "bound_pre_cancel",
    "bound_activate_once",
    "bound_owner_admission",
  ];
  for case in cases {
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
    let fault = match case {
      "legacy_stay" | "bound_stay" | "bound_press_stay" | "bound_combo_stay" | "bound_paste_stay" => Some("stay"),
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
    if case == "bound_activate_once" {
      let (other, _) = discover_by_hwnd(&session, b.hwnd())?;
      session.window().activate(&other)?;
      ensure!(b.request(json!({"cmd": "prepare_keyboard"}))?["focused"] == true, "alternate lost its first responder");
    }
    let a_before = a.request(json!({"cmd": "state"}))?;
    let b_before = b.request(json!({"cmd": "state"}))?;
    ensure!(a_before["typing"]["text"] == "" && b_before["typing"]["text"] == "", "fixture text must start empty");
    let before = watch.snapshot();
    ensure!(
      before.foreground
        == if case == "bound_activate_once" {
          b.hwnd()
        } else {
          a.hwnd()
        }
        && before.physical_keys == 0
        && before.physical_mouse == 0,
      "stop before keyboard input: recipient or physical-input gate failed"
    );
    let target = if case == "legacy_stay" {
      InputTarget::Foreground
    } else {
      let mut observed = window.clone();
      if case == "bound_owner_admission" {
        // A real second owner supplies the mismatching PID; the HWND stays A.
        let (other, _) = discover_by_hwnd(&session, b.hwnd())?;
        observed.process_id = other.process_id;
      }
      InputTarget::Window(observed)
    };
    let paste_case = case.starts_with("bound_paste_");
    let clipboard_before = paste_case.then(auv_driver_windows::clipboard::snapshot).transpose()?;
    let paste_text = format!("auv-paste-{}", run_id());
    ensure!(clipboard_before.as_ref() != Some(&paste_text), "paste fixture text must differ from the original clipboard");
    let type_input = |text: &str| KeyboardInput::TypeText {
      text: text.into(),
      options: TypeTextOptions {
        policy: InputPolicy::ForegroundPreferred,
        // Faults are acknowledged before submission; application state below
        // proves the cut between characters, rather than a timing assumption.
        inter_char_delay: Duration::from_millis(if case == "bound_cancel_after_char" {
          600
        } else {
          400
        }),
        ..Default::default()
      },
    };
    let inputs = match case {
      "bound_press_success" | "bound_press_stay" | "bound_combo_stay" | "bound_pre_cancel" => vec![KeyboardInput::PressKeys {
        policy: InputPolicy::ForegroundPreferred,
        options: PressKeysOptions {
          keys: if matches!(case, "bound_combo_stay" | "bound_pre_cancel") {
            vec!["shift".into(), "a".into()]
          } else {
            vec!["a".into()]
          },
          count: 3,
          interval: Duration::from_millis(400),
          settle: Duration::from_millis(150),
        },
      }],
      "bound_paste_success" | "bound_paste_stay" => vec![
        KeyboardInput::PasteText {
          policy: InputPolicy::ForegroundPreferred,
          options: PasteTextOptions {
            text: paste_text.clone(),
            settle: Duration::from_millis(150),
            ..Default::default()
          },
        },
        type_input("tail"),
      ],
      _ => vec![type_input("abc")],
    };
    let cancellation = Arc::new(InputCancellation::default());
    let (result, cancellation_receipt) = if case == "bound_cancel_after_char" {
      // The observer reads only fixture A. It cancels after the application
      // acknowledges a character; the actor retains the same batch admission.
      std::thread::scope(|scope| -> Result<_> {
        let observer_cancellation = cancellation.clone();
        let receiver = &mut a;
        let observer = scope.spawn(move || {
          let receipt = (|| -> Result<Value> {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
              let state = receiver.request(json!({"cmd": "state"}))?;
              let text = state["typing"]["text"].as_str().context("cancel observer omitted text")?;
              if !text.is_empty() {
                return Ok(json!({"after_text": text, "requested": true}));
              }
              ensure!(Instant::now() < deadline, "application never acknowledged the first character");
              std::thread::sleep(Duration::from_millis(10));
            }
          })();
          observer_cancellation.cancel();
          receipt
        });
        let operation = with_input_cancellation(cancellation, || session.input().input_keyboard(&target, inputs, false));
        let receipt = observer.join().map_err(|_| anyhow::anyhow!("cancel observer panicked"))??;
        Ok((operation, receipt))
      })?
    } else if case == "bound_pre_cancel" {
      cancellation.cancel();
      (
        with_input_cancellation(cancellation, || session.input().input_keyboard(&target, inputs, false)),
        json!({"requested_before_call": true}),
      )
    } else {
      (session.input().input_keyboard(&target, inputs, false), Value::Null)
    };
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
    let target_lost = result.as_ref().err().is_some_and(|error| {
      matches!(&error.cause,
      DriverError::InputAction { cause, .. } if matches!(cause.as_ref(), DriverError::TargetLost { .. }))
    });
    let cancelled = result.as_ref().err().is_some_and(|error| matches!(&error.cause,
      DriverError::InputAction { cause, .. } if matches!(cause.as_ref(), DriverError::InvalidInput { message } if message.contains("cancelled"))));
    let text_a = a_after.as_ref().and_then(|state| state["typing"]["text"].as_str());
    let text_b = b_after["typing"]["text"].as_str().context("alternate omitted text")?;
    let b_keys = b_after["typing"]["keys"].as_array().context("alternate omitted key log")?.len();
    let a_keys = a_after.as_ref().and_then(|state| state["typing"]["keys"].as_array()).map(Vec::len);
    // A submitted chord can finish releasing its own modifier after takeover.
    // These releases prevent stuck keys; they do not authorize another key-down.
    let owned_releases_only = b_after["typing"]["keys"]
      .as_array()
      .unwrap()
      .iter()
      .all(|key| key["up"] == true && key["vk"] == if case == "bound_combo_stay" { 16 } else { 17 });
    let clipboard_text_restored = match &clipboard_before {
      Some(original) => auv_driver_windows::clipboard::snapshot()? == *original,
      None => true,
    };
    // SAFETY: read-only key-state queries on the isolated input desktop.
    let modifiers_released =
      [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN].iter().all(|key| unsafe { GetAsyncKeyState(i32::from(key.0)) >= 0 });
    let fired = fault_receipt.as_ref().is_some_and(|value| value["receipt"]["reached_target"] == true);
    // SAFETY: read-only liveness query of the HWND owned by fixture A.
    let closed = unsafe { !IsWindow(HWND(a.hwnd() as _)).as_bool() };
    let passed = (match case {
      "legacy_stay" => result.is_ok() && fired && text_a == Some("a") && text_b == "bc",
      "bound_success" => result.is_ok() && text_a == Some("abc") && text_b.is_empty() && b_keys == 0,
      "bound_activate_once" => {
        result.is_ok()
          && text_a == Some("abc")
          && text_b.is_empty()
          && b_keys == 0
          && after.foreground == a.hwnd()
          && a_after
            .as_ref()
            .and_then(|state| state["activated"].as_u64())
            .zip(a_before["activated"].as_u64())
            .is_some_and(|(after, before)| after == before + 1)
      }
      "bound_press_success" => result.is_ok() && text_a == Some("aaa") && text_b.is_empty() && b_keys == 0,
      "bound_press_stay" | "bound_combo_stay" => {
        result.as_ref().err().is_some_and(|error| error.progress.completed_presses == 1)
          && fired
          && text_a == Some(if case == "bound_combo_stay" { "A" } else { "a" })
          && text_b.is_empty()
          && (b_keys == 0 || (case == "bound_combo_stay" && owned_releases_only))
          && matches!(native_progress, Some(InputActionProgress::Submitted { .. }))
      }
      "bound_paste_success" => result.is_ok() && text_a == Some(format!("{paste_text}tail").as_str()) && text_b.is_empty() && b_keys == 0,
      "bound_paste_stay" => {
        result.is_err()
          && fired
          && text_a == Some(paste_text.as_str())
          && text_b.is_empty()
          && owned_releases_only
          && matches!(native_progress, Some(InputActionProgress::Submitted { .. }))
      }
      "bound_cancel_after_char" => {
        result.is_err()
          && cancellation_receipt["after_text"] == "a"
          && text_a == Some("a")
          && text_b.is_empty()
          && b_keys == 0
          && matches!(native_progress, Some(InputActionProgress::Submitted { .. }))
      }
      "bound_pre_cancel" | "bound_owner_admission" => {
        result.is_err()
          && text_a == Some("")
          && a_keys == Some(0)
          && text_b.is_empty()
          && b_keys == 0
          && after.injected_keys == before.injected_keys
          && matches!(native_progress, Some(InputActionProgress::NotDelivered))
      }
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
    }) && clipboard_text_restored
      && modifiers_released;
    let expected_injected_keys = match case {
      "legacy_stay" | "bound_success" | "bound_activate_once" | "bound_press_success" => 6,
      "bound_combo_stay" | "bound_paste_stay" => 4,
      "bound_paste_success" => 12,
      "bound_pre_cancel" | "bound_owner_admission" => 0,
      _ => 2,
    };
    let expected_target_loss = !matches!(
      case,
      "legacy_stay"
        | "bound_success"
        | "bound_activate_once"
        | "bound_press_success"
        | "bound_paste_success"
        | "bound_pre_cancel"
        | "bound_cancel_after_char"
    );
    let expected_cancellation = matches!(case, "bound_pre_cancel" | "bound_cancel_after_char");
    let passed = passed
      && target_lost == expected_target_loss
      && cancelled == expected_cancellation
      && after.injected_keys.saturating_sub(before.injected_keys) == expected_injected_keys;
    let passed = passed
      && (!matches!(case, "bound_stay" | "bound_press_stay" | "bound_combo_stay" | "bound_paste_stay") || after.foreground == b.hwnd());
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
      "cancellation": cancellation_receipt, "clipboard_text_restored": clipboard_text_restored,
      "modifiers_released": modifiers_released,
      "expected_injected_keys": expected_injected_keys,
      "target_lost": target_lost, "cancelled": cancelled,
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
    json!({"passed": all_passed, "cases": cases.len(),
    "pointer_restored": finish.cursor == original.cursor, "foreground_restored": finish.foreground == original.foreground,
    "physical_keys": finish.physical_keys, "physical_mouse": finish.physical_mouse, "input_retries": 0}),
  )?;
  Ok(all_passed)
}
