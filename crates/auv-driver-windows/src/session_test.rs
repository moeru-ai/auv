use auv_driver_common::Driver;
use auv_driver_common::capture::{Activation, CaptureOptions};
#[cfg(target_os = "windows")]
use auv_driver_common::geometry::RelativeRect;
use auv_driver_common::geometry::{CoordinateSpace, Rect, ScreenPoint, WindowPoint};
use auv_driver_common::input::{ClickOptions, InputPolicy, Scroll, ScrollDeliveryCandidate, ScrollOptions, WaitOptions, WindowInput};
use auv_driver_common::window::{Window, WindowRef};
use auv_driver_common::{InputTarget, KeyboardInput, PressKeysOptions};

use super::{screen_point_for_window_point, window_point_for_screen_point};
use crate::WindowsDriver;

fn session() -> crate::WindowsDriverSession {
  WindowsDriver::new().open_local().expect("session opens")
}

#[test]
fn keyboard_batch_rejects_invalid_tail_before_any_delivery() {
  let actions = vec![
    KeyboardInput::PressKeys {
      options: PressKeysOptions {
        keys: vec!["a".into()],
        ..Default::default()
      },
      policy: InputPolicy::ForegroundPreferred,
    },
    KeyboardInput::PressKeys {
      options: PressKeysOptions {
        keys: vec!["control".into(), "a".into()],
        count: 0,
        ..Default::default()
      },
      policy: InputPolicy::ForegroundPreferred,
    },
  ];

  let error = session().input().input_keyboard(&InputTarget::Foreground, actions, false).unwrap_err();

  assert_eq!(error.progress.action_index, 1);
  assert!(error.progress.completed.is_empty());
  assert_eq!(error.progress.completed_presses, 0);
}

#[test]
fn keyboard_dry_run_accepts_foreground_press_and_text_without_delivery() {
  let actions = vec![
    KeyboardInput::PressKeys {
      options: PressKeysOptions {
        keys: vec!["control".into(), "a".into()],
        ..Default::default()
      },
      policy: InputPolicy::ForegroundPreferred,
    },
    KeyboardInput::TypeText {
      text: "AUV probe".into(),
      options: auv_driver_common::TypeTextOptions {
        policy: InputPolicy::ForegroundPreferred,
        ..Default::default()
      },
    },
  ];

  assert_eq!(session().input().input_keyboard(&InputTarget::Foreground, actions, true).unwrap(), None);
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
fn window_point_screen_point_bidirectional_roundtrip_isomorphism() {
  let window = sample_window(); // frame origin (100.0, 200.0), size (800.0, 600.0)

  let test_window_points = [
    WindowPoint::new(0.0, 0.0),
    WindowPoint::new(12.345, 67.891),
    WindowPoint::new(400.0, 300.0),
    WindowPoint::new(800.0, 600.0),
    WindowPoint::new(-50.0, -25.0),
    WindowPoint::new(1500.0, 1200.0),
  ];

  for wp in test_window_points {
    let sp = screen_point_for_window_point(&window, wp);
    let back_to_wp = window_point_for_screen_point(&window, sp);
    assert!(
      (back_to_wp.point().x - wp.point().x).abs() < 1e-12 && (back_to_wp.point().y - wp.point().y).abs() < 1e-12,
      "Roundtrip failed for WindowPoint {wp:?} -> {back_to_wp:?}"
    );
  }

  let test_screen_points = [
    ScreenPoint::new(0.0, 0.0),
    ScreenPoint::new(100.0, 200.0),
    ScreenPoint::new(900.0, 800.0),
    ScreenPoint::new(-1920.0, 100.0),
    ScreenPoint::new(1920.5, 1080.75),
  ];

  for sp in test_screen_points {
    let wp = window_point_for_screen_point(&window, sp);
    let back_to_sp = screen_point_for_window_point(&window, wp);
    assert!(
      (back_to_sp.point().x - sp.point().x).abs() < 1e-12 && (back_to_sp.point().y - sp.point().y).abs() < 1e-12,
      "Roundtrip failed for ScreenPoint {sp:?} -> {back_to_sp:?}"
    );
  }
}

#[test]
fn coordinate_mapping_with_negative_and_nonzero_window_origins() {
  // Case A: Secondary monitor positioned to the left of the primary monitor in Windows display settings.
  // Secondary monitor origin is (-1920.0, 0.0), window on that monitor has negative origin.
  let mut left_window = sample_window();
  left_window.frame = Rect::new(-1500.0, 100.0, 1000.0, 800.0);

  // Top-left of window maps to its negative screen frame origin
  assert_eq!(screen_point_for_window_point(&left_window, WindowPoint::new(0.0, 0.0)), ScreenPoint::new(-1500.0, 100.0));
  // Interior point
  assert_eq!(screen_point_for_window_point(&left_window, WindowPoint::new(250.0, 300.0)), ScreenPoint::new(-1250.0, 400.0));
  // Inverse mapping preserves negative coordinates
  assert_eq!(window_point_for_screen_point(&left_window, ScreenPoint::new(-1250.0, 400.0)), WindowPoint::new(250.0, 300.0));

  // Case B: Secondary monitor positioned above the primary monitor (negative Y).
  let mut top_window = sample_window();
  top_window.frame = Rect::new(200.0, -1080.0, 1920.0, 1080.0);

  assert_eq!(screen_point_for_window_point(&top_window, WindowPoint::new(100.0, 80.0)), ScreenPoint::new(300.0, -1000.0));
  assert_eq!(window_point_for_screen_point(&top_window, ScreenPoint::new(300.0, -1000.0)), WindowPoint::new(100.0, 80.0));
}

#[test]
fn coordinate_mapping_updates_after_window_movement_and_resizing() {
  let mut window = sample_window(); // Initial: origin (100.0, 200.0), size (800.0, 600.0)

  let local_point = WindowPoint::new(50.0, 50.0);
  assert_eq!(screen_point_for_window_point(&window, local_point), ScreenPoint::new(150.0, 250.0));

  // 1. Move window to new origin (500.0, 600.0)
  window.frame.origin = auv_driver_common::geometry::Point::new(500.0, 600.0);
  // The same window-local point now projects to the updated screen location
  assert_eq!(screen_point_for_window_point(&window, local_point), ScreenPoint::new(550.0, 650.0));
  // The prior screen point now maps to an offset outside the window
  assert_eq!(window_point_for_screen_point(&window, ScreenPoint::new(150.0, 250.0)), WindowPoint::new(-350.0, -350.0));

  // 2. Resize window to (1200.0, 900.0)
  window.frame.size = auv_driver_common::geometry::Size::new(1200.0, 900.0);
  assert_eq!(window.frame.center(), auv_driver_common::geometry::Point::new(1100.0, 1050.0));
  assert_eq!(window.frame.at(0.5, 0.5), auv_driver_common::geometry::Point::new(1100.0, 1050.0));
  // Points that were outside the old size are now inside the resized window frame
  let new_interior_screen = ScreenPoint::new(1500.0, 1200.0);
  let new_interior_local = window_point_for_screen_point(&window, new_interior_screen);
  assert_eq!(new_interior_local, WindowPoint::new(1000.0, 600.0));
  assert!(window.frame.contains_point(new_interior_screen.point()));
}

#[test]
fn window_region_boundaries_and_out_of_bounds_detection() {
  let window = sample_window(); // frame: Rect::new(100.0, 200.0, 800.0, 600.0)

  // 1. Exact boundary values (corners and edges must be inclusive)
  let top_left = screen_point_for_window_point(&window, WindowPoint::new(0.0, 0.0)).point();
  let top_right = screen_point_for_window_point(&window, WindowPoint::new(800.0, 0.0)).point();
  let bottom_left = screen_point_for_window_point(&window, WindowPoint::new(0.0, 600.0)).point();
  let bottom_right = screen_point_for_window_point(&window, WindowPoint::new(800.0, 600.0)).point();

  assert_eq!(top_left, auv_driver_common::geometry::Point::new(100.0, 200.0));
  assert_eq!(top_right, auv_driver_common::geometry::Point::new(900.0, 200.0));
  assert_eq!(bottom_left, auv_driver_common::geometry::Point::new(100.0, 800.0));
  assert_eq!(bottom_right, auv_driver_common::geometry::Point::new(900.0, 800.0));

  assert!(window.frame.contains_point(top_left));
  assert!(window.frame.contains_point(top_right));
  assert!(window.frame.contains_point(bottom_left));
  assert!(window.frame.contains_point(bottom_right));

  // 2. Out-of-bounds boundary values (just outside)
  let outside_left = screen_point_for_window_point(&window, WindowPoint::new(-0.01, 300.0)).point();
  let outside_right = screen_point_for_window_point(&window, WindowPoint::new(800.01, 300.0)).point();
  let outside_top = screen_point_for_window_point(&window, WindowPoint::new(400.0, -0.01)).point();
  let outside_bottom = screen_point_for_window_point(&window, WindowPoint::new(400.0, 600.01)).point();

  assert!(!window.frame.contains_point(outside_left));
  assert!(!window.frame.contains_point(outside_right));
  assert!(!window.frame.contains_point(outside_top));
  assert!(!window.frame.contains_point(outside_bottom));

  // 3. Grossly out of bounds
  let far_negative = screen_point_for_window_point(&window, WindowPoint::new(-1000.0, -1000.0)).point();
  let far_positive = screen_point_for_window_point(&window, WindowPoint::new(5000.0, 5000.0)).point();

  assert!(!window.frame.contains_point(far_negative));
  assert!(!window.frame.contains_point(far_positive));
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

// The failure comes from the Win32 call; other platforms return `Unsupported`.
#[cfg(target_os = "windows")]
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
  let Some(window) = session.window().list().expect("list windows").into_iter().find(|w| session.window().capture(w).is_ok()) else {
    return;
  };

  let matches = session
    .window()
    .find_text(&window, "auv-driver-windows-find-text-query-that-never-matches", RelativeRect::new(0.0, 0.0, 1.0, 1.0), short_wait())
    .expect("find_text succeeds even without a match");

  assert!(matches.matches.is_empty());
}

#[cfg(target_os = "windows")]
#[test]
fn wait_text_fails_with_not_found_for_unmatchable_query_on_a_live_window() {
  let session = session();
  let Some(window) = session.window().list().expect("list windows").into_iter().find(|w| session.window().capture(w).is_ok()) else {
    return;
  };

  let error = session
    .window()
    .wait_text(&window, "auv-driver-windows-wait-text-query-that-never-matches", RelativeRect::new(0.0, 0.0, 1.0, 1.0), short_wait())
    .expect_err("wait_text fails when the query never matches before the timeout");

  assert!(error.to_string().contains("before timeout"));
}

#[test]
fn background_click_rejects_alt_meta_before_target_activation() {
  let modifiers = auv_driver_common::ClickModifiers {
    meta: true,
    ..Default::default()
  };
  let driver = session();
  let window = sample_window();
  for policy in [
    InputPolicy::BackgroundOnly,
    InputPolicy::BackgroundPreferred,
  ] {
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
    assert!(error.to_string().contains("windows background click supports only shift/control"));
  }
}
