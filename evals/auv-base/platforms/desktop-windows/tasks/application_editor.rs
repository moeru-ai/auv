//! Editor acceptance uses AUV commands for every action and the application's
//! saved file as its semantic oracle. Failed or uncertain actions are never retried.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use auv_driver_common::{Driver, window::Window};
use auv_driver_windows::WindowsDriver;
use serde_json::{Value, json};

use super::super::super::super::records::Records;
use super::super::super::super::watch::DesktopWatch;
use super::{helper_command, inspect_controls, native};

fn invoke(
  processes: &mut native::Processes,
  auv: &Path,
  directory: &Path,
  command_id: &str,
  args: &[&str],
  target: &str,
  watch: &DesktopWatch,
) -> Result<Value> {
  let witness = watch.snapshot();
  ensure!(witness.physical_keys == 0 && witness.physical_mouse == 0, "physical input: action refused");
  std::fs::create_dir(directory)?;
  let mut command = helper_command("app-host", directory)?;
  command
    .arg("--program")
    .arg(auv)
    .arg("--logs")
    .arg(directory)
    .arg("--")
    .args(["invoke", command_id])
    .args(args)
    .args(["--target", target, "--json", "--no-overlay", "--store-root"])
    .arg(directory.join("store"));
  let child = processes.start(command)?;
  let exit = processes.wait(child, Duration::from_secs(30))?;
  let stdout = std::fs::read(directory.join("app.stdout.log"))?;
  let reply: Value = serde_json::from_slice(&stdout).context("AUV command omitted structured JSON; raw stdout/stderr retained")?;
  Ok(json!({"exit": exit, "reply": reply}))
}

fn saved_text(path: &Path) -> Result<(String, &'static str, usize)> {
  let bytes = std::fs::read(path)?;
  let (text, encoding) = if let Some(body) = bytes.strip_prefix(&[0xff, 0xfe]) {
    let (pairs, remainder) = body.as_chunks::<2>();
    ensure!(remainder.is_empty(), "incomplete UTF-16 saved file");
    let units: Vec<u16> = pairs.iter().copied().map(u16::from_le_bytes).collect();
    (String::from_utf16(&units)?, "utf16le")
  } else {
    (String::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes).to_vec())?, "utf8")
  };
  Ok((text.replace("\r\n", "\n"), encoding, bytes.len()))
}

pub(super) fn run(
  app: &str,
  auv: &Path,
  directory: &Path,
  fixture: &Path,
  window: &Window,
  processes: &mut native::Processes,
  watch: &DesktopWatch,
) -> Result<Value> {
  let marker = fixture.file_stem().context("fixture omitted marker")?.to_string_lossy();
  let mut records = Records::create(&directory.join("editor-steps.jsonl"), &marker, "application-editor-steps")?;
  let app_target = format!("app:{}", window.app_name.as_deref().context("window omitted executable")?);
  let window_target = format!("window:{}", window.reference.id);
  let before_file = std::fs::read(fixture)?;
  let result = (|| -> Result<Value> {
    // The public semantic command selects by application. Refuse a competing
    // main window before using that selector; check the result's PID as well.
    let windows = WindowsDriver::new().open_local()?.window().list()?;
    let candidates: Vec<_> =
      windows.iter().filter(|candidate| candidate.is_visible && candidate.is_main && candidate.app_name == window.app_name).collect();
    ensure!(
      candidates.len() == 1 && candidates[0].reference == window.reference && candidates[0].process_id == window.process_id,
      "application selector has competing or changed main windows"
    );
    let before = watch.snapshot();
    let missing = format!("AUV-ABSENT-{marker}");
    let refusal = invoke(processes, auv, &directory.join("01-refusal"), "input.focusText", &[&missing], &app_target, watch)?;
    let after = watch.snapshot();
    let refusal_ok = refusal["reply"]["status"] == "failed"
      && refusal["reply"]["failure_details"]["code"] == "not_found"
      && refusal["reply"]["failure_details"]["input_progress"]["state"] == "not_delivered"
      && after.injected_keys == before.injected_keys
      && after.injected_mouse == before.injected_mouse
      && after.foreground == before.foreground
      && after.cursor == before.cursor
      && std::fs::read(fixture)? == before_file;
    records.write("refusal", json!({"passed": refusal_ok, "operation": refusal,
      "injected_keys": after.injected_keys.saturating_sub(before.injected_keys), "injected_mouse": after.injected_mouse.saturating_sub(before.injected_mouse)}))?;
    ensure!(refusal_ok, "absent focus query did not preserve the zero-input refusal contract");

    let strategy = if app == "notepadnext" {
      "planned_ocr_click"
    } else {
      "semantic_focus"
    };
    let focus = if app == "notepadnext" {
      // Explicit strategy from the recorded Qt inventory: the editor has no
      // usable UIA name/value. This is not a fallback after an uncertain action.
      invoke(
        processes,
        auv,
        &directory.join("02-focus"),
        "window.clickText",
        &[
          "AUV controlled editor",
          "--input-policy",
          "foreground-preferred",
        ],
        &window_target,
        watch,
      )?
    } else {
      let query = match app {
        "edge" => "AUV editor".to_string(),
        "powershell-ise" => "Text Editor".to_string(),
        "vscode" => fixture.file_name().unwrap().to_string_lossy().into_owned(),
        _ => unreachable!(),
      };
      invoke(processes, auv, &directory.join("02-focus"), "input.focusText", &[&query], &app_target, watch)?
    };
    records.write("focus", json!({"strategy": strategy, "operation": focus}))?;
    ensure!(focus["reply"]["status"] == "completed", "planned editor focus failed; do not retry");
    if app != "notepadnext" {
      let result = &focus["reply"]["result"];
      ensure!(
        result["pid"].as_u64() == window.process_id.map(u64::from)
          && result["role"] == "Edit"
          && result["input_action_result"]["verified"] == true,
        "semantic focus did not verify the owned editor"
      );
    }
    let focused = directory.join("focused");
    std::fs::create_dir(&focused)?;
    let readback = inspect_controls(processes, window, &focused)?;
    let focused_editor = readback["tree"]["nodes"].as_array().context("focus readback omitted controls")?.iter().any(|node| {
      node["focused"] == true
        && node["keyboard_focusable"] == true
        && node["enabled"] == true
        && ((node["role"] == "Edit" && node["name"] == focus["reply"]["result"]["title"])
          || (app == "notepadnext" && node["role"] == "Group"))
    });
    records.write("focus_readback", json!({"focused_editor": focused_editor, "snapshot": readback}))?;
    ensure!(focused_editor, "independent snapshot did not confirm editor focus");

    let expected = format!("# AUV saved {app} {marker}");
    for (step, command_id, args) in [
      ("03-select", "input.keys", vec!["ctrl", "a", "--input-policy", "foreground-preferred"]),
      ("04-type", "input.typeText", vec![expected.as_str(), "--input-policy", "foreground-preferred"]),
      ("05-save", "input.keys", vec!["ctrl", "s", "--input-policy", "foreground-preferred"]),
    ] {
      ensure!(processes.contains(window.process_id.context("editor omitted PID")?)?, "editor no longer belongs to controller");
      let operation = invoke(processes, auv, &directory.join(step), command_id, &args, &window_target, watch)?;
      records.write("command", json!({"step": step, "command_id": command_id, "operation": operation}))?;
      ensure!(operation["reply"]["status"] == "completed", "{step} failed or has uncertain delivery; no retry");
    }
    let saved = if app == "edge" {
      directory.join("downloads").join(format!("{marker}.txt"))
    } else {
      fixture.to_path_buf()
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let (actual, encoding, bytes) = loop {
      if let Ok(value) = saved_text(&saved)
        && value.0 == expected
      {
        break value;
      }
      ensure!(Instant::now() < deadline, "app-owned saved file did not match expected text");
      std::thread::sleep(Duration::from_millis(25));
    };
    Ok(json!({"passed": true, "strategy": strategy, "expected": expected, "saved_text": actual,
      "saved_path": saved, "encoding": encoding, "byte_length": bytes, "refusal_verified": true}))
  })();
  let receipt = match result {
    Ok(value) => value,
    Err(error) => json!({"passed": false, "error": format!("{error:#}"), "delivery_retry": false}),
  };
  std::fs::write(directory.join("editor-case.json"), serde_json::to_vec_pretty(&receipt)?)?;
  Ok(receipt)
}
