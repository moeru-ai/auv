use auv_driver::geometry::WindowPoint;

use crate::types::{MinecraftProjectedPoint, ProjectionVisibility};

pub fn projected_window_point(projected: &MinecraftProjectedPoint) -> Option<WindowPoint> {
  if projected.visibility != ProjectionVisibility::Visible {
    return None;
  }

  let screen_point = projected.screen_point?;
  // NOTICE(mc3-window-point-contract): MC-2 projection emits viewport-relative pixels,
  // so the current offline seam treats `screen_point` as window-relative and wraps it
  // in `WindowPoint`; if future live telemetry proves these are true screen pixels,
  // MC-3 wiring must convert screen->window before dispatch to avoid double-applying
  // the window origin at the driver boundary.
  Some(WindowPoint::from(screen_point))
}

#[cfg(test)]
mod tests {
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
}
