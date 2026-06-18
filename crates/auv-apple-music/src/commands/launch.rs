//! `open-window` command: ensure Apple Music is running and its window is
//! visible, optionally waiting for the window to appear after launch.
//!
//! The command tries three steps in order:
//!
//! 1. **Resolve** — check whether the Apple Music window is already visible.
//! 2. **Launch** — if not found, attempt to start Apple Music via the MSIX
//!    app URI (`shell:AppsFolder\AppleInc.AppleMusic_...`).
//! 3. **Wait** — poll for the window to appear up to `settle_ms` milliseconds.
//!
//! Only step 1 is executed on non-Windows targets; steps 2 and 3 are
//! Windows-only.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::app::{APPLE_MUSIC_TITLE, AppleMusicWindow, ResolveOptions, resolve_window};

// NOTICE: the MSIX package family name below is the canonical Microsoft Store
// value for Apple Music on Windows as of 2026. If Apple republishes under a
// different package name the launch step will silently fail and the user must
// open the app manually. A future slice could probe
// `Get-AppxPackage -Name "AppleInc.AppleMusic*"` to discover the live name.
const APPLE_MUSIC_APP_ID: &str = "AppleInc.AppleMusic_nzyj5cx40ttqa!AppleMusic";

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
    // Step 2: launch via the MSIX shell URI.
    let launch_outcome = launch_via_shell_uri(APPLE_MUSIC_APP_ID);
    result.push(
      "launch",
      if launch_outcome.is_ok() {
        "ok"
      } else {
        "failed"
      },
      launch_outcome.err(),
    );

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
      result.push(
        "wait",
        "timeout",
        Some(format!(
          "window '{}' did not appear within {}ms",
          APPLE_MUSIC_TITLE, inputs.settle_ms
        )),
      );
    }
  }

  #[cfg(not(target_os = "windows"))]
  {
    result.push(
      "launch",
      "unsupported",
      Some("Apple Music launch is only supported on Windows".to_string()),
    );
  }

  Ok(result)
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
  Command::new("explorer.exe")
    .arg(&target)
    .spawn()
    .map_err(|e| format!("explorer.exe launch failed: {e}"))?;
  Ok(())
}
