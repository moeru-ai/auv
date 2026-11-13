#[cfg(target_os = "linux")]
use crate::atspi;
#[cfg(target_os = "linux")]
use crate::capture::capture_display;
use crate::error::{invalid_input, not_found};
use auv_driver_capture_common::Capture;
use auv_driver_common::error::DriverResult;
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
pub fn capture_window(
  state: &std::sync::Arc<std::sync::Mutex<crate::driver::LinuxDriverSessionState>>,
  window: &Window,
) -> DriverResult<Capture> {
  atspi::ObjectRef::decode(&window.reference.id)?;
  auv_driver_capture::linux::window::capture_window(window, list_windows, || capture_display(state, None))
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
    return matches.first().map(|window| (*window).clone()).ok_or_else(|| missing_window(windows, selector, "main visible window"));
  }

  match matches.as_slice() {
    [window] => Ok((*window).clone()),
    [] => Err(missing_window(windows, selector, "window selector")),
    _ => Err(invalid_input(format!("window selector was ambiguous: {} windows matched", matches.len()))),
  }
}

/// Names the visible applications when an application selector matched no
/// window, so the caller sees which ids it could use.
fn missing_window(windows: &[Window], selector: &WindowSelector, target: &str) -> auv_driver_common::error::DriverError {
  match &selector.app {
    Some(app) if !app.frontmost && (app.id.is_some() || app.name.is_some()) => {
      let owned = windows.iter().any(|window| window.is_visible && matches_app_selector(window, app));
      if owned {
        not_found(target)
      } else {
        // The desktop shell's own surfaces are never resolved for an app target.
        let applications = windows.iter().filter(|window| !is_desktop_shell_surface(window)).cloned().collect::<Vec<_>>();
        auv_driver_common::window::application_not_found(app, &applications)
      }
    }
    _ => not_found(target),
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
  if let Some(matcher) = &selector.id {
    let Some(application_id) = &window.application_id else {
      return false;
    };
    if !matches_text(application_id, matcher) {
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
    || matches!(window.application_id.as_deref(), Some("org.gnome.Shell" | "org.kde.plasmashell"))
}

fn matches_text(value: &str, matcher: &TextMatcher) -> bool {
  match matcher {
    TextMatcher::Exact(expected) => value == expected,
    TextMatcher::Contains(needle) => value.to_lowercase().contains(&needle.to_lowercase()),
  }
}

#[cfg(test)]
#[path = "window_test.rs"]
mod tests;
