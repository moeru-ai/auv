#[cfg(target_os = "macos")]
use super::binding::ffi::{NativeWindowCaptureRequest, NativeWindowCaptureResponse, capture_window_image, window_ax_size};
use super::types::AuvResult;

#[derive(Clone, Debug, PartialEq)]
pub struct NativeWindowCapture {
  pub image_width: i64,
  pub image_height: i64,
  /// Window frame in points at capture time, as ScreenCaptureKit saw it.
  pub window_frame: auv_driver_common::Rect,
  pub rgba_bytes: Vec<u8>,
}

#[cfg(target_os = "macos")]
/// Captures a window at its backing resolution, or at one pixel per point
/// when `logical`.
pub fn capture_window_rgba(window_id: i64, logical: bool) -> AuvResult<NativeWindowCapture> {
  decode_window_capture_response(capture_window_image(NativeWindowCaptureRequest { window_id, logical }))
}

#[cfg(not(target_os = "macos"))]
pub fn capture_window_rgba(_window_id: i64, _logical: bool) -> AuvResult<NativeWindowCapture> {
  Err("macOS native window capture is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
fn decode_window_capture_response(response: NativeWindowCaptureResponse) -> AuvResult<NativeWindowCapture> {
  if response.error_message.is_some() {
    return super::error::native_result("capture_window_image", None, response.error_message, response.recovery_hint);
  }
  let expected_len = response
    .image_width
    .checked_mul(response.image_height)
    .and_then(|pixels| pixels.checked_mul(4))
    .ok_or_else(|| "native window capture dimensions overflowed".to_string())?;
  if expected_len < 0 || response.rgba_bytes.len() != expected_len as usize {
    return Err(format!(
      "native window capture returned {} RGBA bytes for {}x{} image; expected {}",
      response.rgba_bytes.len(),
      response.image_width,
      response.image_height,
      expected_len
    ));
  }
  Ok(NativeWindowCapture {
    image_width: response.image_width,
    image_height: response.image_height,
    window_frame: auv_driver_common::Rect::new(response.window_x, response.window_y, response.window_width, response.window_height),
    rgba_bytes: response.rgba_bytes,
  })
}

/// A window's size and minimized state as its application reports them over
/// Accessibility.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowAxSize {
  pub size: auv_driver_common::Size,
  pub minimized: bool,
}

/// `None` when the window has no AX element or Accessibility is not granted.
#[cfg(target_os = "macos")]
pub fn window_ax_size_for(pid: u32, window_number: i64) -> Option<WindowAxSize> {
  let response = window_ax_size(i64::from(pid), window_number);
  response.found.then(|| WindowAxSize {
    size: auv_driver_common::Size::new(response.width, response.height),
    minimized: response.minimized,
  })
}
