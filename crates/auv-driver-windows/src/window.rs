//! Top-level window enumeration and selector resolution.
//!
//! Mirrors the macOS driver's window listing/resolution surface, producing the
//! shared [`Window`] type so window-targeted consumers stay backend-agnostic.
//! Enumeration uses Win32 `EnumWindows`; frames prefer the DWM extended frame
//! bounds (the visible window rectangle) and fall back to `GetWindowRect`.

use auv_driver_common::error::DriverResult;
use auv_driver_common::selector::{AppSelector, TextMatcher, WindowSelector};
use auv_driver_common::window::Window;

use crate::error::{invalid_input, not_found};

#[cfg(target_os = "windows")]
pub fn list_windows() -> DriverResult<Vec<Window>> {
  native::list_windows()
}

#[cfg(not(target_os = "windows"))]
pub fn list_windows() -> DriverResult<Vec<Window>> {
  Err(auv_driver_common::error::DriverError::unsupported("window.list"))
}

pub fn resolve_window(selector: &WindowSelector) -> DriverResult<Window> {
  let windows = list_windows()?;
  resolve_from_windows(&windows, selector)
}

/// Restores and foregrounds a top-level window for `SendInput` delivery.
#[cfg(target_os = "windows")]
pub fn activate_window(window: &Window) -> DriverResult<()> {
  native::activate_window(window)
}

#[cfg(not(target_os = "windows"))]
pub fn activate_window(_window: &Window) -> DriverResult<()> {
  Err(auv_driver_common::error::DriverError::unsupported("window.activate"))
}

/// Parses the native `HWND` value previously encoded in a [`Window`] reference.
///
/// Window references carry the `HWND` as a decimal `isize` string (see
/// `observe_window`). Mutation and window capture both need to recover the raw
/// handle, so the parse lives here next to the encoding it mirrors.
#[cfg(target_os = "windows")]
pub(crate) fn window_handle(window: &Window) -> DriverResult<windows::Win32::Foundation::HWND> {
  let raw: isize = window
    .reference
    .id
    .parse()
    .map_err(|_| invalid_input(format!("window reference {:?} is not a valid window handle", window.reference.id)))?;
  Ok(windows::Win32::Foundation::HWND(raw as *mut std::ffi::c_void))
}

/// Resolves a single window from an already-observed list.
///
/// Kept independent of the Win32 enumeration so selector semantics are unit
/// testable without a live desktop. For `main_visible` selectors this prefers
/// the foreground window, then a titled window, then the largest frame, which
/// mirrors the macOS resolver's ordering.
fn resolve_from_windows(windows: &[Window], selector: &WindowSelector) -> DriverResult<Window> {
  let mut matches: Vec<&Window> = windows.iter().filter(|window| matches_window_selector_except_main_visible(window, selector)).collect();

  if selector.main_visible {
    matches.sort_by_key(|window| {
      std::cmp::Reverse((
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
    return window.is_main;
  }
  if let Some(pid) = selector.process_id
    && window.process_id != Some(pid)
  {
    return false;
  }
  // NOTICE: app_bundle_id is a macOS concept and is always None on Windows, so
  // a bundle matcher can never match here. Selectors should use name or pid.
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

#[cfg(target_os = "windows")]
mod native {
  use std::ffi::c_void;
  use std::mem::size_of;
  use std::path::Path;

  use auv_driver_common::error::DriverResult;
  use auv_driver_common::geometry::{CoordinateSpace, Rect};
  use auv_driver_common::window::{Window, WindowRef};
  use windows::Win32::Foundation::{BOOL, CloseHandle, FALSE, HWND, LPARAM, RECT, TRUE};
  use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
  use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
  };
  use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible, SW_RESTORE, SetForegroundWindow, ShowWindow,
  };
  use windows::core::PWSTR;

  use crate::error::backend;

  pub(super) fn list_windows() -> DriverResult<Vec<Window>> {
    let mut handles: Vec<HWND> = Vec::new();
    // SAFETY: `enum_proc` only pushes into the Vec referenced by `lparam`, which
    // outlives the synchronous EnumWindows call.
    let enumeration = unsafe { EnumWindows(Some(enum_proc), LPARAM(&mut handles as *mut Vec<HWND> as isize)) };
    if let Err(error) = enumeration
      && error.code().is_err()
    {
      return Err(backend(format!("EnumWindows failed: {error}")));
    }
    // NOTICE(windows-enum-zero-last-error): some interactive desktop sessions
    // return FALSE from EnumWindows while leaving the last-error code at
    // ERROR_SUCCESS, even though the callback populated the complete window
    // list. Treat only a real HRESULT failure as an enumeration error. Remove
    // this tolerance if the Win32 wrapper begins preserving callback-stop
    // state separately from GetLastError.

    let foreground = unsafe { GetForegroundWindow() };
    let mut windows = Vec::new();
    for hwnd in handles {
      if let Some(window) = observe_window(hwnd, foreground)? {
        windows.push(window);
      }
    }
    Ok(windows)
  }

  pub(super) fn activate_window(window: &Window) -> DriverResult<()> {
    let hwnd = super::window_handle(window)?;
    let current_thread = unsafe { GetCurrentThreadId() };
    let foreground = unsafe { GetForegroundWindow() };
    let foreground_thread = if foreground.is_invalid() {
      0
    } else {
      unsafe { GetWindowThreadProcessId(foreground, None) }
    };
    let target_thread = unsafe { GetWindowThreadProcessId(hwnd, None) };
    let attached_foreground = foreground_thread != 0
      && foreground_thread != current_thread
      && unsafe { AttachThreadInput(current_thread, foreground_thread, true) }.as_bool();
    let attached_target = target_thread != 0
      && target_thread != current_thread
      && target_thread != foreground_thread
      && unsafe { AttachThreadInput(current_thread, target_thread, true) }.as_bool();
    unsafe {
      let _ = ShowWindow(hwnd, SW_RESTORE);
      let _ = BringWindowToTop(hwnd);
    }
    let activated = unsafe { SetForegroundWindow(hwnd) }.as_bool() || unsafe { GetForegroundWindow() } == hwnd;
    if attached_target {
      let _ = unsafe { AttachThreadInput(current_thread, target_thread, false) };
    }
    if attached_foreground {
      let _ = unsafe { AttachThreadInput(current_thread, foreground_thread, false) };
    }
    if activated {
      Ok(())
    } else {
      Err(backend("SetForegroundWindow was refused; interact with NetEase Music once and retry"))
    }
  }

  unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam carries the &mut Vec<HWND> created in list_windows.
    let handles = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    handles.push(hwnd);
    TRUE
  }

  fn observe_window(hwnd: HWND, foreground: HWND) -> DriverResult<Option<Window>> {
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
      return Ok(None);
    }
    if is_cloaked(hwnd) {
      return Ok(None);
    }
    let frame = window_frame(hwnd)?;
    // Skip zero-area windows; visible top-level windows include many invisible
    // helper surfaces with degenerate frames that are not useful targets.
    if frame.size.width <= 0.0 || frame.size.height <= 0.0 {
      return Ok(None);
    }

    let mut pid: u32 = 0;
    unsafe {
      GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }

    Ok(Some(Window {
      reference: WindowRef {
        id: (hwnd.0 as isize).to_string(),
      },
      title: window_title(hwnd),
      app_name: process_name(pid),
      // NOTICE: app_bundle_id is macOS-only; Windows has no equivalent.
      app_bundle_id: None,
      process_id: (pid != 0).then_some(pid),
      frame,
      coordinate_space: CoordinateSpace::Screen,
      is_main: hwnd == foreground,
      is_visible: true,
    }))
  }

  fn window_title(hwnd: HWND) -> Option<String> {
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
      return None;
    }
    let mut buffer = vec![0u16; (len + 1) as usize];
    let copied = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    if copied <= 0 {
      return None;
    }
    let text = String::from_utf16_lossy(&buffer[..copied as usize]);
    (!text.is_empty()).then_some(text)
  }

  /// Reads the visible window rectangle, preferring the DWM extended frame
  /// bounds and falling back to `GetWindowRect` when DWM is unavailable.
  fn window_frame(hwnd: HWND) -> DriverResult<Rect> {
    let mut rect = RECT::default();
    let dwm =
      unsafe { DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut RECT as *mut c_void, size_of::<RECT>() as u32) };
    if dwm.is_err() {
      unsafe {
        GetWindowRect(hwnd, &mut rect).map_err(|error| backend(format!("GetWindowRect failed: {error}")))?;
      }
    }
    Ok(Rect::new(f64::from(rect.left), f64::from(rect.top), f64::from(rect.right - rect.left), f64::from(rect.bottom - rect.top)))
  }

  /// Reports whether DWM has cloaked the window (for example a UWP app on a
  /// virtual desktop), which `IsWindowVisible` still reports as visible.
  fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    let result = unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, size_of::<u32>() as u32) };
    result.is_ok() && cloaked != 0
  }

  /// Resolves a process's executable file name for the `app_name` field.
  fn process_name(pid: u32) -> Option<String> {
    if pid == 0 {
      return None;
    }
    unsafe {
      let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid).ok()?;
      let mut buffer = [0u16; 260];
      let mut len = buffer.len() as u32;
      let query = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut len);
      let _ = CloseHandle(handle);
      query.ok()?;
      let path = String::from_utf16_lossy(&buffer[..len as usize]);
      Path::new(&path).file_name().map(|name| name.to_string_lossy().into_owned())
    }
  }
}

#[cfg(test)]
mod tests {
  use auv_driver_common::geometry::{CoordinateSpace, Rect};
  use auv_driver_common::selector::{App, TextMatcher, Window as WindowQuery, WindowSelector};
  use auv_driver_common::window::{Window, WindowRef};

  use super::*;

  fn window(id: &str, title: Option<&str>, app: Option<&str>, pid: u32, frame: Rect) -> Window {
    Window {
      reference: WindowRef { id: id.to_string() },
      title: title.map(str::to_string),
      app_name: app.map(str::to_string),
      app_bundle_id: None,
      process_id: (pid != 0).then_some(pid),
      frame,
      coordinate_space: CoordinateSpace::Screen,
      is_main: false,
      is_visible: true,
    }
  }

  #[test]
  fn resolve_returns_single_title_match() {
    let windows = vec![
      window("1", Some("Editor"), Some("app.exe"), 10, Rect::new(0.0, 0.0, 100.0, 100.0)),
      window("2", Some("Browser"), Some("web.exe"), 20, Rect::new(0.0, 0.0, 100.0, 100.0)),
    ];

    let selector = WindowQuery::title_contains("Brow");
    let resolved = resolve_from_windows(&windows, &selector).expect("one window matches");

    assert_eq!(resolved.reference.id, "2");
  }

  #[test]
  fn resolve_reports_ambiguous_match() {
    let windows = vec![
      window("1", Some("Doc"), Some("app.exe"), 10, Rect::new(0.0, 0.0, 100.0, 100.0)),
      window("2", Some("Doc"), Some("app.exe"), 11, Rect::new(0.0, 0.0, 100.0, 100.0)),
    ];

    let selector = WindowQuery::title_exact("Doc");

    assert!(resolve_from_windows(&windows, &selector).is_err());
  }

  #[test]
  fn resolve_reports_not_found_when_nothing_matches() {
    let windows = vec![window(
      "1",
      Some("Doc"),
      Some("app.exe"),
      10,
      Rect::new(0.0, 0.0, 100.0, 100.0),
    )];

    let selector = WindowQuery::title_exact("Missing");

    assert!(resolve_from_windows(&windows, &selector).is_err());
  }

  #[test]
  fn main_visible_prefers_foreground_then_largest() {
    let mut foreground = window("fg", Some("Front"), Some("app.exe"), 10, Rect::new(0.0, 0.0, 50.0, 50.0));
    foreground.is_main = true;
    let big = window("big", Some("Big"), Some("app.exe"), 11, Rect::new(0.0, 0.0, 800.0, 600.0));
    let windows = vec![big, foreground];

    let resolved = resolve_from_windows(&windows, &WindowQuery::main_visible()).expect("a main visible window resolves");

    assert_eq!(resolved.reference.id, "fg");
  }

  #[test]
  fn main_visible_falls_back_to_largest_without_foreground() {
    let windows = vec![
      window("small", Some("Small"), Some("app.exe"), 10, Rect::new(0.0, 0.0, 50.0, 50.0)),
      window("big", Some("Big"), Some("app.exe"), 11, Rect::new(0.0, 0.0, 800.0, 600.0)),
    ];

    let resolved = resolve_from_windows(&windows, &WindowQuery::main_visible()).expect("largest window resolves");

    assert_eq!(resolved.reference.id, "big");
  }

  #[test]
  fn app_selector_matches_by_name_and_pid() {
    let windows = vec![
      window("1", Some("A"), Some("editor.exe"), 10, Rect::new(0.0, 0.0, 100.0, 100.0)),
      window("2", Some("B"), Some("browser.exe"), 20, Rect::new(0.0, 0.0, 100.0, 100.0)),
    ];

    let by_name = WindowSelector::default().owned_by(App::name("browser.exe"));
    assert_eq!(resolve_from_windows(&windows, &by_name).unwrap().reference.id, "2");

    let by_pid = WindowSelector::default().owned_by(App::pid(10));
    assert_eq!(resolve_from_windows(&windows, &by_pid).unwrap().reference.id, "1");
  }

  #[test]
  fn invisible_windows_are_never_matched() {
    let mut hidden = window("1", Some("Doc"), Some("app.exe"), 10, Rect::new(0.0, 0.0, 100.0, 100.0));
    hidden.is_visible = false;

    let selector = WindowQuery::title_exact("Doc");

    assert!(resolve_from_windows(&[hidden], &selector).is_err());
  }

  #[test]
  fn bundle_selector_never_matches_on_windows() {
    let windows = vec![window(
      "1",
      Some("Doc"),
      Some("app.exe"),
      10,
      Rect::new(0.0, 0.0, 100.0, 100.0),
    )];

    let selector = WindowSelector::default().owned_by(AppSelector {
      bundle: Some(TextMatcher::Exact("com.example.app".to_string())),
      ..AppSelector::default()
    });

    assert!(resolve_from_windows(&windows, &selector).is_err());
  }
}
