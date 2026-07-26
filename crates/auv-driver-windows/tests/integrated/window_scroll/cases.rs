use auv_driver_common::{
  CoordinateSpace, Driver, InputPolicy, Rect, Scroll, ScrollDeliveryCandidate, ScrollDeliveryStrategy, ScrollOptions, Window, WindowPoint,
  WindowRef,
};
use auv_driver_windows::WindowsDriver;

#[test]
fn window_scroll_rejects_background_only_delivery() {
  // ROOT CAUSE:
  //
  // If the runtime was checked on Windows, its scroll-scan source failed to
  // compile because Windows WindowApi did not expose the window-level scroll
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
  let options = ScrollOptions {
    policy: InputPolicy::BackgroundOnly,
    ..ScrollOptions::default()
  };

  let error = session
    .window()
    .scroll(&window, WindowPoint::new(20.0, 30.0), Scroll::new(0.0, -120.0), options)
    .expect_err("background-only scroll should be rejected before native input");

  assert_eq!(error.to_string(), "windows window.scroll cannot use background_only input policy");
}

#[test]
fn window_scroll_requires_foreground_fallback_for_background_preferred_delivery() {
  let session = WindowsDriver::new().open_local().expect("session should open");
  let window = Window {
    reference: WindowRef {
      id: "fixture-window".to_string(),
    },
    title: None,
    app_name: None,
    app_bundle_id: None,
    process_id: Some(42),
    frame: Rect::new(100.0, 200.0, 800.0, 600.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };
  let options = ScrollOptions {
    policy: InputPolicy::BackgroundPreferred,
    delivery_strategy: ScrollDeliveryStrategy {
      candidates: vec![ScrollDeliveryCandidate::AxScroll],
    },
    ..ScrollOptions::default()
  };

  let error = session
    .window()
    .scroll(&window, WindowPoint::new(20.0, 30.0), Scroll::new(0.0, -120.0), options)
    .expect_err("background-preferred scroll needs an available foreground fallback");

  assert_eq!(
    error.to_string(),
    "windows window.scroll needs ForegroundHid in the delivery strategy because background window scroll is not available"
  );
}
