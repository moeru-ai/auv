//! Real application controls and independent evidence. Setup and teardown belong
//! to the controller; application input belongs to AUV's typed execution path.

use std::ffi::OsString;
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use auv_driver_common::{Driver, window::Window};
use auv_driver_windows::{WindowsDriver, WindowsDriverSession};
use serde_json::{Value, json};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use super::super::super::background::run_id;
use super::super::super::records::Records;
use super::super::super::watch::DesktopWatch;

#[path = "application_editor.rs"]
mod editor;
#[path = "application_process.rs"]
mod native;

/// Executables prepared by the isolated controller, never the user's profiles.
pub struct Options {
  pub output: PathBuf,
  pub edge: PathBuf,
  pub vscode: PathBuf,
  pub notepadnext: PathBuf,
  pub powershell_ise: PathBuf,
}

fn isolated() -> Result<()> {
  ensure!(std::env::var("AUV_ISOLATED_DESKTOP_EVAL").as_deref() == Ok("1"), "requires AUV_ISOLATED_DESKTOP_EVAL=1");
  Ok(())
}

fn admitted() -> Result<()> {
  isolated()?;
  let mut byte = [0];
  std::io::stdin().read_exact(&mut byte).context("controller admission pipe closed")?;
  ensure!(byte == *b"\n", "invalid controller admission");
  Ok(())
}

/// Internal subprocess entry. It cannot create an app before job assignment.
#[doc(hidden)]
pub fn launch_host(program: &Path, logs: &Path, args: &[OsString]) -> Result<bool> {
  admitted()?;
  let mut child = Command::new(program)
    .creation_flags(CREATE_NO_WINDOW.0)
    .args(args)
    .stdin(Stdio::null())
    .stdout(std::fs::File::create(logs.join("app.stdout.log"))?)
    .stderr(std::fs::File::create(logs.join("app.stderr.log"))?)
    .spawn()?;
  std::fs::write(logs.join("launch.json"), serde_json::to_vec_pretty(&json!({"pid": child.id(), "program": program}))?)?;
  let status = child.wait()?;
  std::fs::write(logs.join("launcher-exit.json"), serde_json::to_vec_pretty(&json!({"exit": status.code()}))?)?;
  Ok(status.success())
}

/// Internal subprocess entry. A bounded owner can end a blocked provider call.
#[doc(hidden)]
pub fn snapshot_controls(window_id: &str, process_id: u32, output: &Path) -> Result<bool> {
  admitted()?;
  let session = WindowsDriver::new().open_local()?;
  let window = session
    .window()
    .list()?
    .into_iter()
    .find(|window| window.reference.id == window_id && window.process_id == Some(process_id))
    .context("inspector window identity changed")?;
  let mut tree_attempts = Vec::new();
  for _ in 0..3 {
    let tree = match auv_driver_windows::accessibility::snapshot_window(&window) {
      Ok(tree) => json!({"truncated": tree.truncated, "nodes": tree.nodes.iter().map(|node| json!({
      "path": node.path, "depth": node.depth, "role": node.role, "control_type": node.control_type,
      "name": node.name, "value": node.value, "automation_id": node.automation_id,
      "class_name": node.class_name, "focused": node.focused, "enabled": node.enabled,
      "keyboard_focusable": node.keyboard_focusable, "offscreen": node.offscreen, "bounds": node.bounds,
    })).collect::<Vec<_>>() }),
      Err(error) => json!({"error": error.to_string()}),
    };
    let complete = tree["truncated"] == false && tree["nodes"].as_array().is_some_and(|nodes| !nodes.is_empty());
    tree_attempts.push(tree);
    if complete {
      break;
    }
    // This retries only a read-only startup snapshot. No action has been delivered.
    std::thread::sleep(Duration::from_millis(100));
  }
  let tree = tree_attempts.last().context("missing tree attempt")?;
  // Pixels are explicitly requested by this controller; no image bytes cross a client round trip.
  let capture = match session.window().capture(&window) {
    Ok(capture) => {
      capture.image.save(output.join("window.png"))?;
      json!({"file": "window.png", "backend": capture.backend, "width": capture.image.width(), "height": capture.image.height()})
    }
    Err(error) => json!({"error": error.to_string()}),
  };
  std::fs::write(
    output.join("snapshot.json"),
    serde_json::to_vec_pretty(&json!({"window": window, "tree": tree, "tree_attempts": tree_attempts, "capture": capture}))?,
  )?;
  Ok(true)
}

fn helper_command(mode: &str, directory: &Path) -> Result<Command> {
  let mut command = Command::new(std::env::current_exe()?);
  command
    .arg(mode)
    .stdout(std::fs::File::create(directory.join(format!("{mode}.stdout.log")))?)
    .stderr(std::fs::File::create(directory.join(format!("{mode}.stderr.log")))?);
  Ok(command)
}

fn owned_windows(session: &WindowsDriverSession, processes: &native::Processes) -> Result<Vec<Window>> {
  let mut owned = Vec::new();
  for window in session.window().list()? {
    if let Some(pid) = window.process_id
      && processes.contains(pid)?
    {
      owned.push(window);
    }
  }
  Ok(owned)
}

/// The caller supplies only windows whose process handles belong to its job.
fn editor_window<'a>(windows: &'a [Window], app: &str, marker: &str) -> Result<Option<&'a Window>> {
  let matches: Vec<_> =
    windows.iter().filter(|window| window.is_visible && window.title.as_deref().is_some_and(|title| title.contains(marker))).collect();
  ensure!(matches.len() <= 1, "multiple owned windows have the document marker");
  if let Some(window) = matches.first() {
    return Ok(Some(*window));
  }
  if app == "powershell-ise" {
    // NOTICE: ISE keeps an application caption even with an opened file. The
    // failed inventory at job 124298121385 recorded this exact caption; require
    // one owned main window rather than accepting another process by title.
    let main: Vec<_> = windows
      .iter()
      .filter(|window| {
        window.is_visible && window.is_main && window.title.as_deref().is_some_and(|title| title.ends_with("Windows PowerShell ISE"))
      })
      .collect();
    ensure!(main.len() <= 1, "multiple owned ISE main windows");
    return Ok(main.first().copied());
  }
  Ok(None)
}

fn inspect_controls(processes: &mut native::Processes, window: &Window, directory: &Path) -> Result<Value> {
  let pid = window.process_id.context("owned window omitted PID")?;
  ensure!(processes.contains(pid)?, "inspector target no longer belongs to controller");
  let mut inspect = helper_command("app-snapshot", directory)?;
  inspect.arg("--window-id").arg(&window.reference.id).arg("--process-id").arg(pid.to_string()).arg("--output").arg(directory);
  let inspector = processes.start(inspect)?;
  ensure!(processes.wait(inspector, Duration::from_secs(25))? == 0, "application inspector failed");
  Ok(serde_json::from_slice(&std::fs::read(directory.join("snapshot.json"))?)?)
}

/// Launches four real toolkits, records their controls/pixels, and verifies ownership cleanup.
/// NOTICE: this inventory issues no application input and never accepts an edit/save workflow.
pub fn run_controls(options: &Options) -> Result<bool> {
  run(options, None)
}

/// Runs explicit AUV editor/save cases and verifies app-owned files. This does
/// not accept the later scroll/dialog, DPI, or full cross-frontend workflow gates.
pub fn run_editor(options: &Options, auv: &Path) -> Result<bool> {
  ensure!(auv.is_file(), "explicit AUV executable is required");
  run(options, Some(auv))
}

fn run(options: &Options, auv: Option<&Path>) -> Result<bool> {
  isolated()?;
  ensure!(!options.output.exists(), "controls output must be a fresh directory");
  std::fs::create_dir_all(&options.output)?;
  let session = WindowsDriver::new().open_local()?;
  let watch = DesktopWatch::start()?;
  let original = watch.snapshot();
  let original_window = session.window().list()?.into_iter().find(|window| window.reference.id == original.foreground.to_string());
  let run = run_id();
  let scenario = if auv.is_some() {
    "application-editor"
  } else {
    "application-controls"
  };
  let mut records = Records::create(&options.output.join(format!("{scenario}.jsonl")), &run, scenario)?;
  let mut complete = true;
  let mut editors_passed = true;
  let mut attempts = 0;
  for (name, program) in [
    ("edge", &options.edge),
    ("vscode", &options.vscode),
    ("notepadnext", &options.notepadnext),
    ("powershell-ise", &options.powershell_ise),
  ] {
    let before = watch.snapshot();
    if before.physical_keys != original.physical_keys || before.physical_mouse != original.physical_mouse {
      records.write("abort", json!({"reason": "physical input", "next_app": name}))?;
      complete = false;
      break;
    }
    attempts += 1;
    let directory = options.output.join(name);
    std::fs::create_dir(&directory)?;
    let marker = format!("auv-editor-{run}");
    let fixture = directory.join(format!(
      "{marker}.{}",
      if name == "edge" {
        "html"
      } else if name == "powershell-ise" {
        "ps1"
      } else {
        "txt"
      }
    ));
    if name == "edge" {
      std::fs::write(&fixture, include_str!("../test-objects/web/editor.html").replace("AUV_EDITOR_MARKER", &marker))?;
    } else {
      std::fs::write(&fixture, format!("# AUV controlled editor {marker}\r\n"))?;
    }
    let mut processes = native::Processes::new()?;
    let attempt = (|| -> Result<Value> {
      let mut host = helper_command("app-host", &directory)?;
      host.arg("--program").arg(program).arg("--logs").arg(&directory).arg("--");
      match name {
        "edge" => {
          let url = url::Url::from_file_path(&fixture).map_err(|_| anyhow::anyhow!("invalid fixture URL"))?;
          let profile = directory.join("profile/Default");
          let downloads = directory.join("downloads");
          std::fs::create_dir_all(&profile)?;
          std::fs::create_dir(&downloads)?;
          // The app writes downloaded text into this owned directory. The pref is
          // Chromium's `kDownloadDefaultDirectory` in `chrome/common/pref_names.h`.
          std::fs::write(profile.join("Preferences"), serde_json::to_vec_pretty(&json!({"download": {"default_directory": downloads}}))?)?;
          host
            .arg(format!("--user-data-dir={}", directory.join("profile").display()))
            .args([
              "--no-first-run",
              "--no-default-browser-check",
              "--force-renderer-accessibility",
              "--window-size=800,620",
            ])
            .arg(format!("--app={url}"));
        }
        "vscode" => {
          let settings = directory.join("profile/User");
          std::fs::create_dir_all(&settings)?;
          std::fs::write(
            settings.join("settings.json"),
            serde_json::to_vec_pretty(&json!({
              "editor.accessibilitySupport": "on", "workbench.startupEditor": "none", "window.restoreWindows": "none",
              "security.workspace.trust.enabled": false, "files.autoSave": "off", "files.hotExit": "off",
              "telemetry.telemetryLevel": "off", "update.mode": "none", "extensions.autoCheckUpdates": false,
            }))?,
          )?;
          host
            .arg("--user-data-dir")
            .arg(directory.join("profile"))
            .arg("--extensions-dir")
            .arg(directory.join("extensions"))
            .args([
              "--disable-extensions",
              "--disable-workspace-trust",
              "--new-window",
              "--force-renderer-accessibility",
            ])
            .arg(&fixture);
        }
        "powershell-ise" => {
          host.args(["-NoProfile", "-File"]).arg(&fixture);
        }
        "notepadnext" => {
          host.env("QT_ACCESSIBILITY", "1").arg(&fixture);
        }
        _ => unreachable!(),
      }
      processes.start(host)?;
      let deadline = Instant::now() + Duration::from_secs(30);
      let window = loop {
        let windows = owned_windows(&session, &processes)?;
        if let Some(window) = editor_window(&windows, name, &marker)? {
          break window.clone();
        }
        if Instant::now() >= deadline {
          records.write("owned_windows", json!({"app": name, "windows": windows}))?;
          anyhow::bail!("owned editor window did not appear");
        }
        std::thread::sleep(Duration::from_millis(100));
      };
      let snapshot = inspect_controls(&mut processes, &window, &directory)?;
      let mut editor_result = Value::Null;
      if let Some(auv) = auv {
        editor_result = editor::run(name, auv, &directory, &fixture, &window, &mut processes, &watch)?;
        let after = directory.join("after");
        std::fs::create_dir(&after)?;
        let readback = inspect_controls(&mut processes, &window, &after);
        records.write(
          "after_editor",
          json!({"app": name, "snapshot": readback.as_ref().ok(), "error": readback.as_ref().err().map(ToString::to_string)}),
        )?;
      }
      Ok(json!({"window": window, "snapshot": snapshot, "fixture": fixture, "launched_program": program, "editor": editor_result}))
    })();
    let cleanup = processes.finish();
    let after = watch.snapshot();
    let restored = if after.physical_keys == original.physical_keys && after.physical_mouse == original.physical_mouse {
      if let Some(window) = &original_window {
        session.window().activate(window).map_err(|error| error.to_string())
      } else {
        Ok(())
      }
    } else {
      Err("physical input: foreground restoration skipped".to_string())
    };
    complete &= attempt.is_ok() && cleanup.is_ok();
    if auv.is_some() {
      editors_passed &= attempt.as_ref().is_ok_and(|result| result["editor"]["passed"] == true);
    }
    records.write("app", json!({"app": name, "inventory": attempt.as_ref().ok(), "error": attempt.as_ref().err().map(ToString::to_string),
      "cleanup": cleanup.as_ref().ok(), "cleanup_error": cleanup.as_ref().err().map(ToString::to_string), "restore_error": restored.err(),
      "physical_keys": after.physical_keys.saturating_sub(original.physical_keys), "physical_mouse": after.physical_mouse.saturating_sub(original.physical_mouse),
      "injected_keys": after.injected_keys.saturating_sub(original.injected_keys), "injected_mouse": after.injected_mouse.saturating_sub(original.injected_mouse),
      "foreground_before": before.foreground, "foreground_after": after.foreground}))?;
    if auv.is_some() && !editors_passed {
      // A failed/uncertain action ends this input run. Preserve evidence and
      // cleanup rather than driving the remaining applications blindly.
      records.write("abort", json!({"reason": "editor case failed; input run stopped", "app": name}))?;
      break;
    }
  }
  let final_state = watch.snapshot();
  let restored = final_state.foreground == original.foreground && final_state.cursor == original.cursor;
  complete &= attempts == 4
    && restored
    && final_state.physical_keys == original.physical_keys
    && final_state.physical_mouse == original.physical_mouse;
  // TODO(windows-application-workflow): scrolling/dialogs, target-loss cases,
  // DPI, and cross-frontend acceptance remain separate owner-named goal gates;
  // see 2026-10-31-windows-application-workflow.md before expanding these cases.
  records.write("summary", json!({"apps": attempts, "inventory_complete": complete, "edit_save_accepted": auv.is_some() && editors_passed && complete, "workflow_accepted": false,
    "foreground_restored": final_state.foreground == original.foreground, "pointer_restored": final_state.cursor == original.cursor,
    "physical_keys": final_state.physical_keys.saturating_sub(original.physical_keys), "physical_mouse": final_state.physical_mouse.saturating_sub(original.physical_mouse)}))?;
  Ok(complete && (auv.is_none() || editors_passed))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ise_windows() -> Vec<Window> {
    serde_json::from_str(include_str!("../cases/fixtures/powershell-ise-windows.json")).unwrap()
  }

  // https://github.com/moeru-ai/auv/actions/runs/41124132593/job/124298121385
  // ROOT CAUSE:
  // If ISE opened the owned file, discovery still timed out because its window
  // caption omitted the document name. Selection now accepts exactly one ISE
  // main window from the already verified controller job, never an unowned PID.
  #[test]
  fn ise_window_selection_ci_124298121385_accepts_application_caption() {
    let windows = ise_windows();
    let selected = editor_window(&windows, "powershell-ise", "auv-editor-owned").unwrap().unwrap();
    assert_eq!(selected.reference.id, "393326");
    assert_eq!(selected.process_id, Some(9368));
  }

  #[test]
  fn ise_window_selection_refuses_two_owned_main_windows() {
    let mut windows = ise_windows();
    let mut other = windows[0].clone();
    other.reference.id = "another-owned-window".into();
    windows.push(other);
    assert!(editor_window(&windows, "powershell-ise", "auv-editor-owned").unwrap_err().to_string().contains("multiple owned ISE"));
  }

  #[test]
  fn other_apps_still_require_the_owned_document_marker() {
    let windows = ise_windows();
    assert!(editor_window(&windows, "edge", "auv-editor-owned").unwrap().is_none());
  }
}
