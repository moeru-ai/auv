use auv_driver_common::{Driver, DriverDescriptor, DriverResult, DriverSession};

use crate::descriptor::{WindowsDriverDescriptor, windows_driver_descriptor};

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsDriver;

impl WindowsDriver {
  pub fn new() -> Self {
    Self
  }

  pub fn windows_descriptor(&self) -> WindowsDriverDescriptor {
    windows_driver_descriptor()
  }
}

#[derive(Clone, Copy, Debug)]
pub struct WindowsDriverSession {
  pub(crate) _private: (),
}

impl WindowsDriverSession {
  pub fn windows_descriptor(&self) -> WindowsDriverDescriptor {
    windows_driver_descriptor()
  }
}

impl Driver for WindowsDriver {
  type Session = WindowsDriverSession;

  fn descriptor(&self) -> DriverDescriptor {
    self.windows_descriptor().as_driver_descriptor()
  }

  fn open_local(&self) -> DriverResult<Self::Session> {
    // The host owns its DPI awareness; the driver only reports a host that would get
    // virtualized coordinates. See NOTICE(windows-dpi-awareness).
    #[cfg(target_os = "windows")]
    crate::dpi::warn_if_not_per_monitor_aware();
    Ok(WindowsDriverSession { _private: () })
  }
}

impl DriverSession for WindowsDriverSession {
  fn descriptor(&self) -> DriverDescriptor {
    self.windows_descriptor().as_driver_descriptor()
  }
}

#[cfg(test)]
#[path = "driver_test.rs"]
mod tests;
