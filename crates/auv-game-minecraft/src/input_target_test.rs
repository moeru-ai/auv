use auv_driver::geometry::Point;

use super::*;

fn projected_point(visibility: ProjectionVisibility, screen_point: Option<Point>) -> MinecraftProjectedPoint {
  MinecraftProjectedPoint {
    screen_point,
    visibility,
    match_radius_px: 12.0,
    basis_frame_id: "frame-1".to_string(),
    confidence: 1.0,
  }
}

#[test]
fn returns_window_point_for_visible_projection() {
  let projected = projected_point(ProjectionVisibility::Visible, Some(Point::new(320.0, 240.0)));

  let window_point = projected_window_point(&projected).expect("window point");

  assert_eq!(window_point, WindowPoint::new(320.0, 240.0));
}

#[test]
fn returns_none_for_non_visible_projection() {
  for visibility in [
    ProjectionVisibility::BehindCamera,
    ProjectionVisibility::OutOfFrustum,
    ProjectionVisibility::OutsideWindow,
  ] {
    let projected = projected_point(visibility, Some(Point::new(320.0, 240.0)));
    assert_eq!(projected_window_point(&projected), None);
  }
}

#[test]
fn returns_none_when_visible_projection_has_no_point() {
  let projected = projected_point(ProjectionVisibility::Visible, None);

  assert_eq!(projected_window_point(&projected), None);
}
