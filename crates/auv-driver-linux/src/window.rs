#[cfg(target_os = "linux")]
use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
use crate::atspi;
#[cfg(target_os = "linux")]
use crate::capture::capture_display;
#[cfg(target_os = "linux")]
use crate::driver::LinuxDriverSessionState;
use crate::error::{invalid_input, not_found};
use auv_driver_common::capture::Capture;
use auv_driver_common::error::DriverResult;
#[cfg(any(target_os = "linux", test))]
use auv_driver_common::geometry::Rect;
use auv_driver_common::selector::{AppSelector, TextMatcher, WindowSelector};
use auv_driver_common::window::Window;

#[cfg(target_os = "linux")]
pub fn list_windows() -> DriverResult<Vec<Window>> {
  atspi::list_windows()
}

#[cfg(not(target_os = "linux"))]
pub fn list_windows() -> DriverResult<Vec<Window>> {
  Err(auv_driver_common::error::DriverError::unsupported("window.list"))
}

pub fn resolve_window(selector: &WindowSelector) -> DriverResult<Window> {
  let windows = list_windows()?;
  resolve_from_windows(&windows, selector)
}

#[cfg(target_os = "linux")]
pub fn capture_window(state: &Arc<Mutex<LinuxDriverSessionState>>, window: &Window) -> DriverResult<Capture> {
  atspi::ObjectRef::decode(&window.reference.id)?;
  // TODO(linux-window-source-target-binding): XDG portal WINDOW capture is
  // picker-driven and does not expose a stable mapping to an AT-SPI WindowRef.
  // Re-enable it only when the selected portal source can be proven to identify
  // this exact window; geometry similarity alone is insufficient for input.
  validate_display_crop_fallback(window, "portal WINDOW source is not identity-bound to the requested AT-SPI WindowRef")?;
  let display = capture_display(state, None)?;
  let crop = crop_capture_to_window(&display.capture, window.frame)?;
  Ok(Capture {
    image: crop,
    bounds: window.frame,
    scale_factor: display.capture.scale_factor,
    backend: format!("atspi.extents+{}.crop", display.capture.backend),
    fallback_reason: Some(display_crop_reason(display.capture.fallback_reason)),
  })
}

#[cfg(not(target_os = "linux"))]
pub fn capture_window(
  _state: &std::sync::Arc<std::sync::Mutex<crate::driver::LinuxDriverSessionState>>,
  _window: &Window,
) -> DriverResult<Capture> {
  Err(auv_driver_common::error::DriverError::unsupported("window.capture"))
}

fn resolve_from_windows(windows: &[Window], selector: &WindowSelector) -> DriverResult<Window> {
  let mut matches: Vec<&Window> = windows.iter().filter(|window| matches_window_selector_except_main_visible(window, selector)).collect();

  if selector.main_visible {
    matches.sort_by_key(|window| {
      std::cmp::Reverse((
        !is_desktop_shell_surface(window),
        window.is_main,
        window.title.as_ref().is_some_and(|title| !title.trim().is_empty()),
        (window.frame.size.width * window.frame.size.height).round() as i64,
      ))
    });
    return matches.first().map(|window| (*window).clone()).ok_or_else(|| not_found("main visible window"));
  }

  match matches.as_slice() {
    [window] => Ok((*window).clone()),
    [] => Err(not_found("window selector")),
    _ => Err(invalid_input(format!("window selector was ambiguous: {} windows matched", matches.len()))),
  }
}

fn matches_window_selector_except_main_visible(window: &Window, selector: &WindowSelector) -> bool {
  if !window.is_visible {
    return false;
  }
  if let Some(app) = &selector.app
    && !matches_app_selector(window, app)
  {
    return false;
  }
  if let Some(title) = &selector.title {
    let Some(window_title) = &window.title else {
      return false;
    };
    return matches_text(window_title, title);
  }
  true
}

fn matches_app_selector(window: &Window, selector: &AppSelector) -> bool {
  if selector.frontmost {
    // GNOME Shell can expose its focused stage as the first/main AT-SPI
    // surface even while a normal application window is foreground. Keep all
    // applications eligible here and let main-visible ranking reject desktop
    // shell surfaces before considering AT-SPI's main hint.
    return true;
  }
  if let Some(pid) = selector.process_id
    && window.process_id != Some(pid)
  {
    return false;
  }
  if let Some(bundle) = &selector.bundle {
    let Some(app_bundle_id) = &window.app_bundle_id else {
      return false;
    };
    if !matches_text(app_bundle_id, bundle) {
      return false;
    }
  }
  if let Some(name) = &selector.name {
    let Some(app_name) = &window.app_name else {
      return false;
    };
    if !matches_text(app_name, name) {
      return false;
    }
  }
  true
}

fn is_desktop_shell_surface(window: &Window) -> bool {
  matches!(window.app_name.as_deref(), Some("gnome-shell" | "plasmashell"))
    || matches!(window.app_bundle_id.as_deref(), Some("org.gnome.Shell" | "org.kde.plasmashell"))
}

fn matches_text(value: &str, matcher: &TextMatcher) -> bool {
  match matcher {
    TextMatcher::Exact(expected) => value == expected,
    TextMatcher::Contains(needle) => value.to_lowercase().contains(&needle.to_lowercase()),
  }
}

#[cfg(any(target_os = "linux", test))]
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

#[cfg(target_os = "linux")]
fn display_crop_reason(display_fallback_reason: Option<String>) -> String {
  match display_fallback_reason {
    Some(reason) => format!("{reason}; window pixels were cropped from display capture using AT-SPI screen extents"),
    None => "window pixels were cropped from display capture using AT-SPI screen extents".to_string(),
  }
}

#[cfg(target_os = "linux")]
fn validate_display_crop_fallback(window: &Window, window_source_error: &str) -> DriverResult<()> {
  let windows = list_windows()?;
  if let Some(other) = windows.iter().find(|other| {
    other.reference.id != window.reference.id
      && other.is_visible
      && same_point(other.frame.origin.x, window.frame.origin.x)
      && same_point(other.frame.origin.y, window.frame.origin.y)
  }) {
    return Err(invalid_input(format!(
      "xdg-desktop-portal.screencast WINDOW source failed ({window_source_error}); display crop fallback is unsafe because target window {:?} shares AT-SPI origin {:?} with visible window {:?}",
      window.title, window.frame.origin, other.title
    )));
  }
  Ok(())
}

#[cfg(target_os = "linux")]
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
