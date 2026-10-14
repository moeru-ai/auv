#![cfg(target_os = "linux")]

use auv_driver::{Driver, DriverSession, LocalDriver, LocalDriverSession};

/// Regression for the facade selecting the Wayland driver in a pure X11
/// daemon process. Run only inside a dedicated X11 session because xcap pins
/// DISPLAY/XAUTHORITY process-wide.
#[test]
#[ignore = "requires a dedicated X11 DISPLAY with XRandR and XTEST"]
fn local_driver_selects_x11_and_exposes_display_capability() {
  assert!(std::env::var_os("WAYLAND_DISPLAY").is_none_or(|value| value.is_empty()));
  assert!(std::env::var_os("DISPLAY").is_some_and(|value| !value.is_empty()));

  let driver = LocalDriver::new();
  assert_eq!(driver.descriptor().id, "linux.x11");
  let session = driver.open_local().expect("pure X11 LocalDriver session should open");
  assert!(matches!(session, LocalDriverSession::LinuxX11(_)));
  assert_eq!(session.descriptor().id, "linux.x11");
  assert!(!session.display().list().expect("X11 display listing should work through the facade").displays.is_empty());
}
