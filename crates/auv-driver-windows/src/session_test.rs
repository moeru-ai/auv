use auv_driver_common::Driver;
use auv_driver_common::capture::{Activation, CaptureOptions};
use auv_driver_common::geometry::{CoordinateSpace, Rect, ScreenPoint, WindowPoint};
use auv_driver_common::window::{Window, WindowRef};

use super::{screen_point_for_window_point, window_point_for_screen_point};
use crate::WindowsDriver;

fn session() -> crate::WindowsDriverSession {
  WindowsDriver::new().open_local().expect("session opens")
}

fn sample_window() -> Window {
  Window {
    reference: WindowRef {
      id: "42".to_string(),
    },
    title: None,
    app_name: None,
    app_bundle_id: None,
    process_id: Some(123),
    frame: Rect::new(100.0, 200.0, 800.0, 600.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  }
}

#[test]
fn window_point_converts_to_screen_point() {
  let window = sample_window();

  let point = screen_point_for_window_point(&window, WindowPoint::new(25.0, 30.0));

  assert_eq!(point, ScreenPoint::new(125.0, 230.0));
}

#[test]
fn screen_point_converts_to_window_point() {
  let window = sample_window();

  let point = window_point_for_screen_point(&window, ScreenPoint::new(125.0, 230.0));

  assert_eq!(point, WindowPoint::new(25.0, 30.0));
}

#[test]
fn capture_rejects_region_option() {
  let options = CaptureOptions {
    region: Some(Rect::new(0.0, 0.0, 10.0, 10.0)),
    ..CaptureOptions::default()
  };

  assert!(session().display().capture(options).is_err());
}

#[test]
fn capture_rejects_activation_without_app_target() {
  let options = CaptureOptions {
    activation: Activation::ActivateFirst {
      settle: std::time::Duration::from_millis(0),
    },
    ..CaptureOptions::default()
  };

  assert!(session().display().capture(options).is_err());
}

#[test]
fn capture_region_requires_region() {
  assert!(session().display().capture_region(CaptureOptions::default()).is_err());
}
