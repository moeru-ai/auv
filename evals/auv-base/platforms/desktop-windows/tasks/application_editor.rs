//! Editor acceptance uses AUV commands for every action and the application's
//! saved file as its semantic oracle. Uncertain input is never retried. A stale,
//! undelivered semantic focus may be reacquired once after an independent read.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use auv_driver_common::{Driver, window::Window};
use auv_driver_windows::WindowsDriver;
use serde_json::{Value, json};

use super::super::super::super::records::Records;
use super::super::super::super::watch::DesktopWatch;
use super::{helper_command, inspect_controls, native};

/// Reacquisition requires both the producer's pre-delivery receipt and the
/// independent desktop witness. Provider errors alone cannot authorize input.
fn focus_reacquisition_allowed(app: &str, reply: &Value, zero_input_witness: bool) -> bool {
  app == "vscode"
    && zero_input_witness
    && reply["status"] == "failed"
    && reply["failure_details"]["code"] == "stale_ui_reference"
    && reply["failure_details"]["input_progress"]["state"] == "not_delivered"
}

fn vscode_onboarding_visible(snapshot: &Value) -> bool {
  // TODO(windows-app-readiness): other modal/occlusion checks need their own
  // recorded cohort/version case; see 2026-10-31-windows-application-workflow.md.
  snapshot["tree"]["nodes"].as_array().is_some_and(|nodes| {
    nodes.iter().any(|node| node["role"] == "Button" && node["name"] == "Continue without Signing In" && node["offscreen"] == false)
  })
}

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
  ensure!(watch.modifiers_released(), "modifier already down: action refused without cleanup input");
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
    if app == "vscode" {
      let initial: Value = serde_json::from_slice(&std::fs::read(directory.join("snapshot.json"))?)?;
      if vscode_onboarding_visible(&initial) {
        // Pixel review of CI 124482099674 found the welcome modal still covering
        // an edited buffer. Dismiss it through AUV and require a fresh readback;
        // saved-file equality alone cannot accept that visually hidden workflow.
        let operation = invoke(
          processes,
          auv,
          &directory.join("00-onboarding-dismiss"),
          "input.key",
          &["escape", "--input-policy", "foreground-preferred"],
          &window_target,
          watch,
        )?;
        records.write("onboarding", json!({"operation": operation}))?;
        ensure!(operation["reply"]["status"] == "completed", "onboarding dismissal failed; stop");
        let ready = directory.join("ready");
        std::fs::create_dir(&ready)?;
        let snapshot = inspect_controls(processes, window, &ready)?;
        records.write("onboarding_readback", json!({"snapshot": snapshot}))?;
        ensure!(snapshot["tree"]["truncated"] == false && !vscode_onboarding_visible(&snapshot), "onboarding still covers the editor");
      }
    }
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
    let query = match app {
      "edge" => "AUV editor".to_string(),
      "powershell-ise" => "Text Editor".to_string(),
      "vscode" => fixture.file_name().unwrap().to_string_lossy().into_owned(),
      "notepadnext" => "AUV controlled editor".to_string(),
      _ => unreachable!(),
    };
    let before_focus = watch.snapshot();
    let mut focus = if app == "notepadnext" {
      // Explicit strategy from the recorded Qt inventory: the editor has no
      // usable UIA name/value. This is not a fallback after an uncertain action.
      invoke(
        processes,
        auv,
        &directory.join("02-focus"),
        "window.clickText",
        &[
          &query,
          "--title",
          &marker,
          "--input-policy",
          "foreground-preferred",
        ],
        // NOTICE: unlike keyboard commands, window.clickText accepts app:
        // targets. The title retains the owned document constraint (CI 124478233297).
        &app_target,
        watch,
      )?
    } else {
      invoke(processes, auv, &directory.join("02-focus"), "input.focusText", &[&query], &app_target, watch)?
    };
    records.write("focus", json!({"strategy": strategy, "operation": focus}))?;
    let mut reacquired = false;
    let after_focus = watch.snapshot();
    let zero_input_witness = after_focus.injected_keys == before_focus.injected_keys
      && after_focus.injected_mouse == before_focus.injected_mouse
      && after_focus.physical_keys == 0
      && after_focus.physical_mouse == 0;
    if focus_reacquisition_allowed(app, &focus["reply"], zero_input_witness) {
      // CI 124478233117: a startup provider element vanished during a read.
      // No SetFocus ran. Require independent zero-input evidence and a new,
      // complete tree before one new query; submitted/uncertain input never enters here.
      let refreshed = directory.join("reacquired");
      std::fs::create_dir(&refreshed)?;
      let snapshot = inspect_controls(processes, window, &refreshed)?;
      let candidates = snapshot["tree"]["nodes"]
        .as_array()
        .context("fresh tree omitted nodes")?
        .iter()
        .filter(|node| {
          node["role"] == "Edit"
            && node["name"] == query
            && node["enabled"] == true
            && node["keyboard_focusable"] == true
            && node["offscreen"] == false
        })
        .count();
      records.write("focus_reacquire", json!({"snapshot": snapshot, "matching_editors": candidates, "prior_delivery": "not_delivered"}))?;
      ensure!(snapshot["tree"]["truncated"] == false && candidates == 1, "fresh tree did not identify one complete editor candidate");
      focus = invoke(processes, auv, &directory.join("02-focus-refreshed"), "input.focusText", &[&query], &app_target, watch)?;
      records.write("focus", json!({"strategy": strategy, "reacquired": true, "operation": focus}))?;
      reacquired = true;
    }
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
    ensure!(app != "vscode" || !vscode_onboarding_visible(&readback), "onboarding appeared before typing; stop");
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
      "saved_path": saved, "encoding": encoding, "byte_length": bytes, "refusal_verified": true, "focus_reacquired": reacquired}))
  })();
  let receipt = match result {
    Ok(value) => value,
    Err(error) => json!({"passed": false, "error": format!("{error:#}"), "uncertain_delivery_retry": false}),
  };
  std::fs::write(directory.join("editor-case.json"), serde_json::to_vec_pretty(&receipt)?)?;
  Ok(receipt)
}

#[cfg(test)]
mod tests {
  use super::*;

  // https://github.com/moeru-ai/auv/actions/runs/41181010921/job/124478233117
  // ROOT CAUSE:
  // A VS Code startup element disappeared before SetFocus. Reacquisition must
  // require not_delivered and independent zero input; a stale code alone cannot
  // distinguish that safe boundary from a failed readback after native delivery.
  #[test]
  fn vscode_focus_reacquisition_ci_124478233117_requires_independent_zero_delivery() {
    let reply = json!({"status": "failed", "failure_details": {"code": "stale_ui_reference", "input_progress": {"state": "not_delivered"}}});
    assert!(focus_reacquisition_allowed("vscode", &reply, true));
    assert!(!focus_reacquisition_allowed("vscode", &reply, false));
    assert!(!focus_reacquisition_allowed("powershell-ise", &reply, true));
  }

  #[test]
  fn focus_reacquisition_refuses_uncertain_submitted_and_missing_receipts() {
    for state in [json!("submitted"), json!("delivery_uncertain"), Value::Null] {
      let reply = json!({"status": "failed", "failure_details": {"code": "stale_ui_reference", "input_progress": {"state": state}}});
      assert!(!focus_reacquisition_allowed("vscode", &reply, true));
    }
    let other = json!({"status": "failed", "failure_details": {"code": "backend", "input_progress": {"state": "not_delivered"}}});
    assert!(!focus_reacquisition_allowed("vscode", &other, true));
  }

  // https://github.com/moeru-ai/auv/actions/runs/41182331780/job/124482099674
  // Pixel review found this visible modal covering a correctly saved editor.
  #[test]
  fn vscode_onboarding_ci_124482099674_blocks_visible_editor_acceptance() {
    let mut snapshot = json!({"tree": {"nodes": [{"role": "Button", "name": "Continue without Signing In", "offscreen": false}]}});
    assert!(vscode_onboarding_visible(&snapshot));
    snapshot["tree"]["nodes"][0]["offscreen"] = json!(true);
    assert!(!vscode_onboarding_visible(&snapshot));
    snapshot["tree"]["nodes"] = json!([]);
    assert!(!vscode_onboarding_visible(&snapshot));
  }
}
