use auv_driver_common::{Driver, DriverSession, PlatformKind};

use crate::MacosDriver;

#[test]
fn descriptor_uses_desktop_namespace() {
  let descriptor = MacosDriver::new().macos_descriptor();

  assert_eq!(descriptor.id, "macos.desktop");
  assert_eq!(descriptor.platform, PlatformKind::Macos);
}

#[test]
fn session_exposes_driver_descriptor() {
  let driver = MacosDriver::new();
  let session = driver.open_local().expect("session should open");

  assert_eq!(session.macos_descriptor(), driver.macos_descriptor());
  assert_eq!(DriverSession::descriptor(&session), driver.macos_descriptor().as_driver_descriptor());
}
