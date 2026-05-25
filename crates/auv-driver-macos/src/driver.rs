use auv_driver::{Driver, DriverDescriptor, DriverResult, DriverSession};

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
  _private: (),
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
mod tests {
  use auv_driver::{Driver, DriverSession, PlatformKind};

  use crate::{MacosDriver, macos_legacy_descriptor_metadata};

  #[test]
  fn descriptor_uses_desktop_namespace() {
    let descriptor = MacosDriver::new().macos_descriptor();

    assert_eq!(descriptor.id, "macos.desktop");
    assert_eq!(descriptor.platform, PlatformKind::Macos);
    let metadata = macos_legacy_descriptor_metadata();
    assert_eq!(metadata.descriptor, descriptor);
    assert!(
      metadata
        .capabilities
        .iter()
        .any(|capability| *capability == "desktop.capture-window")
    );
    assert!(
      !metadata
        .capabilities
        .iter()
        .any(|capability| capability.starts_with("observe."))
    );
  }

  #[test]
  fn session_exposes_driver_descriptor() {
    let driver = MacosDriver::new();
    let session = driver.open_local().expect("session should open");

    assert_eq!(session.macos_descriptor(), driver.macos_descriptor());
    assert_eq!(
      DriverSession::descriptor(&session),
      driver.macos_descriptor().as_driver_descriptor()
    );
  }
}
