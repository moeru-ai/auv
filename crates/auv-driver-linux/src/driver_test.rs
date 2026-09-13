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

#[cfg(target_os = "linux")]
#[test]
fn portal_identity_accepts_ashpd_app_id_at_configuration() {
  // The former three-component rule rejected IDs accepted by Portal registration.
  let driver = LinuxDriver::new().with_portal_app_id("org.Example".into()).unwrap();
  let session = driver.open_local().unwrap();
  let state = session.state.lock().unwrap();
  assert_eq!(state.portal_app_id.as_deref(), Some("org.Example"));
}

#[cfg(target_os = "linux")]
#[test]
fn portal_identity_reports_invalid_input_before_opening_a_session() {
  assert!(matches!(
    LinuxDriver::new().with_portal_app_id("org.123.Example".into()),
    Err(auv_driver_common::DriverError::InvalidInput { .. })
  ));
}
