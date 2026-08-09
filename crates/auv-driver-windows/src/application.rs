//! Process-scoped application control for Windows workflows that identify a
//! target by process name rather than a `CGWindowID`/bundle id.
//!
//! Windows has no single "activate this app" API: bringing an application
//! forward means resolving one of its windows and foregrounding that window.
//! This reuses the existing window resolver/activator (`window.rs`) with a
//! `main_visible` selector so the same foreground/titled/largest-window
//! preference used by `WindowSelector::main_visible` picks the app's most
//! relevant window.

use std::time::Duration;

use auv_driver_common::error::DriverResult;
use auv_driver_common::selector::{App, WindowSelector};
use auv_driver_common::{ProcessActivationResult, ProcessActivationVerification};

use crate::driver::WindowsDriverSession;
use crate::error::invalid_input;
use crate::window::{activate_window, list_windows, resolve_window};

/// Typed application control that is independent of a pre-resolved `Window`.
///
/// Use this for foreground workflows that target an application by process
/// name. Screenshot, coordinate, and window-targeted input paths should
/// continue to resolve a concrete window through `WindowApi`.
pub trait ApplicationControl {
  fn activate_process_name(&self, process_name: &str, settle: Duration) -> DriverResult<ProcessActivationResult>;
}

impl ApplicationControl for WindowsDriverSession {
  fn activate_process_name(&self, process_name: &str, settle: Duration) -> DriverResult<ProcessActivationResult> {
    let _ = self;
    let process_name = process_name.trim();
    if process_name.is_empty() {
      return Err(invalid_input("application activation requires a non-empty process name"));
    }

    let selector = WindowSelector {
      app: Some(App::name(process_name)),
      main_visible: true,
      ..WindowSelector::default()
    };
    let window = resolve_window(&selector)?;
    activate_window(&window)?;

    if !settle.is_zero() {
      std::thread::sleep(settle);
    }

    let requested_process = process_name.to_string();
    let observation = list_windows()
      .map(|windows| windows.into_iter().find(|window| window.is_main).and_then(|window| window.app_name))
      .map_err(|error| format!("foreground window observation failed: {error}"));
    Ok(ProcessActivationResult {
      verification: activation_verification(&requested_process, observation),
      requested_process,
    })
  }
}

fn activation_verification(requested_process: &str, observation: Result<Option<String>, String>) -> ProcessActivationVerification {
  match observation {
    Ok(Some(observed_process)) if observed_process.eq_ignore_ascii_case(requested_process) => {
      ProcessActivationVerification::VerifiedForeground { observed_process }
    }
    Ok(Some(observed_process)) => ProcessActivationVerification::ForegroundMismatch { observed_process },
    Ok(None) => ProcessActivationVerification::Unavailable {
      reason: "foreground window observation did not identify an owning process name".to_string(),
    },
    Err(reason) => ProcessActivationVerification::Unavailable { reason },
  }
}

#[cfg(test)]
#[path = "application_test.rs"]
mod tests;
