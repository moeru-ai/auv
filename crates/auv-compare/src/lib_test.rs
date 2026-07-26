use super::{ScreenPoint, screen_points_match_with_tolerance};

#[test]
fn screen_points_match_with_tolerance_uses_max_radius_floor() {
  let left = ScreenPoint { x: 0.0, y: 0.0 };
  let near = ScreenPoint { x: 1.0, y: 0.0 };
  let far = ScreenPoint { x: 3.0, y: 0.0 };

  assert!(screen_points_match_with_tolerance(left, near, None, None));
  assert!(!screen_points_match_with_tolerance(left, far, None, None));
  assert!(screen_points_match_with_tolerance(left, far, Some(3.0), None));
  assert!(screen_points_match_with_tolerance(left, near, Some(0.5), Some(2.0)));
}
