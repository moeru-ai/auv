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
#[path = "input_target_test.rs"]
mod tests;
