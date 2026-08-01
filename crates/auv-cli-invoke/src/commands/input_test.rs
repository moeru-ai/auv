use super::*;
use crate::{InvokeOutputOptions, InvokeResult};
use auv_driver::{InputActionResult, InputDeliveryPath};
use auv_tracing::{Context, MemoryTracingStore, RunId, TraceRecord, configure, dispatcher};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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

#[derive(Clone)]
struct ControlledWindowCapability {
  window: auv_driver::Window,
  action: InputActionResult,
  resolve_calls: Arc<AtomicUsize>,
  click_calls: Arc<AtomicUsize>,
}

impl ControlledWindowCapability {
  fn new() -> Self {
    Self {
      window: test_window(),
      action: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
      resolve_calls: Arc::new(AtomicUsize::new(0)),
      click_calls: Arc::new(AtomicUsize::new(0)),
    }
  }

  fn resolve_count(&self) -> usize {
    self.resolve_calls.load(Ordering::SeqCst)
  }

  fn click_count(&self) -> usize {
    self.click_calls.load(Ordering::SeqCst)
  }

  fn with_action(mut self, action: InputActionResult) -> Self {
    self.action = action;
    self
  }
}

impl WindowPointCapability for ControlledWindowCapability {
  fn resolve(&self, _selector: auv_driver::WindowSelector) -> auv_driver::DriverResult<auv_driver::Window> {
    self.resolve_calls.fetch_add(1, Ordering::SeqCst);
    Ok(self.window.clone())
  }

  fn click(
    &self,
    _window: &auv_driver::Window,
    _point: auv_driver::geometry::WindowPoint,
    _options: auv_driver::ClickOptions,
  ) -> auv_driver::DriverResult<auv_driver::InputActionResult> {
    self.click_calls.fetch_add(1, Ordering::SeqCst);
    Ok(self.action.clone())
  }
}

#[test]
fn click_window_point_missing_point_args_returns_error() {
  let inputs = BTreeMap::new();
  let input = InvokeCommandInput {
    command_id: "input.clickWindowPoint".to_string(),
    target_application_id: Some("com.example.App".to_string()),
    inputs,
    typed_args: None,
    dry_run: false,
    cancellation: crate::InvokeCancellation::new(),
  };
  let error = futures_executor::block_on(click_window_point_invoke_command().invoke(input)).expect_err("missing point args should fail");
  assert!(error.contains("requires --offset-x/--offset-y or --relative-x/--relative-y"));
}

#[test]
fn click_window_point_valid_dry_run_resolves_window_without_clicking() {
  let capability = ControlledWindowCapability::new();
  let mut inputs = BTreeMap::new();
  inputs.insert("offset-x".to_string(), "640".to_string());
  inputs.insert("offset-y".to_string(), "360".to_string());
  let input = InvokeCommandInput {
    command_id: "input.clickWindowPoint".to_string(),
    target_application_id: Some("com.example.App".to_string()),
    inputs,
    typed_args: None,
    dry_run: true,
    cancellation: crate::InvokeCancellation::new(),
  };
  let outcome = futures_executor::block_on(click_resolved_point_with_capability(
    input,
    None,
    offset_point(640.0, 360.0),
    auv_driver::ClickOptions::default(),
    &capability,
  ))
  .expect("dry run should succeed");

  assert!(matches!(outcome, WindowPointClickOutcome::Validated { .. }));
  assert_eq!(capability.resolve_count(), 1);
  assert_eq!(capability.click_count(), 0);
}

#[test]
fn click_window_point_out_of_bounds_dry_run_fails_without_clicking() {
  let capability = ControlledWindowCapability::new();
  let input = InvokeCommandInput {
    command_id: "input.clickWindowPoint".to_string(),
    target_application_id: Some("com.example.App".to_string()),
    inputs: BTreeMap::from([
      ("offset-x".to_string(), "1280.01".to_string()),
      ("offset-y".to_string(), "360".to_string()),
    ]),
    typed_args: None,
    dry_run: true,
    cancellation: crate::InvokeCancellation::new(),
  };

  // ROOT CAUSE:
  //
  // If a syntactically valid positive offset exceeded the resolved window,
  // dry-run completed because the handler returned before window resolution.
  //
  // Before the fix, this input completed without consulting window geometry.
  // The fix resolves containment before dry-run returns and never clicks.
  let error = futures_executor::block_on(click_resolved_point_with_capability(
    input,
    None,
    offset_point(1280.01, 360.0),
    auv_driver::ClickOptions::default(),
    &capability,
  ))
  .expect_err("out-of-bounds dry-run offset must fail");

  assert!(error.contains("outside target window bounds"), "{error}");
  assert_eq!(capability.resolve_count(), 1);
  assert_eq!(capability.click_count(), 0);
}

#[test]
fn click_window_point_live_resolves_once_before_clicking() {
  let capability = ControlledWindowCapability::new();
  let input = InvokeCommandInput {
    command_id: "input.clickWindowPoint".to_string(),
    target_application_id: Some("com.example.App".to_string()),
    inputs: BTreeMap::from([
      ("offset-x".to_string(), "640".to_string()),
      ("offset-y".to_string(), "360".to_string()),
    ]),
    typed_args: None,
    dry_run: false,
    cancellation: crate::InvokeCancellation::new(),
  };

  let outcome = futures_executor::block_on(click_resolved_point_with_capability(
    input,
    None,
    offset_point(640.0, 360.0),
    auv_driver::ClickOptions::default(),
    &capability,
  ))
  .expect("valid live point");

  assert!(matches!(outcome, WindowPointClickOutcome::Delivered { .. }));
  assert_eq!(capability.resolve_count(), 1);
  assert_eq!(capability.click_count(), 1);
}

#[tokio::test]
async fn resolved_window_click_returns_direct_action_and_publishes_through_typed_driver_contract() {
  let capability = ControlledWindowCapability::new();
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().expect("memory dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let expected = InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse);
  let future = root.in_scope(|| {
    click_resolved_window_point(
      &capability,
      test_window(),
      auv_driver::geometry::WindowPoint::new(640.0, 360.0),
      auv_driver::ClickOptions::default(),
    )
  });

  let delivered = root.instrument(future).await.expect("direct window click result");
  dispatch.flush().await.expect("flush input action telemetry");
  let records = store.records();

  assert_eq!(delivered.action, expected);
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
  let capability = ControlledWindowCapability::new().with_action(invalid.clone());
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().expect("memory dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let future = root.in_scope(|| {
    click_resolved_window_point(
      &capability,
      test_window(),
      auv_driver::geometry::WindowPoint::new(640.0, 360.0),
      auv_driver::ClickOptions::default(),
    )
  });

  let delivered = root.instrument(future).await.expect("artifact preparation must not replace the direct input result");
  dispatch.flush().await.expect("typed preparation diagnostic should flush");

  assert_eq!(delivered.action, invalid);
  assert_eq!(capability.click_count(), 1, "artifact preparation must not reexecute direct input");
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
fn click_window_point_negative_offset_dry_run_fails_without_driver() {
  let input = InvokeCommandInput {
    command_id: "input.clickWindowPoint".to_string(),
    target_application_id: Some("com.example.App".to_string()),
    inputs: BTreeMap::from([
      ("offset-x".to_string(), "-0.01".to_string()),
      ("offset-y".to_string(), "20".to_string()),
    ]),
    typed_args: None,
    dry_run: true,
    cancellation: crate::InvokeCancellation::new(),
  };

  let error = futures_executor::block_on(click_window_point_invoke_command().invoke(input))
    .expect_err("negative dry-run offset must fail before driver work");

  assert!(error.contains("non-negative"), "{error}");
}

#[test]
fn resolve_click_window_point_accepts_inclusive_offset_boundaries() {
  let window = test_window();
  for (x, y) in [(0.0, 0.0), (1280.0, 720.0)] {
    let point = offset_point(x, y).resolve(&window, "input.clickWindowPoint").expect("inclusive window boundary");
    assert_eq!(point, auv_driver::geometry::WindowPoint::new(x, y));
  }
}

#[test]
fn resolve_click_window_point_rejects_offsets_outside_window_bounds() {
  let window = test_window();
  for (name, x, y) in [
    ("oversized x", 1280.01, 20.0),
    ("oversized y", 10.0, 720.01),
  ] {
    let error = offset_point(x, y).resolve(&window, "input.clickWindowPoint").expect_err("out-of-window offset must fail");
    assert!(error.contains("outside target window"), "{name}: {error}");
  }
}

#[test]
fn resolve_click_window_point_converts_relative_pair() {
  let window = test_window();
  let point = relative_point(0.5, 0.5).resolve(&window, "input.clickWindowPoint").expect("relative pair");
  assert_eq!(point, auv_driver::geometry::WindowPoint::new(640.0, 360.0));
}

#[test]
fn resolve_click_window_point_accepts_inclusive_relative_boundaries() {
  let window = test_window();
  for (relative_x, relative_y, expected_x, expected_y) in [(0.0, 0.0, 0.0, 0.0), (1.0, 1.0, 1280.0, 720.0)] {
    let point = relative_point(relative_x, relative_y).resolve(&window, "input.clickWindowPoint").expect("inclusive relative boundary");
    assert_eq!(point, auv_driver::geometry::WindowPoint::new(expected_x, expected_y));
  }
}

fn offset_point(x: f64, y: f64) -> WindowPointInput {
  WindowPointInput(WindowPointKind::Offset(auv_driver::geometry::WindowPoint::new(x, y)))
}

fn relative_point(x: f64, y: f64) -> WindowPointInput {
  WindowPointInput(WindowPointKind::Relative(RelativeWindowPoint { x, y }))
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
fn window_point_click_result_keeps_resolved_target_and_delivery_together() {
  let click = WindowPointClick {
    window: test_window(),
    point: auv_driver::geometry::WindowPoint::new(640.0, 360.0),
    action: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
  };

  let output =
    window_point_click_output(WindowPointClickOutcome::Delivered { click }.into_result(), crate::commands::overlay::OverlayStatus::Disabled)
      .expect("click result should serialize");
  let result = output.result().expect("click should have a result");

  assert_eq!(result["window"]["reference"]["id"], "window-1");
  assert_eq!(result["point"]["x"], 640.0);
  assert_eq!(result["point"]["y"], 360.0);
  assert_eq!(result["action"]["selected_path"], "window_targeted_mouse");
  let report = output.report.as_ref().expect("window point click report");
  assert_eq!(field_value(report, "Delivery"), "delivered");
  assert_eq!(field_value(report, "Verification"), "delivery_only");
}

#[test]
fn window_point_dry_run_reports_validation_without_delivery() {
  let output = window_point_click_output(
    WindowPointClickOutcome::Validated {
      window: test_window(),
      point: auv_driver::geometry::WindowPoint::new(640.0, 360.0),
    }
    .into_result(),
    crate::commands::overlay::OverlayStatus::Disabled,
  )
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
