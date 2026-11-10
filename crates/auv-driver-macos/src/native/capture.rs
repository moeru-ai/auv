// NOTICE(capture-migration): existing native paths delegate to the canonical
// capture implementation while aggregate callers migrate.
#[cfg(target_os = "macos")]
use super::binding::ffi::window_ax_size;
#[cfg(target_os = "macos")]
pub use auv_driver_capture::macos::native::capture_display_rgba_without_self;
pub use auv_driver_capture::macos::native::{NativeWindowCapture, capture_window_rgba};
pub use auv_driver_capture_common::macos::WindowAxSize;

/// `None` when the window has no AX element or Accessibility is not granted.
#[cfg(target_os = "macos")]
pub fn window_ax_size_for(pid: u32, window_number: i64) -> Option<WindowAxSize> {
  let response = window_ax_size(i64::from(pid), window_number);
  response.found.then(|| WindowAxSize {
    size: auv_driver_common::Size::new(response.width, response.height),
    minimized: response.minimized,
  })
}
