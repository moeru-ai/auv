#[swift_bridge::bridge]
pub(crate) mod ffi {
  // swift-bridge 0.1.59 has not proven Vec<transparent struct> reliable for
  // candidate lists in this repo. Use split vectors or an explicit Rust decode
  // layer for repeated structured records until a newer spike proves otherwise.

  enum NativePermissionStatus {
    Granted,
    Missing,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativePermissionProbeResponse {
    screen_recording: NativePermissionStatus,
    accessibility: NativePermissionStatus,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeDisplayListResponse {
    captured_at: String,
    ids: Vec<i64>,
    main_flags: Vec<bool>,
    built_in_flags: Vec<bool>,
    bounds_x_values: Vec<i64>,
    bounds_y_values: Vec<i64>,
    bounds_width_values: Vec<i64>,
    bounds_height_values: Vec<i64>,
    visible_x_values: Vec<i64>,
    visible_y_values: Vec<i64>,
    visible_width_values: Vec<i64>,
    visible_height_values: Vec<i64>,
    scale_factors: Vec<f64>,
    pixel_width_values: Vec<i64>,
    pixel_height_values: Vec<i64>,
    error_message: Option<String>,
    recovery_hint: Option<String>,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeWindowListRequest {
    limit: i64,
    app_filter: String,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeWindowListResponse {
    observed_at: String,
    frontmost_app_name: String,
    frontmost_app_bundle_id: String,
    frontmost_window_title: String,
    app_names: Vec<String>,
    owner_pids: Vec<i64>,
    owner_bundle_ids: Vec<String>,
    window_numbers: Vec<i64>,
    layers: Vec<i64>,
    titles: Vec<String>,
    x_values: Vec<i64>,
    y_values: Vec<i64>,
    width_values: Vec<i64>,
    height_values: Vec<i64>,
    error_message: Option<String>,
    recovery_hint: Option<String>,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeBundleIdsByPidRequest {
    pids: Vec<i64>,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeBundleIdsByPidResponse {
    pids: Vec<i64>,
    bundle_ids: Vec<String>,
    error_message: Option<String>,
    recovery_hint: Option<String>,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeAxTreeRequest {
    app: String,
    max_depth: i64,
    max_children: i64,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeAxTreeResponse {
    observed_at: String,
    app_name: String,
    bundle_id: String,
    pid: i64,
    window_title: String,
    root_role: String,
    depths: Vec<i64>,
    paths: Vec<String>,
    roles: Vec<String>,
    subroles: Vec<String>,
    titles: Vec<String>,
    descriptions: Vec<String>,
    helps: Vec<String>,
    identifiers: Vec<String>,
    placeholders: Vec<String>,
    values: Vec<String>,
    x_values: Vec<i64>,
    y_values: Vec<i64>,
    width_values: Vec<i64>,
    height_values: Vec<i64>,
    error_message: Option<String>,
    recovery_hint: Option<String>,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeAxActionRequest {
    pid: i64,
    path: String,
    expected_role: String,
    action_name: String,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeAxActionResponse {
    performed_action: String,
    available_actions: String,
    role: String,
    subrole: String,
    title: String,
    description: String,
    identifier: String,
    error_message: Option<String>,
    recovery_hint: Option<String>,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeOcrTextRequest {
    image_path: String,
    query: String,
    exact: bool,
    case_sensitive: bool,
    max_observations: i64,
    crop_enabled: bool,
    crop_x: i64,
    crop_y: i64,
    crop_width: i64,
    crop_height: i64,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeOcrTextResponse {
    recognized_at: String,
    image_path: String,
    image_width: i64,
    image_height: i64,
    query: String,
    exact: bool,
    case_sensitive: bool,
    normalized_query: String,
    crop_enabled: bool,
    crop_x: i64,
    crop_y: i64,
    crop_width: i64,
    crop_height: i64,
    ocr_scale_factor: f64,
    match_indices: Vec<i64>,
    texts: Vec<String>,
    confidences: Vec<f64>,
    x_values: Vec<i64>,
    y_values: Vec<i64>,
    width_values: Vec<i64>,
    height_values: Vec<i64>,
    error_message: Option<String>,
    recovery_hint: Option<String>,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeVisualRowsRequest {
    image_path: String,
    crop_enabled: bool,
    crop_x: i64,
    crop_y: i64,
    crop_width: i64,
    crop_height: i64,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeVisualRowsResponse {
    detected_at: String,
    image_path: String,
    image_width: i64,
    image_height: i64,
    crop_enabled: bool,
    crop_x: i64,
    crop_y: i64,
    crop_width: i64,
    crop_height: i64,
    analysis_strip_x: i64,
    analysis_strip_y: i64,
    analysis_strip_width: i64,
    analysis_strip_height: i64,
    row_indices: Vec<i64>,
    x_values: Vec<i64>,
    y_values: Vec<i64>,
    width_values: Vec<i64>,
    height_values: Vec<i64>,
    peak_densities: Vec<f64>,
    error_message: Option<String>,
    recovery_hint: Option<String>,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeActionResponse {
    ok: bool,
    error_message: Option<String>,
    recovery_hint: Option<String>,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeClipboardSnapshotResponse {
    payload: Option<String>,
    error_message: Option<String>,
    recovery_hint: Option<String>,
  }

  extern "Swift" {
    type NativeOverlayController;

    fn probe_permissions() -> NativePermissionProbeResponse;
    fn list_displays() -> NativeDisplayListResponse;
    fn list_windows(request: NativeWindowListRequest) -> NativeWindowListResponse;
    fn bundle_ids_by_pid(request: NativeBundleIdsByPidRequest) -> NativeBundleIdsByPidResponse;
    fn capture_ax_tree(request: NativeAxTreeRequest) -> NativeAxTreeResponse;
    fn perform_ax_action(request: NativeAxActionRequest) -> NativeAxActionResponse;
    fn find_ocr_text(request: NativeOcrTextRequest) -> NativeOcrTextResponse;
    fn find_visual_rows(request: NativeVisualRowsRequest) -> NativeVisualRowsResponse;
    fn click_point(
      x: f64,
      y: f64,
      button_code: i32,
      click_count: i64,
      click_interval_ms: u64,
    ) -> NativeActionResponse;
    fn scroll_point(x: f64, y: f64, delta_x: f64, delta_y: f64) -> NativeActionResponse;
    fn capture_clipboard() -> NativeClipboardSnapshotResponse;
    fn restore_clipboard(snapshot_payload: String) -> NativeActionResponse;
    fn set_clipboard_text(text: String) -> NativeActionResponse;
    fn make_overlay_controller() -> NativeOverlayController;
    fn show_overlay_cursor(
      self: &NativeOverlayController,
      x: f64,
      y: f64,
      label: String,
    ) -> NativeActionResponse;
    fn hide_overlay_cursor(self: &NativeOverlayController) -> NativeActionResponse;
    fn shutdown_overlay_cursor(self: &NativeOverlayController) -> NativeActionResponse;
    fn pump_overlay_events(duration_ms: u64) -> NativeActionResponse;
  }
}
