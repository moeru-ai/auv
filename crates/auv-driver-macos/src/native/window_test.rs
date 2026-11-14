use super::*;

// TODO(window-management-live-tests): live macOS mutation tests are deferred
// because this slice keeps CI-safe decoder/unit coverage only; add app-backed
// set_frame/set_state/focus tests when a stable GUI fixture is approved.
fn base_mutation_response() -> DecodedWindowMutationResponse {
  DecodedWindowMutationResponse {
    performed_action: "set AXMinimized=true".to_string(),
    path: "pid=42 window_number=7".to_string(),
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
fn decode_display_response_converts_cocoa_frames_to_global_coordinates() {
  // The owner's layout in Cocoa coordinates: the primary display with a
  // 30 pt menu bar, a display below it, and a portrait display to the right
  // whose Dock takes 46 pt on the right.
  let snapshot = decode_display_response(DecodedDisplayListResponse {
    captured_at: "2026-11-03T00:00:00Z".to_string(),
    ids: vec![1, 2, 3],
    main_flags: vec![true, false, false],
    built_in_flags: vec![false; 3],
    bounds_x_values: vec![0, 0, 3008],
    bounds_y_values: vec![0, -1692, -2210],
    bounds_width_values: vec![3008, 3008, 1692],
    bounds_height_values: vec![1692, 1692, 3008],
    visible_x_values: vec![0, 0, 3008],
    visible_y_values: vec![0, -1692, -2210],
    visible_width_values: vec![3008, 3008, 1646],
    visible_height_values: vec![1662, 1692, 3008],
    scale_factors: vec![2.0; 3],
    pixel_width_values: vec![6016, 6016, 3384],
    pixel_height_values: vec![3384, 3384, 6016],
    error_message: None,
    recovery_hint: None,
  })
  .unwrap();

  let frames = snapshot.displays.iter().map(|display| (display.bounds.clone(), display.visible_bounds.clone())).collect::<Vec<_>>();
  let rect = |x, y, width, height| ObservedRect {
    x,
    y,
    width,
    height,
  };
  assert_eq!(
    frames,
    vec![
      (rect(0, 0, 3008, 1692), rect(0, 30, 3008, 1662)),
      (rect(0, 1692, 3008, 1692), rect(0, 1692, 3008, 1692)),
      (rect(3008, 894, 1692, 3008), rect(3008, 894, 1646, 3008)),
    ]
  );
}

fn window_list_response(windows: usize) -> DecodedWindowListResponse {
  DecodedWindowListResponse {
    observed_at: "2026-05-20T00:00:00Z".to_string(),
    frontmost_app_name: "Notes".to_string(),
    frontmost_app_bundle_id: "com.apple.Notes".to_string(),
    frontmost_window_title: "Todo".to_string(),
    app_names: vec!["Notes".to_string(); windows],
    owner_pids: vec![10; windows],
    owner_bundle_ids: vec!["com.apple.Notes".to_string(); windows],
    window_numbers: (1..=windows as i64).collect(),
    layers: vec![0; windows],
    titles: vec!["Todo".to_string(); windows],
    x_values: vec![0; windows],
    y_values: vec![0; windows],
    width_values: vec![640; windows],
    height_values: vec![480; windows],
    on_screen_flags: vec![true; windows],
    ax_state_flags: vec![false; windows],
    minimized_flags: vec![false; windows],
    fullscreen_flags: vec![false; windows],
    focused_flags: vec![false; windows],
    focus_known: false,
    error_message: None,
    recovery_hint: None,
  }
}

#[test]
fn decode_window_response_rejects_mismatched_vectors() {
  let mut response = window_list_response(1);
  response.owner_pids.clear();

  let error = decode_window_response(response).unwrap_err();

  assert!(error.contains("mismatched vector lengths"));
}

#[test]
fn decode_window_response_leaves_unread_ax_state_unknown() {
  let mut response = window_list_response(2);
  response.on_screen_flags = vec![true, false];
  response.ax_state_flags = vec![false, true];
  response.minimized_flags = vec![false, true];

  let windows = decode_window_response(response).unwrap().windows;

  assert_eq!(
    windows[0].state,
    ObservedWindowState {
      off_screen: false,
      minimized: None,
      fullscreen: None,
      focused: None,
    }
  );
  assert_eq!(
    windows[1].state,
    ObservedWindowState {
      off_screen: true,
      minimized: Some(true),
      fullscreen: Some(false),
      focused: None,
    }
  );
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

  assert_eq!(response.performed_action, "set AXMinimized=true");
  assert_eq!(response.path, "pid=42 window_number=7");
}

#[test]
fn decode_window_mutation_response_maps_native_error() {
  let mut response = base_mutation_response();
  response.error_message = Some("target AX window was not found".to_string());
  response.recovery_hint = Some("refresh the window list and retry".to_string());

  let error = decode_window_mutation_response(response).unwrap_err();

  assert_eq!(error, "macos native mutate_window failed: target AX window was not found; recovery=refresh the window list and retry");
}
