use auv_driver_common::geometry::{Point, Rect, Size};
use auv_driver_common::input::DisturbanceLevel;
use auv_driver_common::window::{WindowMutationKind, WindowMutationPath, WindowMutationVerification, WindowState};

use super::*;

fn outcome(before: Rect, after: Rect) -> NativeMutationOutcome {
  NativeMutationOutcome {
    before_frame: before,
    after_frame: after,
    before_minimized: false,
    after_minimized: false,
    before_visible: true,
    after_visible: true,
  }
}

#[test]
fn move_result_reports_platform_native_path() {
  let kind = WindowMutationKind::MoveTo {
    point: Point::new(10.0, 20.0),
  };
  let result = window_mutation_result(kind, outcome(Rect::new(0.0, 0.0, 100.0, 80.0), Rect::new(10.0, 20.0, 100.0, 80.0)));

  assert_eq!(result.selected_path, WindowMutationPath::PlatformNative);
  assert_eq!(result.focus_disturbance, DisturbanceLevel::None);
  assert!(result.attempts[0].succeeded);
}

#[test]
fn state_change_reports_foreground_focus_disturbance() {
  let result =
    window_mutation_result(WindowMutationKind::Minimize, outcome(Rect::new(0.0, 0.0, 100.0, 80.0), Rect::new(0.0, 0.0, 100.0, 80.0)));

  assert_eq!(result.focus_disturbance, DisturbanceLevel::Foreground);
}

#[test]
fn move_within_tolerance_passes_verification() {
  let kind = WindowMutationKind::MoveTo {
    point: Point::new(10.0, 20.0),
  };
  let result = window_mutation_result(kind, outcome(Rect::new(0.0, 0.0, 100.0, 80.0), Rect::new(11.0, 19.0, 100.0, 80.0)));

  assert!(verify_window_mutation(kind, &WindowMutationVerification::FrameTolerance { points: 2.0 }, &result).is_ok());
}

#[test]
fn move_beyond_tolerance_fails_verification() {
  let kind = WindowMutationKind::MoveTo {
    point: Point::new(10.0, 20.0),
  };
  let result = window_mutation_result(kind, outcome(Rect::new(0.0, 0.0, 100.0, 80.0), Rect::new(50.0, 20.0, 100.0, 80.0)));

  assert!(verify_window_mutation(kind, &WindowMutationVerification::FrameTolerance { points: 2.0 }, &result).is_err());
}

#[test]
fn resize_verifies_size_only() {
  let kind = WindowMutationKind::Resize {
    size: Size::new(640.0, 480.0),
  };
  let result = window_mutation_result(
    kind,
    // Origin moved but size matched; resize verification ignores origin.
    outcome(Rect::new(0.0, 0.0, 100.0, 80.0), Rect::new(33.0, 44.0, 640.0, 480.0)),
  );

  assert!(verify_window_mutation(kind, &WindowMutationVerification::FrameTolerance { points: 1.0 }, &result).is_ok());
}

#[test]
fn minimize_state_verification_requires_minimized_flag() {
  let kind = WindowMutationKind::Minimize;
  let mut result = window_mutation_result(kind, outcome(Rect::new(0.0, 0.0, 100.0, 80.0), Rect::new(0.0, 0.0, 100.0, 80.0)));

  // Default outcome leaves after_minimized false -> verification fails.
  assert!(verify_window_mutation(kind, &WindowMutationVerification::BestEffortState, &result).is_err());

  result.after_state = Some(WindowState {
    is_minimized: Some(true),
    is_visible: Some(false),
  });
  assert!(verify_window_mutation(kind, &WindowMutationVerification::BestEffortState, &result).is_ok());
}

#[test]
fn validate_rejects_non_finite_and_non_positive() {
  assert!(
    validate_window_mutation_kind(WindowMutationKind::MoveTo {
      point: Point::new(f64::NAN, 0.0),
    })
    .is_err()
  );
  assert!(
    validate_window_mutation_kind(WindowMutationKind::Resize {
      size: Size::new(0.0, 100.0),
    })
    .is_err()
  );
  assert!(
    validate_window_mutation_kind(WindowMutationKind::SetFrame {
      frame: Rect::new(0.0, 0.0, 100.0, 100.0),
    })
    .is_ok()
  );
}
