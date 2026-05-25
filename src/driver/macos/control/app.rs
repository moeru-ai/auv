// File: src/driver/macos/control/app.rs
use std::thread;
use std::time::Duration;

use super::super::*;

pub(crate) fn activate_app(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call)
    .filter(|value| !value.is_empty())
    .ok_or_else(|| "missing target application id for activate_app".to_string())?;
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(250);
  activate_target_app(&app)?;
  if settle_ms > 0 {
    thread::sleep(Duration::from_millis(settle_ms));
  }

  let artifact = build_text_artifact(
    "activate-app",
    "txt",
    &format!("activate-app-{}", sanitize_file_component(&app)),
    render_activate_app_report(&app, settle_ms),
    "Activated the target app through AppleScript before a foreground-dependent action.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Activated {} and waited {} ms for the foreground app to settle.",
      app, settle_ms
    ),
    backend: Some("macos.osascript.activate-app".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes: vec![format!("app={app}"), format!("settleMs={settle_ms}")],
    artifacts: vec![artifact],
  })
}
