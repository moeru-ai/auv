use auv_driver_common::error::DriverError;
use auv_driver_common::geometry::{Point, Rect, Size};
use auv_driver_common::window::{WindowState, WindowStateTarget};

use super::*;

// Owner decision 2026-11-03: a maximized window may take a frame and ends normal; a minimized
// window is still refused.
#[test]
fn set_frame_allows_a_maximized_window_and_refuses_a_minimized_one() {
  let normal = WindowState {
    is_minimized: Some(false),
    is_maximized: Some(false),
    is_fullscreen: Some(false),
    ..Default::default()
  };
  assert!(frame_precondition(&normal).is_ok());
  assert!(
    frame_precondition(&WindowState {
      is_maximized: Some(true),
      ..normal.clone()
    })
    .is_ok()
  );

  let minimized = WindowState {
    is_minimized: Some(true),
    is_maximized: Some(true),
    ..normal
  };
  let Err(DriverError::FailedPrecondition { message, recovery }) = frame_precondition(&minimized) else {
    panic!("a minimized window must fail the precondition");
  };
  assert!(message.contains("minimized"), "{message}");
  assert_eq!(recovery.as_deref(), Some("call set_state(Normal) first"));
}

#[test]
fn a_frame_request_keeps_the_parts_it_leaves_out() {
  let current = Rect::new(10.0, 20.0, 300.0, 200.0);

  assert_eq!(requested_frame(current, Some(Point::new(50.0, 60.0)), None), Rect::new(50.0, 60.0, 300.0, 200.0));
  assert_eq!(requested_frame(current, None, Some(Size::new(640.0, 480.0))), Rect::new(10.0, 20.0, 640.0, 480.0));
  assert!(validate_frame_request(None, None).is_err());
}

// The window either lands on the request, settles elsewhere because the app adjusted it, or did
// not change: only the last one fails verification.
#[test]
fn a_settled_frame_is_matched_adjusted_or_unchanged() {
  let before = Rect::new(0.0, 0.0, 300.0, 200.0);
  let requested = Rect::new(100.0, 100.0, 100.0, 50.0);

  assert!(!judge_frame(before, Rect::new(101.0, 99.0, 100.0, 50.0), requested, 2.0).unwrap());
  // Moved, but the app's minimum size kept it larger than requested.
  assert!(judge_frame(before, Rect::new(100.0, 100.0, 160.0, 90.0), requested, 2.0).unwrap());
  assert!(judge_frame(before, before, requested, 2.0).is_err());
}

#[test]
fn a_minimized_window_counts_as_minimized_whatever_it_restores_to() {
  let minimized_from_maximized = WindowState {
    is_minimized: Some(true),
    is_maximized: Some(true),
    ..Default::default()
  };
  let maximized = WindowState {
    is_minimized: Some(false),
    is_maximized: Some(true),
    ..Default::default()
  };

  assert_eq!(current_target(&minimized_from_maximized), WindowStateTarget::Minimized);
  assert_eq!(current_target(&maximized), WindowStateTarget::Maximized);
  assert_eq!(current_target(&WindowState::default()), WindowStateTarget::Normal);
}
