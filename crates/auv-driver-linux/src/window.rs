use auv_driver::capture::Capture;
use auv_driver::error::DriverResult;
use auv_driver::geometry::{CoordinateSpace, Rect};
use auv_driver::selector::{AppSelector, TextMatcher, WindowSelector};
use auv_driver::window::{Window, WindowRef};

use crate::error::{backend, invalid_input, not_found};

#[cfg(target_os = "linux")]
const WINDOW_CAPTURE_BACKEND: &str = "xcap.linux.window";

#[cfg(target_os = "linux")]
pub fn list_windows() -> DriverResult<Vec<Window>> {
  let windows = xcap::Window::all()
    .map_err(|error| backend(format!("failed to enumerate windows: {error}")))?;
  windows
    .into_iter()
    .enumerate()
    .filter_map(|(index, window)| match observe_window(index, &window) {
      Ok(Some(window)) => Some(Ok(window)),
      Ok(None) => None,
      Err(error) => Some(Err(error)),
    })
    .collect()
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
pub fn capture_window(window: &Window) -> DriverResult<Capture> {
  let native_id = window.reference.id.parse::<u32>().map_err(|_| {
    invalid_input(format!(
      "window reference {:?} is not a valid Linux xcap window id",
      window.reference.id
    ))
  })?;
  let windows = xcap::Window::all()
    .map_err(|error| backend(format!("failed to enumerate windows: {error}")))?;
  let xcap_window = windows
    .into_iter()
    .find(|candidate| candidate.id().ok() == Some(native_id))
    .ok_or_else(|| not_found(format!("window id {native_id}")))?;
  let image = xcap_window
    .capture_image()
    .map_err(|error| backend(format!("failed to capture window: {error}")))?;
  let scale_factor = if window.frame.size.width > 0.0 {
    f64::from(image.width()) / window.frame.size.width
  } else {
    1.0
  };
  let image = image::RgbaImage::from_raw(image.width(), image.height(), image.into_raw())
    .ok_or_else(|| backend("failed to decode captured window RGBA image"))?;
  Ok(Capture {
    image,
    bounds: window.frame,
    scale_factor,
    backend: WINDOW_CAPTURE_BACKEND.to_string(),
    fallback_reason: None,
  })
}

#[cfg(not(target_os = "linux"))]
pub fn capture_window(_window: &Window) -> DriverResult<Capture> {
  Err(auv_driver::error::DriverError::unsupported(
    "window.capture",
  ))
}

#[cfg(target_os = "linux")]
fn observe_window(index: usize, window: &xcap::Window) -> DriverResult<Option<Window>> {
  if window.is_minimized().unwrap_or(false) {
    return Ok(None);
  }
  let id = window
    .id()
    .map_err(|error| backend(format!("failed to read window id: {error}")))?;
  let title = window.title().ok().filter(|title| !title.trim().is_empty());
  let app_name = window
    .app_name()
    .ok()
    .filter(|app_name| !app_name.trim().is_empty());
  let process_id = window.pid().ok();
  let x = window
    .x()
    .map_err(|error| backend(format!("failed to read window x: {error}")))? as f64;
  let y = window
    .y()
    .map_err(|error| backend(format!("failed to read window y: {error}")))? as f64;
  let width = window
    .width()
    .map_err(|error| backend(format!("failed to read window width: {error}")))?
    as f64;
  let height = window
    .height()
    .map_err(|error| backend(format!("failed to read window height: {error}")))?
    as f64;
  if width <= 0.0 || height <= 0.0 {
    return Ok(None);
  }
  Ok(Some(Window {
    reference: WindowRef { id: id.to_string() },
    title,
    app_name,
    // NOTICE: Linux desktop files/application IDs are not reliably exposed by
    // the xcap window snapshot. Keep this None until a portal or compositor
    // source provides a stable app identity.
    app_bundle_id: None,
    process_id,
    frame: Rect::new(x, y, width, height),
    coordinate_space: CoordinateSpace::Screen,
    is_main: index == 0,
    is_visible: true,
  }))
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
    TextMatcher::Contains(needle) => value.contains(needle),
  }
}

#[cfg(test)]
mod tests {
  use auv_driver::selector::Window as SelectWindow;

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
}
