use auv_driver_common::{Driver, DriverSession, PlatformKind};

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
  assert_eq!(DriverSession::descriptor(&session), driver.windows_descriptor().as_driver_descriptor());
}
