//! Application-scoped control for macOS workflows that do not require a
//! WindowServer capture target.

use std::time::Duration;

#[cfg(target_os = "macos")]
use std::process::Command;

use auv_driver_common::error::{DriverError, DriverResult};

use crate::MacosDriverSession;

/// Typed application control that is independent of `CGWindowID` discovery.
///
/// Use this for foreground AX/keyboard workflows that target an application by
/// bundle id. Screenshot, coordinate, and window-targeted input paths should
/// continue to resolve a concrete window through `WindowApi`.
pub trait ApplicationControl {
  fn activate_bundle_id(&self, bundle_id: &str, settle: Duration) -> DriverResult<()>;
}

impl ApplicationControl for MacosDriverSession {
  fn activate_bundle_id(&self, bundle_id: &str, settle: Duration) -> DriverResult<()> {
    let _ = self;
    let script = activation_script(bundle_id)?;
    run_activation_script(&script)?;

    let _ = settle;
    #[cfg(target_os = "macos")]
    if !settle.is_zero() {
      std::thread::sleep(settle);
    }

    Ok(())
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
mod tests {
  use super::*;

  #[test]
  fn activation_script_is_scoped_to_exact_bundle_id() {
    assert_eq!(activation_script("com.apple.TextEdit").expect("script"), "tell application id \"com.apple.TextEdit\" to activate");
  }

  #[test]
  fn activation_script_rejects_blank_bundle_id() {
    let error = activation_script("   ").expect_err("blank bundle id should fail");
    assert!(error.to_string().contains("non-empty bundle id"));
  }

  #[test]
  fn activation_script_escapes_applescript_string_content() {
    assert_eq!(
      activation_script("com.example.\\\"quoted").expect("script"),
      "tell application id \"com.example.\\\\\\\"quoted\" to activate"
    );
  }
}
