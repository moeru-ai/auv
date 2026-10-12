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

use auv_driver_common::application::{
  ApplicationActivationMethod, ApplicationActivationResult, ApplicationActivationVerification, ApplicationIdKind,
};
use auv_driver_common::error::DriverResult;
use auv_driver_common::selector::{App, WindowSelector};

use crate::driver::WindowsDriverSession;
use crate::error::invalid_input;
use crate::window::{activate_window, list_windows, resolve_window};

/// Typed application control that is independent of a pre-resolved `Window`.
///
/// Use this for foreground workflows that target an application by
/// application id, which on Windows is the process image name that
/// `window.list` reports in `app_name`. Screenshot, coordinate, and
/// window-targeted input paths should continue to resolve a concrete window
/// through `WindowApi`.
#[derive(Clone, Copy, Debug)]
pub struct ApplicationApi<'a> {
  _session: &'a WindowsDriverSession,
}

impl WindowsDriverSession {
  pub fn applications(&self) -> ApplicationApi<'_> {
    ApplicationApi { _session: self }
  }
}

impl ApplicationApi<'_> {
  /// Foregrounds the main visible window of the process with this image name,
  /// waits `settle`, and reports which process then owns the foreground.
  pub fn activate(&self, application_id: &str, settle: Duration) -> DriverResult<ApplicationActivationResult> {
    let process_name = application_id.trim();
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

    let requested_application_id = process_name.to_string();
    let observation = list_windows()
      .map(|windows| windows.into_iter().find(|window| window.is_main).and_then(|window| window.app_name))
      .map_err(|error| format!("foreground window observation failed: {error}"));
    Ok(ApplicationActivationResult {
      verification: activation_verification(&requested_application_id, observation),
      requested_application_id,
      application_id_kind: ApplicationIdKind::ProcessName,
      method: ApplicationActivationMethod::ForegroundWindow,
      activated_window: Some(window.reference),
    })
  }
}

fn activation_verification(requested_process: &str, observation: Result<Option<String>, String>) -> ApplicationActivationVerification {
  match observation {
    Ok(Some(observed_process)) if observed_process.eq_ignore_ascii_case(requested_process) => {
      ApplicationActivationVerification::VerifiedForeground {
        observed_application_id: observed_process,
      }
    }
    Ok(Some(observed_process)) => ApplicationActivationVerification::ForegroundMismatch {
      observed_application_id: observed_process,
    },
    Ok(None) => ApplicationActivationVerification::Unavailable {
      reason: "foreground window observation did not identify an owning process name".to_string(),
    },
    Err(reason) => ApplicationActivationVerification::Unavailable { reason },
  }
}

#[cfg(test)]
#[path = "application_test.rs"]
mod tests;
