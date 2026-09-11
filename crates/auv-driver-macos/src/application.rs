//! Application-scoped control for macOS workflows that do not require a
//! WindowServer capture target.

use std::time::Duration;

#[cfg(target_os = "macos")]
use std::process::Command;

use auv_driver_common::application::{ApplicationActivationResult, ApplicationActivationVerification};
use auv_driver_common::error::{DriverError, DriverResult};

use crate::MacosDriverSession;
use crate::native::window::ListWindowsOptions;

/// Typed application control that is independent of `CGWindowID` discovery.
///
/// Use this for foreground AX/keyboard workflows that target an application by
/// bundle id. Screenshot, coordinate, and window-targeted input paths should
/// continue to resolve a concrete window through `WindowApi`.
pub trait ApplicationControl {
  fn activate_bundle_id(&self, bundle_id: &str, settle: Duration) -> DriverResult<ApplicationActivationResult>;
}

impl ApplicationControl for MacosDriverSession {
  fn activate_bundle_id(&self, bundle_id: &str, settle: Duration) -> DriverResult<ApplicationActivationResult> {
    let _ = self;
    let script = activation_script(bundle_id)?;
    run_activation_script(&script)?;

    let _ = settle;
    #[cfg(target_os = "macos")]
    if !settle.is_zero() {
      std::thread::sleep(settle);
    }

    let requested_bundle_id = bundle_id.trim().to_string();
    let observation = crate::native::window::list_windows(ListWindowsOptions::all_visible(1))
      .map(|snapshot| snapshot.frontmost_app_bundle_id)
      .map_err(|error| format!("frontmost application observation failed: {error}"));
    Ok(ApplicationActivationResult {
      verification: activation_verification(&requested_bundle_id, observation),
      requested_bundle_id,
    })
  }
}

fn activation_verification(requested_bundle_id: &str, observation: Result<String, String>) -> ApplicationActivationVerification {
  match observation {
    Ok(observed_bundle_id) if observed_bundle_id.eq_ignore_ascii_case(requested_bundle_id) => {
      ApplicationActivationVerification::VerifiedForeground { observed_bundle_id }
    }
    Ok(observed_bundle_id) if observed_bundle_id.trim().is_empty() => ApplicationActivationVerification::Unavailable {
      reason: "WindowServer observation did not identify a frontmost application bundle id".to_string(),
    },
    Ok(observed_bundle_id) => ApplicationActivationVerification::ForegroundMismatch { observed_bundle_id },
    Err(reason) => ApplicationActivationVerification::Unavailable { reason },
  }
}

fn activation_script(bundle_id: &str) -> DriverResult<String> {
  let bundle_id = bundle_id.trim();
  if bundle_id.is_empty() {
    return Err(DriverError::InvalidInput {
      message: "application activation requires a non-empty bundle id".to_string(),
    });
  }

  Ok(format!("tell application id \"{}\" to activate", escape_applescript(bundle_id)))
}

fn escape_applescript(value: &str) -> String {
  value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "macos")]
fn run_activation_script(script: &str) -> DriverResult<()> {
  let output = Command::new("osascript").arg("-e").arg(script).output().map_err(|error| DriverError::Backend {
    message: format!("failed to launch osascript for application activation: {error}"),
  })?;

  if output.status.success() {
    return Ok(());
  }

  let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
  Err(DriverError::Backend {
    message: if stderr.is_empty() {
      format!("osascript application activation exited with {}", output.status)
    } else {
      format!("osascript application activation failed: {stderr}")
    },
  })
}

#[cfg(not(target_os = "macos"))]
fn run_activation_script(_script: &str) -> DriverResult<()> {
  Err(DriverError::unsupported("application.activate_bundle_id"))
}

#[cfg(test)]
#[path = "application_test.rs"]
mod tests;
