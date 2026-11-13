//! Identity checks and display-crop binding for existing Linux window capture.
use auv_driver_capture_common::{Capture, DisplayCapture};
use auv_driver_common::error::{invalid_input, not_found};
use auv_driver_common::{DriverResult, Rect, Window, WindowRef};

pub fn capture_window(
  window: &Window,
  list_windows: impl Fn() -> DriverResult<Vec<Window>>,
  capture_display: impl FnOnce() -> DriverResult<DisplayCapture>,
) -> DriverResult<Capture> {
  // TODO(linux-window-source-target-binding): XDG portal WINDOW capture is
  // picker-driven and does not expose a stable mapping to an AT-SPI WindowRef.
  // Re-enable it only when the selected portal source can be proven to identify
  // this exact window; geometry similarity alone is insufficient for input.
  let windows = list_windows()?;
  let window = resolve_capture_target(&windows, &window.reference, None)?;
  let display = capture_display()?;
  // Portal consent can outlive the target or its geometry. Recheck the exact
  // reference before assigning window coordinates to the captured pixels.
  let windows_after = list_windows()?;
  let window = resolve_capture_target(&windows_after, &window.reference, Some(window.frame))?;
  // TODO(linux-window-frame-time-binding): before/after snapshots cannot detect
  // motion that returns to the same frame or occlusion. Defer atomic binding
  // until an owner-approved identity-bound capture source is available.
  let crop = crop_capture_to_window(&display.capture, window.frame)?;
  Ok(Capture {
    origin: Some(auv_driver_common::Position::in_window(&window.reference, auv_driver_common::WindowPoint::new(0.0, 0.0))),
    image: crop,
    bounds: window.frame,
    scale_factor: display.capture.scale_factor,
    backend: format!("atspi.extents+{}.crop", display.capture.backend),
    fallback_reason: Some(display_crop_reason(display.capture.fallback_reason)),
  })
}

fn crop_capture_to_window(capture: &Capture, frame: Rect) -> DriverResult<image::RgbaImage> {
  let scale_x = f64::from(capture.image.width()) / capture.bounds.size.width;
  let scale_y = f64::from(capture.image.height()) / capture.bounds.size.height;
  let local_x = scaled_capture_dimension("x", frame.origin.x - capture.bounds.origin.x, scale_x)?;
  let local_y = scaled_capture_dimension("y", frame.origin.y - capture.bounds.origin.y, scale_y)?;
  let width = scaled_positive_capture_dimension("width", frame.size.width, scale_x)?;
  let height = scaled_positive_capture_dimension("height", frame.size.height, scale_y)?;
  if local_x + width > capture.image.width() || local_y + height > capture.image.height() {
    return Err(invalid_input(format!("AT-SPI window frame {:?} exceeds display capture bounds {:?}", frame, capture.bounds)));
  }
  Ok(image::imageops::crop_imm(&capture.image, local_x, local_y, width, height).to_image())
}

fn display_crop_reason(display_fallback_reason: Option<String>) -> String {
  match display_fallback_reason {
    Some(reason) => format!("{reason}; window pixels were cropped from display capture using AT-SPI screen extents"),
    None => "window pixels were cropped from display capture using AT-SPI screen extents".to_string(),
  }
}

fn resolve_capture_target<'a>(windows: &'a [Window], reference: &WindowRef, expected_frame: Option<Rect>) -> DriverResult<&'a Window> {
  let window = windows
    .iter()
    .find(|window| window.reference == *reference && window.is_visible)
    .ok_or_else(|| not_found(format!("visible AT-SPI window {}", reference.id)))?;
  if let Some(expected_frame) = expected_frame
    && window.frame != expected_frame
  {
    return Err(invalid_input(format!("AT-SPI window {} changed frame during display capture; retry capture", reference.id)));
  }
  if let Some(other) = windows.iter().find(|other| {
    other.reference.id != window.reference.id
      && other.is_visible
      && same_point(other.frame.origin.x, window.frame.origin.x)
      && same_point(other.frame.origin.y, window.frame.origin.y)
  }) {
    return Err(invalid_input(format!(
      "display crop fallback is unsafe because target window {:?} shares AT-SPI origin {:?} with visible window {:?}",
      window.title, window.frame.origin, other.title
    )));
  }
  Ok(window)
}

fn same_point(left: f64, right: f64) -> bool {
  (left - right).abs() <= 0.5
}

fn scaled_capture_dimension(name: &str, value: f64, scale: f64) -> DriverResult<u32> {
  let value = (value * scale).round();
  if !(0.0..=f64::from(u32::MAX)).contains(&value) {
    return Err(invalid_input(format!("window {name} must be within u32 capture bounds")));
  }
  Ok(value as u32)
}

fn scaled_positive_capture_dimension(name: &str, value: f64, scale: f64) -> DriverResult<u32> {
  let value = scaled_capture_dimension(name, value, scale)?;
  if value == 0 {
    return Err(invalid_input(format!("window {name} must be positive")));
  }
  Ok(value)
}

#[cfg(test)]
#[path = "window_test.rs"]
mod tests;
