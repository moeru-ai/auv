use super::*;
use crate::{InvokeOutputOptions, InvokeResult};
use auv_driver::{InputActionResult, InputDeliveryPath};
use auv_tracing::{Context, MemoryTracingStore, RunId, TraceRecord, configure, dispatcher};
use std::sync::Arc;

#[test]
fn window_click_options_parse_policy_and_repeated_clicks() {
  let options = click_options(Some(auv_driver::InputPolicy::ForegroundPreferred), Some(3), Some(60));
  assert_eq!(options.policy, auv_driver::InputPolicy::ForegroundPreferred);
  assert_eq!(
    options.click,
    auv_driver::Click::Repeated {
      count: 3,
      interval: std::time::Duration::from_millis(60),
    }
  );
}

#[tokio::test]
async fn input_action_publishes_through_typed_driver_contract() {
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().expect("memory dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let expected = InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse);
  let future = root.in_scope(|| async { emit_input_action_result(&expected) });
  root.instrument(future).await;
  dispatch.flush().await.expect("flush input action telemetry");
  let records = store.records();

  let metadata = records
    .iter()
    .find_map(|record| match record {
      TraceRecord::Artifact { metadata, .. } => Some(metadata),
      _ => None,
    })
    .expect("input action artifact");
  assert_eq!(records.iter().filter(|record| matches!(record, TraceRecord::Artifact { .. })).count(), 1);
  assert_eq!(metadata.purpose().as_str(), INPUT_ACTION_RESULT_PURPOSE);
  assert_eq!(metadata.content_type().to_string(), "application/json");
  let bytes = store.artifact(metadata.uri()).expect("input action artifact body");
  let recorded: InputActionResult = serde_json::from_slice(&bytes).expect("typed input action payload");
  assert_eq!(recorded, expected);
}

#[tokio::test]
async fn invalid_input_artifact_does_not_change_the_typed_call_or_reexecute_driver_input() {
  let invalid = InputActionResult {
    selected_path: InputDeliveryPath::WindowTargetedMouse,
    attempts: vec![auv_driver::InputAttempt::success(
      InputDeliveryPath::AxPress,
    )],
    verified: false,
    mouse_disturbance: auv_driver::DisturbanceLevel::None,
    focus_disturbance: auv_driver::DisturbanceLevel::None,
    clipboard_disturbance: auv_driver::DisturbanceLevel::None,
  };
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().expect("memory dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let future = root.in_scope(|| async { emit_input_action_result(&invalid) });
  root.instrument(future).await;
  dispatch.flush().await.expect("typed preparation diagnostic should flush");

  assert!(
    store.records().iter().all(|record| !matches!(record, TraceRecord::Artifact { .. })),
    "invalid evidence must not commit an artifact"
  );
}

#[tokio::test]
async fn input_action_emission_short_circuits_without_run_context() {
  let invalid = InputActionResult {
    selected_path: InputDeliveryPath::WindowTargetedMouse,
    attempts: vec![auv_driver::InputAttempt::success(
      InputDeliveryPath::AxPress,
    )],
    verified: false,
    mouse_disturbance: auv_driver::DisturbanceLevel::None,
    focus_disturbance: auv_driver::DisturbanceLevel::None,
    clipboard_disturbance: auv_driver::DisturbanceLevel::None,
  };

  emit_input_action_result(&invalid);
}

#[test]
fn input_action_artifact_enforces_domain_and_four_mibibyte_bounds() {
  let invalid = InputActionResult {
    selected_path: InputDeliveryPath::WindowTargetedMouse,
    attempts: vec![auv_driver::InputAttempt::success(
      InputDeliveryPath::AxPress,
    )],
    verified: false,
    mouse_disturbance: auv_driver::DisturbanceLevel::None,
    focus_disturbance: auv_driver::DisturbanceLevel::None,
    clipboard_disturbance: auv_driver::DisturbanceLevel::None,
  };
  let domain_error = input_action_result_artifact(&invalid).err().expect("mismatched successful attempt must fail");
  assert!(domain_error.contains("successful input attempt must match selected_path"));

  let oversized = InputActionResult {
    selected_path: InputDeliveryPath::WindowTargetedMouse,
    attempts: vec![
      auv_driver::InputAttempt::failure(InputDeliveryPath::AxPress, "x".repeat(ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT as usize)),
      auv_driver::InputAttempt::success(InputDeliveryPath::WindowTargetedMouse),
    ],
    verified: false,
    mouse_disturbance: auv_driver::DisturbanceLevel::None,
    focus_disturbance: auv_driver::DisturbanceLevel::None,
    clipboard_disturbance: auv_driver::DisturbanceLevel::None,
  };
  let size_error = input_action_result_artifact(&oversized).err().expect("oversized input action must fail");
  assert!(size_error.contains("4194304-byte limit"));
}

#[test]
fn click_point_projects_normalized_local_coordinates() {
  let point = resolve_local_point(0.5, 0.5, true, auv_driver::Size::new(1280.0, 720.0), "window").expect("normalized point");
  assert_eq!(point, auv_driver::Point::new(640.0, 360.0));
}

#[test]
fn click_point_rejects_local_coordinates_outside_target_bounds() {
  let error =
    resolve_local_point(1280.01, 20.0, false, auv_driver::Size::new(1280.0, 720.0), "window").expect_err("out-of-window point must fail");
  assert!(error.contains("outside target window bounds"), "{error}");
}

#[test]
fn click_point_rejects_incompatible_target_and_coordinate_basis() {
  let target = crate::ExecutionTarget::Display {
    id: "primary".to_string(),
  };
  let error = click_point_basis(Some(&target), Some("window"), false, false, false).expect_err("display target cannot use window basis");
  assert!(error.contains("incompatible"), "{error}");
}

fn test_window() -> auv_driver::Window {
  use auv_driver::geometry::{CoordinateSpace, Point, Rect, Size};
  use auv_driver::window::{Window, WindowRef};

  Window {
    reference: WindowRef {
      id: "window-1".to_string(),
    },
    title: Some("Example".to_string()),
    app_name: Some("Example".to_string()),
    app_bundle_id: Some("com.example.App".to_string()),
    process_id: Some(1),
    frame: Rect {
      origin: Point::new(0.0, 0.0),
      size: Size::new(1280.0, 720.0),
    },
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  }
}

#[test]
fn input_action_output_reports_explicit_domain_values() {
  let result = InputActionResult {
    selected_path: InputDeliveryPath::WindowTargetedKeyboardScroll,
    attempts: vec![],
    verified: false,
    mouse_disturbance: auv_driver::DisturbanceLevel::None,
    focus_disturbance: auv_driver::DisturbanceLevel::Foreground,
    clipboard_disturbance: auv_driver::DisturbanceLevel::Temporary,
  };

  let output = input_action_output(&result).expect("input result should serialize");

  let report = output.report.as_ref().expect("input action report");
  assert_eq!(field_value(report, "Delivery"), "delivered");
  assert_eq!(field_value(report, "Verification"), "delivery_only");
  assert_eq!(field_value(report, "Path"), "window_targeted_keyboard_scroll");
  assert_eq!(field_value(report, "Mouse disturbance"), "none");
  assert_eq!(field_value(report, "Focus disturbance"), "foreground");
  assert_eq!(field_value(report, "Clipboard disturbance"), "temporary");
  assert_eq!(output.result(), Some(&serde_json::to_value(&result).expect("fixture should serialize")));
}

#[test]
fn input_action_output_reports_semantic_verification_when_present() {
  let mut result = InputActionResult::single_success(InputDeliveryPath::AxPress);
  result.verified = true;

  let output = input_action_output(&result).expect("verified input result should serialize");
  let report = output.report.as_ref().expect("input action report");

  assert_eq!(field_value(report, "Verification"), "verified");
}

#[test]
fn focus_text_human_output_exposes_target_selection_delivery_and_verification_boundary() {
  let result = test_focus_result("Search documents");

  for (candidate, command) in [
    ("", focus_text_input_invoke_command()),
    ("root/AXTextArea[0]", ax_focus_text_input_invoke_command()),
  ] {
    let output = focus_text_output(&result, candidate).expect("focus result should serialize");
    assert_eq!(output.result(), Some(&serde_json::to_value(&result).expect("fixture should serialize")));

    let invoke_result = InvokeResult::from_command_result(RunId::new(), &command, Ok(output));
    let human = invoke_result.render_to_string(InvokeOutputOptions::default()).expect("human output should render");

    assert!(human.contains("Delivery: delivered"), "focus output omitted its delivery boundary: {human}");
    assert!(human.contains("Target: com.example.Editor"), "focus output omitted its target: {human}");
    if candidate.is_empty() {
      assert!(human.contains("Query: Search documents"), "focus output omitted its query: {human}");
    } else {
      assert!(human.contains("Candidate: root/AXTextArea[0]"), "focus output omitted its candidate: {human}");
    }
    assert!(human.contains("Resolved AX path: root/AXTextArea[0]"), "focus output omitted its resolved path: {human}");
    assert!(human.contains("Focus method: ax_focus"), "focus output omitted its delivery method: {human}");
    assert!(
      human.contains("Verification: delivery_only; focused element was not read back after AX delivery"),
      "focus output omitted its verification boundary: {human}"
    );
  }
}

#[test]
fn click_point_result_keeps_resolved_target_and_delivery_together() {
  let output = click_point_output(ClickPointResult {
    relative_to: "window".to_string(),
    requested_point: auv_driver::Point::new(640.0, 360.0),
    normalized: false,
    screen_point: auv_driver::ScreenPoint::new(640.0, 360.0),
    window: Some(test_window()),
    display: None,
    action: Some(InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse)),
  })
  .expect("click result should serialize");
  let result = output.result().expect("click should have a result");

  assert_eq!(result["window"]["reference"]["id"], "window-1");
  assert_eq!(result["screen_point"]["x"], 640.0);
  assert_eq!(result["screen_point"]["y"], 360.0);
  assert_eq!(result["action"]["selected_path"], "window_targeted_mouse");
  let report = output.report.as_ref().expect("window point click report");
  assert_eq!(field_value(report, "Delivery"), "delivered");
  assert_eq!(field_value(report, "Verification"), "delivery_only");
}

#[test]
fn click_point_dry_run_reports_validation_without_delivery() {
  let output = click_point_output(ClickPointResult {
    relative_to: "window".to_string(),
    requested_point: auv_driver::Point::new(640.0, 360.0),
    normalized: false,
    screen_point: auv_driver::ScreenPoint::new(640.0, 360.0),
    window: Some(test_window()),
    display: None,
    action: None,
  })
  .expect("validated point should serialize");

  let report = output.report.as_ref().expect("window point validation report");
  assert_eq!(field_value(report, "Delivery"), "not_performed");
  assert_eq!(field_value(report, "Verification"), "validation_only");
  assert_eq!(output.result().expect("validated target result")["action"], serde_json::Value::Null);
}

#[test]
fn generic_dry_run_report_does_not_claim_delivery() {
  let output = validation_only_output();
  let report = output.report.as_ref().expect("validation-only report");

  assert_eq!(field_value(report, "Delivery"), "not_performed");
  assert_eq!(field_value(report, "Verification"), "validation_only");
}

fn test_focus_result(query: &str) -> auv_driver::AxFocusResult {
  auv_driver::AxFocusResult {
    app: "com.example.Editor".to_string(),
    pid: 42,
    path: "root/AXTextArea[0]".to_string(),
    role: "AXTextArea".to_string(),
    title: "Document".to_string(),
    value: "draft".to_string(),
    query: query.to_string(),
    input_action_result: InputActionResult::single_success(InputDeliveryPath::AxFocus),
  }
}

fn field_value<'a>(report: &'a InvokeReport, label: &str) -> &'a str {
  report.fields.iter().find(|field| field.label == label).map(|field| field.value.as_str()).expect("field should exist")
}

// ROOT CAUSE:
//
// Generic help advertised --target while the input handler rejected every
// target, including validation-only requests. Both frontends must accept the
// same application target without delivering events during dry-run.
#[tokio::test]
#[ignore = "requires a running NetEaseMusic window and macOS Accessibility permission"]
async fn keyboard_application_target_dry_run_is_supported() {
  for (command, name, value) in [
    (press_key_invoke_command(), "key", "cmd+a"),
    (type_text_invoke_command(), "text", "Arielle's Wish"),
    (paste_text_preserve_clipboard_invoke_command(), "text", "Arielle's Wish"),
  ] {
    let output = command
      .invoke(crate::InvokeCommandInput {
        command_id: command.id.to_string(),
        target: Some(crate::ExecutionTarget::Application {
          id: "com.netease.163music".to_string(),
        }),
        inputs: [(name.to_string(), value.to_string())].into(),
        typed_args: None,
        dry_run: true,
        cancellation: Default::default(),
      })
      .await
      .expect("application targets must support validation without input");
    assert_eq!(field_value(output.report.as_ref().expect("validation report"), "Delivery"), "not_performed");
  }
}

#[test]
fn keyboard_help_distinguishes_application_activation_from_control_focus() {
  let help = crate::render_command_help(&press_key_invoke_command());
  assert!(help.contains("text-control focus"), "{help}");
  assert!(!help.contains("display:<display-id>"), "{help}");
}

#[tokio::test]
async fn keyboard_display_target_fails_consistently_before_local_or_runner_io() {
  for command in [
    press_key_invoke_command(),
    type_text_invoke_command(),
    paste_text_preserve_clipboard_invoke_command(),
  ] {
    let input = crate::InvokeCommandInput {
      command_id: command.id.into(),
      target: Some(crate::ExecutionTarget::Display {
        id: "primary".into(),
      }),
      inputs: Default::default(),
      typed_args: None,
      dry_run: false,
      cancellation: Default::default(),
    };
    let local = command.invoke(input.clone()).await.unwrap_err();
    let remote = crate::runner::invoke(input, Default::default()).await.unwrap_err();
    assert_eq!(local.code, crate::FailureCode::InvalidTarget);
    assert_eq!(local, remote);
    assert!(local.message.contains(command.id));
  }
}

#[test]
fn keyboard_target_payload_reuses_driver_options_and_preserves_text() {
  let input = crate::InvokeCommandInput {
    command_id: "input.typeText".into(),
    target: Some(crate::ExecutionTarget::Application {
      id: "com.example".into(),
    }),
    inputs: [("text".into(), "Arielle's Wish".into())].into(),
    typed_args: None,
    dry_run: false,
    cancellation: Default::default(),
  };
  assert_eq!(
    decode_keyboard_input(&input).unwrap(),
    vec![auv_driver::KeyboardInput::TypeText {
      text: "Arielle's Wish".into(),
      options: auv_driver::TypeTextOptions {
        policy: auv_driver::InputPolicy::ForegroundPreferred,
        ..Default::default()
      }
    }]
  );
}

#[tokio::test]
async fn focus_text_requires_application_even_in_dry_run() {
  let command = focus_text_input_invoke_command();
  let error = command
    .invoke(crate::InvokeCommandInput {
      command_id: command.id.into(),
      target: None,
      inputs: [("query".into(), "Search".into())].into(),
      typed_args: None,
      dry_run: true,
      cancellation: Default::default(),
    })
    .await
    .unwrap_err();
  assert_eq!(error.code, crate::FailureCode::InvalidTarget);
}

// A target selects the recipient; an explicit delivery policy must survive parsing.
#[test]
fn keyboard_commands_accept_explicit_delivery_policy() {
  for command in [
    press_key_invoke_command(),
    type_text_invoke_command(),
    paste_text_preserve_clipboard_invoke_command(),
  ] {
    command
      .parse_cli_args(&[
        "a".into(),
        "--target".into(),
        "app:com.example".into(),
        "--input-policy".into(),
        "background-only".into(),
      ])
      .expect("keyboard commands must expose their activation policy");
  }
}

// ROOT CAUSE: without the serde input-policy rename, protocol/MCP arguments
// silently lost the explicit background policy and used the foreground default.
#[tokio::test]
async fn background_keyboard_without_target_fails_before_local_or_runner_io() {
  for (command, name, value) in [
    (press_key_invoke_command(), "key", "Escape"),
    (type_text_invoke_command(), "text", "must not type"),
    (paste_text_preserve_clipboard_invoke_command(), "text", "must not paste"),
  ] {
    let input = crate::InvokeCommandInput {
      command_id: command.id.into(),
      target: None,
      inputs: [
        (name.into(), value.into()),
        ("input-policy".into(), "background-only".into()),
      ]
      .into(),
      typed_args: None,
      dry_run: true,
      cancellation: Default::default(),
    };
    let local = command.invoke(input.clone()).await.unwrap_err();
    let remote = crate::runner::invoke(input, Default::default()).await.unwrap_err();
    assert_eq!(local, remote);
    assert!(local.message.contains("requires --target"));
  }
}

#[tokio::test]
async fn repeated_key_requires_interval_in_local_dry_run() {
  let command = press_key_invoke_command();
  let error = command
    .invoke(crate::InvokeCommandInput {
      command_id: command.id.into(),
      target: None,
      inputs: [("key".into(), "a".into()), ("count".into(), "2".into())].into(),
      typed_args: None,
      dry_run: true,
      cancellation: Default::default(),
    })
    .await
    .unwrap_err();
  assert_eq!(error.code, crate::FailureCode::InvalidInput, "{}", error.message);
  assert!(error.message.contains("interval"));
}

#[tokio::test]
async fn chord_protocol_arguments_preserve_keys_and_repeat_options() {
  let command = press_keys_invoke_command();
  let input = crate::InvokeCommandInput {
    command_id: command.id.into(),
    target: None,
    inputs: [
      ("keys".into(), r#"["cmd","return"]"#.into()),
      ("count".into(), "3".into()),
      ("interval-ms".into(), "15".into()),
    ]
    .into(),
    typed_args: None,
    dry_run: true,
    cancellation: Default::default(),
  };
  let decoded = decode_keyboard_input(&input).unwrap();
  assert_eq!(
    decoded,
    vec![auv_driver::KeyboardInput::PressKeys {
      policy: auv_driver::InputPolicy::ForegroundPreferred,
      options: auv_driver::PressKeysOptions {
        keys: vec!["cmd".into(), "return".into()],
        count: 3,
        interval: std::time::Duration::from_millis(15),
        ..Default::default()
      },
    }]
  );
  command.invoke(input).await.unwrap();
}
