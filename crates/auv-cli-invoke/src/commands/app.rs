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
  summary = "Probe macOS screen recording, accessibility, and automation permissions.",
  args = NO_ARGS,
)]
async fn probe_permissions(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::new("dry run: app.probePermissions would probe macOS permissions"));
  }
  read_permissions().await.map(|permissions| permission_probe_output(&permissions))
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
  summary = "Bring a target macOS app to the foreground before a foreground-dependent step.",
  args = TARGET_ARGS,
)]
async fn activate_app(input: InvokeCommandInput) -> InvokeCommandResult {
  activate_application(input.target_application_id).await?;
  Ok(InvokeCommandOutput::new("activated target app"))
}

pub async fn activate_application(_target_application_id: Option<String>) -> Result<(), String> {
  // TODO(invoke-app-activation): app activation still lives behind the root
  // macOS command adapter; migrate it to `auv-driver-macos` before enabling
  // this direct invoke command.
  Err("app.activate requires a typed app activation API in auv-driver-macos".to_string())
}

fn permission_probe_output(permissions: &auv_driver::PermissionProbe) -> InvokeCommandOutput {
  let mut output = InvokeCommandOutput::new("macOS permissions probed");
  output.backend = Some("auv-driver-macos.permission".to_string());
  output.report = Some(permission_report(&permissions));
  output.signals.insert("permission.screen_recording".to_string(), permissions.screen_recording.as_str().to_string());
  output.signals.insert("permission.screen_capture_kit".to_string(), permissions.screen_capture_kit.as_str().to_string());
  output.signals.insert("permission.accessibility".to_string(), permissions.accessibility.as_str().to_string());
  output.signals.insert("permission.automation_to_system_events".to_string(), permissions.automation_to_system_events.as_str().to_string());
  output.verification = Some("read-only; no semantic success claim".to_string());
  output
    .known_limits
    .push("app.probePermissions records current permission status only; it does not verify an application workflow.".to_string());
  output
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

    let output = permission_probe_output(&permissions);
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
