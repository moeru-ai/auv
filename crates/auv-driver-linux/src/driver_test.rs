use auv_driver_common::{Driver, DriverSession, PlatformKind};

use crate::LinuxDriver;

#[test]
fn descriptor_uses_desktop_namespace() {
  let descriptor = LinuxDriver::new().linux_descriptor();

  assert_eq!(descriptor.id, "linux.desktop");
  assert_eq!(descriptor.platform, PlatformKind::Linux);
}

#[test]
fn session_exposes_driver_descriptor() {
  let driver = LinuxDriver::new();
  let session = driver.open_local().expect("session should open");

  assert_eq!(session.linux_descriptor(), driver.linux_descriptor());
  assert_eq!(DriverSession::descriptor(&session), driver.linux_descriptor().as_driver_descriptor());
}
