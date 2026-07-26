use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
  arg::{NO_ARGS, TARGET_ARGS},
  invoke_command,
};
use crate::{InvokeReport, InvokeReportField, InvokeReportSection};

pub fn group() -> CommandGroup {
  CommandGroup::new("app", "APP").command(probe_permissions_invoke_command()).command(activate_app_invoke_command())
}

#[invoke_command(
  id = "app.probePermissions",
  group = "app",
  description = "Probe macOS screen recording, accessibility, and automation permissions.",
  args = NO_ARGS,
)]
async fn probe_permissions(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let permissions = read_permissions().await?;
  Ok(InvokeCommandOutput::from_result(&permissions)?.with_report(permission_report(&permissions)))
}

pub async fn read_permissions() -> Result<auv_driver::PermissionProbe, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    session.permission().probe().map_err(|error| error.to_string())
  }
  #[cfg(not(target_os = "macos"))]
  {
    Err("app.probePermissions is only available on macOS".to_string())
  }
}

#[invoke_command(
  id = "app.activate",
  group = "app",
  description = "Bring a target macOS app to the foreground before a foreground-dependent step.",
  args = TARGET_ARGS,
)]
async fn activate_app(input: InvokeCommandInput) -> InvokeCommandResult {
  activate_application(input.target_application_id).await?;
  Ok(InvokeCommandOutput::completed())
}

pub async fn activate_application(_target_application_id: Option<String>) -> Result<(), String> {
  let target_application_id = _target_application_id
    .as_deref()
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| "app.activate requires --target".to_string())?;
  #[cfg(target_os = "macos")]
  {
    use auv_driver_macos::ApplicationControl;

    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    session.activate_bundle_id(target_application_id, std::time::Duration::from_millis(150)).map_err(|error| error.to_string())
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = target_application_id;
    Err("app.activate is only available on macOS".to_string())
  }
}

fn permission_report(permissions: &auv_driver::PermissionProbe) -> InvokeReport {
  InvokeReport::new(
    vec![InvokeReportField::new("Result", "permissions probed")],
    vec![InvokeReportSection {
      title: "Permissions".to_string(),
      fields: vec![
        InvokeReportField::new("Screen Recording", permissions.screen_recording.as_str()),
        InvokeReportField::new("ScreenCaptureKit", permissions.screen_capture_kit.as_str()),
        InvokeReportField::new("Accessibility", permissions.accessibility.as_str()),
        InvokeReportField::new("Automation to System Events", permissions.automation_to_system_events.as_str()),
      ],
    }],
  )
}

#[cfg(test)]
#[path = "app_test.rs"]
mod tests;
