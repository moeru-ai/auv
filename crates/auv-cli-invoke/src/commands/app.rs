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
  permission_probe_output(&permissions)
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
  // TODO(invoke-app-activation): app activation still lives behind the root
  // macOS command adapter; migrate it to `auv-driver-macos` before enabling
  // this direct invoke command.
  Err("app.activate requires a typed app activation API in auv-driver-macos".to_string())
}

fn permission_probe_output(permissions: &auv_driver::PermissionProbe) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(permissions)?;
  output.report = Some(permission_report(&permissions));
  Ok(output)
}

fn permission_report(permissions: &auv_driver::PermissionProbe) -> InvokeReport {
  InvokeReport::new(
    vec![report_field("Result", "permissions probed")],
    vec![InvokeReportSection {
      title: "Permissions".to_string(),
      fields: vec![
        report_field("Screen Recording", permissions.screen_recording.as_str()),
        report_field("ScreenCaptureKit", permissions.screen_capture_kit.as_str()),
        report_field("Accessibility", permissions.accessibility.as_str()),
        report_field("Automation to System Events", permissions.automation_to_system_events.as_str()),
      ],
    }],
  )
}

fn report_field(label: &str, value: impl Into<String>) -> InvokeReportField {
  InvokeReportField::new(label, value)
}

#[cfg(test)]
mod tests {
  use auv_driver::{PermissionProbe, PermissionStatus};

  use super::*;

  #[test]
  fn permission_report_groups_readable_statuses() {
    let permissions = PermissionProbe {
      screen_recording: PermissionStatus::Granted,
      screen_capture_kit: PermissionStatus::Missing,
      accessibility: PermissionStatus::Unknown,
      automation_to_system_events: PermissionStatus::Granted,
    };

    let output = permission_probe_output(&permissions).expect("permission result should serialize");
    assert!(
      output.report.is_some(),
      "app.probePermissions live path calls this helper after OS probing, so this stable helper test verifies report population without requiring live permission state"
    );
    let report = output.report.as_ref().expect("report should be set");
    let section = &report.sections[0];

    assert_eq!(report.fields[0].value, "permissions probed");
    assert_eq!(section.title, "Permissions");
    assert_eq!(field_value(section, "Screen Recording"), "granted");
    assert_eq!(field_value(section, "ScreenCaptureKit"), "missing");
    assert_eq!(field_value(section, "Accessibility"), "unknown");
    assert_eq!(field_value(section, "Automation to System Events"), "granted");
    assert_eq!(output.result(), Some(&serde_json::to_value(&permissions).expect("fixture should serialize")));
  }

  #[test]
  fn typed_app_activation_api_is_callable_without_cli_context() {
    let error = futures_executor::block_on(activate_application(None)).expect_err("deferred activation should fail");

    assert!(error.contains("typed app activation API"));
  }

  fn field_value<'a>(section: &'a InvokeReportSection, label: &str) -> &'a str {
    section.fields.iter().find(|field| field.label == label).map(|field| field.value.as_str()).expect("field should exist")
  }
}
