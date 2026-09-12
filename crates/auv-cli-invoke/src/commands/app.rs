use crate::{CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, invoke_command};
use crate::{InvokeReport, InvokeReportField, InvokeReportSection};
use clap::Args;

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke app.probePermissions")]
struct ProbePermissionsArgs {}

pub fn group() -> CommandGroup {
  CommandGroup::new("app", "APP").command(probe_permissions_invoke_command()).command(activate_app_invoke_command())
}

#[invoke_command(
  id = "app.probePermissions",
  group = "app",
  description = "Probe macOS screen recording, accessibility, and automation permissions.",
  input = ProbePermissionsArgs,
)]
async fn probe_permissions(input: InvokeCommandInput, _args: ProbePermissionsArgs) -> InvokeCommandResult {
  if input.target.is_some() {
    return Err("app.probePermissions cannot use --target".to_string());
  }
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let permissions = read_permissions().await?;
  permission_probe_output(&permissions)
}

pub fn permission_probe_output(permissions: &auv_driver::PermissionProbe) -> InvokeCommandResult {
  Ok(InvokeCommandOutput::from_result(permissions)?.with_report(permission_report(permissions)))
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

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke app.activate --target com.apple.TextEdit\n  auv invoke app.activate --target javaw")]
struct ActivateAppArgs {}

#[invoke_command(
  id = "app.activate",
  target = RequiredApplication,
  group = "app",
  description = "Bring a target application to the foreground before a foreground-dependent step.",
  input = ActivateAppArgs,
)]
async fn activate_app(input: InvokeCommandInput, _args: ActivateAppArgs) -> InvokeCommandResult {
  let target = input.application_target()?.map(str::to_string);
  #[cfg(target_os = "macos")]
  {
    let result = activate_application(target).await?;
    activation_output(&result)
  }
  #[cfg(target_os = "windows")]
  {
    let result = activate_process_application(target).await?;
    process_activation_output(&result)
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    let _ = target;
    Err("app.activate is not available on this platform".to_string())
  }
}

pub fn activation_output(result: &auv_driver::ApplicationActivationResult) -> InvokeCommandResult {
  let mut fields = vec![
    InvokeReportField::new("Requested target", &result.requested_bundle_id),
    InvokeReportField::new("Result", "activation request completed"),
    InvokeReportField::new("Verification", result.verification.status()),
  ];
  if let Some(observed_bundle_id) = result.verification.observed_bundle_id() {
    fields.push(InvokeReportField::new("Observed foreground", observed_bundle_id));
  }
  if let auv_driver::ApplicationActivationVerification::Unavailable { reason } = &result.verification {
    fields.push(InvokeReportField::new("Verification detail", reason));
  }
  Ok(InvokeCommandOutput::from_result(result)?.with_report(InvokeReport::new(fields, Vec::new())))
}

pub fn process_activation_output(result: &auv_driver::ProcessActivationResult) -> InvokeCommandResult {
  let mut fields = vec![
    InvokeReportField::new("Requested target", &result.requested_process),
    InvokeReportField::new("Result", "activation request completed"),
    InvokeReportField::new("Verification", result.verification.status()),
  ];
  if let Some(observed_process) = result.verification.observed_process() {
    fields.push(InvokeReportField::new("Observed foreground", observed_process));
  }
  if let auv_driver::ProcessActivationVerification::Unavailable { reason } = &result.verification {
    fields.push(InvokeReportField::new("Verification detail", reason));
  }
  Ok(InvokeCommandOutput::from_result(result)?.with_report(InvokeReport::new(fields, Vec::new())))
}

pub fn require_activation_target(target: Option<String>) -> Result<String, String> {
  target.as_deref().filter(|value| !value.trim().is_empty()).map(str::to_string).ok_or_else(|| "app.activate requires --target".to_string())
}

#[cfg(target_os = "macos")]
pub async fn activate_application(target_application_id: Option<String>) -> Result<auv_driver::ApplicationActivationResult, String> {
  let target_application_id = require_activation_target(target_application_id)?;
  use auv_driver_macos::ApplicationControl;

  let session = auv_driver::open_local().map_err(|error| error.to_string())?;
  session.activate_bundle_id(&target_application_id, std::time::Duration::from_millis(150)).map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
pub async fn activate_process_application(process_name: Option<String>) -> Result<auv_driver::ProcessActivationResult, String> {
  let process_name = require_activation_target(process_name)?;
  use auv_driver_windows::ApplicationControl;

  let session = auv_driver::open_local().map_err(|error| error.to_string())?;
  session.activate_process_name(&process_name, std::time::Duration::from_millis(150)).map_err(|error| error.to_string())
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
