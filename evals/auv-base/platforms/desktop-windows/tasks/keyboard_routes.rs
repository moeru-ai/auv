//! Independent Win32 effects for one keyboard batch across public frontends.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use auv_driver_common::input::InputPolicy;
use auv_driver_common::window::Window;
use auv_driver_common::{Driver, DriverError, InputTarget, KeyboardInput, PressKeysOptions};
use auv_driver_windows::{WindowsDriver, WindowsDriverSession};
use serde_json::{Value, json};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::IsWindow;

use super::super::super::background::{discover_by_hwnd, run_id};
use super::super::super::receiver_process::{Receiver, ReceiverSpec, receiver_binary};
use super::super::super::records::Records;
use super::super::super::watch::DesktopWatch;
use super::{Runner, command_output, runner_cleanup_verified};

#[path = "keyboard_routes_mcp.rs"]
mod mcp;

fn cli(auv: &Path, runner: Option<&Runner>, store: &Path, window: &Window) -> Result<Value> {
  let mut command = match runner {
    Some(runner) => {
      let mut command = runner.command();
      command.args(["--run", &runner.run]);
      command
    }
    None => {
      let mut command = Command::new(auv);
      command.stdin(Stdio::null()).creation_flags(0x0800_0000);
      command
    }
  };
  command
    .args([
      "invoke",
      "input.key",
      "a",
      "--count",
      "3",
      "--interval-ms",
      "400",
      "--input-policy",
      "foreground-preferred",
      "--target",
      &format!("window:{}", window.reference.id),
      "--json",
      "--no-overlay",
      "--store-root",
    ])
    .arg(store);
  let output = command_output(command, Duration::from_secs(20))?;
  let reply: Value = serde_json::from_slice(&output.stdout)
    .with_context(|| format!("CLI omitted JSON (exit {:?}): {}", output.status.code(), String::from_utf8_lossy(&output.stderr)))?;
  Ok(json!({"exit": output.status.code(), "reply": reply}))
}

fn direct(session: &WindowsDriverSession, window: &Window) -> Value {
  match session.input().input_keyboard(
    &InputTarget::Window(window.clone()),
    vec![KeyboardInput::PressKeys {
      options: PressKeysOptions {
        keys: vec!["a".into()],
        count: 3,
        interval: Duration::from_millis(400),
        settle: Duration::ZERO,
      },
      policy: InputPolicy::ForegroundPreferred,
    }],
    false,
  ) {
    Ok(actions) => json!({"status": "completed", "actions": actions}),
    Err(error) => {
      let (target_lost, native) = match &error.cause {
        DriverError::InputAction { cause, progress } => (matches!(cause.as_ref(), DriverError::TargetLost { .. }), Some(progress)),
        _ => (false, None),
      };
      // Preserve the shared domain receipts; the app/witness checks below are
      // separate from these driver claims and from frontend JSON projections.
      json!({"status": "failed", "cause": format!("{:?}", error.cause), "target_lost": target_lost,
        "keyboard_progress": error.progress, "input_progress": native})
    }
  }
}

fn arguments(window: &Window, store: &Path, dry_run: bool) -> Value {
  json!({"command_id": "input.key", "target": {"window_id": window.reference.id},
    "inputs": {"key": "a", "count": "3", "interval-ms": "400", "input-policy": "foreground-preferred"},
    "dry_run": dry_run, "store_root": store.to_string_lossy()})
}

pub fn run(output: &Path, auv: &Path) -> Result<bool> {
  ensure!(
    std::env::var("AUV_ISOLATED_DESKTOP_EVAL").as_deref() == Ok("1"),
    "native keyboard routes require AUV_ISOLATED_DESKTOP_EVAL=1 on an isolated desktop"
  );
  ensure!(!output.exists(), "keyboard route evidence needs a fresh output directory");
  std::fs::create_dir_all(output)?;
  let auv = auv.canonicalize().context("AUV binary missing")?;
  let mut records = Records::create(&output.join("keyboard-routes.jsonl"), &run_id(), "keyboard-routes")?;
  let session = WindowsDriver::new().open_local()?;
  let watch = DesktopWatch::start()?;
  let original = watch.snapshot();
  ensure!(original.physical_keys == 0 && original.physical_mouse == 0, "physical input prevents native route admission");
  let binary = receiver_binary()?;
  let root = output.join("owned-runner");
  let mut runner = match Runner::start(&auv, &root, output) {
    Ok(runner) => runner,
    Err(mut failure) => {
      let cleanup = failure.started.as_mut().map(Runner::finish).transpose();
      records.write(
        "runner_admission",
        json!({"admitted": false, "error": failure.error.to_string(),
        "cleanup": cleanup.as_ref().ok(), "cleanup_error": cleanup.as_ref().err().map(ToString::to_string)}),
      )?;
      return Ok(false);
    }
  };
  records.write(
    "runner_admission",
    json!({"admitted": true, "run": runner.run,
    "endpoint": runner.endpoint, "auv": auv, "source": std::env::var("AUV_EVAL_SOURCE_SHA").ok()}),
  )?;
  let mut all_passed = true;
  let mut case_count = 0;
  // These are record labels for this scoped batch evaluation. The full GUI
  // workflow keeps its own three-route enum until its separate MCP gate lands.
  for route in ["rust", "invoke", "runner", "mcp-local", "mcp-runner"] {
    let route_directory = output.join(route);
    std::fs::create_dir(&route_directory)?;
    let mut client = if route.starts_with("mcp-") {
      let mut command = runner.command();
      if route == "mcp-runner" {
        command.args(["--run", &runner.run]);
      }
      Some(mcp::Client::spawn(command, &route_directory.join("mcp.stderr.log"))?)
    } else {
      None
    };
    let route_result = (|| -> Result<()> {
      if let Some(client) = &mut client {
        let admission = client.initialize()?;
        records.write("mcp_admission", json!({"route": route, "admitted": true, "receipt": admission}))?;
      }
      let kinds: &[&str] = if client.is_some() {
        &["success", "takeover", "cancel"]
      } else {
        &["success", "takeover"]
      };
      for kind in kinds {
        case_count += 1;
        let directory = route_directory.join(kind);
        std::fs::create_dir(&directory)?;
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
        let mut a = Receiver::spawn_detached_with(&binary, &spec(0, "AUV keyboard route A", 40), &["--foreground-input"])?;
        let mut b = Receiver::spawn_detached_with(&binary, &spec(1, "AUV keyboard route B", 510), &["--foreground-input"])?;
        let evaluation = (|| -> Result<bool> {
          a.request(json!({"cmd": "keep_foreground"}))?;
          b.request(json!({"cmd": "keep_foreground"}))?;
          ensure!(b.request(json!({"cmd": "prepare_keyboard"}))?["focused"] == true, "alternate control not ready");
          let (window, _) = discover_by_hwnd(&session, a.hwnd())?;
          let (other, _) = discover_by_hwnd(&session, b.hwnd())?;
          session.window().activate(&window)?;
          ensure!(a.request(json!({"cmd": "prepare_keyboard"}))?["focused"] == true, "recipient control not ready");
          if *kind == "takeover" {
            ensure!(
              a.request(json!({"cmd": "arm_keyboard_fault", "kind": "stay", "target": b.hwnd().to_string()}))?["armed"] == true,
              "takeover fault was not acknowledged"
            );
          }
          let before_a = a.request(json!({"cmd": "state"}))?;
          let before_b = b.request(json!({"cmd": "state"}))?;
          let gate = watch.snapshot();
          ensure!(
            gate.foreground == a.hwnd() && gate.physical_keys == 0 && gate.physical_mouse == 0,
            "native route admission lost foreground or detected physical input"
          );
          ensure!(before_a["typing"]["text"] == "" && before_b["typing"]["text"] == "", "fixture text is not empty");
          let store = directory.join("store");
          if let Some(client) = &client {
            let dry = client.invoke(arguments(&window, &store, true))?;
            let after = watch.snapshot();
            let admitted = dry["structuredContent"]["status"] == "completed"
              && after.injected_keys == gate.injected_keys
              && after.injected_mouse == gate.injected_mouse
              && after.foreground == gate.foreground
              && after.cursor == gate.cursor;
            records.write("mcp_route_admission", json!({"route": route, "case": kind, "admitted": admitted, "reply": dry}))?;
            ensure!(admitted, "MCP dry run did not admit the real route without input");
          }
          let before = watch.snapshot();
          let operation = match route {
            "rust" => direct(&session, &window),
            "invoke" => cli(&auv, None, &store, &window)?,
            "runner" => cli(&auv, Some(&runner), &store, &window)?,
            _ if *kind == "cancel" => client.as_ref().unwrap().cancel_after_first(arguments(&window, &store, false), &mut a)?,
            _ => client.as_ref().unwrap().invoke(arguments(&window, &store, false))?,
          };
          let after_a = if *kind == "success" {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
              let state = a.request(json!({"cmd": "state"}))?;
              if state["typing"]["text"] == "aaa" {
                break state;
              }
              ensure!(Instant::now() < deadline, "application did not acknowledge all three keys");
              std::thread::sleep(Duration::from_millis(10));
            }
          } else {
            a.request(json!({"cmd": "state"}))?
          };
          let after_b = b.request(json!({"cmd": "state"}))?;
          let fault = a.request(json!({"cmd": "keyboard_fault_receipt"}))?;
          let after = watch.snapshot();
          let reply = if route.starts_with("mcp-") {
            &operation["structuredContent"]
          } else if route == "rust" {
            &operation
          } else {
            &operation["reply"]
          };
          let details = if route == "rust" {
            &operation
          } else {
            &reply["failure_details"]
          };
          let typed_loss = if route == "rust" {
            operation["target_lost"] == true
          } else {
            details["code"] == "target_lost"
          };
          let keys_b = after_b["typing"]["keys"].as_array().context("alternate omitted key messages")?;
          let injected_keys = after.injected_keys.saturating_sub(before.injected_keys);
          let passed = match *kind {
            "success" => reply["status"] == "completed" && after_a["typing"]["text"] == "aaa" && injected_keys == 6,
            "takeover" => {
              reply["status"] == "failed"
                && typed_loss
                && details["keyboard_progress"]["completed_presses"] == 1
                && details["input_progress"]["state"] == "submitted"
                && after_a["typing"]["text"] == "a"
                && injected_keys == 2
                && fault["receipt"]["reached_target"] == true
                && after.foreground == b.hwnd()
            }
            "cancel" => {
              operation["cancellation_sent"] == true
                && operation["acknowledged_text"] == "a"
                && after_a["typing"]["text"] == "a"
                && injected_keys == 2
                && after.foreground == a.hwnd()
            }
            _ => unreachable!(),
          } && after_b["typing"]["text"] == ""
            && keys_b.is_empty()
            && after.physical_keys == 0
            && after.physical_mouse == 0
            && after.injected_mouse == before.injected_mouse
            && after.cursor == before.cursor;
          // Controller-owned AUV captures are a separate visual evidence channel.
          // Full pixels are requested here and never sent through the MCP client.
          // NOTICE: PNGs are retained for independent visual review; IPC text
          // remains the automated semantic oracle, not the capture return flag.
          let a_pixels = session.window().capture(&window)?;
          let b_pixels = session.window().capture(&other)?;
          a_pixels.image.save(directory.join("a-after.png"))?;
          b_pixels.image.save(directory.join("b-after.png"))?;
          let events: Vec<Value> = watch
            .timeline(before.taken_ms, after.taken_ms)
            .foreground_events
            .iter()
            .map(|event| json!({"at": event.at, "named": event.window, "actual": event.actual}))
            .collect();
          records.write(
            "case",
            json!({"route": route, "case": kind, "passed": passed, "operation": operation,
            "before_a": before_a, "before_b": before_b, "after_a": after_a, "after_b": after_b, "fault": fault,
            "injected_keys": injected_keys, "injected_mouse": after.injected_mouse.saturating_sub(before.injected_mouse),
            "physical_keys": after.physical_keys, "physical_mouse": after.physical_mouse, "events": events,
            "foreground_before": before.foreground, "foreground_after": after.foreground,
            "a_capture": {"file": "a-after.png", "backend": a_pixels.backend, "size": [a_pixels.image.width(), a_pixels.image.height()]},
            "b_capture": {"file": "b-after.png", "backend": b_pixels.backend, "size": [b_pixels.image.width(), b_pixels.image.height()]},
            "store_root": store, "input_retries": 0}),
          )?;
          Ok(passed)
        })();
        if let Err(error) = &evaluation {
          records.write("case_error", json!({"route": route, "case": kind, "error": error.to_string()}))?;
        }
        all_passed &= evaluation.as_ref().is_ok_and(|passed| *passed);
        let current = watch.snapshot();
        if current.foreground == a.hwnd() {
          let _ = a.yield_foreground(original.foreground);
        } else if current.foreground == b.hwnd() {
          let _ = b.yield_foreground(original.foreground);
        }
        for receiver in [&mut a, &mut b] {
          let hwnd = receiver.hwnd();
          let exited = receiver.close().is_ok();
          // SAFETY: read-only liveness check after closing our own receiver.
          let destroyed = unsafe { !IsWindow(HWND(hwnd as _)).as_bool() };
          records.write(
            "receiver_cleanup",
            json!({"route": route, "case": kind, "window": hwnd,
            "process_exited": exited, "window_destroyed": destroyed}),
          )?;
          all_passed &= exited && destroyed;
        }
      }
      Ok(())
    })();
    if let Err(error) = route_result {
      all_passed = false;
      records.write("route_error", json!({"route": route, "error": error.to_string()}))?;
    }
    if let Some(client) = &mut client {
      let cleanup = client.close();
      all_passed &= cleanup.as_ref().is_ok_and(|receipt| receipt["process_exited"] == true && receipt["forced_kill"] == false);
      records.write(
        "mcp_cleanup",
        json!({"route": route, "receipt": cleanup.as_ref().ok(),
        "error": cleanup.as_ref().err().map(ToString::to_string)}),
      )?;
    }
  }
  let cleanup = runner.finish();
  all_passed &= cleanup.as_ref().is_ok_and(runner_cleanup_verified);
  records.write("runner_cleanup", json!({"receipt": cleanup.as_ref().ok(), "error": cleanup.as_ref().err().map(ToString::to_string)}))?;
  let end = watch.snapshot();
  let passed = all_passed
    && case_count == 12
    && end.foreground == original.foreground
    && end.cursor == original.cursor
    && end.physical_keys == 0
    && end.physical_mouse == 0;
  records.write(
    "summary",
    json!({"passed": passed, "cases": case_count, "pointer_restored": end.cursor == original.cursor,
    "foreground_restored": end.foreground == original.foreground, "physical_keys": end.physical_keys,
    "physical_mouse": end.physical_mouse, "input_retries": 0}),
  )?;
  Ok(passed)
}
