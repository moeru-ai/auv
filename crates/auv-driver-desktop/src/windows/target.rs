//! Shared read-only Win32 target queries, used by capture and existing driver consumers.
#![cfg_attr(not(target_os = "windows"), allow(dead_code, unused_imports))]
#[cfg(target_os = "windows")]
use ::windows::Win32::Foundation::{HWND, RECT};
#[cfg(target_os = "windows")]
use ::windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
#[cfg(target_os = "windows")]
use ::windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindow};
use auv_driver_common::error::{backend, invalid_input};
use auv_driver_common::{DriverResult, Rect, Window};
#[cfg(target_os = "windows")]
use std::{ffi::c_void, mem::size_of};

pub use auv_driver_desktop_common::windows::WindowProbe;

/// Parses the native `HWND` value previously encoded in a [`Window`] reference.
///
/// Window references carry the `HWND` as a decimal `isize` string (see
/// `observe_window`). Mutation and window capture both need to recover the raw
/// handle, so the parse lives here next to the encoding it mirrors.
#[cfg(target_os = "windows")]
pub fn window_handle(window: &Window) -> DriverResult<::windows::Win32::Foundation::HWND> {
  let raw: isize = window
    .reference
    .id
    .parse()
    .map_err(|_| invalid_input(format!("window reference {:?} is not a valid window handle", window.reference.id)))?;
  Ok(::windows::Win32::Foundation::HWND(raw as *mut std::ffi::c_void))
}

#[cfg(target_os = "windows")]
pub fn probe_window(raw: isize) -> WindowProbe {
  let hwnd = HWND(raw as *mut c_void);
  // SAFETY: read-only queries on a handle that may have been destroyed; each fails harmlessly.
  unsafe {
    if !IsWindow(hwnd).as_bool() {
      return WindowProbe::gone();
    }
    let mut process_id = 0u32;
    let thread_id = GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    let mut name = [0u16; 256];
    let length = GetClassNameW(hwnd, &mut name).max(0) as usize;
    WindowProbe {
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

#[cfg(target_os = "windows")]
/// Reads the visible window rectangle, preferring the DWM extended frame
/// bounds and falling back to `GetWindowRect` when DWM is unavailable.
pub fn window_frame(hwnd: HWND) -> DriverResult<Rect> {
  let mut rect = RECT::default();
  let dwm =
    unsafe { DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut RECT as *mut c_void, size_of::<RECT>() as u32) };
  if dwm.is_err() {
    unsafe {
      GetWindowRect(hwnd, &mut rect).map_err(|error| backend(format!("GetWindowRect failed: {error}")))?;
    }
  }
  Ok(rect_from(rect))
}

/// Reports whether DWM has cloaked the window (for example a UWP app on a
/// virtual desktop), which `IsWindowVisible` still reports as visible.
#[cfg(target_os = "windows")]
pub fn is_cloaked(hwnd: HWND) -> bool {
  let mut cloaked: u32 = 0;
  let result = unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, size_of::<u32>() as u32) };
  result.is_ok() && cloaked != 0
}

#[cfg(target_os = "windows")]
pub fn rect_from(rect: RECT) -> Rect {
  Rect::new(f64::from(rect.left), f64::from(rect.top), f64::from(rect.right - rect.left), f64::from(rect.bottom - rect.top))
}
