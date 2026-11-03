#![cfg(target_os = "linux")]

use auv_driver_common::{Driver, DriverError, Point, Size, WindowMutationOptions, WindowStateTarget};
use auv_driver_linux::LinuxDriver;

/// Window mutations are not written for Linux yet; each one must refuse with the
/// pull-requests-welcome recovery and leave the window where it was.
///
/// NOTICE: the case uses a window already on the desktop instead of opening its own, because no
/// Linux mutation touches a window today. Open a dedicated window here when
/// `TODO(linux-window-management)` in `auv-driver-linux` is implemented.
#[test]
#[ignore = "needs a Linux desktop session with AT-SPI and at least one window"]
fn linux_window_mutations_are_unsupported_with_recovery() {
  let session = LinuxDriver::new().open_local().unwrap();
  let window = session.window().list().unwrap().into_iter().next().expect("the desktop must list a window");

  let options = WindowMutationOptions::default;
  let results = [
    ("window.setFrame", session.window().set_frame(&window, Some(Point::new(10.0, 10.0)), Some(Size::new(400.0, 300.0)), options())),
    ("window.setState", session.window().set_state(&window, WindowStateTarget::Minimized, options())),
    ("window.focus", session.window().focus(&window, options())),
  ];
  for (expected, result) in results {
    match result {
      Err(DriverError::Unsupported {
        operation,
        recovery,
      }) => {
        assert_eq!(operation, expected);
        assert!(recovery.is_some_and(|recovery| recovery.contains("pull requests are welcome")), "{operation}: {recovery:?}");
      }
      other => panic!("{expected} returned {other:?}, not Unsupported"),
    }
  }

  let after = session.window().get(&window.reference.id).unwrap();
  assert_eq!(after.frame, window.frame, "a refused mutation moved the window");
}
