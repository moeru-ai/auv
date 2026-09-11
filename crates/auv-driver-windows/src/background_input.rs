//! Window-targeted background pointer/wheel delivery via posted Win32 window
//! messages.
//!
//! `crate::input` delivers input through `SendInput`, which requires the
//! target window to be foreground and moves the real system cursor. This
//! module instead posts `WM_LBUTTONDOWN`/`WM_LBUTTONUP`/`WM_LBUTTONDBLCLK`/
//! `WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL` directly to the control hit-tested under
//! the target point via `PostMessageW`, so it never raises, focuses, or
//! activates the window.
//!
//! Delivery is best-effort: a successful `PostMessageW` call only means the
//! message was queued on the target's thread, not that the target processed
//! it. Classic Win32/MFC/WinForms controls read these messages reliably;
//! Chromium/Electron/WinUI/UWP surfaces and most GPU-rendered custom UI read
//! real HID input instead and will silently ignore this path. There is no
//! second, more-compatible Windows route today, so `WindowClickStrategy`'s
//! two variants both resolve to this same delivery on Windows.

use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::Point;
use auv_driver_common::input::{Click, Scroll};
use auv_driver_common::window::Window;

#[cfg(target_os = "windows")]
pub(crate) fn click_at_window(window: &Window, screen_point: Point, click: Click) -> DriverResult<()> {
  native::click(window, screen_point, click)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn click_at_window(_window: &Window, _screen_point: Point, _click: Click) -> DriverResult<()> {
  Err(auv_driver_common::error::DriverError::unsupported("window.click background delivery"))
}

#[cfg(target_os = "windows")]
pub(crate) fn scroll_at_window(window: &Window, screen_point: Point, scroll: Scroll) -> DriverResult<()> {
  native::scroll(window, screen_point, scroll)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn scroll_at_window(_window: &Window, _screen_point: Point, _scroll: Scroll) -> DriverResult<()> {
  Err(auv_driver_common::error::DriverError::unsupported("window.scroll background delivery"))
}

#[cfg(target_os = "windows")]
mod native {
  use auv_driver_common::error::DriverResult;
  use auv_driver_common::geometry::Point;
  use auv_driver_common::input::{Click, Scroll};
  use auv_driver_common::window::Window;
  use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
  use windows::Win32::Graphics::Gdi::ScreenToClient;
  use windows::Win32::System::SystemServices::MK_LBUTTON;
  use windows::Win32::UI::WindowsAndMessaging::{
    CWP_SKIPDISABLED, CWP_SKIPINVISIBLE, CWP_SKIPTRANSPARENT, ChildWindowFromPointEx, PostMessageW, WHEEL_DELTA, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEWHEEL,
  };

  use crate::error::backend;
  use crate::input::click_parts;
  use crate::window::window_handle;

  // NOTICE: bounds hit-test descent against a pathological or cyclic child
  // hierarchy so resolution cannot loop forever.
  const MAX_HIT_TEST_DEPTH: usize = 32;

  pub(super) fn click(window: &Window, screen_point: Point, click: Click) -> DriverResult<()> {
    let target = resolve_target_hwnd(window, screen_point)?;
    let client = screen_to_client(target, screen_point)?;
    let lparam = make_lparam(client.x, client.y);
    let (count, interval) = click_parts(&click)?;
    // Windows' own double-click detection runs on hardware input translation,
    // which posting messages directly bypasses. Posting a second
    // WM_LBUTTONDOWN/UP pair looks like two independent single clicks to
    // controls (list/tree/edit) that key off WM_LBUTTONDBLCLK, so the second
    // press of a `Click::Double` is posted as WM_LBUTTONDBLCLK instead.
    let is_double = matches!(click, Click::Double { .. });
    for index in 0..count {
      let down_message = if is_double && index == 1 {
        WM_LBUTTONDBLCLK
      } else {
        WM_LBUTTONDOWN
      };
      post(target, down_message, WPARAM(MK_LBUTTON.0 as usize), lparam)?;
      post(target, WM_LBUTTONUP, WPARAM(0), lparam)?;
      if index + 1 < count && !interval.is_zero() {
        std::thread::sleep(interval);
      }
    }
    Ok(())
  }

  pub(super) fn scroll(window: &Window, screen_point: Point, scroll: Scroll) -> DriverResult<()> {
    let target = resolve_target_hwnd(window, screen_point)?;
    // WM_MOUSEWHEEL/WM_MOUSEHWHEEL report the pointer position in *screen*
    // coordinates, unlike WM_LBUTTONDOWN/UP which use client coordinates.
    let lparam = make_lparam(round_to_i32(screen_point.x), round_to_i32(screen_point.y));
    let vertical = wheel_amount(scroll.delta_y);
    if vertical != 0 {
      post(target, WM_MOUSEWHEEL, make_wheel_wparam(vertical), lparam)?;
    }
    let horizontal = wheel_amount(scroll.delta_x);
    if horizontal != 0 {
      post(target, WM_MOUSEHWHEEL, make_wheel_wparam(horizontal), lparam)?;
    }
    Ok(())
  }

  /// Descends from the window's top-level `HWND` to the innermost child under
  /// `screen_point`, so posted messages reach the actual control (e.g. a
  /// button or edit box) instead of only the top-level frame.
  fn resolve_target_hwnd(window: &Window, screen_point: Point) -> DriverResult<HWND> {
    let mut current = window_handle(window)?;
    for _ in 0..MAX_HIT_TEST_DEPTH {
      let client_point = screen_to_client(current, screen_point)?;
      let child = unsafe { ChildWindowFromPointEx(current, client_point, CWP_SKIPINVISIBLE | CWP_SKIPDISABLED | CWP_SKIPTRANSPARENT) };
      if child.is_invalid() || child == current {
        break;
      }
      current = child;
    }
    Ok(current)
  }

  fn screen_to_client(hwnd: HWND, screen_point: Point) -> DriverResult<POINT> {
    let mut point = POINT {
      x: round_to_i32(screen_point.x),
      y: round_to_i32(screen_point.y),
    };
    let converted = unsafe { ScreenToClient(hwnd, &mut point) };
    if !converted.as_bool() {
      return Err(backend("ScreenToClient failed to map the target point into window-local coordinates"));
    }
    Ok(point)
  }

  fn post(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> DriverResult<()> {
    unsafe { PostMessageW(hwnd, message, wparam, lparam) }.map_err(|error| backend(format!("PostMessageW failed: {error}")))
  }

  fn round_to_i32(value: f64) -> i32 {
    value.round() as i32
  }

  /// Packs coordinates into an `LPARAM` the way `MAKELPARAM` does: zero-extended
  /// low/high 16-bit words, not sign-extended.
  pub(super) fn make_lparam(x: i32, y: i32) -> LPARAM {
    let low = (x as u32) & 0xFFFF;
    let high = (y as u32) & 0xFFFF;
    LPARAM(((high << 16) | low) as isize)
  }

  /// Packs a signed wheel delta into `WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL`'s
  /// `wParam` high word, matching `MAKEWPARAM(0, delta)`.
  pub(super) fn make_wheel_wparam(delta: i32) -> WPARAM {
    let high = (delta as i16 as u16) as u32;
    WPARAM((high << 16) as usize)
  }

  pub(super) fn wheel_amount(delta: f64) -> i32 {
    if !delta.is_finite() {
      return 0;
    }
    (delta * f64::from(WHEEL_DELTA)).round() as i32
  }
}

#[cfg(all(test, target_os = "windows"))]
#[path = "background_input_test.rs"]
mod tests;
