#[swift_bridge::bridge]
pub(crate) mod ffi {
  #[swift_bridge(swift_repr = "struct")]
  struct NativeActionResponse {
    ok: bool,
    error_message: Option<String>,
    recovery_hint: Option<String>,
  }

  // FFI value for an optional native cursor silhouette shadow.
  // NOTICE: swift-bridge-ir 0.1.59 rejects doc attributes on shared structs
  // (src/parse/parse_struct.rs:143). Use a plain comment until it supports them.
  #[swift_bridge(swift_repr = "struct")]
  struct NativeCursorShadow {
    enabled: bool,
    red: f64,
    green: f64,
    blue: f64,
    alpha: f64,
    blur_radius: f64,
    offset_x: f64,
    offset_y: f64,
  }

  extern "Swift" {
    type NativeOverlayController;

    fn make_overlay_controller() -> NativeOverlayController;
    fn move_overlay_cursor(
      self: &NativeOverlayController,
      cursor_id: String,
      x: f64,
      y: f64,
      label: String,
      variant: String,
      duration_ms: u64,
      foreground_red: f64,
      foreground_green: f64,
      foreground_blue: f64,
      foreground_alpha: f64,
      background_red: f64,
      background_green: f64,
      background_blue: f64,
      background_alpha: f64,
      padding_top: f64,
      padding_right: f64,
      padding_bottom: f64,
      padding_left: f64,
      corner_radius: f64,
      sprite_size: f64,
      label_gap: f64,
      shadow: NativeCursorShadow,
    ) -> NativeActionResponse;
    fn move_overlay_cursor_svg(
      self: &NativeOverlayController,
      cursor_id: String,
      x: f64,
      y: f64,
      label: String,
      svg: String,
      duration_ms: u64,
      foreground_red: f64,
      foreground_green: f64,
      foreground_blue: f64,
      foreground_alpha: f64,
      background_red: f64,
      background_green: f64,
      background_blue: f64,
      background_alpha: f64,
      padding_top: f64,
      padding_right: f64,
      padding_bottom: f64,
      padding_left: f64,
      corner_radius: f64,
      sprite_size: f64,
      label_gap: f64,
      shadow: NativeCursorShadow,
    ) -> NativeActionResponse;
    fn show_overlay_outline(
      self: &NativeOverlayController,
      layer_id: String,
      x: f64,
      y: f64,
      width: f64,
      height: f64,
      label: String,
      red: f64,
      green: f64,
      blue: f64,
      alpha: f64,
      border_width: f64,
      corner_radius: f64,
      duration_ms: u64,
    ) -> NativeActionResponse;
    fn show_overlay_status(
      self: &NativeOverlayController,
      layer_id: String,
      x: f64,
      y: f64,
      text: String,
      foreground_red: f64,
      foreground_green: f64,
      foreground_blue: f64,
      foreground_alpha: f64,
      background_red: f64,
      background_green: f64,
      background_blue: f64,
      background_alpha: f64,
      padding_top: f64,
      padding_right: f64,
      padding_bottom: f64,
      padding_left: f64,
      corner_radius: f64,
      duration_ms: u64,
    ) -> NativeActionResponse;
    fn hide_overlay_cursor(self: &NativeOverlayController) -> NativeActionResponse;
    fn shutdown_overlay_cursor(self: &NativeOverlayController) -> NativeActionResponse;
    fn pump_overlay_events(duration_ms: u64) -> NativeActionResponse;
  }
}
