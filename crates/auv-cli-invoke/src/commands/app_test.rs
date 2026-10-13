use super::*;
use auv_driver::{PermissionProbe, PermissionStatus};

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

#[test]
fn permission_probe_rejects_target_before_platform_access() {
  let input = crate::InvokeCommandInput {
    command_id: "app.probePermissions".to_string(),
    target: Some(crate::ExecutionTarget::Application {
      id: "com.example.App".to_string(),
    }),
    inputs: Default::default(),
    typed_args: None,
    dry_run: false,
    cancellation: crate::InvokeCancellation::new(),
  };
  let error = futures_executor::block_on(probe_permissions_invoke_command().invoke(input)).expect_err("target must fail before probing");
  assert_eq!(error.code, crate::FailureCode::InvalidTarget);
}

// ROOT CAUSE:
//
// If `app.probePermissions` or `app.activate` ran where its cell is `Unsupported` (Linux and
// Windows), the refusal reached the caller as `command_failed`, because the handler returned a
// plain `String` and `InvokeFailure::from(String)` assigns that code.
//
// Before the fix, a caller could not tell a platform without a path from a probe or an activation
// that failed. The fix keeps every refusal for an `Unsupported` cell typed as `unsupported`.
// Neither refusal touches the platform, so the commands run for real here.
#[cfg(not(target_os = "macos"))]
#[test]
fn app_commands_refuse_as_unsupported_off_macos() {
  let input = |command_id: &str, target: Option<&str>| crate::InvokeCommandInput {
    command_id: command_id.to_string(),
    target: target.map(|id| crate::ExecutionTarget::Application { id: id.to_string() }),
    inputs: Default::default(),
    typed_args: None,
    dry_run: false,
    cancellation: crate::InvokeCancellation::new(),
  };
  let commands = [
    (probe_permissions_invoke_command(), input("app.probePermissions", None)),
    (activate_app_invoke_command(), input("app.activate", Some("com.example.App"))),
  ];

  for (command, input) in commands {
    let id = input.command_id.clone();
    let error = futures_executor::block_on(command.invoke(input)).expect_err("the command has no path on this platform");
    assert_eq!(error.code, crate::FailureCode::Unsupported, "{id}: {}", error.message);
  }
}
