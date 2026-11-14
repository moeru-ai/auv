use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use auv_driver_common::geometry::Rect;
use auv_driver_common::{Driver, DriverDescriptor, DriverResult, DriverSession};

use crate::descriptor::{MacosDriverDescriptor, macos_driver_descriptor};

#[derive(Clone, Copy, Debug, Default)]
pub struct MacosDriver;

impl MacosDriver {
  pub fn new() -> Self {
    Self
  }

  pub fn macos_descriptor(&self) -> MacosDriverDescriptor {
    macos_driver_descriptor()
  }
}

/// Clones share one session state, so every frontend of one Runner sees the
/// same recorded frames.
#[derive(Clone, Debug)]
pub struct MacosDriverSession {
  /// The frames of each window `set_state(Maximized)` resized, keyed by
  /// window id.
  ///
  /// NOTICE(macos-maximize-restore-frame): macOS keeps no restore frame for a
  /// frame AUV wrote, so the driver keeps it in memory. It is not persisted:
  /// after a Runner restart (each `auv invoke` without `--run` gets its own
  /// Runner), or for a window maximized by hand, `set_state(Normal)` leaves the
  /// frame as it is. Persist only with an owner-approved need.
  pub(crate) maximized_windows: Arc<Mutex<HashMap<String, MaximizedFrames>>>,
}

/// The frames of one window that `set_state(Maximized)` resized.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MaximizedFrames {
  /// The frame before maximizing, which `set_state(Normal)` restores.
  pub(crate) normal: Rect,
  /// The frame the window settled at. It differs from the display's visible
  /// frame when the application constrained the frame (`adjusted_by_app`).
  pub(crate) maximized: Rect,
}

impl MacosDriverSession {
  pub(crate) fn new() -> Self {
    Self {
      maximized_windows: Arc::default(),
    }
  }

  pub fn macos_descriptor(&self) -> MacosDriverDescriptor {
    macos_driver_descriptor()
  }
}

impl Driver for MacosDriver {
  type Session = MacosDriverSession;

  fn descriptor(&self) -> DriverDescriptor {
    self.macos_descriptor().as_driver_descriptor()
  }

  fn open_local(&self) -> DriverResult<Self::Session> {
    Ok(MacosDriverSession::new())
  }
}

impl DriverSession for MacosDriverSession {
  fn descriptor(&self) -> DriverDescriptor {
    self.macos_descriptor().as_driver_descriptor()
  }
}

#[cfg(test)]
#[path = "driver_test.rs"]
mod tests;
