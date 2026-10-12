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
// If `app.probePermissions` ran where its cell is `Unsupported` (Linux and Windows), the refusal
// reached the caller as `command_failed`, because the handler returned a plain `String` and
// `InvokeFailure::from(String)` assigns that code.
//
// Before the fix, a caller could not tell a platform without a path from a probe that failed. The
// fix keeps every refusal for an `Unsupported` cell typed as `unsupported`. The refusal does not
// touch the platform, so the command runs for real here. `app.activate` has a Windows path through
// the activation contract; its Linux refusal is covered by
// `linux_activation_reaches_the_invoke_caller_as_unsupported`.
#[cfg(not(target_os = "macos"))]
#[test]
fn permission_probe_refuses_as_unsupported_off_macos() {
  let input = crate::InvokeCommandInput {
    command_id: "app.probePermissions".to_string(),
    target: None,
    inputs: Default::default(),
    typed_args: None,
    dry_run: false,
    cancellation: crate::InvokeCancellation::new(),
  };
  let error =
    futures_executor::block_on(probe_permissions_invoke_command().invoke(input)).expect_err("the command has no path on this platform");
  assert_eq!(error.code, crate::FailureCode::Unsupported, "{}", error.message);
}

fn report_value<'a>(output: &'a InvokeCommandOutput, label: &str) -> Option<&'a str> {
  output.report.as_ref()?.fields.iter().find(|field| field.label == label).map(|field| field.value.as_str())
}

#[test]
fn activation_report_names_id_kind_method_window_and_observed_foreground() {
  let result = auv_driver::ApplicationActivationResult {
    requested_application_id: "notepad.exe".to_string(),
    application_id_kind: auv_driver::ApplicationIdKind::ProcessName,
    method: auv_driver::ApplicationActivationMethod::ForegroundWindow,
    activated_window: Some(auv_driver::WindowRef {
      id: "0x1234".to_string(),
    }),
    verification: auv_driver::ApplicationActivationVerification::ForegroundMismatch {
      observed_application_id: "explorer.exe".to_string(),
    },
  };

  let output = activation_output(&result).expect("activation output");
  assert_eq!(report_value(&output, "Requested target"), Some("notepad.exe"));
  assert_eq!(report_value(&output, "Application id kind"), Some("process_name"));
  assert_eq!(report_value(&output, "Method"), Some("foreground_window"));
  assert_eq!(report_value(&output, "Activated window"), Some("window:0x1234"));
  assert_eq!(report_value(&output, "Verification"), Some("activation_only_foreground_mismatch"));
  assert_eq!(report_value(&output, "Observed foreground"), Some("explorer.exe"));
  assert_eq!(output.result(), Some(&serde_json::to_value(&result).expect("fixture should serialize")));
}

#[test]
fn activation_report_omits_window_when_the_platform_activated_the_application() {
  let output = activation_output(&auv_driver::ApplicationActivationResult {
    requested_application_id: "com.apple.TextEdit".to_string(),
    application_id_kind: auv_driver::ApplicationIdKind::BundleId,
    method: auv_driver::ApplicationActivationMethod::AppleScriptActivate,
    activated_window: None,
    verification: auv_driver::ApplicationActivationVerification::Unavailable {
      reason: "observation denied".to_string(),
    },
  })
  .expect("activation output");
  assert_eq!(report_value(&output, "Activated window"), None);
  assert_eq!(report_value(&output, "Observed foreground"), None);
  assert_eq!(report_value(&output, "Verification detail"), Some("observation denied"));
}

#[test]
fn linux_activation_reaches_the_invoke_caller_as_unsupported() {
  // The Linux facade and a Linux Runner both produce this driver error; the
  // in-process handler converts it with `?` and must keep the category.
  for kind in [
    auv_driver::ApplicationIdKind::DesktopAppId,
    auv_driver::ApplicationIdKind::WmClass,
  ] {
    let failure = crate::InvokeFailure::from(auv_driver::application::unsupported_activation(kind));
    assert_eq!(failure.code, crate::FailureCode::Unsupported);
    assert!(failure.message.contains("Linux"), "{}", failure.message);
  }
}
