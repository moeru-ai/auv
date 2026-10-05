use super::*;
use crate::{InvokeOutputOptions, InvokeResult};
use auv_driver::{InputActionResult, InputDeliveryPath};
use auv_tracing::{Context, MemoryTracingStore, RunId, TraceRecord, configure, dispatcher};
use std::sync::Arc;

#[test]
fn click_point_accepts_repeated_and_comma_separated_modifiers() {
  for values in [
    vec!["cmd", "shift"],
    vec!["cmd,shift"],
    vec!["cmd", "shift,option"],
  ] {
    let mut arguments = vec!["10".into(), "20".into()];
    for value in &values {
      arguments.extend(["--modifiers".into(), (*value).into()]);
    }
    let crate::InvokeCommandCliParse::Invoke {
      inputs, typed_args, ..
    } = click_point_invoke_command().parse_cli_args(&arguments).unwrap()
    else {
      panic!("expected parsed invocation");
    };
    assert_eq!(inputs["modifiers"], values.join(","));
    let protocol_args: ClickPointArgs = crate::command::decode_args(&InvokeCommandInput {
      command_id: "input.clickPoint".into(),
      target: None,
      inputs,
      typed_args: None,
      dry_run: true,
      cancellation: Default::default(),
    })
    .unwrap();
    let replayed: ClickPointArgs = serde_json::from_value(serde_json::json!({
      "x": 10.0, "y": 20.0, "modifiers": values.join(",")
    }))
    .unwrap();
    assert_eq!(protocol_args.click_options().unwrap().modifiers, replayed.click_options().unwrap().modifiers);
    assert_eq!(typed_args.get::<ClickPointArgs>().unwrap().click_options().unwrap().modifiers, replayed.click_options().unwrap().modifiers);
    assert_eq!(
      replayed.click_options().unwrap().modifiers,
      auv_driver::ClickModifiers {
        meta: true,
        shift: true,
        alt: values.contains(&"shift,option"),
        ..Default::default()
      }
    );
  }
}

#[test]
fn click_point_preserves_modifiers_in_typed_replay_input() {
  let args: ClickPointArgs = serde_json::from_value(serde_json::json!({
    "x": 10.0, "y": 20.0, "modifiers": "cmd,shift,option,control"
  }))
  .unwrap();
  let recorded = serde_json::to_value(&args).unwrap();
  assert_eq!(recorded["modifiers"], "cmd,shift,option,control");
  let replayed: ClickPointArgs = serde_json::from_value(recorded).unwrap();
  assert_eq!(
    replayed.click_options().unwrap().modifiers,
    auv_driver::ClickModifiers {
      shift: true,
      control: true,
      alt: true,
      meta: true,
    }
  );
}

#[test]
fn click_modifier_parser_rejects_unknown_keys_and_duplicate_aliases() {
  assert!(parse_click_modifiers(Some("cmd,meta")).unwrap_err().contains("duplicate"));
  assert!(parse_click_modifiers(Some("space")).unwrap_err().contains("unknown"));
  assert!(parse_click_modifiers(Some("61")).unwrap_err().contains("unknown"));
  assert!(parse_click_modifiers(Some("")).is_err());
  assert!(parse_click_modifiers(None).unwrap().is_empty());
}

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
  let point =
    resolve_local_point("input.clickPoint", 0.5, 0.5, true, auv_driver::Size::new(1280.0, 720.0), "window").expect("normalized point");
  assert_eq!(point, auv_driver::Point::new(640.0, 360.0));
}

#[test]
fn click_point_rejects_local_coordinates_outside_target_bounds() {
  let error = resolve_local_point("input.clickPoint", 1280.01, 20.0, false, auv_driver::Size::new(1280.0, 720.0), "window")
    .expect_err("out-of-window point must fail");
  assert!(error.contains("outside target window bounds"), "{error}");
}

#[test]
fn click_point_rejects_incompatible_target_and_coordinate_basis() {
  let target = crate::ExecutionTarget::Display {
    id: "primary".to_string(),
  };
  let error =
    point_basis("input.clickPoint", Some(&target), Some("window"), false, false, false).expect_err("display target cannot use window basis");
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

#[test]
fn press_keys_alias_uses_the_existing_keyboard_action() {
  let input = crate::InvokeCommandInput {
    command_id: "input.pressKeys".into(),
    target: None,
    inputs: [("keys".into(), "[\"cmd\",\"a\"]".into())].into(),
    typed_args: None,
    dry_run: true,
    cancellation: Default::default(),
  };

  let mut old = input.clone();
  old.command_id = "input.keys".into();

  assert_eq!(decode_keyboard_input(&input).unwrap(), decode_keyboard_input(&old).unwrap());
}

#[test]
fn hold_keys_validates_duration_and_target_policy_before_io() {
  let command = hold_keys_invoke_command();
  let crate::InvokeCommandCliParse::Invoke { inputs, .. } =
    command.parse_cli_args(&["shift".into(), "--duration-ms".into(), "800".into()]).unwrap()
  else {
    panic!("expected parsed invocation");
  };

  let input = crate::InvokeCommandInput {
    command_id: command.id.into(),
    target: None,
    inputs,
    typed_args: None,
    dry_run: true,
    cancellation: Default::default(),
  };

  let (keys, policy, duration) = decode_hold_keys(&input).unwrap();
  assert_eq!(keys, vec!["shift"]);
  assert_eq!(policy, auv_driver::InputPolicy::ForegroundPreferred);
  assert_eq!(duration, std::time::Duration::from_millis(800));

  let mut invalid = input.clone();
  invalid.inputs.insert("duration-ms".into(), "30001".into());
  assert_eq!(decode_hold_keys(&invalid).unwrap_err().code, crate::FailureCode::InvalidInput);

  invalid.inputs.insert("duration-ms".into(), "800".into());
  invalid.inputs.insert("input-policy".into(), "background-only".into());
  assert_eq!(decode_hold_keys(&invalid).unwrap_err().code, crate::FailureCode::InvalidInput);
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
async fn key_combination_protocol_arguments_preserve_keys_and_repeat_options() {
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

#[cfg(target_os = "linux")]
#[tokio::test]
async fn linux_keyboard_commands_reach_driver_validation() {
  // ROOT CAUSE: invoke returned Unsupported before calling the Linux driver.
  // Exercise command dispatch, not just argument conversion.
  for (command, inputs) in [
    (press_key_invoke_command(), [("key".into(), "a".into())].into()),
    (press_keys_invoke_command(), [("keys".into(), "[\"ctrl\",\"a\"]".into())].into()),
    (input_keyboard_invoke_command(), [("actions".into(), "[{\"kind\":\"press\",\"keys\":[\"a\"]}]".into())].into()),
  ] {
    command
      .invoke(InvokeCommandInput {
        command_id: command.id.into(),
        target: None,
        inputs,
        typed_args: None,
        dry_run: true,
        cancellation: Default::default(),
      })
      .await
      .unwrap();
  }
}

#[test]
fn click_button_survives_cli_protocol_and_recorded_replay() {
  for (name, button) in [
    ("left", auv_driver::MouseButton::Left),
    ("right", auv_driver::MouseButton::Right),
    ("middle", auv_driver::MouseButton::Middle),
  ] {
    let crate::InvokeCommandCliParse::Invoke {
      inputs, typed_args, ..
    } = click_point_invoke_command().parse_cli_args(&["10".into(), "20".into(), "--button".into(), name.into()]).unwrap()
    else {
      panic!("expected invocation")
    };
    assert_eq!(inputs["button"], name);
    assert_eq!(typed_args.get::<ClickPointArgs>().unwrap().click_options().unwrap().button, button);
    let input = InvokeCommandInput {
      command_id: "input.clickPoint".into(),
      target: None,
      inputs,
      typed_args: None,
      dry_run: true,
      cancellation: Default::default(),
    };
    let args: ClickPointArgs = crate::command::decode_args(&input).unwrap();
    let recorded = serde_json::to_value(args).unwrap();
    assert_eq!(recorded["button"], name);
    let replay: ClickPointArgs = serde_json::from_value(recorded).unwrap();
    assert_eq!(replay.click_options().unwrap().button, button);
  }
  let ordinary: ClickPointArgs = serde_json::from_value(serde_json::json!({"x": 10, "y": 20})).unwrap();
  assert_eq!(ordinary.click_options().unwrap().button, auv_driver::MouseButton::Left);
  assert!(click_point_invoke_command().parse_cli_args(&["10".into(), "20".into(), "--button".into(), "back".into()]).is_err());
  assert!(parse_click_button(Some("back")).is_err());
}

fn drag_input(inputs: &[(&str, &str)], target: Option<crate::ExecutionTarget>) -> InvokeCommandInput {
  InvokeCommandInput {
    command_id: "input.drag".into(),
    target,
    inputs: inputs.iter().map(|(key, value)| ((*key).to_string(), (*value).to_string())).collect(),
    typed_args: None,
    dry_run: true,
    cancellation: Default::default(),
  }
}

// CLI arguments, MCP inputs, and Runner dispatch must decode to one plan.
#[test]
fn drag_cli_and_protocol_inputs_decode_to_the_same_plan() {
  let crate::InvokeCommandCliParse::Invoke {
    inputs, typed_args, ..
  } = drag_invoke_command()
    .parse_cli_args(&[
      "--button".into(),
      "right".into(),
      "--".into(),
      "10".into(),
      "-20".into(),
      "30".into(),
      "40".into(),
    ])
    .unwrap()
  else {
    panic!("expected parsed invocation");
  };
  assert_eq!(inputs["start_y"], "-20.0");

  let protocol: DragArgs = crate::command::decode_args(&drag_input(
    &[
      ("start_x", "10"),
      ("start_y", "-20"),
      ("end_x", "30"),
      ("end_y", "40"),
      ("button", "right"),
    ],
    None,
  ))
  .unwrap();
  let typed = typed_args.get::<DragArgs>().unwrap().clone();
  for args in [protocol, typed] {
    let plan = args.plan(None).unwrap();
    assert_eq!(plan.basis, RelativeToArg::Screen);
    assert_eq!(plan.start, auv_driver::Point::new(10.0, -20.0));
    assert_eq!(plan.end, auv_driver::Point::new(30.0, 40.0));
    assert_eq!(plan.button, auv_driver::MouseButton::Right);
    assert_eq!(plan.duration, std::time::Duration::from_millis(300));
  }
}

#[test]
fn drag_plan_rejects_invalid_requests_before_io() {
  let window = Some(crate::ExecutionTarget::Window {
    id: "window-1".to_string(),
  });
  let display = Some(crate::ExecutionTarget::Display {
    id: "primary".to_string(),
  });
  let points = [
    ("start_x", "0.1"),
    ("start_y", "0.1"),
    ("end_x", "0.2"),
    ("end_y", "0.2"),
  ];
  for (extra, target, expected) in [
    (vec![("duration-ms", "30001")], None, "input.drag --duration-ms must be within 0..=30000"),
    (vec![("normalized", "true")], None, "input.drag --normalized is valid only relative to a window or display"),
    (vec![("relative-to", "window")], display, "input.drag --target kind is incompatible with --relative-to window"),
    (vec![("normalized", "true"), ("end_x", "1.5")], window, "input.drag --normalized coordinates must be within 0..=1"),
  ] {
    let mut inputs = points.to_vec();
    inputs.retain(|(key, _)| extra.iter().all(|(extra_key, _)| extra_key != key));
    inputs.extend(extra);
    let input = drag_input(&inputs, target);
    let error = crate::command::decode_args::<DragArgs>(&input).unwrap().plan(input.target.as_ref()).unwrap_err();
    assert_eq!(error, expected);
  }
}

// A window drag keeps the driver default unless the caller names a policy.
// Foreground preparation exists only for window targets, like clickPoint.
#[test]
fn drag_input_policy_requires_window_basis_and_reaches_the_plan() {
  let window = Some(crate::ExecutionTarget::Window {
    id: "window-1".to_string(),
  });
  let points = [
    ("start_x", "10"),
    ("start_y", "20"),
    ("end_x", "30"),
    ("end_y", "20"),
  ];
  let plan = |inputs: &[(&str, &str)], target: Option<crate::ExecutionTarget>| {
    let input = drag_input(inputs, target);
    crate::command::decode_args::<DragArgs>(&input).unwrap().plan(input.target.as_ref())
  };

  assert_eq!(plan(&points, window.clone()).unwrap().policy, None);

  let mut foreground = points.to_vec();
  foreground.push(("input-policy", "foreground-preferred"));
  assert_eq!(plan(&foreground, window).unwrap().policy, Some(auv_driver::InputPolicy::ForegroundPreferred));
  assert_eq!(plan(&foreground, None).unwrap_err(), "input.drag --input-policy is valid only with --relative-to window");
}

#[test]
fn drag_projects_both_endpoints_from_the_target_frame() {
  let input = drag_input(
    &[
      ("start_x", "0.25"),
      ("start_y", "0.5"),
      ("end_x", "0.75"),
      ("end_y", "0.5"),
      ("normalized", "true"),
    ],
    Some(crate::ExecutionTarget::Window {
      id: "window-1".to_string(),
    }),
  );
  let plan = crate::command::decode_args::<DragArgs>(&input).unwrap().plan(input.target.as_ref()).unwrap();
  let frame = auv_driver::Rect {
    origin: auv_driver::Point::new(100.0, 50.0),
    size: auv_driver::Size::new(800.0, 600.0),
  };
  let (start, end) = plan.screen_points(frame, "window").unwrap();
  assert_eq!(start, ScreenPoint::new(300.0, 350.0));
  assert_eq!(end, ScreenPoint::new(700.0, 350.0));

  let mut outside = plan.clone();
  outside.normalized = false;
  outside.end = auv_driver::Point::new(800.5, 10.0);
  let error = outside.screen_points(frame, "window").unwrap_err();
  assert_eq!(error.code, crate::FailureCode::InvalidInput);
  assert!(error.message.starts_with("input.drag point 800.5,10 is outside target window bounds"), "{error}");
}

// The driver receives one complete gesture: down at the start, motion along
// the chord, and up at the end point after the requested duration.
#[test]
fn drag_movement_is_one_straight_screen_path_to_the_end_point() {
  let start = auv_driver::Point::new(100.0, 200.0);
  let end = auv_driver::Point::new(400.0, 260.0);
  for (duration_ms, expected_samples) in [(0, 2), (300, 19)] {
    let input = drag_input(
      &[
        ("start_x", "100"),
        ("start_y", "200"),
        ("end_x", "400"),
        ("end_y", "260"),
        ("duration-ms", &duration_ms.to_string()),
      ],
      None,
    );
    let plan = crate::command::decode_args::<DragArgs>(&input).unwrap().plan(None).unwrap();
    let target = auv_driver::InputTarget::Window(test_window());
    let request = plan.movement(target.clone(), ScreenPoint::new(start.x, start.y), ScreenPoint::new(end.x, end.y));
    assert_eq!(request.target, Some(target));
    assert_eq!(request.start, auv_driver::MouseStart::Screen(start));

    let samples = request.samples(start).unwrap();
    assert_eq!(samples.len(), expected_samples);
    assert_eq!(samples.at(0).point, start);
    let last = samples.at(samples.len() - 1);
    assert_eq!(last.point, end);
    assert_eq!(last.elapsed, std::time::Duration::from_millis(duration_ms));
    for index in 0..samples.len() {
      let point = samples.at(index).point;
      let cross = (point.x - start.x) * (end.y - start.y) - (point.y - start.y) * (end.x - start.x);
      assert!(cross.abs() < 1e-6, "sample {index} left the drag line: {point:?}");
      assert!((start.x..=end.x).contains(&point.x), "sample {index} passed an endpoint: {point:?}");
    }
  }
}

#[test]
fn drag_dry_run_reports_resolved_endpoints_without_delivery() {
  let input = drag_input(
    &[
      ("start_x", "10"),
      ("start_y", "20"),
      ("end_x", "30"),
      ("end_y", "40"),
    ],
    None,
  );
  let plan = crate::command::decode_args::<DragArgs>(&input).unwrap().plan(None).unwrap();
  let mut result = DragResult::planned(&plan, ScreenPoint::new(110.0, 70.0), ScreenPoint::new(130.0, 90.0));
  result.window = Some(test_window());
  let output = drag_output(result).unwrap();

  let value = output.result().expect("drag result");
  assert_eq!(value["window"]["reference"]["id"], "window-1");
  assert_eq!(value["screen_end"]["x"], 130.0);
  assert_eq!(value["action"], serde_json::Value::Null);
  let report = output.report.as_ref().expect("drag report");
  assert_eq!(field_value(report, "Delivery"), "not_performed");
  assert_eq!(field_value(report, "Verification"), "validation_only");
  assert_eq!(field_value(report, "Screen start"), "110.0,70.0");
  assert_eq!(field_value(report, "Window ID"), "window-1");
}

fn scroll_input(inputs: &[(&str, &str)], target: Option<crate::ExecutionTarget>) -> InvokeCommandInput {
  InvokeCommandInput {
    command_id: "input.scroll".into(),
    target,
    inputs: inputs.iter().map(|(key, value)| ((*key).to_string(), (*value).to_string())).collect(),
    typed_args: None,
    dry_run: true,
    cancellation: Default::default(),
  }
}

fn scroll_window_target() -> Option<crate::ExecutionTarget> {
  Some(crate::ExecutionTarget::Window {
    id: "window-1".to_string(),
  })
}

// CLI arguments, MCP inputs, and Runner dispatch must decode to one plan,
// including negative deltas that look like flags.
#[test]
fn scroll_cli_and_protocol_inputs_decode_to_the_same_plan() {
  let crate::InvokeCommandCliParse::Invoke {
    inputs, typed_args, ..
  } = scroll_invoke_command()
    .parse_cli_args(&[
      "10".into(),
      "20".into(),
      "--dy".into(),
      "-300".into(),
      "--dx".into(),
      "15".into(),
      "--input-policy".into(),
      "background-only".into(),
      "--settle-ms".into(),
      "50".into(),
    ])
    .unwrap()
  else {
    panic!("expected parsed invocation");
  };
  assert_eq!(inputs["dy"], "-300.0");

  let protocol: ScrollArgs = crate::command::decode_args(&scroll_input(
    &[
      ("x", "10"),
      ("y", "20"),
      ("dx", "15"),
      ("dy", "-300"),
      ("input-policy", "background-only"),
      ("settle-ms", "50"),
    ],
    scroll_window_target(),
  ))
  .unwrap();
  let typed = typed_args.get::<ScrollArgs>().unwrap().clone();
  for args in [protocol, typed] {
    let plan = args.plan(scroll_window_target().as_ref()).unwrap();
    assert_eq!(plan.point, auv_driver::Point::new(10.0, 20.0));
    assert_eq!(plan.scroll, auv_driver::Scroll::new(15.0, -300.0));
    assert_eq!(plan.options.policy, auv_driver::InputPolicy::BackgroundOnly);
    assert_eq!(plan.options.settle, std::time::Duration::from_millis(50));
    assert_eq!(plan.options.delivery_strategy, auv_driver::ScrollDeliveryStrategy::default());
  }
}

#[test]
fn scroll_plan_rejects_invalid_requests_before_io() {
  let app = Some(crate::ExecutionTarget::Application {
    id: "com.example.App".to_string(),
  });
  let display = Some(crate::ExecutionTarget::Display {
    id: "primary".to_string(),
  });
  for (inputs, target, expected) in [
    (vec![("dy", "100")], None, "input.scroll requires --target app: or window:"),
    (vec![("dy", "100")], display, "input.scroll requires --target app: or window:"),
    (vec![("dy", "100"), ("title", "Doc")], scroll_window_target(), "input.scroll --title requires --target app:"),
    (vec![], app.clone(), "input.scroll requires a non-zero --dx or --dy"),
    (vec![("dx", "NaN")], app.clone(), "input.scroll requires finite --dx and --dy"),
    (vec![("dy", "10"), ("normalized", "true")], app.clone(), "input.scroll --normalized coordinates must be within 0..=1"),
    (vec![("dy", "10"), ("settle-ms", "30001")], app, "input.scroll --settle-ms must be within 0..=30000"),
  ] {
    let mut all = vec![("x", "40"), ("y", "50")];
    all.extend(inputs);
    let input = scroll_input(&all, target);
    let error = crate::command::decode_args::<ScrollArgs>(&input).unwrap().plan(input.target.as_ref()).unwrap_err();
    assert_eq!(error, expected);
  }
}

#[test]
fn scroll_plan_resolves_window_point_and_reports_screen_point() {
  let input = scroll_input(
    &[
      ("x", "0.5"),
      ("y", "0.25"),
      ("normalized", "true"),
      ("dy", "120"),
    ],
    scroll_window_target(),
  );
  let plan = crate::command::decode_args::<ScrollArgs>(&input).unwrap().plan(input.target.as_ref()).unwrap();
  let mut window = test_window();
  window.frame.origin = auv_driver::Point::new(100.0, 50.0);

  let point = plan.window_point(&window).unwrap();
  assert_eq!(point.point(), auv_driver::Point::new(640.0, 180.0));
  let result = plan.result(window, point);
  assert_eq!(result.screen_point.point(), auv_driver::Point::new(740.0, 230.0));
  assert_eq!(result.scroll, auv_driver::Scroll::new(0.0, 120.0));
  assert_eq!(result.policy, auv_driver::InputPolicy::BackgroundPreferred);
  assert!(result.action.is_none());
}

#[test]
fn timing_function_names_and_cubic_bezier_forms_parse_to_driver_functions() {
  assert_eq!(parse_timing_function("linear").unwrap(), auv_driver::TimingFunction::Linear);
  assert_eq!(parse_timing_function("ease-in").unwrap(), auv_driver::TimingFunction::EaseInCubic);
  assert_eq!(parse_timing_function("ease-out").unwrap(), auv_driver::TimingFunction::EaseOutCubic);
  assert_eq!(parse_timing_function("ease-in-out").unwrap(), auv_driver::TimingFunction::EaseInOutCubic);
  let bezier = auv_driver::TimingFunction::CubicBezier {
    x1: 0.2,
    y1: 0.8,
    x2: 0.2,
    y2: 1.0,
  };
  assert_eq!(parse_timing_function("cubic-bezier:0.2,0.8,0.2,1").unwrap(), bezier);
  assert_eq!(parse_timing_function("cubic-bezier(0.2, 0.8, 0.2, 1)").unwrap(), bezier);
  assert!(parse_timing_function("bounce").is_err());
  assert!(parse_timing_function("cubic-bezier:0.2,0.8,0.2").is_err());
  assert!(parse_timing_function("cubic-bezier:1.2,0,0.5,1").is_err());
}

#[test]
fn timed_scroll_cli_and_protocol_inputs_decode_to_the_same_motion() {
  let crate::InvokeCommandCliParse::Invoke { typed_args, .. } = scroll_invoke_command()
    .parse_cli_args(&[
      "10".into(),
      "20".into(),
      "--dy".into(),
      "900".into(),
      "--duration-ms".into(),
      "600".into(),
      "--easing".into(),
      "ease-out".into(),
      "--sample-rate-hz".into(),
      "120".into(),
    ])
    .unwrap()
  else {
    panic!("expected parsed invocation");
  };
  let protocol: ScrollArgs = crate::command::decode_args(&scroll_input(
    &[
      ("x", "10"),
      ("y", "20"),
      ("dy", "900"),
      ("duration-ms", "600"),
      ("easing", "ease-out"),
      ("sample-rate-hz", "120"),
    ],
    scroll_window_target(),
  ))
  .unwrap();
  let expected = auv_driver::ScrollMotion {
    total: auv_driver::Scroll::new(0.0, 900.0),
    timing: auv_driver::MotionTiming::FixedDuration {
      duration: std::time::Duration::from_millis(600),
      function: auv_driver::TimingFunction::EaseOutCubic,
    },
    sample_rate_hz: 120,
  };
  for args in [protocol, typed_args.get::<ScrollArgs>().unwrap().clone()] {
    assert_eq!(args.plan(scroll_window_target().as_ref()).unwrap().motion, Some(expected));
  }
  // Instant scrolls keep the default rate but carry no motion.
  let instant: ScrollArgs =
    crate::command::decode_args(&scroll_input(&[("x", "1"), ("y", "1"), ("dy", "10")], scroll_window_target())).unwrap();
  assert_eq!(instant.plan(scroll_window_target().as_ref()).unwrap().motion, None);
}

#[test]
fn timed_scroll_plan_rejects_invalid_timing_before_io() {
  for (inputs, expected) in [
    (vec![("easing", "ease-in")], "input.scroll --easing requires a positive --duration-ms"),
    (vec![("duration-ms", "60001")], "input.scroll --duration-ms must be within 0..=60000"),
    (vec![("duration-ms", "100"), ("sample-rate-hz", "0")], "input.scroll --sample-rate-hz must be within 1..=1000"),
  ] {
    let mut all = vec![("x", "40"), ("y", "50"), ("dy", "100")];
    all.extend(inputs);
    let input = scroll_input(&all, scroll_window_target());
    let error = crate::command::decode_args::<ScrollArgs>(&input).unwrap().plan(input.target.as_ref()).unwrap_err();
    assert_eq!(error, expected);
  }
}
