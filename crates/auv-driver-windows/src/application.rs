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
  ActivationWindowObservation, ApplicationActivationMethod, ApplicationActivationResult, ApplicationActivationVerification,
  ApplicationIdKind,
};
use auv_driver_common::error::DriverResult;
use auv_driver_common::selector::{App, TextMatcher, WindowSelector};

use crate::driver::WindowsDriverSession;
use crate::error::invalid_input;
use crate::window::{activate_window, list_windows, matches_executable, resolve_window};

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
    // One enumeration feeds both the foreground verification and the window
    // observation, so the two facts describe the same instant.
    let listed = list_windows();
    let foreground = listed
      .as_ref()
      .map(|windows| windows.iter().find(|window| window.is_main).and_then(|window| window.app_name.clone()))
      .map_err(|error| format!("foreground window observation failed: {error}"));
    let window_observation = match listed.as_ref() {
      Ok(windows) => activation_window_observation(process_name, windows),
      Err(error) => ActivationWindowObservation::Unknown {
        reason: format!("window observation failed: {error}"),
      },
    };
    Ok(ApplicationActivationResult {
      verification: activation_verification(&requested_application_id, foreground),
      requested_application_id,
      application_id_kind: ApplicationIdKind::ProcessName,
      method: ApplicationActivationMethod::ForegroundWindow,
      activated_window: Some(window.reference),
      window_observation: Some(window_observation),
    })
  }
}

/// Derives the post-activation window observation for one process name from
/// the enumeration activation already paid for. Presence is a fact, not a
/// usability promise: a minimized window is still reported `Present`, as the
/// window the driver would target next.
///
/// Absence demands proof: the enumerator can fail to read a window's process
/// name (the `OpenProcess` or image-name query) while the enumeration itself
/// succeeds, leaving listed windows with no owner. When nothing matched and
/// any window's ownership is unreadable, the observation degrades to
/// `Unknown` — an unattributable window may be the target application's.
fn activation_window_observation(requested_process: &str, windows: &[auv_driver_common::window::Window]) -> ActivationWindowObservation {
  let mut matching: Vec<&auv_driver_common::window::Window> = windows
    .iter()
    .filter(|window| {
      window.app_name.as_deref().is_some_and(|app_name| matches_executable(app_name, &TextMatcher::Exact(requested_process.to_string())))
    })
    .collect();
  // The resolver's own main-window preference, shared through one rank.
  matching.sort_by_key(|window| std::cmp::Reverse(crate::window::main_visible_rank(window)));
  match matching.first() {
    Some(window) => ActivationWindowObservation::Present {
      reference: window.reference.clone(),
    },
    None => {
      let unattributed = windows.iter().filter(|window| window.app_name.is_none()).count();
      if unattributed > 0 {
        ActivationWindowObservation::Unknown {
          reason: format!("window enumeration left {unattributed} window(s) without a process name, so absence is not confirmed"),
        }
      } else {
        ActivationWindowObservation::Absent
      }
    }
  }
}

fn activation_verification(requested_process: &str, observation: Result<Option<String>, String>) -> ApplicationActivationVerification {
  match observation {
    // The same executable rule as window selection, so `notepad` verifies
    // against `Notepad.exe` instead of reporting a mismatch.
    Ok(Some(observed_process)) if matches_executable(&observed_process, &TextMatcher::Exact(requested_process.to_string())) => {
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
