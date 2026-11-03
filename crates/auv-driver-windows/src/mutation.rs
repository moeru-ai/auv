//! Window geometry, state and focus changes via Win32 `SetWindowPos`, `ShowWindow` and
//! `SetForegroundWindow`.
//!
//! Produces the shared [`WindowMutationResult`]. Windows has a single native mutation path, so
//! this module always reports [`WindowMutationPath::PlatformNative`]; the macOS AX-specific
//! candidates in [`WindowMutationOptions::strategy`] and its `policy` do not apply here and are
//! intentionally ignored. See
//! `docs/ai/references/driver/2026-11-02-window-management-api-v1-design.md` ("Mutation").
// TODO(windows-window-mutation-fallback): a foreground/SendInput fallback is
// reserved but unimplemented; the native SetWindowPos/ShowWindow path is the
// only candidate this slice wires. Add a fallback only when an owner-approved
// reliability gap appears for background repositioning.
// Off Windows only the unit tests call the pure helpers below.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use auv_driver_common::error::{DriverError, DriverResult};
use auv_driver_common::geometry::{Point, Rect, Size};
use auv_driver_common::input::DisturbanceLevel;
use auv_driver_common::window::{
  Window, WindowMutationAttempt, WindowMutationOptions, WindowMutationPath, WindowMutationResult, WindowMutationVerification, WindowState,
  WindowStateTarget,
};

use crate::error::{backend, invalid_input};
use crate::window::WindowReading;

/// How often a mutation reads the window back before it gives up waiting for it to settle.
// NOTICE: 6 reads 40 ms apart (200 ms) follow trycua/cua `set_window_frame.rs`. `SetWindowPos` and
// `ShowWindow` on another process's window return once the message is handled, so the first read
// usually matches; the polls cover apps that adjust their frame asynchronously.
const READBACK_ATTEMPTS: usize = 6;
const READBACK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(40);

/// Moves and/or resizes a window so its visible frame lands on the request. A missing origin keeps
/// the position; a missing size keeps the size. A maximized window becomes a normal window at the
/// requested frame, and its restore frame fills the missing parts.
pub fn set_frame(
  window: &Window,
  origin: Option<Point>,
  size: Option<Size>,
  options: WindowMutationOptions,
) -> DriverResult<WindowMutationResult> {
  validate_frame_request(origin, size)?;
  frame_tolerance(&options.verification)?;
  native_set_frame(window, origin, size, &options)
}

/// Moves a window to `state`. A window already in it succeeds without a call.
pub fn set_state(window: &Window, state: WindowStateTarget, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
  native_set_state(window, state, &options)
}

/// Raises a window and gives it keyboard focus, restoring it first when it is minimized.
pub fn focus(window: &Window, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
  native_focus(window, &options)
}

fn validate_frame_request(origin: Option<Point>, size: Option<Size>) -> DriverResult<()> {
  if origin.is_none() && size.is_none() {
    return Err(invalid_input("window.setFrame needs an origin, a size, or both"));
  }
  if let Some(origin) = origin {
    rounded_i32(origin.x, "origin.x")?;
    rounded_i32(origin.y, "origin.y")?;
  }
  if let Some(size) = size {
    rounded_positive_i32(size.width, "size.width")?;
    rounded_positive_i32(size.height, "size.height")?;
  }
  Ok(())
}

/// `set_frame` refuses a minimized window, which has no frame on screen to move, and a full-screen
/// window. A maximized window is allowed: the frame request makes it normal (owner decision
/// 2026-11-03, the same on macOS and Windows; see the design doc's "Decided" list).
fn frame_precondition(state: &WindowState) -> DriverResult<()> {
  let blocking = if state.is_minimized == Some(true) {
    "minimized"
  } else if state.is_fullscreen == Some(true) {
    "full screen"
  } else {
    return Ok(());
  };
  Err(DriverError::FailedPrecondition {
    message: format!("window.setFrame needs a normal window, but the window is {blocking}"),
    recovery: Some("call set_state(Normal) first".into()),
  })
}

/// The visible frame a request asks for: the current frame with the requested parts replaced.
fn requested_frame(current: Rect, origin: Option<Point>, size: Option<Size>) -> Rect {
  Rect {
    origin: origin.unwrap_or(current.origin),
    size: size.unwrap_or(current.size),
  }
}

fn frame_tolerance(verification: &WindowMutationVerification) -> DriverResult<f64> {
  match *verification {
    WindowMutationVerification::FrameTolerance { points } if points.is_finite() && points >= 0.0 => Ok(points),
    WindowMutationVerification::FrameTolerance { .. } => {
      Err(invalid_input("window mutation frame tolerance must be a finite non-negative value"))
    }
    // NOTICE: best effort still checks the frame with the default 2 px, because a frame request
    // has no other evidence of its effect.
    WindowMutationVerification::BestEffortState => Ok(2.0),
  }
}

fn frames_close(a: Rect, b: Rect, tolerance: f64) -> bool {
  (a.origin.x - b.origin.x).abs() <= tolerance
    && (a.origin.y - b.origin.y).abs() <= tolerance
    && (a.size.width - b.size.width).abs() <= tolerance
    && (a.size.height - b.size.height).abs() <= tolerance
}

/// Judges a settled frame: `Ok(false)` when it matches the request, `Ok(true)` when the window
/// changed but the app settled it elsewhere (minimum size, grid snapping, a DPI change), and an
/// error only when the window did not change at all.
fn judge_frame(before: Rect, after: Rect, requested: Rect, tolerance: f64) -> DriverResult<bool> {
  if frames_close(after, requested, tolerance) {
    return Ok(false);
  }
  if frames_close(after, before, tolerance) {
    return Err(backend(format!("window mutation verification failed: the window stayed at {after:?} instead of moving to {requested:?}")));
  }
  Ok(true)
}

/// The state target a window is in now. A minimized window that restores to maximized counts as
/// minimized.
fn current_target(state: &WindowState) -> WindowStateTarget {
  if state.is_minimized == Some(true) {
    WindowStateTarget::Minimized
  } else if state.is_maximized == Some(true) {
    WindowStateTarget::Maximized
  } else {
    WindowStateTarget::Normal
  }
}

fn target_name(target: WindowStateTarget) -> &'static str {
  match target {
    WindowStateTarget::Normal => "normal",
    WindowStateTarget::Minimized => "minimized",
    WindowStateTarget::Maximized => "maximized",
  }
}

/// A foreground change is a focus disturbance; a mutation that left the same window in front
/// did not disturb focus.
fn focus_disturbance(before: &WindowReading, after: &WindowReading) -> DisturbanceLevel {
  if before.foreground == after.foreground {
    DisturbanceLevel::None
  } else {
    DisturbanceLevel::Foreground
  }
}

fn mutation_result(
  attempt: WindowMutationAttempt,
  before: &WindowReading,
  after: &WindowReading,
  adjusted_by_app: bool,
) -> WindowMutationResult {
  WindowMutationResult {
    selected_path: WindowMutationPath::PlatformNative,
    attempts: vec![attempt],
    before_frame: Some(before.frame),
    after_frame: Some(after.frame),
    before_state: Some(before.state.clone()),
    after_state: Some(after.state.clone()),
    adjusted_by_app,
    focus_disturbance: focus_disturbance(before, after),
    mouse_disturbance: DisturbanceLevel::None,
  }
}

/// Rounds an `f64` coordinate into the `i32` range used by the Win32 window
/// APIs, rejecting non-finite or out-of-range values.
fn rounded_i32(value: f64, field: &str) -> DriverResult<i32> {
  if !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
    return Err(invalid_input(format!("{field} must be a finite i32-sized value")));
  }
  Ok(value.round() as i32)
}

fn rounded_positive_i32(value: f64, field: &str) -> DriverResult<i32> {
  let rounded = rounded_i32(value, field)?;
  if rounded <= 0 {
    return Err(invalid_input(format!("{field} must be greater than zero")));
  }
  Ok(rounded)
}

#[cfg(not(target_os = "windows"))]
fn native_set_frame(_: &Window, _: Option<Point>, _: Option<Size>, _: &WindowMutationOptions) -> DriverResult<WindowMutationResult> {
  Err(DriverError::unsupported("window.setFrame"))
}

#[cfg(not(target_os = "windows"))]
fn native_set_state(_: &Window, _: WindowStateTarget, _: &WindowMutationOptions) -> DriverResult<WindowMutationResult> {
  Err(DriverError::unsupported("window.setState"))
}

#[cfg(not(target_os = "windows"))]
fn native_focus(_: &Window, _: &WindowMutationOptions) -> DriverResult<WindowMutationResult> {
  Err(DriverError::unsupported("window.focus"))
}

#[cfg(target_os = "windows")]
fn native_set_frame(
  window: &Window,
  origin: Option<Point>,
  size: Option<Size>,
  options: &WindowMutationOptions,
) -> DriverResult<WindowMutationResult> {
  use windows::Win32::UI::WindowsAndMessaging::{SET_WINDOW_POS_FLAGS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos};

  use crate::window::{invisible_border, outer_frame_for_visible, read_window, window_handle};

  let tolerance = frame_tolerance(&options.verification)?;
  let hwnd = window_handle(window)?;
  // REVIEW(windows-window-dpi): the request is in the physical pixels `window.list` reports, and
  // `SetWindowPos` takes them so in the per-monitor-aware process (`crate::dpi`). A move onto a
  // display with another scale makes the target resize itself on `WM_DPICHANGED`; that lands as
  // `adjusted_by_app`, not a failure. Only one 150% display has been measured.
  let before = read_window(window)?;
  frame_precondition(&before.state)?;
  if before.state.is_maximized == Some(true) {
    return set_frame_from_maximized(window, hwnd, &before, origin, size, tolerance);
  }
  let requested = requested_frame(before.frame, origin, size);
  // NOTICE: `window.list` reports the visible frame (DWM extended frame bounds), but
  // `SetWindowPos` sets the outer rectangle, which on Windows 10/11 includes an invisible resize
  // border (11 px at 150%). Without this conversion a move landed 11 px right of the request
  // and a resize 22 px narrower (luoling-windows-11, 2026-11-03).
  let outer = outer_frame_for_visible(requested, invisible_border(before.outer, before.frame));
  let x = rounded_i32(outer.origin.x, "origin.x")?;
  let y = rounded_i32(outer.origin.y, "origin.y")?;
  let width = rounded_positive_i32(outer.size.width, "size.width")?;
  let height = rounded_positive_i32(outer.size.height, "size.height")?;
  let mut flags: SET_WINDOW_POS_FLAGS = SWP_NOACTIVATE | SWP_NOZORDER;
  if origin.is_none() {
    flags |= SWP_NOMOVE;
  }
  if size.is_none() {
    flags |= SWP_NOSIZE;
  }
  unsafe { SetWindowPos(hwnd, None, x, y, width, height, flags) }.map_err(|error| backend(format!("SetWindowPos failed: {error}")))?;
  let after = poll(window, |reading| frames_close(reading.frame, requested, tolerance))?;
  let adjusted_by_app = judge_frame(before.frame, after.frame, requested, tolerance)?;
  let attempt = WindowMutationAttempt::success(
    WindowMutationPath::PlatformNative,
    format!("SetWindowPos to outer {x},{y} {width}x{height} for visible {requested:?}"),
  );
  Ok(mutation_result(attempt, &before, &after, adjusted_by_app))
}

/// `set_frame` on a maximized window: one `SetWindowPlacement` makes it normal at the requested
/// frame without activating it. `SW_RESTORE` + `SetWindowPos` would first show it at its old
/// restore frame and then move it.
#[cfg(target_os = "windows")]
fn set_frame_from_maximized(
  window: &Window,
  hwnd: windows::Win32::Foundation::HWND,
  before: &WindowReading,
  origin: Option<Point>,
  size: Option<Size>,
  tolerance: f64,
) -> DriverResult<WindowMutationResult> {
  use auv_driver_common::geometry::Insets;
  use windows::Win32::UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos};

  use crate::window::native::{place_normal, restore_rect};
  use crate::window::{invisible_border, outer_frame_for_visible};

  // NOTICE: a maximized window hangs past its work area on every side, and that overhang is not
  // the invisible border it has once normal. luoling-windows-11 (150%, 2026-11-03): a per-monitor
  // aware WinForms window hung 11 px past every side of `0,0 3840x2088` but had 9 px left, right
  // and bottom and none on top once normal; a DPI-unaware one 12 px against 11 px. The placement
  // uses the overhang without a top border.
  // NOTICE: some apps put the window back at a restore frame of their own when it leaves
  // maximized. A WinForms form showed the requested frame and 4 ms later its remembered restore
  // bounds (fixture bounds log, same host and date). So when the window is normal but not at the
  // request, one `SetWindowPos` with the border read from the normal window moves it there; it
  // also corrects a wrong border guess. A window that still settles elsewhere is `adjusted_by_app`.
  // The app's own frame shows for a few milliseconds; that jump is the app's, not this call's.
  let overhang = invisible_border(before.outer, before.frame);
  let guess = Insets {
    top: 0.0,
    ..overhang
  };
  let restore_outer = restore_rect(hwnd)?;
  let mut requested = requested_frame(restore_outer.inset(guess), origin, size);
  let outer = rounded_outer(outer_frame_for_visible(requested, guess))?;
  place_normal(hwnd, outer)?;
  let settled = |reading: &WindowReading, requested: Rect| {
    reading.state.is_maximized == Some(false) && frames_close(reading.frame, requested, tolerance)
  };
  // Correct as soon as the window reads normal: an app's own restore frame stays on screen until
  // then (about 360 ms when this waited for the full settle poll).
  let mut after = poll(window, |reading| reading.state.is_maximized == Some(false))?;
  let mut detail = format!("SetWindowPlacement(SW_SHOWNOACTIVATE) to outer {}", outer_text(outer));
  if after.state.is_maximized == Some(false) && !frames_close(after.frame, requested, tolerance) {
    detail.push_str(&format!(", normal at visible {:?}", after.frame));
    let border = invisible_border(after.outer, after.frame);
    requested = requested_frame(restore_outer.inset(border), origin, size);
    let corrected = rounded_outer(outer_frame_for_visible(requested, border))?;
    unsafe {
      SetWindowPos(
        hwnd,
        None,
        corrected.left,
        corrected.top,
        corrected.right - corrected.left,
        corrected.bottom - corrected.top,
        SWP_NOACTIVATE | SWP_NOZORDER,
      )
    }
    .map_err(|error| backend(format!("SetWindowPos failed: {error}")))?;
    after = poll(window, |reading| settled(reading, requested))?;
    detail.push_str(&format!(", then SetWindowPos to outer {} with the normal border {border:?}", outer_text(corrected)));
  }
  if after.state.is_maximized != Some(false) {
    return Err(backend(format!("window mutation verification failed: the window is still maximized after {detail}")));
  }
  let adjusted_by_app = judge_frame(before.frame, after.frame, requested, tolerance)?;
  let attempt = WindowMutationAttempt::success(WindowMutationPath::PlatformNative, format!("{detail} for visible {requested:?}"));
  Ok(mutation_result(attempt, before, &after, adjusted_by_app))
}

/// The outer rectangle as the integer `RECT` that `SetWindowPlacement` takes.
#[cfg(target_os = "windows")]
fn rounded_outer(outer: Rect) -> DriverResult<windows::Win32::Foundation::RECT> {
  let left = rounded_i32(outer.origin.x, "origin.x")?;
  let top = rounded_i32(outer.origin.y, "origin.y")?;
  let width = rounded_positive_i32(outer.size.width, "size.width")?;
  let height = rounded_positive_i32(outer.size.height, "size.height")?;
  Ok(windows::Win32::Foundation::RECT {
    left,
    top,
    right: left.saturating_add(width),
    bottom: top.saturating_add(height),
  })
}

#[cfg(target_os = "windows")]
fn outer_text(rect: windows::Win32::Foundation::RECT) -> String {
  format!("{},{} {}x{}", rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top)
}

#[cfg(target_os = "windows")]
fn native_set_state(window: &Window, target: WindowStateTarget, options: &WindowMutationOptions) -> DriverResult<WindowMutationResult> {
  use windows::Win32::UI::WindowsAndMessaging::{SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, ShowWindow};

  use crate::window::{read_window, window_handle};

  // NOTICE: `options.verification` does not change how a state is checked: `IsIconic` and
  // `IsZoomed` are exact on Windows, so every target is verified.
  let _ = options;
  let hwnd = window_handle(window)?;
  let before = read_window(window)?;
  if current_target(&before.state) == target {
    let attempt =
      WindowMutationAttempt::success(WindowMutationPath::PlatformNative, format!("already {}; no call made", target_name(target)));
    return Ok(mutation_result(attempt, &before, &before, false));
  }
  let command = match target {
    WindowStateTarget::Normal => SW_RESTORE,
    WindowStateTarget::Minimized => SW_MINIMIZE,
    WindowStateTarget::Maximized => SW_MAXIMIZE,
  };
  // ShowWindow returns the prior visibility as a BOOL and does not signal
  // failure, so the return value is intentionally ignored.
  let _ = unsafe { ShowWindow(hwnd, command) };
  let mut after = poll(window, |reading| current_target(&reading.state) != current_target(&before.state))?;
  let mut calls = 1;
  // A minimized window that was maximized restores to maximized; a second SW_RESTORE makes it
  // normal.
  if target == WindowStateTarget::Normal && current_target(&after.state) == WindowStateTarget::Maximized {
    let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
    after = poll(window, |reading| current_target(&reading.state) == WindowStateTarget::Normal)?;
    calls += 1;
  }
  let reached = current_target(&after.state);
  if reached != target {
    return Err(backend(format!(
      "window mutation verification failed: window is {} after {calls} ShowWindow call(s), not {}",
      target_name(reached),
      target_name(target)
    )));
  }
  let attempt =
    WindowMutationAttempt::success(WindowMutationPath::PlatformNative, format!("ShowWindow x{calls} to {}", target_name(target)));
  Ok(mutation_result(attempt, &before, &after, false))
}

#[cfg(target_os = "windows")]
fn native_focus(window: &Window, options: &WindowMutationOptions) -> DriverResult<WindowMutationResult> {
  use crate::window::{activate_window, read_window};

  let before = read_window(window)?;
  if before.state.is_focused == Some(true) {
    let attempt = WindowMutationAttempt::success(WindowMutationPath::PlatformNative, "already focused; no call made");
    return Ok(mutation_result(attempt, &before, &before, false));
  }
  // NOTICE: `activate_window` restores a minimized window (a focused window must be shown) but
  // keeps a maximized one maximized. Its refusal (`SetForegroundWindow` denied by the foreground
  // lock) is the error.
  activate_window(window)?;
  if !options.settle.is_zero() {
    std::thread::sleep(options.settle);
  }
  let after = read_window(window)?;
  if after.state.is_focused != Some(true) {
    return Err(backend("window mutation verification failed: another window is in front after focus settled"));
  }
  let attempt = WindowMutationAttempt::success(WindowMutationPath::PlatformNative, "SetForegroundWindow");
  Ok(mutation_result(attempt, &before, &after, false))
}

/// Reads the window up to `READBACK_ATTEMPTS` times until `settled` holds, returning the last
/// reading either way.
#[cfg(target_os = "windows")]
fn poll(window: &Window, settled: impl Fn(&WindowReading) -> bool) -> DriverResult<WindowReading> {
  let mut reading = crate::window::read_window(window)?;
  for _ in 1..READBACK_ATTEMPTS {
    if settled(&reading) {
      break;
    }
    std::thread::sleep(READBACK_INTERVAL);
    reading = crate::window::read_window(window)?;
  }
  Ok(reading)
}

#[cfg(test)]
#[path = "mutation_test.rs"]
mod tests;
