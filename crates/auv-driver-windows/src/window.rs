//! Top-level window enumeration and selector resolution.
//!
//! Mirrors the macOS driver's window listing/resolution surface, producing the
//! shared [`Window`] type so window-targeted consumers stay backend-agnostic.
//! Enumeration uses Win32 `EnumWindows`; frames prefer the DWM extended frame
//! bounds (the visible window rectangle) and fall back to `GetWindowRect`.
//!
//! NOTICE(windows-window-dpi): every rectangle here is in physical screen pixels. The DWM frame
//! bounds are physical whatever the caller's DPI awareness, while `GetWindowRect`,
//! `GetWindowPlacement` and `SetWindowPos` are virtualized for a DPI-unaware process. At 150%
//! scaling on luoling-windows-11 (2026-11-03) a window at logical (300, 200) 640x480 had DWM
//! bounds (461, 300) 938x709 and an outer rect of (300, 200) 640x480 unaware, (450, 300) 960x720
//! per-monitor aware. The host makes the process per-monitor aware
//! (`crate::dpi::set_process_per_monitor_aware`), so the native reads and writes agree.

use std::sync::atomic::{AtomicU64, Ordering};

use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::{Insets, Point, Rect};
use auv_driver_common::selector::{AppSelector, TextMatcher, WindowSelector};
use auv_driver_common::window::{Window, WindowState, select_main_window};

use crate::error::{invalid_input, not_found};

static ENUMERATIONS: AtomicU64 = AtomicU64::new(0);

/// How many times this process has enumerated every top-level window. A diagnostic: a session
/// that caches its targets leaves it unchanged after it has attached them.
pub fn window_enumerations() -> u64 {
  ENUMERATIONS.load(Ordering::Relaxed)
}

pub(crate) use auv_driver_desktop::windows::target::WindowProbe;
#[cfg(target_os = "windows")]
pub(crate) use auv_driver_desktop::windows::target::probe_window;

/// Lists every top-level window a user could switch to, front to back: minimized windows (with
/// the frame they restore to) and windows on other virtual desktops included.
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

/// One window's frame and state read fresh, without enumerating. `outer` is the `GetWindowRect`
/// rectangle that `SetWindowPos` addresses; `frame` is the visible frame `list_windows` reports.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WindowReading {
  pub(crate) frame: Rect,
  pub(crate) outer: Rect,
  /// `stack_index` is `None`: a stacking position needs the whole enumeration.
  pub(crate) state: WindowState,
  /// The raw foreground handle at read time, for focus-disturbance reporting.
  pub(crate) foreground: isize,
}

#[cfg(target_os = "windows")]
pub(crate) fn read_window(window: &Window) -> DriverResult<WindowReading> {
  native::read_one(window_handle(window)?)
}

/// The invisible resize border: how far the outer rectangle (`GetWindowRect`) reaches past the
/// visible frame (DWM extended frame bounds) on each side. On Windows 10 and 11 a standard frame
/// has one on the left, right and bottom (7 px at 100%, 11 px at 150%) and none on top.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn invisible_border(outer: Rect, visible: Rect) -> Insets {
  Insets {
    top: visible.origin.y - outer.origin.y,
    left: visible.origin.x - outer.origin.x,
    right: (outer.origin.x + outer.size.width) - (visible.origin.x + visible.size.width),
    bottom: (outer.origin.y + outer.size.height) - (visible.origin.y + visible.size.height),
  }
}

/// The invisible border that turns the outer restore rectangle (`rcNormalPosition`) of a minimized
/// or maximized window into the visible frame it has once normal, from the outer and visible
/// rectangles the window has now. A normal window's own border is [`invisible_border`].
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn restore_border(outer: Rect, visible: Rect, minimized: bool) -> Insets {
  let border = invisible_border(outer, visible);
  if minimized {
    // NOTICE: a minimized window's rectangles describe its iconic stub (at -32000,-32000 or the
    // minimized-window arrangement). The stub keeps the window's left and right invisible border,
    // but a per-monitor aware window's stub has none at the bottom. luoling-windows-11 (150%,
    // 2026-11-04): Paint and a per-monitor aware WinForms form (sizable and fixed dialog) had a
    // 9 px border on the left, right and bottom when normal, and a stub inset of 9, 9 and 0, so the
    // listed frame was 9 px too tall (1600x1009 for a 1600x1000 Paint). DPI-unaware stubs kept all
    // three (11 px); a borderless form's stub had none. The sizing frame is as wide at the bottom
    // as at the sides (`SM_CXSIZEFRAME` and `SM_CYSIZEFRAME` were both 5 at 144 DPI), so the
    // stub's side inset stands in for the bottom.
    // The system metrics do not give this border: `GetSystemMetricsForDpi(SM_CXSIZEFRAME)` +
    // `SM_CXPADDEDBORDER` (and `AdjustWindowRectExForDpi`) give the whole sizing frame, 11 px for
    // the per-monitor aware form and 12 px (8 px scaled by 1.5) for a DPI-unaware one, against a
    // measured 9 px and 11 px; how much of the frame DWM counts inside the extended frame bounds
    // is not documented. The style does not tell either: the fixed dialog had no `WS_THICKFRAME`
    // but the same 9 px border.
    // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getsystemmetricsfordpi
    // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getsystemmetrics
    // https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute
    // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowrect
    return Insets {
      bottom: border.bottom.max(border.left.min(border.right)),
      ..border
    };
  }
  // NOTICE: a maximized window hangs past its work area on every side, and that overhang is not
  // the invisible border it has once normal. luoling-windows-11 (150%, 2026-11-03): a per-monitor
  // aware WinForms window hung 11 px past every side of `0,0 3840x2088` but had 9 px left, right
  // and bottom and none on top once normal; a DPI-unaware one 12 px against 11 px. The placement
  // uses the overhang without a top border.
  Insets { top: 0.0, ..border }
}

/// The outer rectangle to hand `SetWindowPos` so the visible frame lands on `visible`.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn outer_frame_for_visible(visible: Rect, border: Insets) -> Rect {
  visible.inset(Insets {
    top: -border.top,
    right: -border.right,
    bottom: -border.bottom,
    left: -border.left,
  })
}

/// How far a monitor's workspace coordinates sit from its screen coordinates: screen = workspace +
/// offset. Workspace (0,0) is the top-left of the work area, which excludes a top or left taskbar.
// NOTICE: `WINDOWPLACEMENT.rcNormalPosition` is in workspace coordinates for every top-level window
// without `WS_EX_TOOLWINDOW`; tool windows use screen coordinates.
// https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-windowplacement
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn workspace_offset(monitor: Rect, work: Rect) -> Point {
  Point::new(work.origin.x - monitor.origin.x, work.origin.y - monitor.origin.y)
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

#[cfg(target_os = "windows")]
pub(crate) use auv_driver_desktop::windows::target::window_handle;

/// Whether `window` is a surface a person can switch to: shown, minimized, or on another virtual
/// desktop. The listing already drops hidden, zero-area and cloaked-on-this-desktop surfaces; this
/// pre-filter keeps a hand-built or stale list to the same rule before the shared cascade runs.
fn switchable(window: &Window) -> bool {
  window.is_visible || window.state.is_minimized == Some(true) || !on_current_desktop(window)
}

fn on_current_desktop(window: &Window) -> bool {
  window.state.desktop.as_ref().is_none_or(|desktop| desktop.is_current)
}

/// Resolves a single window from an already-observed list.
///
/// Kept independent of the Win32 enumeration so selector semantics are unit
/// testable without a live desktop. A `main_visible` selector picks with the
/// shared `select_main_window` cascade, which `application.activate`'s window
/// observation also uses, so the two never disagree about the main window.
fn resolve_from_windows(windows: &[Window], selector: &WindowSelector) -> DriverResult<Window> {
  let matches: Vec<&Window> = windows.iter().filter(|window| matches_window_selector_except_main_visible(window, selector)).collect();

  if selector.main_visible {
    return select_main_window(matches).cloned().ok_or_else(|| missing_window(windows, selector, "main visible window"));
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
      let owned = windows.iter().any(|window| switchable(window) && matches_app_selector(window, app));
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
  if !switchable(window) {
    return false;
  }
  // NOTICE: only the main-window cascade may pick a window on another virtual desktop (and only
  // when the app has no window on this one). A title or application selector skips them, as it
  // did before the list reported them; a caller that wants one targets it by id.
  // TODO(windows-window-select-other-desktop): selecting across desktops by title needs an
  // owner-approved selector field; it is not part of the window management v1 design.
  if !selector.main_visible && !on_current_desktop(window) {
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

pub(crate) fn without_exe_suffix(value: &str) -> &str {
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
pub(crate) mod native {
  use std::mem::size_of;
  use std::path::Path;

  use auv_driver_common::error::DriverResult;
  use auv_driver_common::geometry::{CoordinateSpace, Rect};
  use auv_driver_common::window::{Window, WindowDesktop, WindowRef, WindowState};
  use windows::Win32::Foundation::{BOOL, CloseHandle, FALSE, HWND, LPARAM, RECT, SetLastError, TRUE, WIN32_ERROR};
  use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect, MonitorFromWindow,
  };
  use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
  use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
  };
  use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
  use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GWL_EXSTYLE, GWL_STYLE, GetForegroundWindow, GetWindowLongW, GetWindowPlacement, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, IsZoomed, SW_RESTORE, SW_SHOW,
    SW_SHOWNOACTIVATE, SetForegroundWindow, SetWindowPlacement, ShowWindow, WINDOWPLACEMENT, WINDOWPLACEMENT_FLAGS, WS_CAPTION,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
  };
  use windows::core::{GUID, PWSTR};

  use super::{WindowReading, restore_border};
  use crate::error::backend;

  /// The public virtual desktop API, or `None` when the shell does not provide it (no Explorer,
  /// Server Core); windows then carry no desktop and cloaked windows are dropped as before.
  fn virtual_desktops() -> Option<IVirtualDesktopManager> {
    unsafe { CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL) }.ok()
  }

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

    let _com = crate::com::init_com();
    let desktops = virtual_desktops();
    let foreground = unsafe { GetForegroundWindow() };
    let mut windows = Vec::new();
    for hwnd in handles {
      if let Some(mut window) = observe_window(hwnd, foreground, desktops.as_ref())? {
        // EnumWindows walks the Z order from the top, so the listing position is the stacking order.
        window.state.stack_index = Some(windows.len() as u32);
        windows.push(window);
      }
    }
    Ok(windows)
  }

  pub(super) fn read_one(hwnd: HWND) -> DriverResult<WindowReading> {
    if !unsafe { IsWindow(hwnd) }.as_bool() {
      return Err(crate::error::not_found(format!("window:{}", hwnd.0 as isize)));
    }
    let _com = crate::com::init_com();
    let desktops = virtual_desktops();
    read(hwnd, unsafe { GetForegroundWindow() }, desktops.as_ref())
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

  fn observe_window(hwnd: HWND, foreground: HWND, desktops: Option<&IVirtualDesktopManager>) -> DriverResult<Option<Window>> {
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
      return Ok(None);
    }
    let desktop = desktops.and_then(|desktops| window_desktop(desktops, hwnd));
    // NOTICE(windows-window-cloaked): DWM cloaks a window on another virtual desktop, and also
    // windows nobody can switch to on this one: suspended UWP frames, hidden shell surfaces and
    // pre-launched apps. A cloaked window is listed only when the virtual desktop API places it
    // on another desktop; a cloaked window on the current desktop (or with no desktop) is dropped
    // as before. Without the API (`desktops` is `None`) every cloaked window is dropped.
    let on_other_desktop = desktop.as_ref().is_some_and(|desktop| !desktop.is_current);
    if is_cloaked(hwnd) && !on_other_desktop {
      return Ok(None);
    }
    let reading = read_with_desktop(hwnd, foreground, desktop)?;
    // Skip zero-area windows; visible top-level windows include many invisible
    // helper surfaces with degenerate frames that are not useful targets.
    if reading.frame.size.width <= 0.0 || reading.frame.size.height <= 0.0 {
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
      frame: reading.frame,
      coordinate_space: CoordinateSpace::Screen,
      is_main: hwnd == foreground,
      is_visible: reading.state.is_visible == Some(true),
      state: reading.state,
    }))
  }

  fn read(hwnd: HWND, foreground: HWND, desktops: Option<&IVirtualDesktopManager>) -> DriverResult<WindowReading> {
    read_with_desktop(hwnd, foreground, desktops.and_then(|desktops| window_desktop(desktops, hwnd)))
  }

  /// Reads a window's frames and state.
  fn read_with_desktop(hwnd: HWND, foreground: HWND, desktop: Option<WindowDesktop>) -> DriverResult<WindowReading> {
    let minimized = unsafe { IsIconic(hwnd) }.as_bool();
    let outer = outer_rect(hwnd)?;
    let visible = window_frame(hwnd)?;
    // A minimized window's rectangles describe its iconic stub at (-32000, -32000). Its frame is
    // the one it restores to; the stub's border (see `restore_border`) turns the restore rectangle
    // into a visible frame.
    let (frame, outer) = if minimized {
      let restore_outer = restore_rect(hwnd)?;
      (restore_outer.inset(restore_border(outer, visible, true)), restore_outer)
    } else {
      (visible, outer)
    };
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
    let ex_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
    let maximized = unsafe { IsZoomed(hwnd) }.as_bool();
    let on_current_desktop = desktop.as_ref().is_none_or(|desktop| desktop.is_current);
    Ok(WindowReading {
      frame,
      outer,
      state: WindowState {
        is_minimized: Some(minimized),
        is_maximized: Some(maximized),
        // NOTICE: Win32 has no full-screen state for another application's window. Best effort:
        // a window without a caption whose outer rectangle covers its whole monitor (borderless
        // games, F11 browsers, presentations). A maximized window keeps its caption.
        is_fullscreen: Some(!minimized && style & WS_CAPTION.0 != WS_CAPTION.0 && covers_monitor(hwnd, outer)),
        is_visible: Some(!minimized && on_current_desktop),
        is_focused: Some(hwnd == foreground),
        is_always_on_top: Some(ex_style & WS_EX_TOPMOST.0 != 0),
        stack_index: None,
        desktop,
      },
      foreground: foreground.0 as isize,
    })
  }

  fn outer_rect(hwnd: HWND) -> DriverResult<Rect> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.map_err(|error| backend(format!("GetWindowRect failed: {error}")))?;
    Ok(rect_from(rect))
  }

  /// The outer rectangle a minimized or maximized window restores to, in screen coordinates.
  pub(crate) fn restore_rect(hwnd: HWND) -> DriverResult<Rect> {
    let placement = placement(hwnd)?;
    let normal = rect_from(placement.rcNormalPosition);
    if is_tool_window(hwnd) {
      return Ok(normal);
    }
    let monitor = unsafe { MonitorFromRect(&placement.rcNormalPosition, MONITOR_DEFAULTTONEAREST) };
    let Some(offset) = monitor_workspace_offset(monitor) else {
      return Ok(normal);
    };
    Ok(normal.offset(offset.x, offset.y))
  }

  /// Makes a window normal at the outer rectangle `outer` (screen coordinates) with one
  /// `SetWindowPlacement`, without activating it. A maximized window goes straight to `outer`
  /// instead of first restoring to its old normal frame, as `SW_RESTORE` + `SetWindowPos` would.
  pub(crate) fn place_normal(hwnd: HWND, outer: RECT) -> DriverResult<()> {
    let mut placement = placement(hwnd)?;
    let mut normal = outer;
    if !is_tool_window(hwnd) {
      // The workspace of the monitor the window lands on.
      let monitor = unsafe { MonitorFromRect(&outer, MONITOR_DEFAULTTONEAREST) };
      if let Some(offset) = monitor_workspace_offset(monitor) {
        let (dx, dy) = (offset.x as i32, offset.y as i32);
        normal = RECT {
          left: outer.left - dx,
          top: outer.top - dy,
          right: outer.right - dx,
          bottom: outer.bottom - dy,
        };
      }
    }
    placement.rcNormalPosition = normal;
    placement.showCmd = SW_SHOWNOACTIVATE.0 as u32;
    // No WPF_SETMINPOSITION or WPF_RESTORETOMAXIMIZED: the window ends normal.
    placement.flags = WINDOWPLACEMENT_FLAGS(0);
    unsafe { SetWindowPlacement(hwnd, &placement) }.map_err(|error| backend(format!("SetWindowPlacement failed: {error}")))
  }

  fn placement(hwnd: HWND) -> DriverResult<WINDOWPLACEMENT> {
    let mut placement = WINDOWPLACEMENT {
      length: size_of::<WINDOWPLACEMENT>() as u32,
      ..Default::default()
    };
    unsafe { GetWindowPlacement(hwnd, &mut placement) }.map_err(|error| backend(format!("GetWindowPlacement failed: {error}")))?;
    Ok(placement)
  }

  /// Tool windows keep their placement in screen coordinates (see [`super::workspace_offset`]).
  fn is_tool_window(hwnd: HWND) -> bool {
    let ex_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
    ex_style & WS_EX_TOOLWINDOW.0 != 0
  }

  fn monitor_workspace_offset(monitor: HMONITOR) -> Option<auv_driver_common::geometry::Point> {
    let mut info = MONITORINFO {
      cbSize: size_of::<MONITORINFO>() as u32,
      ..Default::default()
    };
    unsafe { GetMonitorInfoW(monitor, &mut info) }
      .as_bool()
      .then(|| super::workspace_offset(rect_from(info.rcMonitor), rect_from(info.rcWork)))
  }

  fn covers_monitor(hwnd: HWND, outer: Rect) -> bool {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
      cbSize: size_of::<MONITORINFO>() as u32,
      ..Default::default()
    };
    unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() && outer.contains_rect(rect_from(info.rcMonitor))
  }

  /// The virtual desktop a window is on, from the public `IVirtualDesktopManager`.
  // TODO(windows-window-desktop-pinned): a window the user pinned to every desktop, and a window
  // the shell has not placed on a desktop, report the empty GUID; they carry no desktop (`None`)
  // rather than a made-up id. Model "every desktop" only if a caller needs it.
  fn window_desktop(desktops: &IVirtualDesktopManager, hwnd: HWND) -> Option<WindowDesktop> {
    let id = unsafe { desktops.GetWindowDesktopId(hwnd) }.ok()?;
    if id == GUID::zeroed() {
      return None;
    }
    let is_current = unsafe { desktops.IsWindowOnCurrentVirtualDesktop(hwnd) }.ok()?.as_bool();
    Some(WindowDesktop {
      id: format!("{id:?}"),
      is_current,
    })
  }

  use auv_driver_desktop::windows::target::rect_from;

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

  pub(crate) use auv_driver_desktop::windows::target::{is_cloaked, window_frame};

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
