use auv_driver_common::Driver;
use auv_driver_common::capture::{Activation, CaptureOptions};
use auv_driver_common::geometry::{CoordinateSpace, RatioRect, Rect, ScreenPoint, WindowPoint};
use auv_driver_common::input::{ClickOptions, InputPolicy, Scroll, ScrollDeliveryCandidate, ScrollOptions, WaitOptions, WindowInput};
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

#[test]
fn window_click_background_only_fails_for_invalid_window_handle() {
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
    .expect_err("background click posts window messages, so a fake test window handle fails at the Win32 call");

  assert!(error.to_string().contains("ScreenToClient"));
}

#[test]
fn window_scroll_background_only_fails_for_invalid_window_handle() {
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
    .expect_err("background scroll posts window messages, so a fake test window handle fails at the Win32 call");

  assert!(error.to_string().contains("background_scroll is not supported"));
}

#[test]
fn window_scroll_attempts_window_targeted_wheel_without_requiring_foreground_candidate() {
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
    .expect_err("the fake test window handle fails the background wheel post, not a missing-ForegroundHid validation error");

  // Fails from the actual background wheel attempt (Win32 call against a fake
  // handle), not from an upfront "needs ForegroundHid" validation error.
  assert!(error.to_string().contains("background_scroll is not supported"));
}

// Live smoke tests: poll a real enumerated top-level window for text that can
// never appear, proving the polling loop terminates at the timeout instead of
// hanging, and that `wait_text` surfaces `NotFound` in that case. Skips
// cleanly when no windows are present (headless session).
fn short_wait() -> WaitOptions {
  WaitOptions {
    timeout: std::time::Duration::from_millis(50),
    poll_interval: std::time::Duration::from_millis(10),
  }
}

#[cfg(target_os = "windows")]
#[test]
fn find_text_returns_no_match_for_unmatchable_query_on_a_live_window() {
  let session = session();
  let Some(window) = session.window().list().expect("list windows").into_iter().next() else {
    return;
  };

  let matches = session
    .window()
    .find_text(&window, "auv-driver-windows-find-text-query-that-never-matches", RatioRect::new(0.0, 0.0, 1.0, 1.0), short_wait())
    .expect("find_text succeeds even without a match");

  assert!(matches.matches.is_empty());
}

#[cfg(target_os = "windows")]
#[test]
fn wait_text_fails_with_not_found_for_unmatchable_query_on_a_live_window() {
  let session = session();
  let Some(window) = session.window().list().expect("list windows").into_iter().next() else {
    return;
  };

  let error = session
    .window()
    .wait_text(&window, "auv-driver-windows-wait-text-query-that-never-matches", RatioRect::new(0.0, 0.0, 1.0, 1.0), short_wait())
    .expect_err("wait_text fails when the query never matches before the timeout");

  assert!(error.to_string().contains("before timeout"));
}
