//! Ensure the Windows NetEase Cloud Music application has a visible window.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::windows::{DEFAULT_PROCESS_NAME, ResolveOptions};

#[cfg(target_os = "windows")]
use crate::windows::resolve_window;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

const POLL_INTERVAL_MS: u64 = 250;
const FORCE_RENDERER_ACCESSIBILITY_ARG: &str = "--force-renderer-accessibility";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchStep {
  pub name: String,
  pub outcome: String,
  pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchResult {
  pub command: String,
  pub window_found: bool,
  pub window_title: Option<String>,
  pub process_name: String,
  pub executable: Option<String>,
  pub steps: Vec<LaunchStep>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenWindowInputs {
  pub settle_ms: u64,
  pub executable: Option<PathBuf>,
  pub resolve: ResolveOptions,
}

impl Default for OpenWindowInputs {
  fn default() -> Self {
    Self {
      settle_ms: 8_000,
      executable: None,
      resolve: ResolveOptions::default(),
    }
  }
}

impl LaunchResult {
  fn new(inputs: &OpenWindowInputs) -> Self {
    Self {
      command: "open-window".to_string(),
      window_found: false,
      window_title: None,
      process_name: inputs.resolve.process_name.clone(),
      executable: None,
      steps: Vec::new(),
    }
  }

  fn push(&mut self, name: &str, outcome: &str, note: Option<String>) {
    self.steps.push(LaunchStep {
      name: name.to_string(),
      outcome: outcome.to_string(),
      note,
    });
  }
}

pub fn run_open_window(inputs: &OpenWindowInputs) -> Result<LaunchResult, String> {
  platform::run(inputs)
}

/// Resolves the NetEase executable to launch: an explicit path wins outright;
/// otherwise the first candidate under a known Windows install root that
/// exists on disk, falling back to a bare process name for `Command::new` to
/// resolve via `PATH`.
///
/// Takes `lookup` (production callers pass `std::env::var_os`) so the install
/// root search is testable without mutating real process environment state.
fn resolve_executable(explicit: Option<&PathBuf>, lookup: impl Fn(&str) -> Option<std::ffi::OsString>) -> PathBuf {
  if let Some(path) = explicit {
    return path.clone();
  }

  candidate_executables(lookup).into_iter().find(|path| path.is_file()).unwrap_or_else(|| PathBuf::from(DEFAULT_PROCESS_NAME))
}

/// Builds the ordered list of candidate install paths for NetEase's Windows
/// executable, one per environment root that is actually set.
fn candidate_executables(lookup: impl Fn(&str) -> Option<std::ffi::OsString>) -> Vec<PathBuf> {
  let mut candidates = Vec::new();
  for root in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
    if let Some(root) = lookup(root) {
      candidates.push(PathBuf::from(root).join("NetEase").join("CloudMusic").join(DEFAULT_PROCESS_NAME));
    }
  }
  candidates
}

#[cfg(target_os = "windows")]
mod platform {
  use std::process::Command;

  use super::*;
  pub fn run(inputs: &OpenWindowInputs) -> Result<LaunchResult, String> {
    let mut result = LaunchResult::new(inputs);
    if let Some(window) = resolve_window(&inputs.resolve)? {
      result.push("resolve", "found", None);
      activate(&window)?;
      result.push("activate", "ok", None);
      result.window_found = true;
      result.window_title = window.title;
      return Ok(result);
    }
    result.push("resolve", "not_found", None);

    let executable = resolve_executable(inputs.executable.as_ref(), |name| std::env::var_os(name));
    result.executable = Some(executable.display().to_string());
    // NOTICE(netease-windows-cef-uia): NetEase 3.1.35 exposes only its CEF
    // container hierarchy to UIA unless Chromium renderer accessibility is
    // enabled at process start. Keep this launch switch until the client
    // exposes actionable transport controls without it or NetEase documents a
    // different accessibility startup contract.
    Command::new(&executable)
      .arg(FORCE_RENDERER_ACCESSIBILITY_ARG)
      .spawn()
      .map_err(|error| format!("failed to launch {}: {error}", executable.display()))?;
    result.push("launch", "ok", Some(format!("argument={FORCE_RENDERER_ACCESSIBILITY_ARG}")));

    let deadline = Instant::now() + Duration::from_millis(inputs.settle_ms);
    loop {
      if let Some(window) = resolve_window(&inputs.resolve)? {
        result.push("wait", "appeared", None);
        activate(&window)?;
        result.push("activate", "ok", None);
        result.window_found = true;
        result.window_title = window.title;
        return Ok(result);
      }
      if Instant::now() >= deadline {
        break;
      }
      std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    }

    result.push(
      "wait",
      "timeout",
      Some(format!(
        "{DEFAULT_PROCESS_NAME} launched but no visible NetEase window appeared within {}ms; the app may still be tray-only",
        inputs.settle_ms
      )),
    );
    Ok(result)
  }

  fn activate(window: &auv_driver::window::Window) -> Result<(), String> {
    let session = auv_driver::open_local().map_err(|error| format!("failed to open Windows driver: {error}"))?;
    session.window().activate(window).map_err(|error| format!("failed to activate NetEase window: {error}"))
  }
}

#[cfg(not(target_os = "windows"))]
mod platform {
  use super::*;

  pub fn run(inputs: &OpenWindowInputs) -> Result<LaunchResult, String> {
    let mut result = LaunchResult::new(inputs);
    result.push("launch", "unsupported", Some("NetEase open-window is currently supported only on Windows".to_string()));
    Ok(result)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn candidate_executables_includes_all_present_roots_in_order() {
    let lookup = |name: &str| match name {
      "ProgramFiles" => Some(std::ffi::OsString::from("C:\\Program Files")),
      "ProgramFiles(x86)" => Some(std::ffi::OsString::from("C:\\Program Files (x86)")),
      "LOCALAPPDATA" => Some(std::ffi::OsString::from("C:\\Users\\test\\AppData\\Local")),
      _ => None,
    };

    let candidates = candidate_executables(lookup);

    assert_eq!(
      candidates,
      vec![
        PathBuf::from("C:\\Program Files").join("NetEase").join("CloudMusic").join(DEFAULT_PROCESS_NAME),
        PathBuf::from("C:\\Program Files (x86)").join("NetEase").join("CloudMusic").join(DEFAULT_PROCESS_NAME),
        PathBuf::from("C:\\Users\\test\\AppData\\Local").join("NetEase").join("CloudMusic").join(DEFAULT_PROCESS_NAME),
      ]
    );
  }

  #[test]
  fn candidate_executables_skips_roots_that_are_not_set() {
    let lookup = |name: &str| (name == "LOCALAPPDATA").then(|| std::ffi::OsString::from("C:\\Users\\test\\AppData\\Local"));

    let candidates = candidate_executables(lookup);

    assert_eq!(
      candidates,
      vec![PathBuf::from("C:\\Users\\test\\AppData\\Local").join("NetEase").join("CloudMusic").join(DEFAULT_PROCESS_NAME)]
    );
  }

  #[test]
  fn candidate_executables_is_empty_when_no_roots_are_set() {
    assert!(candidate_executables(|_| None).is_empty());
  }

  #[test]
  fn resolve_executable_prefers_an_explicit_path_over_the_lookup() {
    let explicit = PathBuf::from("C:\\Custom\\cloudmusic.exe");

    let resolved = resolve_executable(Some(&explicit), |_| None);

    assert_eq!(resolved, explicit);
  }

  #[test]
  fn default_open_window_inputs_target_cloudmusic() {
    let inputs = OpenWindowInputs::default();

    assert_eq!(inputs.resolve.process_name, DEFAULT_PROCESS_NAME);
    assert_eq!(inputs.settle_ms, 8_000);
    assert_eq!(inputs.executable, None);
  }
}
