//! Application-scoped control for macOS workflows that do not require a
//! WindowServer capture target.

use std::time::Duration;

#[cfg(target_os = "macos")]
use std::process::Command;

use auv_driver_common::application::{
  ApplicationActivationMethod, ApplicationActivationResult, ApplicationActivationVerification, ApplicationIdKind,
};
use auv_driver_common::error::{DriverError, DriverResult};

use crate::MacosDriverSession;
use crate::native::window::ListWindowsOptions;

/// Typed application control that is independent of `CGWindowID` discovery.
///
/// Use this for foreground AX/keyboard workflows that target an application by
/// application id, which on macOS is the bundle id that `window.list` reports.
/// Screenshot, coordinate, and window-targeted input paths should continue to
/// resolve a concrete window through `WindowApi`.
#[derive(Clone, Copy, Debug)]
pub struct ApplicationApi<'a> {
  session: &'a MacosDriverSession,
}

impl MacosDriverSession {
  pub fn applications(&self) -> ApplicationApi<'_> {
    ApplicationApi { session: self }
  }
}

impl ApplicationApi<'_> {
  /// Activates the application with this bundle id, waits `settle`, and
  /// reports whether WindowServer then names it frontmost.
  pub fn activate(&self, application_id: &str, settle: Duration) -> DriverResult<ApplicationActivationResult> {
    let _ = self.session;
    let script = activation_script(application_id)?;
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
      launch_with_launch_services(application_id.trim())?;
      launched_by_launch_services = true;
    }

    let _ = settle;
    #[cfg(target_os = "macos")]
    if !settle.is_zero() {
      std::thread::sleep(settle);
    }

    let requested_application_id = application_id.trim().to_string();
    let verify_within = if launched_by_launch_services {
      LAUNCH_VERIFY_TIMEOUT
    } else {
      ACTIVATE_VERIFY_TIMEOUT
    };
    let verification = wait_for_foreground(&requested_application_id, verify_within, || {
      crate::native::window::list_windows(ListWindowsOptions::all_visible(1))
        .map(|snapshot| snapshot.frontmost_app_bundle_id)
        .map_err(|error| format!("frontmost application observation failed: {error}"))
    });
    Ok(ApplicationActivationResult {
      verification,
      requested_application_id,
      application_id_kind: ApplicationIdKind::BundleId,
      method: ApplicationActivationMethod::AppleScriptActivate,
      // AppleScript activates the application; it does not report which of
      // its windows came forward.
      activated_window: None,
      // TODO(macos-activation-window-observation): populate the #324 window
      // observation by matching the window-list snapshot's owner bundle ids
      // once that matching is validated on macOS. This phase establishes the
      // shared contract and the Windows producer; `None` means this driver
      // version did not observe, never that the app owns no window.
      window_observation: None,
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
const LAUNCH_VERIFY_TIMEOUT: Duration = Duration::from_secs(5);

/// How long activation keeps checking the frontmost application after
/// AppleScript `activate` succeeded for a running application.
///
/// NOTICE: AppleScript `activate` can return before WindowServer reports the
/// application frontmost. On macOS 26, activating TextEdit while Calculator
/// was frontmost observed Calculator after the 150 ms settle, and TextEdit
/// came forward a moment later. A running application comes forward much
/// sooner than a launch, so this budget is shorter.
const ACTIVATE_VERIFY_TIMEOUT: Duration = Duration::from_secs(2);
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
      ApplicationActivationVerification::VerifiedForeground {
        observed_application_id: observed_bundle_id,
      }
    }
    Ok(observed_bundle_id) if observed_bundle_id.trim().is_empty() => ApplicationActivationVerification::Unavailable {
      reason: "WindowServer observation did not identify a frontmost application bundle id".to_string(),
    },
    Ok(observed_bundle_id) => ApplicationActivationVerification::ForegroundMismatch {
      observed_application_id: observed_bundle_id,
    },
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
  Err(DriverError::unsupported("application.activate"))
}

#[cfg(not(target_os = "macos"))]
fn run_activation_script(_script: &str) -> DriverResult<()> {
  Err(DriverError::unsupported("application.activate"))
}

#[cfg(test)]
#[path = "application_test.rs"]
mod tests;
