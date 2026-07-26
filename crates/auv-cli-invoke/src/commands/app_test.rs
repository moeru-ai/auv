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

  let output =
    InvokeCommandOutput::from_result(&permissions).expect("permission result should serialize").with_report(permission_report(&permissions));
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

fn field_value<'a>(section: &'a InvokeReportSection, label: &str) -> &'a str {
  section.fields.iter().find(|field| field.label == label).map(|field| field.value.as_str()).expect("field should exist")
}
