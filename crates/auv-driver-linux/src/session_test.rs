use auv_driver_common::geometry::{CoordinateSpace, Rect, ScreenPoint, WindowPoint};
use auv_driver_common::input::DisturbanceLevel;
use auv_driver_common::window::{Window, WindowRef};
use auv_driver_common::{Driver, WindowInput as _};

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
    application_id: None,
    process_id: Some(123),
    frame: Rect::new(100.0, 200.0, 800.0, 600.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
    state: Default::default(),
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

#[test]
fn modified_background_click_is_rejected_before_portal_input() {
  let modifiers = auv_driver_common::ClickModifiers {
    meta: true,
    ..Default::default()
  };
  let driver = session();
  let window = sample_window();
  for policy in [InputPolicy::BackgroundOnly] {
    let error = driver
      .window()
      .click(
        &window,
        WindowPoint::new(1.0, 1.0),
        ClickOptions {
          policy,
          modifiers,
          ..Default::default()
        },
      )
      .unwrap_err();
    assert!(error.to_string().contains("linux window.click cannot use background_only"));
  }
}

#[test]
fn window_management_is_unsupported_with_a_pull_request_invitation() {
  let driver = session();
  let window = sample_window();
  let api = driver.window();
  let results = [
    ("window.setFrame", api.set_frame(&window, Some(Point::new(0.0, 0.0)), None, WindowMutationOptions::default())),
    ("window.setState", api.set_state(&window, WindowStateTarget::Minimized, WindowMutationOptions::default())),
    ("window.focus", api.focus(&window, WindowMutationOptions::default())),
  ];
  for (expected, result) in results {
    let error = result.unwrap_err();
    assert!(matches!(error, DriverError::Unsupported { operation, .. } if operation == expected));
    assert!(
      error.to_string().contains("Linux support is not written yet; pull requests are welcome at https://github.com/moeru-ai/auv"),
      "{error}"
    );
  }
}

// ROOT CAUSE:
//
// If AT-SPI focused the target window before a foreground portal click, the
// click result failed `InputActionResult::validate` and `window.click`
// returned gRPC `INTERNAL` although the click was delivered, because the
// successful `AxFocus` focus step was recorded as a delivery attempt on a path
// other than `selected_path`.
//
// Before the fix, the successful focus attempt was spliced into `attempts`.
// The fix keeps `attempts` to delivery attempts and fallback reasons: a
// successful focus shows only as a foreground focus disturbance.
#[test]
fn focused_foreground_click_result_passes_input_contract_validation() {
  let mut result = InputActionResult {
    selected_path: InputDeliveryPath::ForegroundSystemEvents,
    attempts: vec![InputAttempt::success(
      InputDeliveryPath::ForegroundSystemEvents,
    )],
    verified: false,
    mouse_disturbance: DisturbanceLevel::Temporary,
    focus_disturbance: DisturbanceLevel::Unknown,
    clipboard_disturbance: DisturbanceLevel::None,
  };
  let focus = InputActionResult {
    focus_disturbance: DisturbanceLevel::Foreground,
    ..InputActionResult::single_success(InputDeliveryPath::AxFocus)
  };

  record_focus_preparation(&mut result, Ok(focus));

  assert_eq!(result.validate(), Ok(()));
  assert_eq!(
    result.attempts,
    vec![InputAttempt::success(
      InputDeliveryPath::ForegroundSystemEvents
    )]
  );
  assert_eq!(result.focus_disturbance, DisturbanceLevel::Foreground);
}
