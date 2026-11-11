//! Win32 desktop attachment and read-only window probe contracts.
use auv_driver_common::Rect;

/// Errors arising from desktop attachment or lifecycle operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DesktopError {
  #[error("Failed to query thread desktop")]
  QueryFailed,
  #[error("Failed to open input desktop")]
  OpenFailed,
  #[error("Failed to switch thread desktop")]
  SwitchFailed,
  #[error("Failed to restore original thread desktop")]
  RestorationFailed,
  #[error("Failed to close desktop handle")]
  CloseFailed,
}

/// What the OS says about one window handle right now, learned without enumerating windows.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowProbe {
  pub exists: bool,
  pub process_id: u32,
  pub thread_id: u32,
  pub class_name: String,
  pub minimized: bool,
  /// Hidden by DWM, for example on another virtual desktop.
  pub cloaked: bool,
  /// The visible frame, as `list_windows` reports it.
  pub frame: Option<Rect>,
}

impl WindowProbe {
  // Off Windows only the background session's tests describe a closed window.
  #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
  pub fn gone() -> Self {
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
