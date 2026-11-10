#[swift_bridge::bridge]
pub(crate) mod ffi {
  #[swift_bridge(swift_repr = "struct")]
  struct NativeWindowCaptureRequest {
    window_id: i64,
    /// One pixel per point instead of the backing scale.
    logical: bool,
  }

  #[swift_bridge(swift_repr = "struct")]
  struct NativeImageCaptureResponse {
    image_width: i64,
    image_height: i64,
    // Window frame in points that ScreenCaptureKit captured (`SCWindow.frame`).
    // Display captures leave these zero; DisplayApi supplies their bounds.
    window_x: f64,
    window_y: f64,
    window_width: f64,
    window_height: f64,
    rgba_bytes: Vec<u8>,
    error_message: Option<String>,
    recovery_hint: Option<String>,
  }
  extern "Swift" {
    fn capture_window_image(request: NativeWindowCaptureRequest) -> NativeImageCaptureResponse;
    fn capture_display_without_self(display_id: u32) -> NativeImageCaptureResponse;
  }
  extern "Rust" {
    fn capture_byte_vec_from_raw(bytes: *const u8, len: usize) -> Vec<u8>;
  }
}

// Each generated bridge needs its own C symbol; both use the canonical copy.
fn capture_byte_vec_from_raw(bytes: *const u8, len: usize) -> Vec<u8> {
  // SAFETY: Swift's withUnsafeBufferPointer keeps the source live through this call.
  unsafe { auv_driver_capture_common::macos::byte_vec_from_raw(bytes, len) }
}
