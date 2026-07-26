use super::*;

// TODO(window-management-live-tests): live macOS mutation tests are deferred
// because this slice keeps CI-safe decoder/unit coverage only; add app-backed
// move/resize/minimize/zoom tests when a stable GUI fixture is approved.
fn base_mutation_response() -> DecodedWindowMutationResponse {
  DecodedWindowMutationResponse {
    performed_action: "move_to".to_string(),
    path: "pid=42 window_number=7".to_string(),
    before_x: 10,
    before_y: 20,
    before_width: 800,
    before_height: 600,
    after_x: 30,
    after_y: 40,
    after_width: 800,
    after_height: 600,
    was_minimized: false,
    is_minimized: false,
    error_message: None,
    recovery_hint: None,
  }
}

#[test]
fn list_window_scope_maps_to_native_app_filter_explicitly() {
  assert_eq!(WindowListScope::AllVisible.app_filter(), "");
  assert_eq!(WindowListScope::App("com.example.App".to_string()).app_filter(), "com.example.App");
}

#[test]
fn decode_display_response_rejects_mismatched_vectors() {
  let error = decode_display_response(DecodedDisplayListResponse {
    captured_at: "2026-05-20T00:00:00Z".to_string(),
    ids: vec![1],
    main_flags: vec![],
    built_in_flags: vec![true],
    bounds_x_values: vec![0],
    bounds_y_values: vec![0],
    bounds_width_values: vec![100],
    bounds_height_values: vec![100],
    visible_x_values: vec![0],
    visible_y_values: vec![0],
    visible_width_values: vec![100],
    visible_height_values: vec![100],
    scale_factors: vec![2.0],
    pixel_width_values: vec![200],
    pixel_height_values: vec![200],
    error_message: None,
    recovery_hint: None,
  })
  .unwrap_err();

  assert!(error.contains("mismatched vector lengths"));
}

#[test]
fn decode_window_response_rejects_mismatched_vectors() {
  let error = decode_window_response(DecodedWindowListResponse {
    observed_at: "2026-05-20T00:00:00Z".to_string(),
    frontmost_app_name: "Notes".to_string(),
    frontmost_app_bundle_id: "com.apple.Notes".to_string(),
    frontmost_window_title: "Todo".to_string(),
    app_names: vec!["Notes".to_string()],
    owner_pids: vec![],
    owner_bundle_ids: vec!["com.apple.Notes".to_string()],
    window_numbers: vec![42],
    layers: vec![0],
    titles: vec!["Todo".to_string()],
    x_values: vec![0],
    y_values: vec![0],
    width_values: vec![640],
    height_values: vec![480],
    error_message: None,
    recovery_hint: None,
  })
  .unwrap_err();

  assert!(error.contains("mismatched vector lengths"));
}

#[test]
fn decode_bundle_ids_by_pid_rejects_mismatched_vectors() {
  let error = decode_bundle_ids_by_pid_response(DecodedBundleIdsByPidResponse {
    pids: vec![1],
    bundle_ids: vec![],
    error_message: None,
    recovery_hint: None,
  })
  .unwrap_err();

  assert!(error.contains("mismatched vector lengths"));
}

#[test]
fn decode_window_mutation_response_preserves_bridge_fields() {
  let response = decode_window_mutation_response(base_mutation_response()).unwrap();

  assert_eq!(response.performed_action, "move_to");
  assert_eq!(response.path, "pid=42 window_number=7");
  assert_eq!(response.before_x, 10);
  assert_eq!(response.before_y, 20);
  assert_eq!(response.before_width, 800);
  assert_eq!(response.before_height, 600);
  assert_eq!(response.after_x, 30);
  assert_eq!(response.after_y, 40);
  assert_eq!(response.after_width, 800);
  assert_eq!(response.after_height, 600);
  assert!(!response.was_minimized);
  assert!(!response.is_minimized);
}

#[test]
fn decode_window_mutation_response_maps_native_error() {
  let mut response = base_mutation_response();
  response.error_message = Some("target AX window was not found".to_string());
  response.recovery_hint = Some("refresh the window list and retry".to_string());

  let error = decode_window_mutation_response(response).unwrap_err();

  assert_eq!(error, "macos native mutate_window failed: target AX window was not found; recovery=refresh the window list and retry");
}
