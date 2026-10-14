use crate::{CommandGroup, FailureCode, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeFailure, invoke_command};
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
  support = { macos: Supported, linux: Unsupported, windows: Unsupported("the result names macOS privacy permissions; the Windows driver's elevation, UIAccess and session probe has no field in it") },
  group = "app",
  description = "Probe macOS screen recording, accessibility, and automation permissions.",
  input = ProbePermissionsArgs,
)]
async fn probe_permissions(input: InvokeCommandInput, _args: ProbePermissionsArgs) -> crate::InvokeExecutionResult {
  if input.target.is_some() {
    return Err("app.probePermissions cannot use --target".to_string().into());
  }
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let permissions = read_permissions().await?;
  Ok(permission_probe_output(&permissions)?)
}

pub fn permission_probe_output(permissions: &auv_driver::PermissionProbe) -> InvokeCommandResult {
  Ok(InvokeCommandOutput::from_result(permissions)?.with_report(permission_report(permissions)))
}

pub async fn read_permissions() -> Result<auv_driver::PermissionProbe, InvokeFailure> {
  #[cfg(target_os = "macos")]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
    Ok(session.permission().probe().map_err(|error| error.to_string())?)
  }
  #[cfg(not(target_os = "macos"))]
  {
    Err(InvokeFailure::new(FailureCode::Unsupported, "app.probePermissions is only available on macOS"))
  }
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke app.activate --target com.apple.TextEdit")]
struct ActivateAppArgs {}

#[invoke_command(
  id = "app.activate",
  support = { macos: Supported, linux: Partial("X11 activates through EWMH; Wayland gives clients no way to activate another application"), windows: Supported },
  target = RequiredApplication,
  group = "app",
  description = "Bring a target app to the foreground before a foreground-dependent step. The target is an application id: a bundle id on macOS, an executable name on Windows (the APP column of window.list, `.exe` optional), a WM_CLASS or desktop id on X11.",
  input = ActivateAppArgs,
)]
async fn activate_app(input: InvokeCommandInput, _args: ActivateAppArgs) -> crate::InvokeExecutionResult {
  let result = activate_application(input.application_target()?.map(str::to_string)).await?;
  Ok(activation_output(&result)?)
}

/// The settle delay `app.activate` waits before checking the foreground, on
/// both the in-process and the Runner path.
pub const ACTIVATION_SETTLE: std::time::Duration = std::time::Duration::from_millis(150);

pub fn activation_output(result: &auv_driver::ApplicationActivationResult) -> InvokeCommandResult {
  let mut fields = vec![
    InvokeReportField::new("Requested target", &result.requested_application_id),
    InvokeReportField::new("Application id kind", result.application_id_kind.as_str()),
    InvokeReportField::new("Result", "activation request completed"),
    InvokeReportField::new("Method", result.method.as_str()),
  ];
  if let Some(window) = &result.activated_window {
    fields.push(InvokeReportField::new("Activated window", format!("window:{}", window.id)));
  }
  fields.push(InvokeReportField::new("Verification", result.verification.status()));
  if let Some(observed_application_id) = result.verification.observed_application_id() {
    fields.push(InvokeReportField::new("Observed foreground", observed_application_id));
  }
  if let auv_driver::ApplicationActivationVerification::Unavailable { reason } = &result.verification {
    fields.push(InvokeReportField::new("Verification detail", reason));
  }
  Ok(InvokeCommandOutput::from_result(result)?.with_report(InvokeReport::new(fields, Vec::new())))
}

/// Activates the target in process through the local driver facade, keeping
/// driver failures typed: Linux sessions fail as `unsupported`.
pub async fn activate_application(
  target_application_id: Option<String>,
) -> Result<auv_driver::ApplicationActivationResult, crate::InvokeFailure> {
  let target_application_id = target_application_id
    .as_deref()
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| crate::InvokeFailure::new(crate::FailureCode::InvalidTarget, "app.activate requires --target"))?;
  let session = auv::local::open()?;
  Ok(session.applications().activate(target_application_id, ACTIVATION_SETTLE)?)
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
