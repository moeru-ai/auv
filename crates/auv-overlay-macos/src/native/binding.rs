#[swift_bridge::bridge]
pub(crate) mod ffi {
  #[swift_bridge(swift_repr = "struct")]
  struct NativeActionResponse {
    ok: bool,
    error_message: Option<String>,
    recovery_hint: Option<String>,
  }

  extern "Swift" {
    type NativeOverlayController;

    fn make_overlay_controller() -> NativeOverlayController;
    fn show_overlay_cursor(self: &NativeOverlayController, x: f64, y: f64, label: String) -> NativeActionResponse;
    fn show_overlay_dual_cursor(self: &NativeOverlayController, x: f64, y: f64, label: String, user_label: String) -> NativeActionResponse;
    fn set_overlay_cursor(
      self: &NativeOverlayController,
      cursor_id: String,
      x: f64,
      y: f64,
      label: String,
      variant: String,
    ) -> NativeActionResponse;
    fn move_overlay_cursor(
      self: &NativeOverlayController,
      cursor_id: String,
      x: f64,
      y: f64,
      label: String,
      variant: String,
      duration_ms: u64,
    ) -> NativeActionResponse;
    fn move_overlay_dual_cursor(
      self: &NativeOverlayController,
      x: f64,
      y: f64,
      label: String,
      user_label: String,
      duration_ms: u64,
    ) -> NativeActionResponse;
    fn flash_overlay_cursor(self: &NativeOverlayController, x: f64, y: f64, label: String, duration_ms: u64) -> NativeActionResponse;
    fn flash_overlay_cursor_id(
      self: &NativeOverlayController,
      cursor_id: String,
      x: f64,
      y: f64,
      label: String,
      duration_ms: u64,
    ) -> NativeActionResponse;
    fn hide_overlay_cursor_id(self: &NativeOverlayController, cursor_id: String) -> NativeActionResponse;
    fn hide_overlay_cursor(self: &NativeOverlayController) -> NativeActionResponse;
    fn shutdown_overlay_cursor(self: &NativeOverlayController) -> NativeActionResponse;
    fn pump_overlay_events(duration_ms: u64) -> NativeActionResponse;
  }
}
