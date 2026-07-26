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

#[derive(Clone, Copy, Debug)]
pub struct MacosDriverSession {
  pub(crate) _private: (),
}

impl MacosDriverSession {
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
    Ok(MacosDriverSession { _private: () })
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
