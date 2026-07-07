//! `open-window` command: ensure Apple Music is running and its window is
//! visible, optionally waiting for the window to appear after launch.
//!
//! The command tries three steps in order:
//!
//! 1. **Resolve** — check whether the Apple Music window is already visible.
//! 2. **Launch** — if not found, discover Apple Music's registered Start app
//!    AppUserModelID and activate it through the MSIX `AppsFolder` shell URI.
//! 3. **Wait** — poll for the window to appear up to `settle_ms` milliseconds.
//!
//! Only step 1 is executed on non-Windows targets; steps 2 and 3 are
//! Windows-only.

use serde::{Deserialize, Serialize};

use crate::app::{AppleMusicWindow, ResolveOptions, resolve_window};

#[cfg(target_os = "windows")]
use crate::app::APPLE_MUSIC_TITLE;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

// NOTICE(apple-music-store-aumid): Microsoft Store apps launch by
// AppUserModelID, not executable path. Prefer `Get-StartApps` at runtime
// because Apple has used more than one package/application ID shape. The list
// below is only an offline fallback for older hosts or locked-down shells.
const FALLBACK_APP_USER_MODEL_IDS: &[&str] = &[
  "AppleInc.AppleMusicWin_nzyj5cx40ttqa!App",
  "AppleInc.AppleMusicWin_nzyj5cx40ttqa!AppleMusic",
  "AppleInc.AppleMusic_nzyj5cx40ttqa!AppleMusic",
];

#[cfg(target_os = "windows")]
const DISCOVER_APP_IDS_SCRIPT: &str = r#"
$apps = Get-StartApps | Where-Object {
  $_.Name -eq 'Apple Music' -or
  $_.Name -like 'Apple Music*' -or
  $_.AppID -like 'AppleInc.AppleMusic*'
}
$apps | Select-Object -ExpandProperty AppID
"#;

/// Default polling interval while waiting for the window to appear (ms).
const POLL_INTERVAL_MS: u64 = 300;

/// A recorded step inside a [`LaunchResult`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LaunchStep {
  pub name: String,
  pub outcome: String,
  pub note: Option<String>,
}

/// Output produced by [`run_open_window`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LaunchResult {
  pub command: String,
  pub window_found: bool,
  pub window_title: Option<String>,
  pub steps: Vec<LaunchStep>,
}

impl LaunchResult {
  fn new() -> Self {
    Self {
      command: "open-window".to_string(),
      window_found: false,
      window_title: None,
      steps: Vec::new(),
    }
  }

  fn push(&mut self, name: impl Into<String>, outcome: impl Into<String>, note: Option<String>) {
    self.steps.push(LaunchStep {
      name: name.into(),
      outcome: outcome.into(),
      note,
    });
  }

  fn record_window(&mut self, window: AppleMusicWindow) {
    self.window_found = true;
    self.window_title = window.window.title.clone();
  }
}

/// Inputs for the `open-window` command.
#[derive(Clone, Debug)]
pub struct OpenWindowInputs {
  /// How long to wait for the window to appear after launching (ms).
  ///
  /// Set to 0 to disable the wait/poll phase entirely.
  pub settle_ms: u64,
  /// Window resolution options (process name, title substring).
  pub resolve: ResolveOptions,
}

impl Default for OpenWindowInputs {
  fn default() -> Self {
    Self {
      settle_ms: 8_000,
      resolve: ResolveOptions::default(),
    }
  }
}

/// Ensures Apple Music is open and returns the window reference.
///
/// See the module doc for the three-step flow. On non-Windows targets this
/// always reports the window as not found (no launch or poll attempt is made).
pub fn run_open_window(inputs: &OpenWindowInputs) -> Result<LaunchResult, String> {
  let mut result = LaunchResult::new();

  // Step 1: resolve existing window.
  match resolve_window(&inputs.resolve)? {
    Some(window) => {
      result.push("resolve", "found", None);
      result.record_window(window);
      return Ok(result);
    }
    None => {
      result.push("resolve", "not-found", None);
    }
  }

  // Steps 2 and 3 are Windows-only.
  #[cfg(target_os = "windows")]
  {
    let discovered = discover_registered_app_user_model_ids();
    match &discovered {
      Ok(ids) if !ids.is_empty() => result.push("discover-launch-target", "found", Some(format!("app_ids={}", ids.join(", ")))),
      Ok(_) => result.push("discover-launch-target", "not-found", Some("Get-StartApps returned no Apple Music AppID".to_string())),
      Err(error) => result.push("discover-launch-target", "failed", Some(error.clone())),
    }

    // Step 2: launch via the MSIX AppsFolder shell URI.
    for app_id in app_user_model_id_candidates(discovered.unwrap_or_default()) {
      match launch_via_shell_uri(&app_id) {
        Ok(()) => {
          result.push("launch", "ok", Some(format!("app_id={app_id}")));
          break;
        }
        Err(error) => {
          result.push("launch", "failed", Some(format!("app_id={app_id}; {error}")));
        }
      }
    }

    // Step 3: poll until the window appears or settle_ms elapses.
    if inputs.settle_ms > 0 {
      let deadline = Instant::now() + Duration::from_millis(inputs.settle_ms);
      let interval = Duration::from_millis(POLL_INTERVAL_MS);
      loop {
        match resolve_window(&inputs.resolve)? {
          Some(window) => {
            result.push("wait", "appeared", None);
            result.record_window(window);
            return Ok(result);
          }
          None => {}
        }
        if Instant::now() >= deadline {
          break;
        }
        std::thread::sleep(interval);
      }
      result.push("wait", "timeout", Some(format!("window '{}' did not appear within {}ms", APPLE_MUSIC_TITLE, inputs.settle_ms)));
    }
  }

  #[cfg(not(target_os = "windows"))]
  {
    result.push("launch", "unsupported", Some("Apple Music launch is only supported on Windows".to_string()));
  }

  Ok(result)
}

#[cfg(target_os = "windows")]
fn discover_registered_app_user_model_ids() -> Result<Vec<String>, String> {
  use std::process::Command;

  let output = Command::new("powershell.exe")
    .args([
      "-NoProfile",
      "-NonInteractive",
      "-ExecutionPolicy",
      "Bypass",
      "-Command",
      DISCOVER_APP_IDS_SCRIPT,
    ])
    .output()
    .map_err(|e| format!("Get-StartApps discovery failed to start: {e}"))?;

  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    return Err(if stderr.is_empty() {
      format!("Get-StartApps discovery exited with status {}", output.status)
    } else {
      format!("Get-StartApps discovery failed: {stderr}")
    });
  }

  Ok(parse_app_user_model_ids(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_app_user_model_ids(output: &str) -> Vec<String> {
  let mut ids = Vec::new();
  for line in output.lines().map(str::trim) {
    if line.is_empty() || ids.iter().any(|existing| existing == line) {
      continue;
    }
    ids.push(line.to_string());
  }
  ids
}

fn app_user_model_id_candidates(discovered: Vec<String>) -> Vec<String> {
  let mut ids = Vec::new();
  for app_id in discovered.into_iter().chain(FALLBACK_APP_USER_MODEL_IDS.iter().map(|id| id.to_string())) {
    if ids.iter().any(|existing| existing == &app_id) {
      continue;
    }
    ids.push(app_id);
  }
  ids
}

/// Launches Apple Music via the Windows shell `AppsFolder` URI.
///
/// This is the standard way to start an MSIX/Store app without knowing its
/// install path. `explorer.exe` interprets the `shell:AppsFolder\<AppId>`
/// argument and activates the package.
#[cfg(target_os = "windows")]
fn launch_via_shell_uri(app_id: &str) -> Result<(), String> {
  use std::process::Command;

  let target = format!("shell:AppsFolder\\{app_id}");
  Command::new("explorer.exe").arg(&target).spawn().map_err(|e| format!("explorer.exe launch failed: {e}"))?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_app_user_model_ids_trims_empty_lines_and_duplicates() {
    let ids =
      parse_app_user_model_ids("\r\n  AppleInc.AppleMusicWin_nzyj5cx40ttqa!App  \r\n\r\nAppleInc.AppleMusicWin_nzyj5cx40ttqa!App\r\n");

    assert_eq!(ids, vec!["AppleInc.AppleMusicWin_nzyj5cx40ttqa!App".to_string()]);
  }

  #[test]
  fn app_user_model_id_candidates_prefer_discovered_ids() {
    let ids = app_user_model_id_candidates(vec![
      "AppleInc.AppleMusicWin_nzyj5cx40ttqa!AppleMusic".to_string(),
      "AppleInc.AppleMusicWin_nzyj5cx40ttqa!App".to_string(),
    ]);

    assert_eq!(ids[0], "AppleInc.AppleMusicWin_nzyj5cx40ttqa!AppleMusic");
    assert_eq!(ids[1], "AppleInc.AppleMusicWin_nzyj5cx40ttqa!App");
    assert_eq!(ids.iter().filter(|id| id.as_str() == "AppleInc.AppleMusicWin_nzyj5cx40ttqa!App").count(), 1);
  }
}
