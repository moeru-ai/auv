//! Top-level window enumeration and selector resolution.
//!
//! Mirrors the macOS driver's window listing/resolution surface, producing the
//! shared [`Window`] type so window-targeted consumers stay backend-agnostic.
//! Enumeration uses Win32 `EnumWindows`; frames prefer the DWM extended frame
//! bounds (the visible window rectangle) and fall back to `GetWindowRect`.

use std::sync::atomic::{AtomicU64, Ordering};

use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::Rect;
use auv_driver_common::selector::{AppSelector, TextMatcher, WindowSelector};
use auv_driver_common::window::Window;

use crate::error::{invalid_input, not_found};

static ENUMERATIONS: AtomicU64 = AtomicU64::new(0);

/// How many times this process has enumerated every top-level window. A diagnostic: a session
/// that caches its targets leaves it unchanged after it has attached them.
pub fn window_enumerations() -> u64 {
  ENUMERATIONS.load(Ordering::Relaxed)
}

/// What the OS says about one window handle right now, learned without enumerating windows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WindowProbe {
  pub(crate) exists: bool,
  pub(crate) process_id: u32,
  pub(crate) thread_id: u32,
  pub(crate) class_name: String,
  pub(crate) minimized: bool,
  /// Hidden by DWM, for example on another virtual desktop.
  pub(crate) cloaked: bool,
  /// The visible frame, as `list_windows` reports it.
  pub(crate) frame: Option<Rect>,
}

impl WindowProbe {
  // Off Windows only the background session's tests describe a closed window.
  #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
  pub(crate) fn gone() -> Self {
    Self {
      exists: false,
      process_id: 0,
      thread_id: 0,
      class_name: String::new(),
      minimized: false,
      cloaked: false,
      frame: None,
    }
  }
}

/// Reads the live state of the window behind `hwnd` (a raw handle value). A handle that no longer
/// names a window comes back with `exists: false`.
#[cfg(target_os = "windows")]
pub(crate) fn probe_window(hwnd: isize) -> WindowProbe {
  native::probe(hwnd)
}

#[cfg(target_os = "windows")]
pub fn list_windows() -> DriverResult<Vec<Window>> {
  ENUMERATIONS.fetch_add(1, Ordering::Relaxed);
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

/// The main-window preference `WindowSelector::main_visible` and the
/// activation window observation share: foreground first, then a titled
/// window, then the largest frame. One rank so the resolver and the
/// observation can never disagree about which window is the app's main one.
pub(crate) fn main_visible_rank(window: &Window) -> (bool, bool, i64) {
  (
    window.is_main,
    window.title.as_ref().is_some_and(|title| !title.trim().is_empty()),
    (window.frame.size.width * window.frame.size.height).round() as i64,
  )
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
    matches.sort_by_key(|window| std::cmp::Reverse(main_visible_rank(window)));
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
        auv_driver_common::window::application_not_found(app, windows)
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
    return window.is_main;
  }
  if let Some(pid) = selector.process_id
    && window.process_id != Some(pid)
  {
    return false;
  }
  // NOTICE: a Windows application id is its executable file name, the APP of
  // `window.list` (`application_id` is always `None`). The CLI's local path
  // selects it by name and a Runner client by application id, so both match
  // the executable name; see `matches_executable`.
  for matcher in [&selector.id, &selector.name].into_iter().flatten() {
    if !window.app_name.as_deref().is_some_and(|app_name| matches_executable(app_name, matcher)) {
      return false;
    }
  }
  true
}

/// Matches an executable file name. Windows file names are case-insensitive
/// (the same program is `Notepad.exe` on disk and `notepad.exe` to the person
/// typing it), and an exact name may leave out `.exe` (`notepad`).
pub(crate) fn matches_executable(app_name: &str, matcher: &TextMatcher) -> bool {
  match matcher {
    TextMatcher::Exact(expected) => {
      app_name.eq_ignore_ascii_case(expected) || without_exe_suffix(app_name).eq_ignore_ascii_case(without_exe_suffix(expected))
    }
    TextMatcher::Contains(needle) => app_name.to_ascii_lowercase().contains(&needle.to_ascii_lowercase()),
  }
}

fn without_exe_suffix(value: &str) -> &str {
  match value.len().checked_sub(4) {
    Some(split) if value.is_char_boundary(split) && value[split..].eq_ignore_ascii_case(".exe") => &value[..split],
    _ => value,
  }
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
  use windows::Win32::Foundation::{BOOL, CloseHandle, FALSE, HWND, LPARAM, RECT, SetLastError, TRUE, WIN32_ERROR};
  use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
  use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
  };
  use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow,
  };
  use windows::core::PWSTR;

  use crate::error::backend;

  pub(super) fn list_windows() -> DriverResult<Vec<Window>> {
    crate::desktop::try_ensure_input_desktop().map_err(|error| backend(format!("input desktop attachment failed: {error}")))?;
    let mut handles: Vec<HWND> = Vec::new();
    // Clear last error prior to EnumWindows. Win32 EnumWindows does not reset last-error,
    // so any stale error (such as ERROR_ALREADY_EXISTS from earlier process startup)
    // would otherwise cause windows-rs to treat a FALSE return as a fatal failure.
    unsafe { SetLastError(WIN32_ERROR(0)) };
    // SAFETY: `enum_proc` only pushes into the Vec referenced by `lparam`, which
    // outlives the synchronous EnumWindows call.
    let enumeration = unsafe { EnumWindows(Some(enum_proc), LPARAM(&mut handles as *mut Vec<HWND> as isize)) };
    if let Err(error) = enumeration
      && error.code().is_err()
      && handles.is_empty()
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

  pub(super) fn probe(raw: isize) -> super::WindowProbe {
    let hwnd = HWND(raw as *mut c_void);
    // SAFETY: read-only queries on a handle that may have been destroyed; each fails harmlessly.
    unsafe {
      if !IsWindow(hwnd).as_bool() {
        return super::WindowProbe::gone();
      }
      let mut process_id = 0u32;
      let thread_id = GetWindowThreadProcessId(hwnd, Some(&mut process_id));
      let mut name = [0u16; 256];
      let length = GetClassNameW(hwnd, &mut name).max(0) as usize;
      super::WindowProbe {
        exists: true,
        process_id,
        thread_id,
        class_name: String::from_utf16_lossy(&name[..length]),
        minimized: IsIconic(hwnd).as_bool(),
        cloaked: is_cloaked(hwnd),
        frame: window_frame(hwnd).ok(),
      }
    }
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
      // NOTICE: SW_RESTORE also turns a maximized window back into a normal one,
      // which moved the window under a click point computed from its frame
      // before activation. Only a minimized window needs restoring; SW_SHOW
      // keeps the activate-and-show side effect at the current size.
      let _ = ShowWindow(
        hwnd,
        if IsIconic(hwnd).as_bool() {
          SW_RESTORE
        } else {
          SW_SHOW
        },
      );
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
      Err(backend("SetForegroundWindow was refused for the selected window"))
    }
  }

  unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam carries the &mut Vec<HWND> created in list_windows.
    let handles = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    handles.push(hwnd);
    TRUE
  }

  fn observe_window(hwnd: HWND, foreground: HWND) -> DriverResult<Option<Window>> {
    // TODO(windows-window-list-minimized): a minimized window passes `IsWindowVisible` and is
    // listed with its off-screen frame (-32000, -32000) and `is_visible: true`. It is not
    // dropped or flagged here because the background session discovers a target through this
    // list and must still be able to answer `skipped_minimized` for it. Decide how a list
    // reports a minimized window before changing it; `capture` already refuses one.
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
      // TODO(windows-application-id): selectors already match an application
      // id against the executable name in app_name. Reporting it here too
      // changes what readers of application_id see (window.list, missing
      // window hints, readiness), so it waits for an owner-approved slice.
      application_id: None,
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
#[path = "window_test.rs"]
mod tests;
