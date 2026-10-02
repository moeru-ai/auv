//! Win32 desktop and window station management.
//!
//! Provides thread desktop attachment helpers to ensure background tasks,
//! benchmarks, and capture sessions interact with the interactive desktop.

#[cfg(target_os = "windows")]
pub fn ensure_input_desktop() {
  unsafe {
    use windows::Win32::System::StationsAndDesktops::{DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS, OpenInputDesktop, SetThreadDesktop};
    if let Ok(desktop) = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_ACCESS_FLAGS(0x000F_01FF)) {
      let _ = SetThreadDesktop(desktop);
    }
  }
}

#[cfg(not(target_os = "windows"))]
pub fn ensure_input_desktop() {}
