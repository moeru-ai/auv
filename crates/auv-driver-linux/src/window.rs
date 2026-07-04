#[cfg(target_os = "linux")]
use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
use crate::atspi;
#[cfg(target_os = "linux")]
use crate::capture::capture_display;
#[cfg(target_os = "linux")]
use crate::driver::LinuxDriverSessionState;
use crate::error::{invalid_input, not_found};
#[cfg(target_os = "linux")]
use crate::native::portal::capture_window_frame;
use auv_driver::capture::Capture;
use auv_driver::error::DriverResult;
#[cfg(any(target_os = "linux", test))]
use auv_driver::geometry::Rect;
use auv_driver::selector::{AppSelector, TextMatcher, WindowSelector};
use auv_driver::window::Window;

#[cfg(target_os = "linux")]
pub fn list_windows() -> DriverResult<Vec<Window>> {
  atspi::list_windows()
}

#[cfg(not(target_os = "linux"))]
pub fn list_windows() -> DriverResult<Vec<Window>> {
  Err(auv_driver::error::DriverError::unsupported("window.list"))
}

pub fn resolve_window(selector: &WindowSelector) -> DriverResult<Window> {
  let windows = list_windows()?;
  resolve_from_windows(&windows, selector)
}

#[cfg(target_os = "linux")]
pub fn capture_window(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  window: &Window,
) -> DriverResult<Capture> {
  atspi::ObjectRef::decode(&window.reference.id)?;
  match capture_window_from_source(window) {
    Ok(capture) => return Ok(capture),
    Err(error) => {
      validate_display_crop_fallback(window, &error.to_string())?;
      let display = capture_display(state, None)?;
      let crop = crop_capture_to_window(&display.capture, window.frame)?;
      return Ok(Capture {
        image: crop,
        bounds: window.frame,
        scale_factor: display.capture.scale_factor,
        backend: format!("atspi.extents+{}.crop", display.capture.backend),
        fallback_reason: Some(window_capture_fallback_reason(
          &error.to_string(),
          display.capture.fallback_reason,
        )),
      });
    }
  }
}

#[cfg(not(target_os = "linux"))]
pub fn capture_window(
  _state: &std::sync::Arc<std::sync::Mutex<crate::driver::LinuxDriverSessionState>>,
  _window: &Window,
) -> DriverResult<Capture> {
  Err(auv_driver::error::DriverError::unsupported(
    "window.capture",
  ))
}

fn resolve_from_windows(windows: &[Window], selector: &WindowSelector) -> DriverResult<Window> {
  let mut matches: Vec<&Window> = windows
    .iter()
    .filter(|window| matches_window_selector_except_main_visible(window, selector))
    .collect();

  if selector.main_visible {
    matches.sort_by_key(|window| {
      std::cmp::Reverse((
        window.is_main,
        window
          .title
          .as_ref()
          .is_some_and(|title| !title.trim().is_empty()),
        (window.frame.size.width * window.frame.size.height).round() as i64,
      ))
    });
    return matches
      .first()
      .map(|window| (*window).clone())
      .ok_or_else(|| not_found("main visible window"));
  }

  match matches.as_slice() {
    [window] => Ok((*window).clone()),
    [] => Err(not_found("window selector")),
    _ => Err(invalid_input(format!(
      "window selector was ambiguous: {} windows matched",
      matches.len()
    ))),
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
    return window.is_main;
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
    return Err(invalid_input(format!(
      "AT-SPI window frame {:?} exceeds display capture bounds {:?}",
      frame, capture.bounds
    )));
  }
  Ok(image::imageops::crop_imm(&capture.image, local_x, local_y, width, height).to_image())
}

#[cfg(target_os = "linux")]
fn capture_window_from_source(window: &Window) -> DriverResult<Capture> {
  let frame = capture_window_frame()?;
  let (image, scale_factor) = normalize_window_source_image(frame.image, window.frame)?;
  Ok(Capture {
    image,
    bounds: window.frame,
    scale_factor,
    backend: "atspi.extents+xdg-desktop-portal.screencast.window.pipewire".to_string(),
    fallback_reason: Some(
      "portal window source is user-selected; Linux Wayland backend cannot yet verify it matches the AT-SPI window reference".to_string(),
    ),
  })
}

#[cfg(any(target_os = "linux", test))]
fn normalize_window_source_image(
  image: image::RgbaImage,
  bounds: Rect,
) -> DriverResult<(image::RgbaImage, f64)> {
  match window_source_scale_factor(&image, bounds) {
    Ok(scale_factor) => Ok((image, scale_factor)),
    Err(original_error) => {
      let Some(trimmed) = trim_portal_window_padding(&image) else {
        return Err(original_error);
      };
      match window_source_scale_factor(&trimmed, bounds) {
        Ok(scale_factor) => Ok((trimmed, scale_factor)),
        Err(_) => Err(original_error),
      }
    }
  }
}

#[cfg(any(target_os = "linux", test))]
fn window_source_scale_factor(image: &image::RgbaImage, bounds: Rect) -> DriverResult<f64> {
  if bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
    return Err(invalid_input("window bounds must be positive"));
  }
  let scale_x = f64::from(image.width()) / bounds.size.width;
  let scale_y = f64::from(image.height()) / bounds.size.height;
  let ratio = scale_x / scale_y;
  if !(0.8..=1.25).contains(&ratio) {
    return Err(invalid_input(format!(
      "portal WINDOW stream size {}x{} is not consistent with AT-SPI window bounds {:?}",
      image.width(),
      image.height(),
      bounds
    )));
  }
  Ok(scale_x)
}

#[cfg(any(target_os = "linux", test))]
fn trim_portal_window_padding(image: &image::RgbaImage) -> Option<image::RgbaImage> {
  let mut min_x = image.width();
  let mut min_y = image.height();
  let mut max_x = 0;
  let mut max_y = 0;
  for (x, y, pixel) in image.enumerate_pixels() {
    if is_window_source_content(*pixel) {
      min_x = min_x.min(x);
      min_y = min_y.min(y);
      max_x = max_x.max(x);
      max_y = max_y.max(y);
    }
  }
  if min_x > max_x || min_y > max_y {
    return None;
  }
  if min_x == 0 && min_y == 0 && max_x + 1 == image.width() && max_y + 1 == image.height() {
    return None;
  }
  Some(
    image::imageops::crop_imm(image, min_x, min_y, max_x - min_x + 1, max_y - min_y + 1).to_image(),
  )
}

#[cfg(any(target_os = "linux", test))]
fn is_window_source_content(pixel: image::Rgba<u8>) -> bool {
  pixel[3] != 0 && (pixel[0] > 8 || pixel[1] > 8 || pixel[2] > 8)
}

#[cfg(target_os = "linux")]
fn window_capture_fallback_reason(
  window_source_error: &str,
  display_fallback_reason: Option<String>,
) -> String {
  // TODO(linux-window-source-target-binding): XDG portal WINDOW capture is
  // picker-driven, and this slice does not have a compositor-independent way to
  // bind the returned stream to the AT-SPI window ref. Revisit when a portal or
  // compositor exposes stable window mapping metadata we can verify.
  match display_fallback_reason {
    Some(reason) => format!(
      "xdg-desktop-portal.screencast WINDOW source failed ({window_source_error}); {reason}; window pixels were cropped from display capture using AT-SPI window extents"
    ),
    None => format!(
      "xdg-desktop-portal.screencast WINDOW source failed ({window_source_error}); window pixels were cropped from display capture using AT-SPI window extents"
    ),
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
    return Err(invalid_input(format!(
      "window {name} must be within u32 capture bounds"
    )));
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
mod tests {
  use auv_driver::geometry::CoordinateSpace;
  use auv_driver::selector::Window as SelectWindow;
  use auv_driver::window::WindowRef;

  use super::*;

  #[test]
  fn resolve_from_windows_matches_title_contains() {
    let window = Window {
      reference: WindowRef {
        id: "1".to_string(),
      },
      title: Some("GNOME Text Editor".to_string()),
      app_name: Some("Text Editor".to_string()),
      app_bundle_id: None,
      process_id: Some(42),
      frame: Rect::new(0.0, 0.0, 500.0, 400.0),
      coordinate_space: CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    };

    let resolved = resolve_from_windows(
      &[window.clone()],
      &SelectWindow::title_contains("Text Editor"),
    )
    .expect("window resolves");

    assert_eq!(resolved, window);
  }

  #[test]
  fn resolve_from_windows_matches_title_contains_case_insensitive() {
    let window = Window {
      reference: WindowRef {
        id: "1".to_string(),
      },
      title: Some("Settings".to_string()),
      app_name: Some("GNOME Settings".to_string()),
      app_bundle_id: None,
      process_id: Some(42),
      frame: Rect::new(0.0, 0.0, 500.0, 400.0),
      coordinate_space: CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    };

    let resolved =
      resolve_from_windows(&[window.clone()], &SelectWindow::title_contains("settings"))
        .expect("window resolves");

    assert_eq!(resolved, window);
  }

  #[test]
  fn resolve_from_windows_matches_app_name_contains_case_insensitive() {
    let window = Window {
      reference: WindowRef {
        id: "1".to_string(),
      },
      title: Some("Settings".to_string()),
      app_name: Some("GNOME Settings".to_string()),
      app_bundle_id: None,
      process_id: Some(42),
      frame: Rect::new(0.0, 0.0, 500.0, 400.0),
      coordinate_space: CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    };

    let resolved = resolve_from_windows(
      &[window.clone()],
      &WindowSelector::default().owned_by(AppSelector {
        name: Some(TextMatcher::Contains("settings".to_string())),
        ..AppSelector::default()
      }),
    )
    .expect("window resolves");

    assert_eq!(resolved, window);
  }

  #[test]
  fn crop_capture_to_window_uses_window_extents_inside_display_capture() {
    let mut image = image::RgbaImage::new(10, 10);
    image.put_pixel(3, 4, image::Rgba([1, 2, 3, 4]));
    let capture = Capture {
      image,
      bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
      scale_factor: 1.0,
      backend: "test".to_string(),
      fallback_reason: None,
    };

    let cropped = crop_capture_to_window(&capture, Rect::new(3.0, 4.0, 2.0, 2.0)).unwrap();

    assert_eq!(cropped.width(), 2);
    assert_eq!(cropped.height(), 2);
    assert_eq!(*cropped.get_pixel(0, 0), image::Rgba([1, 2, 3, 4]));
  }

  #[test]
  fn window_source_scale_rejects_unmatched_stream_bounds() {
    let image = image::RgbaImage::new(5504, 2304);

    let error = window_source_scale_factor(&image, Rect::new(0.0, 0.0, 1505.0, 1077.0))
      .expect_err("non-uniform scale should be rejected");

    assert!(
      error
        .to_string()
        .contains("not consistent with AT-SPI window bounds")
    );
  }

  #[test]
  fn window_source_normalization_trims_black_portal_padding() {
    let mut image = image::RgbaImage::new(11, 6);
    for y in 1..4 {
      for x in 1..5 {
        image.put_pixel(x, y, image::Rgba([20, 20, 20, 255]));
      }
    }

    let (normalized, scale) = normalize_window_source_image(image, Rect::new(0.0, 0.0, 2.0, 1.5))
      .expect("black padding trims to target aspect");

    assert_eq!(normalized.width(), 4);
    assert_eq!(normalized.height(), 3);
    assert_eq!(scale, 2.0);
  }
}
