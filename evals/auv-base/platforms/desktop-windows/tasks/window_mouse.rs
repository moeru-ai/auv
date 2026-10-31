//! Target-recipient regression through real, overlapping Win32 receivers.
//! The application changes foreground only after processing its click or wheel.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use auv_driver_common::input_cancellation::{InputCancellation, with_input_cancellation};
use auv_driver_common::{Click, ClickOptions, Driver, DriverError, InputPolicy, Point, Scroll, ScrollOptions, WindowInput};
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
    "cancel_after_click",
    "close",
    "scroll_success",
    "scroll_stay",
    "scroll_aba",
    "scroll_minimize",
    "scroll_close",
    "scroll_invalid_point",
    "scroll_invalid_delta",
    "scroll_invalid_settle",
    "scroll_pre_cancel",
    "scroll_owner_mismatch",
    "scroll_cancel_after_effect",
  ];
  let mut records = Records::create(&output.join("window-mouse.jsonl"), &run_id(), "window-mouse")?;
  let mut all_passed = true;
  for case in cases {
    let scrolling = case.starts_with("scroll_");
    let kind = case.strip_prefix("scroll_").unwrap_or(case);
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
    let no_input = matches!(
      kind,
      "invalid_count" | "invalid_point" | "invalid_interval" | "invalid_delta" | "invalid_settle" | "pre_cancel" | "owner_mismatch"
    );
    session.window().activate(if no_input { &other } else { &window })?;
    let scroll_point = point_in_window(&window, a.layout.edit_center);
    let point = if kind == "invalid_point" {
      auv_driver_common::geometry::WindowPoint::new(f64::NAN, 10.0)
    } else if scrolling {
      scroll_point
    } else {
      point_in_window(&window, a.layout.button_center)
    };
    if scrolling {
      // Controller setup uses the accepted click path to focus the scrolling
      // edit before arming a wheel fault. Witness accounting starts below.
      let setup = session.window().click(
        &window,
        scroll_point,
        ClickOptions {
          policy: InputPolicy::ForegroundPreferred,
          ..Default::default()
        },
      )?;
      records.write("setup", json!({"case": case, "action": setup}))?;
      if no_input {
        session.window().activate(&other)?;
      }
    }
    let mut requested = window.clone();
    if kind == "owner_mismatch" {
      requested.process_id = Some(0);
    }
    if matches!(kind, "stay" | "aba" | "minimize" | "close") {
      ensure!(
        a.request(json!({"cmd": "arm_mouse_fault", "kind": kind, "target": b.hwnd().to_string(),
          "trigger": if scrolling { "wheel" } else { "click" }}))?["armed"]
          == true,
        "mouse fault was not armed"
      );
    }
    let before_a = a.request(json!({"cmd": "state"}))?;
    let before_b = b.request(json!({"cmd": "state"}))?;
    let before = watch.snapshot();
    ensure!(before.physical_keys == 0 && before.physical_mouse == 0, "physical input: case refused");
    let cancellation = Arc::new(InputCancellation::default());
    if kind == "pre_cancel" {
      cancellation.cancel();
    }
    let deliver = || {
      with_input_cancellation(cancellation.clone(), || {
        if scrolling {
          return session.window().scroll(
            &requested,
            point,
            Scroll::new(
              0.0,
              if kind == "invalid_delta" {
                f64::NAN
              } else {
                100.0
              },
            ),
            ScrollOptions {
              policy: InputPolicy::ForegroundPreferred,
              settle: if kind == "invalid_settle" {
                Duration::MAX
              } else {
                Duration::from_secs(1)
              },
              ..Default::default()
            },
          );
        }
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
      })
    };
    let cancel_after_effect = matches!(kind, "cancel_after_click" | "cancel_after_effect");
    let (operation, cancellation_receipt) = if cancel_after_effect {
      // Cancel only after the application acknowledges delivery, as in the
      // keyboard prefix case. No delay is used to guess whether input arrived.
      std::thread::scope(|scope| -> Result<_> {
        let observer_cancellation = cancellation.clone();
        let receiver = &mut a;
        let observer = scope.spawn(move || {
          let receipt = (|| -> Result<Value> {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
              let state = receiver.request(json!({"cmd": "state"}))?;
              let count = if scrolling {
                state["raw"]["edit_wheel"].as_u64()
              } else {
                state["clicks"].as_u64()
              }
              .context("cancel observer omitted delivered count")?;
              if count > 0 {
                return Ok(if scrolling {
                  json!({"after_wheels": count, "requested": true})
                } else {
                  json!({"after_clicks": count, "requested": true})
                });
              }
              ensure!(Instant::now() < deadline, "application never acknowledged the first mouse effect");
              std::thread::sleep(Duration::from_millis(10));
            }
          })();
          observer_cancellation.cancel();
          receipt
        });
        let operation = deliver();
        let receipt = observer.join().map_err(|_| anyhow::anyhow!("cancel observer panicked"))??;
        Ok((operation, receipt))
      })?
    } else {
      (deliver(), Value::Null)
    };
    let terminal = if kind == "close" {
      a.closing_receipt()?
    } else {
      Value::Null
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    let (after_a, after_b, fault) = loop {
      let a_state = if terminal.is_null() {
        a.request(json!({"cmd": "state"}))?
      } else {
        terminal["state"].clone()
      };
      let b_state = b.request(json!({"cmd": "state"}))?;
      let receipt = if terminal.is_null() {
        a.request(json!({"cmd": "mouse_fault_receipt"}))?
      } else {
        json!({"receipt": terminal["fault"]})
      };
      let total = if scrolling {
        a_state["raw"]["edit_wheel"].as_u64().unwrap_or(0) + b_state["raw"]["edit_wheel"].as_u64().unwrap_or(0)
      } else {
        a_state["clicks"].as_u64().unwrap_or(0) + b_state["clicks"].as_u64().unwrap_or(0)
      };
      let expected = if no_input {
        0
      } else if scrolling {
        1
      } else if operation.is_ok() {
        3
      } else {
        1
      };
      if total >= expected && (no_input || kind == "success" || cancel_after_effect || !receipt["receipt"].is_null()) {
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
    let cancelled = operation.as_ref().err().is_some_and(|error| {
      matches!(error,
      DriverError::InputAction { cause, .. } if matches!(cause.as_ref(),
        DriverError::InvalidInput { message } if message.contains("cancelled")))
    });
    let scrolled = after_a["raw"]["edit_wheel"] == 1
      && after_a["first_line"].as_i64().context("recipient omitted final scroll position")?
        > before_a["first_line"].as_i64().context("recipient omitted initial scroll position")?
      && after_b["raw"]["edit_wheel"] == before_b["raw"]["edit_wheel"]
      && after_b["first_line"] == before_b["first_line"];
    let passed = if no_input {
      operation.is_err()
        && matches!(progress, Some(auv_driver_common::InputActionProgress::NotDelivered))
        && (kind != "owner_mismatch" || target_lost)
        && (kind != "pre_cancel" || cancelled)
        && after_a["clicks"] == 0
        && after_b["clicks"] == 0
        && after_a["raw"]["edit_wheel"] == before_a["raw"]["edit_wheel"]
        && after_a["first_line"] == before_a["first_line"]
        && after_b["raw"]["edit_wheel"] == before_b["raw"]["edit_wheel"]
        && after_b["first_line"] == before_b["first_line"]
        && after.foreground == before.foreground
        && after.cursor == before.cursor
        && after.injected_mouse == before.injected_mouse
        && after.injected_keys == before.injected_keys
    } else if kind == "success" {
      operation.is_ok()
        && if scrolling {
          scrolled
        } else {
          after_a["clicks"] == 3 && after_b["clicks"] == 0
        }
    } else if cancel_after_effect {
      cancelled
        && !target_lost
        && matches!(progress, Some(auv_driver_common::InputActionProgress::Submitted { action })
          if action.mouse_disturbance == auv_driver_common::DisturbanceLevel::Temporary)
        && if scrolling {
          cancellation_receipt["after_wheels"] == 1
        } else {
          cancellation_receipt["after_clicks"] == 1
        }
        && cancellation_receipt["requested"] == true
        && if scrolling {
          scrolled
        } else {
          after_a["clicks"] == 1 && after_b["clicks"] == 0
        }
    } else {
      operation.is_err()
        && target_lost
        && matches!(progress, Some(auv_driver_common::InputActionProgress::Submitted { action })
          if action.mouse_disturbance == auv_driver_common::DisturbanceLevel::Temporary)
        && fault["receipt"]["reached_target"] == true
        && if scrolling {
          scrolled
        } else {
          after_a["clicks"] == 1 && after_b["clicks"] == 0
        }
        && (kind != "aba" || fault["receipt"]["returned_owner"] == true)
        && (kind != "minimize" || after_a["minimized"] == true)
        && (kind != "close" || terminal["event"] == "closing")
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
      "cancelled": cancelled, "cancellation_receipt": cancellation_receipt, "terminal_receipt": terminal,
      "operation": if scrolling { "scroll" } else { "click" },
      "action": operation.as_ref().ok(), "error": operation.as_ref().err().map(ToString::to_string),
      "foreground_before": before.foreground, "foreground_after": after.foreground,
      "cursor_before": before.cursor, "cursor_after": after.cursor,
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
