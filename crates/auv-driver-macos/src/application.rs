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
    let mut launched_by_launch_services = false;
    if let Err(error) = run_activation_script(&script) {
      // NOTICE: some applications cannot be launched by an AppleEvent: on macOS
      // 26, `tell application id "com.apple.calculator" to activate` (and
      // `launch`) fails with "Application isn't running. (-600)" while
      // Calculator is not running, though `open -b` starts it. Launch through
      // LaunchServices instead; the frontmost check below waits for it.
      // A launch that hangs past the script's timeout (-1712) gets the same fallback.
      if !["(-600)", "(-1712)"].iter().any(|code| error.to_string().contains(code)) {
        return Err(error);
      }
      launch_with_launch_services(bundle_id.trim())?;
      launched_by_launch_services = true;
    }

    let _ = settle;
    #[cfg(target_os = "macos")]
    if !settle.is_zero() {
      std::thread::sleep(settle);
    }

    let requested_bundle_id = bundle_id.trim().to_string();
    // AppleScript's activate returns once the application is frontmost, so only
    // the LaunchServices fallback needs time to see it arrive.
    let verify_within = if launched_by_launch_services {
      ACTIVATION_VERIFY_TIMEOUT
    } else {
      Duration::ZERO
    };
    let verification = wait_for_foreground(&requested_bundle_id, verify_within, || {
      crate::native::window::list_windows(ListWindowsOptions::all_visible(1))
        .map(|snapshot| snapshot.frontmost_app_bundle_id)
        .map_err(|error| format!("frontmost application observation failed: {error}"))
    });
    Ok(ApplicationActivationResult {
      verification,
      requested_bundle_id,
    })
  }
}

/// How long activation keeps checking the frontmost application after a
/// LaunchServices launch.
///
/// NOTICE: `open -b` returns before a launched application reaches the
/// front, so one check right after the 150 ms `app.activate` settle reported
/// `foreground_mismatch` for an activation that succeeded. Only a foreground
/// mismatch keeps checking; a verified foreground or an unavailable
/// observation returns at once.
const ACTIVATION_VERIFY_TIMEOUT: Duration = Duration::from_secs(5);
const ACTIVATION_VERIFY_INTERVAL: Duration = Duration::from_millis(50);

/// Re-observes the frontmost application until it is the requested one or
/// `timeout` runs out, and returns the last verification.
fn wait_for_foreground(
  requested_bundle_id: &str,
  timeout: Duration,
  mut observe: impl FnMut() -> Result<String, String>,
) -> ApplicationActivationVerification {
  let deadline = std::time::Instant::now() + timeout;
  loop {
    let verification = activation_verification(requested_bundle_id, observe());
    if !matches!(verification, ApplicationActivationVerification::ForegroundMismatch { .. }) || std::time::Instant::now() >= deadline {
      return verification;
    }
    std::thread::sleep(ACTIVATION_VERIFY_INTERVAL);
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

  // Bound the AppleEvent: the default two-minute timeout stalled app.activate
  // when the application did not answer while launching.
  Ok(format!("with timeout of 10 seconds\ntell application id \"{}\" to activate\nend timeout", escape_applescript(bundle_id)))
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

/// Launches (or brings forward) an application through LaunchServices.
#[cfg(target_os = "macos")]
fn launch_with_launch_services(bundle_id: &str) -> DriverResult<()> {
  let output = Command::new("open").arg("-b").arg(bundle_id).output().map_err(|error| DriverError::Backend {
    message: format!("failed to launch open for application activation: {error}"),
  })?;
  if output.status.success() {
    return Ok(());
  }
  let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
  if stderr.contains("Unable to find application") {
    return Err(DriverError::NotFound {
      target: format!("application {bundle_id:?}"),
    });
  }
  Err(DriverError::Backend {
    message: format!(
      "open -b {bundle_id} failed: {}",
      if stderr.is_empty() {
        output.status.to_string()
      } else {
        stderr
      }
    ),
  })
}

#[cfg(not(target_os = "macos"))]
fn launch_with_launch_services(_bundle_id: &str) -> DriverResult<()> {
  Err(DriverError::unsupported("application.activate_bundle_id"))
}

#[cfg(not(target_os = "macos"))]
fn run_activation_script(_script: &str) -> DriverResult<()> {
  Err(DriverError::unsupported("application.activate_bundle_id"))
}

#[cfg(test)]
#[path = "application_test.rs"]
mod tests;
