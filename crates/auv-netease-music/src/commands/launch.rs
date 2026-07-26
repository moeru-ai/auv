//! Ensure the Windows NetEase Cloud Music application has a visible window.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[cfg(target_os = "windows")]
use crate::windows::DEFAULT_PROCESS_NAME;
use crate::windows::ResolveOptions;

#[cfg(target_os = "windows")]
use crate::windows::resolve_window;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
const POLL_INTERVAL_MS: u64 = 250;
#[cfg(target_os = "windows")]
const FORCE_RENDERER_ACCESSIBILITY_ARG: &str = "--force-renderer-accessibility";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchResult {
  pub command: String,
  pub window_found: bool,
  pub window_title: Option<String>,
  pub process_name: String,
  pub executable: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum LaunchEvent {
  #[cfg(target_os = "windows")]
  ExistingWindowResolved {
    found: bool,
  },
  #[cfg(target_os = "windows")]
  WindowActivated,
  #[cfg(target_os = "windows")]
  ProcessStarted {
    executable: String,
    renderer_accessibility_enabled: bool,
  },
  #[cfg(target_os = "windows")]
  WindowAppeared,
  #[cfg(target_os = "windows")]
  WaitTimedOut {
    timeout_ms: u64,
  },
  UnsupportedPlatform,
}

impl auv_tracing::EventPayload for LaunchEvent {
  const NAME: &'static str = "auv.netease.launch.lifecycle";
  const VERSION: u32 = 1;
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
    }
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
#[cfg(target_os = "windows")]
fn resolve_executable(explicit: Option<&PathBuf>, lookup: impl Fn(&str) -> Option<std::ffi::OsString>) -> PathBuf {
  if let Some(path) = explicit {
    return path.clone();
  }

  candidate_executables(lookup).into_iter().find(|path| path.is_file()).unwrap_or_else(|| PathBuf::from(DEFAULT_PROCESS_NAME))
}

/// Builds the ordered list of candidate install paths for NetEase's Windows
/// executable, one per environment root that is actually set.
#[cfg(target_os = "windows")]
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
      auv_tracing::emit_event!(LaunchEvent::ExistingWindowResolved { found: true });
      activate(&window)?;
      auv_tracing::emit_event!(LaunchEvent::WindowActivated);
      result.window_found = true;
      result.window_title = window.title;
      return Ok(result);
    }
    auv_tracing::emit_event!(LaunchEvent::ExistingWindowResolved { found: false });

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
    auv_tracing::emit_event!(LaunchEvent::ProcessStarted {
      executable: executable.display().to_string(),
      renderer_accessibility_enabled: true,
    });

    let deadline = Instant::now() + Duration::from_millis(inputs.settle_ms);
    loop {
      if let Some(window) = resolve_window(&inputs.resolve)? {
        auv_tracing::emit_event!(LaunchEvent::WindowAppeared);
        activate(&window)?;
        auv_tracing::emit_event!(LaunchEvent::WindowActivated);
        result.window_found = true;
        result.window_title = window.title;
        return Ok(result);
      }
      if Instant::now() >= deadline {
        break;
      }
      std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    }

    auv_tracing::emit_event!(LaunchEvent::WaitTimedOut {
      timeout_ms: inputs.settle_ms,
    });
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
    auv_tracing::emit_event!(LaunchEvent::UnsupportedPlatform);
    Ok(LaunchResult::new(inputs))
  }
}
