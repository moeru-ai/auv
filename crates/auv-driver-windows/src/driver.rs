use auv_driver::{Driver, DriverDescriptor, DriverResult, DriverSession};

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
    Ok(WindowsDriverSession { _private: () })
  }
}

impl DriverSession for WindowsDriverSession {
  fn descriptor(&self) -> DriverDescriptor {
    self.windows_descriptor().as_driver_descriptor()
  }
}

#[cfg(test)]
mod tests {
  use auv_driver::{Driver, DriverSession, PlatformKind};

  use crate::WindowsDriver;

  #[test]
  fn descriptor_uses_desktop_namespace() {
    let descriptor = WindowsDriver::new().windows_descriptor();

    assert_eq!(descriptor.id, "windows.desktop");
    assert_eq!(descriptor.platform, PlatformKind::Windows);
  }

  #[test]
  fn session_exposes_driver_descriptor() {
    let driver = WindowsDriver::new();
    let session = driver.open_local().expect("session should open");

    assert_eq!(session.windows_descriptor(), driver.windows_descriptor());
    assert_eq!(
      DriverSession::descriptor(&session),
      driver.windows_descriptor().as_driver_descriptor()
    );
  }
}
