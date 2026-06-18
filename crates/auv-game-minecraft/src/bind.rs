use crate::types::MinecraftSpatialFrame;

/// A spatial frame bound to a real captured screenshot at (approximately) the
/// same instant, carrying the screenshot artifact reference and the measured
/// capture skew.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundSpatialFrame {
  /// The frame with `screenshot_artifact_ref` and `mc_capture_skew_ms` populated.
  pub frame: MinecraftSpatialFrame,
  /// The signed skew that was written onto the frame, in milliseconds.
  pub capture_skew_ms: i64,
}

impl BoundSpatialFrame {
  pub fn to_core_capture_binding(&self) -> Option<auv_driver::CaptureBinding> {
    self
      .frame
      .screenshot_artifact_ref
      .as_ref()
      .map(|screenshot_artifact_ref| {
        auv_driver::CaptureBinding::new(
          self.frame.spatial_frame_id.clone(),
          screenshot_artifact_ref.clone(),
          self.capture_skew_ms,
        )
        .with_source_timestamp_millis(self.frame.monotonic_timestamp_ms)
        .with_known_limit(
          "minecraft capture binding relies on caller-aligned monotonic clock bases",
        )
      })
  }
}

/// Bind a freshly ingested spatial frame to a real screenshot capture.
///
/// The sidecar stamps each frame with `monotonic_timestamp_ms` from the running
/// client. The capture layer stamps the screenshot with its own
/// `capture_monotonic_timestamp_ms` taken at the capture instant. The skew is
/// their signed difference: `frame_ts - capture_ts`.
///
/// IMPORTANT: the two timestamps come from DIFFERENT monotonic clocks (the MC
/// client vs the capturing process), so the skew is only meaningful when the
/// caller has aligned the clock bases (e.g. both read from the same wall-clock
/// reference, or a calibration offset already applied). This function does not
/// pretend the clocks share a base; it records the difference the caller hands
/// it. Threshold enforcement and the over-skew refusal live in
/// [`crate::verify::evaluate_mismatch_refusal`], which reads the
/// `mc_capture_skew_ms` this function writes.
pub fn bind_capture_to_frame(
  mut frame: MinecraftSpatialFrame,
  screenshot_artifact_ref: impl Into<String>,
  capture_monotonic_timestamp_ms: u64,
) -> BoundSpatialFrame {
  let frame_ts = i64::try_from(frame.monotonic_timestamp_ms).unwrap_or(i64::MAX);
  let capture_ts = i64::try_from(capture_monotonic_timestamp_ms).unwrap_or(i64::MAX);
  let capture_skew_ms = frame_ts.saturating_sub(capture_ts);
  frame.screenshot_artifact_ref = Some(screenshot_artifact_ref.into());
  frame.mc_capture_skew_ms = Some(capture_skew_ms);
  BoundSpatialFrame {
    frame,
    capture_skew_ms,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::types::{PlayerPose, Vec3, Viewport};

  fn frame_at(ts: u64) -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: "frame-1".to_string(),
      world_tick: 1,
      monotonic_timestamp_ms: ts,
      viewport: Viewport::new(1708, 960),
      view_matrix: [0.0; 16],
      projection_matrix: [0.0; 16],
      player_pose: PlayerPose {
        eye_position: Vec3::new(0.0, 0.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: None,
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: None,
      resource_pack_ids: Vec::new(),
    }
  }

  #[test]
  fn populates_screenshot_ref_and_positive_skew() {
    let bound = bind_capture_to_frame(frame_at(2_000), "shot.png", 1_700);
    assert_eq!(bound.capture_skew_ms, 300);
    assert_eq!(
      bound.frame.screenshot_artifact_ref.as_deref(),
      Some("shot.png")
    );
    assert_eq!(bound.frame.mc_capture_skew_ms, Some(300));
  }

  #[test]
  fn bound_frame_exposes_core_capture_binding() {
    let bound = bind_capture_to_frame(frame_at(2_000), "artifact://shot", 1_700);

    let binding = bound
      .to_core_capture_binding()
      .expect("bound frame should expose capture binding");

    assert_eq!(binding.source_observation_id, "frame-1");
    assert_eq!(binding.capture_ref, "artifact://shot");
    assert_eq!(binding.capture_skew_ms, 300);
    assert_eq!(binding.source_timestamp_millis, Some(2_000));
    assert!(
      binding
        .known_limits
        .iter()
        .any(|limit| limit.contains("monotonic clock"))
    );
  }

  #[test]
  fn skew_is_negative_when_capture_is_after_frame() {
    let bound = bind_capture_to_frame(frame_at(1_000), "shot.png", 1_450);
    assert_eq!(bound.capture_skew_ms, -450);
    assert_eq!(bound.frame.mc_capture_skew_ms, Some(-450));
  }

  #[test]
  fn zero_skew_when_timestamps_match() {
    let bound = bind_capture_to_frame(frame_at(5_000), "shot.png", 5_000);
    assert_eq!(bound.capture_skew_ms, 0);
  }

  #[test]
  fn bound_frame_feeds_over_skew_refusal() {
    use crate::types::{BlockPosition, MinecraftBlockTarget, MinecraftProjectedPoint};
    use crate::verify::{MismatchRefusalReason, evaluate_mismatch_refusal};

    // Bind a frame whose skew (600ms) exceeds a 250ms tolerance.
    let bound = bind_capture_to_frame(frame_at(2_600), "shot.png", 2_000);
    assert_eq!(bound.capture_skew_ms, 600);

    let projected = MinecraftProjectedPoint {
      screen_point: Some(auv_driver::geometry::Point::new(100.0, 100.0)),
      visibility: crate::types::ProjectionVisibility::Visible,
      match_radius_px: 10.0,
      basis_frame_id: bound.frame.spatial_frame_id.clone(),
      confidence: 1.0,
    };
    let target = MinecraftBlockTarget::new(BlockPosition::new(0, 0, 0));

    let refusal = evaluate_mismatch_refusal(&bound.frame, &projected, &target, true, Some(250));
    assert!(refusal.refused);
    assert_eq!(
      refusal.reason,
      Some(MismatchRefusalReason::CaptureSkewUnreliable)
    );
  }
}
