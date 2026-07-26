use super::*;

fn base_response() -> DecodedAxTreeResponse {
  DecodedAxTreeResponse {
    observed_at: "2026-05-20T00:00:00Z".to_string(),
    app_name: "Notes".to_string(),
    bundle_id: "com.apple.Notes".to_string(),
    pid: 123,
    window_title: "Todo".to_string(),
    root_role: "AXWindow".to_string(),
    depths: vec![0],
    paths: vec!["0".to_string()],
    roles: vec!["AXStaticText".to_string()],
    subroles: vec!["".to_string()],
    titles: vec!["Title".to_string()],
    descriptions: vec!["Description".to_string()],
    helps: vec!["".to_string()],
    identifiers: vec!["".to_string()],
    placeholders: vec!["".to_string()],
    values: vec!["Value".to_string()],
    focused_values: vec![false],
    x_values: vec![10],
    y_values: vec![20],
    width_values: vec![100],
    height_values: vec![30],
    error_message: None,
    recovery_hint: None,
  }
}

#[test]
fn decode_ax_tree_rejects_mismatched_node_vectors() {
  let mut response = base_response();
  response.paths.clear();

  let error = decode_ax_tree_response(response).unwrap_err();

  assert!(error.contains("mismatched AX node vector lengths"));
}

#[test]
fn decode_ax_tree_preserves_text_priority_fields() {
  let capture = decode_ax_tree_response(base_response()).unwrap();

  assert_eq!(capture.snapshot.nodes[0].value, "Value");
  assert_eq!(capture.snapshot.nodes[0].title, "Title");
  assert_eq!(capture.snapshot.pid, 123);
}

#[test]
fn decode_ax_action_rejects_native_error() {
  let error = decode_ax_action_response(DecodedAxActionResponse {
    performed_action: "".to_string(),
    available_actions: "".to_string(),
    error_message: Some("missing action".to_string()),
    recovery_hint: Some("try another node".to_string()),
  })
  .unwrap_err();

  assert!(error.contains("perform_ax_action failed"));
  assert!(error.contains("missing action"));
}

fn base_focus_response() -> DecodedAxFocusResponse {
  DecodedAxFocusResponse {
    set_attribute: "AXFocused".to_string(),
    was_already_focused: false,
    role: "AXTextArea".to_string(),
    subrole: "".to_string(),
    title: "".to_string(),
    description: "Note Body Text View".to_string(),
    identifier: "".to_string(),
    placeholder: "".to_string(),
    x: 10,
    y: 20,
    width: 300,
    height: 200,
    error_message: None,
    recovery_hint: None,
  }
}

#[test]
fn decode_ax_focus_passes_through_successful_set() {
  let focus = decode_ax_focus_response(base_focus_response()).unwrap();

  assert_eq!(focus.set_attribute, "AXFocused");
  assert!(!focus.was_already_focused);
  assert_eq!(focus.role, "AXTextArea");
  assert_eq!(focus.bounds.width, 300);
}

#[test]
fn decode_ax_focus_preserves_already_focused_signal() {
  let mut response = base_focus_response();
  response.was_already_focused = true;
  let focus = decode_ax_focus_response(response).unwrap();

  assert!(focus.was_already_focused);
  assert_eq!(focus.set_attribute, "AXFocused");
}

fn base_inspection_response() -> DecodedAxNodeInspectionResponse {
  DecodedAxNodeInspectionResponse {
    role: "AXToolbar".to_string(),
    available_actions: vec![],
    available_attributes: vec!["AXRole".to_string(), "AXChildren".to_string()],
    children_count: 0,
    visible_children_count: 2,
    contents_count: 0,
    navigation_children_count: 0,
    error_message: None,
    recovery_hint: None,
  }
}

#[test]
fn decode_ax_node_inspection_reports_attribute_specific_child_counts() {
  let inspection = decode_ax_node_inspection_response("0.1".to_string(), base_inspection_response()).unwrap();

  assert_eq!(inspection.path, "0.1");
  assert_eq!(inspection.children_count, 0);
  assert_eq!(inspection.visible_children_count, 2);
}

#[test]
fn decode_ax_node_inspection_rejects_native_error() {
  let mut response = base_inspection_response();
  response.error_message = Some("AXUIElementCopyAttributeNames returned -25200".to_string());
  response.recovery_hint = Some("verify the AX path still resolves".to_string());

  let error = decode_ax_node_inspection_response("0.1".to_string(), response).unwrap_err();

  assert!(error.contains("inspect_ax_node failed"));
  assert!(error.contains("AXUIElementCopyAttributeNames returned -25200"));
}

#[test]
fn decode_ax_focus_rejects_native_error_as_backend_and_preserves_detail() {
  // An unrecognized AX error code falls through to Backend; the message and
  // recovery hint are still preserved (folded into the Backend message).
  let mut response = base_focus_response();
  response.error_message = Some("AXUIElementSetAttributeValue returned -25204".to_string());
  response.recovery_hint = Some("element may not accept programmatic focus".to_string());

  let error = decode_ax_focus_response(response).unwrap_err();

  assert!(matches!(error, DriverError::Backend { .. }), "expected Backend, got {error:?}");
  let rendered = error.to_string();
  assert!(rendered.contains("AXUIElementSetAttributeValue returned -25204"), "message lost: {rendered}");
  assert!(rendered.contains("element may not accept programmatic focus"), "recovery lost: {rendered}");
}

// ROOT CAUSE:
//
// Before PR 5, `decode_ax_focus_response` flattened every native AX failure
// into one `Result<_, String>` via `native_result`, so callers could not tell
// a malformed path from a shifted tree from a role mismatch without parsing
// message text. This slice classifies the native message (the observed-path
// contract locked in the AX path characterization) into typed `DriverError`
// variants at the decode boundary. These tests pin that mapping.
fn focus_error(message: &str) -> DriverError {
  let mut response = base_focus_response();
  response.error_message = Some(message.to_string());
  response.recovery_hint = Some("capture a fresh AX tree and retry the focus request".to_string());
  decode_ax_focus_response(response).unwrap_err()
}

#[test]
fn classify_malformed_path_as_invalid_input() {
  assert!(matches!(focus_error("AX focus path must begin with 0; got 1.2"), DriverError::InvalidInput { .. }));
  assert!(matches!(focus_error("AX focus path segment x at offset 0 is not a non-negative integer"), DriverError::InvalidInput { .. }));
}

#[test]
fn classify_out_of_range_path_as_stale_observation() {
  let error = focus_error("AX focus path index 3 is out of range at offset 1; element has 2 child(ren)");
  assert!(matches!(error, DriverError::StaleObservation { .. }), "got {error:?}");
  // recovery hint is preserved in the dedicated field (surfaces via Display)
  assert!(error.to_string().contains("capture a fresh AX tree"));
}

#[test]
fn classify_role_mismatch_distinctly_from_stale() {
  let error = focus_error("AX focus expected role AXTextField at path 0.1, got AXButton");
  assert!(matches!(error, DriverError::RoleMismatch { .. }), "got {error:?}");
}

#[test]
fn classify_permission_denied() {
  let error = focus_error("Accessibility permission is required to focus an AX node");
  match error {
    DriverError::PermissionDenied {
      permission,
      message,
      recovery,
    } => {
      assert_eq!(permission, "accessibility");
      assert_eq!(message.as_deref(), Some("Accessibility permission is required to focus an AX node"));
      assert_eq!(recovery.as_deref(), Some("capture a fresh AX tree and retry the focus request"));
    }
    other => panic!("expected PermissionDenied, got {other:?}"),
  }
}
