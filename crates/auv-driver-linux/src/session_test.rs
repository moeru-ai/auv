use auv_driver_common::Driver;
use auv_driver_common::geometry::{CoordinateSpace, Rect, ScreenPoint, WindowPoint};
use auv_driver_common::window::{Window, WindowRef};

use super::*;
use crate::LinuxDriver;

fn session() -> LinuxDriverSession {
  LinuxDriver::new().open_local().expect("session opens")
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

  let point = session().window().to_screen_point(&window, WindowPoint::new(25.0, 30.0)).expect("point maps");

  assert_eq!(point, ScreenPoint::new(125.0, 230.0));
}

#[test]
fn screen_point_converts_to_window_point() {
  let window = sample_window();

  let point = session().window().to_window_point(&window, ScreenPoint::new(125.0, 230.0)).expect("point maps");

  assert_eq!(point, WindowPoint::new(25.0, 30.0));
}

#[test]
fn window_capture_with_rejects_nested_capture_options() {
  let window = sample_window();
  let error = session()
    .window()
    .capture_with(
      &window,
      CaptureOptions {
        window: Some(window.reference.clone()),
        ..CaptureOptions::default()
      },
    )
    .expect_err("nested window capture is rejected before portal capture");

  assert!(error.to_string().contains("window.capture_with"));
}

#[test]
fn window_capture_with_rejects_activation_on_linux() {
  let window = sample_window();
  let error = session()
    .window()
    .capture_with(
      &window,
      CaptureOptions {
        activation: Activation::ActivateFirst {
          settle: std::time::Duration::ZERO,
        },
        ..CaptureOptions::default()
      },
    )
    .expect_err("activation is rejected before portal capture");

  assert!(error.to_string().contains("cannot activate Linux Wayland"));
}

#[test]
fn window_click_rejects_background_only_policy() {
  let window = sample_window();
  let error = session()
    .window()
    .click(
      &window,
      WindowPoint::new(1.0, 1.0),
      ClickOptions {
        policy: InputPolicy::BackgroundOnly,
        ..ClickOptions::default()
      },
    )
    .expect_err("background-only window click is rejected before portal input");

  assert!(error.to_string().contains("background_only"));
}

#[test]
fn window_scroll_rejects_background_only_policy() {
  let window = sample_window();
  let error = session()
    .window()
    .scroll(
      &window,
      WindowPoint::new(1.0, 1.0),
      Scroll::new(0.0, 10.0),
      ScrollOptions {
        policy: InputPolicy::BackgroundOnly,
        ..ScrollOptions::default()
      },
    )
    .expect_err("background-only window scroll is rejected before portal input");

  assert!(error.to_string().contains("background_only"));
}

#[test]
fn window_scroll_requires_foreground_candidate_for_background_preferred_policy() {
  let window = sample_window();
  let error = session()
    .window()
    .scroll(
      &window,
      WindowPoint::new(1.0, 1.0),
      Scroll::new(0.0, 10.0),
      ScrollOptions {
        delivery_strategy: auv_driver_common::input::ScrollDeliveryStrategy {
          candidates: vec![ScrollDeliveryCandidate::WindowTargetedWheel],
        },
        ..ScrollOptions::default()
      },
    )
    .expect_err("foreground fallback must be explicitly allowed");

  assert!(error.to_string().contains("ForegroundHid"));
}
