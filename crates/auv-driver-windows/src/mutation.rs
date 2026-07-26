//! Window geometry and state mutation via Win32 `SetWindowPos`/`ShowWindow`.
//!
//! Mirrors the macOS driver's window mutation surface, producing the shared
//! [`WindowMutationResult`]. Windows has a single native mutation path, so this
//! module always reports [`WindowMutationPath::PlatformNative`]; the macOS
//! AX-specific candidates in [`WindowMutationOptions::strategy`] do not apply
//! here and are intentionally ignored.
// TODO(windows-window-mutation-fallback): a foreground/SendInput fallback is
// reserved but unimplemented; the native SetWindowPos/ShowWindow path is the
// only candidate this slice wires. Add a fallback only when an owner-approved
// reliability gap appears for background repositioning.

use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::Rect;
use auv_driver_common::input::DisturbanceLevel;
use auv_driver_common::window::{
  Window, WindowMutationAttempt, WindowMutationKind, WindowMutationOptions, WindowMutationPath, WindowMutationResult,
  WindowMutationVerification, WindowState,
};

use crate::error::{backend, invalid_input};

pub fn mutate_window(window: &Window, kind: WindowMutationKind, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
  validate_window_mutation_kind(kind)?;
  let outcome = perform_native_mutation(window, kind, options.settle)?;
  let result = window_mutation_result(kind, outcome);
  verify_window_mutation(kind, &options.verification, &result)?;
  Ok(result)
}

/// Native before/after observation produced by the Win32 mutation path.
#[derive(Clone, Copy, Debug, PartialEq)]
struct NativeMutationOutcome {
  before_frame: Rect,
  after_frame: Rect,
  before_minimized: bool,
  after_minimized: bool,
  before_visible: bool,
  after_visible: bool,
}

/// Focus impact of a mutation kind.
///
/// Geometry changes are issued with `SWP_NOACTIVATE`, so they do not move
/// foreground focus. State changes (minimize/restore/zoom) activate or
/// deactivate the window and therefore report a foreground disturbance.
fn focus_disturbance_for(kind: WindowMutationKind) -> DisturbanceLevel {
  match kind {
    WindowMutationKind::MoveTo { .. } | WindowMutationKind::Resize { .. } | WindowMutationKind::SetFrame { .. } => DisturbanceLevel::None,
    WindowMutationKind::Minimize | WindowMutationKind::Restore | WindowMutationKind::Zoom => DisturbanceLevel::Foreground,
  }
}

fn window_mutation_result(kind: WindowMutationKind, outcome: NativeMutationOutcome) -> WindowMutationResult {
  WindowMutationResult {
    selected_path: WindowMutationPath::PlatformNative,
    attempts: vec![WindowMutationAttempt::success(
      WindowMutationPath::PlatformNative,
      format!("{} via SetWindowPos/ShowWindow", window_mutation_kind_name(kind)),
    )],
    before_frame: Some(outcome.before_frame),
    after_frame: Some(outcome.after_frame),
    before_state: Some(WindowState {
      is_minimized: Some(outcome.before_minimized),
      is_visible: Some(outcome.before_visible),
    }),
    after_state: Some(WindowState {
      is_minimized: Some(outcome.after_minimized),
      is_visible: Some(outcome.after_visible),
    }),
    focus_disturbance: focus_disturbance_for(kind),
    mouse_disturbance: DisturbanceLevel::None,
  }
}

fn window_mutation_kind_name(kind: WindowMutationKind) -> &'static str {
  match kind {
    WindowMutationKind::MoveTo { .. } => "move_to",
    WindowMutationKind::Resize { .. } => "resize",
    WindowMutationKind::SetFrame { .. } => "set_frame",
    WindowMutationKind::Minimize => "minimize",
    WindowMutationKind::Restore => "restore",
    WindowMutationKind::Zoom => "zoom",
  }
}

fn validate_window_mutation_kind(kind: WindowMutationKind) -> DriverResult<()> {
  match kind {
    WindowMutationKind::MoveTo { point } => {
      let _ = rounded_i32(point.x, "point.x")?;
      let _ = rounded_i32(point.y, "point.y")?;
    }
    WindowMutationKind::Resize { size } => {
      let _ = rounded_positive_i32(size.width, "size.width")?;
      let _ = rounded_positive_i32(size.height, "size.height")?;
    }
    WindowMutationKind::SetFrame { frame } => {
      let _ = rounded_i32(frame.origin.x, "frame.origin.x")?;
      let _ = rounded_i32(frame.origin.y, "frame.origin.y")?;
      let _ = rounded_positive_i32(frame.size.width, "frame.size.width")?;
      let _ = rounded_positive_i32(frame.size.height, "frame.size.height")?;
    }
    WindowMutationKind::Minimize | WindowMutationKind::Restore | WindowMutationKind::Zoom => {}
  }
  Ok(())
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

fn verify_window_mutation(
  kind: WindowMutationKind,
  verification: &WindowMutationVerification,
  result: &WindowMutationResult,
) -> DriverResult<()> {
  match verification {
    WindowMutationVerification::BestEffortState => verify_window_state(kind, result),
    WindowMutationVerification::FrameTolerance { points } => {
      verify_window_frame(kind, result, *points)?;
      verify_window_state(kind, result)
    }
  }
}

fn verify_window_frame(kind: WindowMutationKind, result: &WindowMutationResult, tolerance: f64) -> DriverResult<()> {
  if !tolerance.is_finite() || tolerance < 0.0 {
    return Err(invalid_input("window mutation frame tolerance must be a finite non-negative value"));
  }
  let Some(after_frame) = result.after_frame else {
    return Err(backend("window mutation did not report an after frame"));
  };
  match kind {
    WindowMutationKind::MoveTo { point } => {
      verify_close(after_frame.origin.x, point.x, tolerance, "frame.origin.x")?;
      verify_close(after_frame.origin.y, point.y, tolerance, "frame.origin.y")?;
    }
    WindowMutationKind::Resize { size } => {
      verify_close(after_frame.size.width, size.width, tolerance, "frame.size.width")?;
      verify_close(after_frame.size.height, size.height, tolerance, "frame.size.height")?;
    }
    WindowMutationKind::SetFrame { frame } => {
      verify_close(after_frame.origin.x, frame.origin.x, tolerance, "frame.origin.x")?;
      verify_close(after_frame.origin.y, frame.origin.y, tolerance, "frame.origin.y")?;
      verify_close(after_frame.size.width, frame.size.width, tolerance, "frame.size.width")?;
      verify_close(after_frame.size.height, frame.size.height, tolerance, "frame.size.height")?;
    }
    // State changes do not assert a frame; geometry after minimize/restore/zoom
    // is window-manager defined.
    WindowMutationKind::Minimize | WindowMutationKind::Restore | WindowMutationKind::Zoom => {}
  }
  Ok(())
}

fn verify_window_state(kind: WindowMutationKind, result: &WindowMutationResult) -> DriverResult<()> {
  match kind {
    WindowMutationKind::Minimize => {
      if minimized_flag(result) != Some(true) {
        return Err(backend(
          "window mutation verification failed: window was not minimized",
        ));
      }
    }
    WindowMutationKind::Restore => {
      if minimized_flag(result) != Some(false) {
        return Err(backend(
          "window mutation verification failed: window was still minimized",
        ));
      }
    }
    WindowMutationKind::MoveTo { .. }
    | WindowMutationKind::Resize { .. }
    | WindowMutationKind::SetFrame { .. }
    // TODO(windows-window-zoom-verification): zoom/maximize state verification is
    // deferred; reading a reliable maximized signal here needs an owner-approved
    // cross-app check, so this slice trusts the ShowWindow call.
    | WindowMutationKind::Zoom => {}
  }
  Ok(())
}

fn minimized_flag(result: &WindowMutationResult) -> Option<bool> {
  result.after_state.as_ref().and_then(|state| state.is_minimized)
}

fn verify_close(actual: f64, expected: f64, tolerance: f64, field: &str) -> DriverResult<()> {
  if (actual - expected).abs() <= tolerance {
    return Ok(());
  }
  Err(backend(format!("window mutation verification failed: {field} expected {expected:.3} got {actual:.3} tolerance {tolerance:.3}")))
}

#[cfg(target_os = "windows")]
fn perform_native_mutation(window: &Window, kind: WindowMutationKind, settle: std::time::Duration) -> DriverResult<NativeMutationOutcome> {
  native::perform(window, kind, settle)
}

#[cfg(not(target_os = "windows"))]
fn perform_native_mutation(
  _window: &Window,
  _kind: WindowMutationKind,
  _settle: std::time::Duration,
) -> DriverResult<NativeMutationOutcome> {
  Err(auv_driver_common::error::DriverError::unsupported("window.mutate"))
}

#[cfg(target_os = "windows")]
mod native {
  use std::ffi::c_void;
  use std::mem::size_of;
  use std::thread;
  use std::time::Duration;

  use auv_driver_common::error::DriverResult;
  use auv_driver_common::geometry::Rect;
  use auv_driver_common::window::{Window, WindowMutationKind};
  use windows::Win32::Foundation::{HWND, RECT};
  use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
  use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsIconic, IsWindowVisible, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SetWindowPos, ShowWindow,
  };

  use super::{NativeMutationOutcome, rounded_i32, rounded_positive_i32};
  use crate::error::backend;
  use crate::window::window_handle;

  pub(super) fn perform(window: &Window, kind: WindowMutationKind, settle: Duration) -> DriverResult<NativeMutationOutcome> {
    let hwnd = window_handle(window)?;
    let before = read_state(hwnd)?;
    apply(hwnd, kind)?;
    if !settle.is_zero() {
      thread::sleep(settle);
    }
    let after = read_state(hwnd)?;
    Ok(NativeMutationOutcome {
      before_frame: before.frame,
      after_frame: after.frame,
      before_minimized: before.minimized,
      after_minimized: after.minimized,
      before_visible: before.visible,
      after_visible: after.visible,
    })
  }

  struct ObservedState {
    frame: Rect,
    minimized: bool,
    visible: bool,
  }

  fn read_state(hwnd: HWND) -> DriverResult<ObservedState> {
    Ok(ObservedState {
      frame: window_frame(hwnd)?,
      minimized: unsafe { IsIconic(hwnd) }.as_bool(),
      visible: unsafe { IsWindowVisible(hwnd) }.as_bool(),
    })
  }

  fn apply(hwnd: HWND, kind: WindowMutationKind) -> DriverResult<()> {
    match kind {
      WindowMutationKind::MoveTo { point } => {
        let x = rounded_i32(point.x, "point.x")?;
        let y = rounded_i32(point.y, "point.y")?;
        unsafe { SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) }
          .map_err(|error| backend(format!("SetWindowPos move failed: {error}")))
      }
      WindowMutationKind::Resize { size } => {
        let width = rounded_positive_i32(size.width, "size.width")?;
        let height = rounded_positive_i32(size.height, "size.height")?;
        unsafe { SetWindowPos(hwnd, None, 0, 0, width, height, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE) }
          .map_err(|error| backend(format!("SetWindowPos resize failed: {error}")))
      }
      WindowMutationKind::SetFrame { frame } => {
        let x = rounded_i32(frame.origin.x, "frame.origin.x")?;
        let y = rounded_i32(frame.origin.y, "frame.origin.y")?;
        let width = rounded_positive_i32(frame.size.width, "frame.size.width")?;
        let height = rounded_positive_i32(frame.size.height, "frame.size.height")?;
        unsafe { SetWindowPos(hwnd, None, x, y, width, height, SWP_NOZORDER | SWP_NOACTIVATE) }
          .map_err(|error| backend(format!("SetWindowPos set_frame failed: {error}")))
      }
      // ShowWindow returns the prior visibility as a BOOL and does not signal
      // failure, so the return value is intentionally ignored.
      WindowMutationKind::Minimize => {
        let _ = unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
        Ok(())
      }
      WindowMutationKind::Restore => {
        let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
        Ok(())
      }
      WindowMutationKind::Zoom => {
        let _ = unsafe { ShowWindow(hwnd, SW_MAXIMIZE) };
        Ok(())
      }
    }
  }

  /// Reads the visible window rectangle, preferring the DWM extended frame
  /// bounds and falling back to `GetWindowRect`. Matches `window::native`.
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
}

#[cfg(test)]
#[path = "mutation_test.rs"]
mod tests;
