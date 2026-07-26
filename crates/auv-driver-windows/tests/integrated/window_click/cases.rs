use auv_driver_common::{ClickOptions, CoordinateSpace, Driver, InputPolicy, Rect, Window, WindowPoint, WindowRef};
use auv_driver_windows::WindowsDriver;

#[test]
fn window_click_rejects_background_only_delivery() {
  // ROOT CAUSE:
  //
  // If the CLI live projection executors were checked on Windows, they failed
  // to compile because Windows WindowApi did not expose the window-level click
  // contract already backed by foreground SendInput delivery.
  //
  // Before the fix, the Windows CI job failed with E0599. The fix keeps
  // unsupported background delivery explicit while exposing the shared API.
  let session = WindowsDriver::new().open_local().expect("session should open");
  let window = Window {
    reference: WindowRef {
      id: "fixture-window".to_string(),
    },
    title: Some("Fixture".to_string()),
    app_name: Some("Fixture App".to_string()),
    app_bundle_id: Some("com.example.fixture".to_string()),
    process_id: Some(42),
    frame: Rect::new(100.0, 200.0, 800.0, 600.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };
  let options = ClickOptions {
    policy: InputPolicy::BackgroundOnly,
    ..ClickOptions::default()
  };

  let error = session
    .window()
    .click(&window, WindowPoint::new(20.0, 30.0), options)
    .expect_err("background-only click should be rejected before native input");

  assert_eq!(error.to_string(), "windows window.click cannot use background_only input policy");
}
