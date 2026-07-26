use super::*;
use crate::types::{PlayerPose, Vec3, Viewport};

fn frame_at(ts: u64) -> MinecraftSpatialFrame {
  MinecraftSpatialFrame {
    spatial_frame_id: "frame-1".to_string(),
    world_tick: 1,
    monotonic_timestamp_ms: ts,
    telemetry_session_id: None,
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
  let bound = bind_capture_to_frame(frame_at(2_000), Some("shot.png".to_string()), 1_700);
  assert_eq!(bound.capture_skew_ms, 300);
  assert_eq!(bound.frame.screenshot_artifact_ref.as_deref(), Some("shot.png"));
  assert_eq!(bound.frame.mc_capture_skew_ms, Some(300));
}

#[test]
fn skew_is_negative_when_capture_is_after_frame() {
  let bound = bind_capture_to_frame(frame_at(1_000), Some("shot.png".to_string()), 1_450);
  assert_eq!(bound.capture_skew_ms, -450);
  assert_eq!(bound.frame.mc_capture_skew_ms, Some(-450));
}

#[test]
fn zero_skew_when_timestamps_match() {
  let bound = bind_capture_to_frame(frame_at(5_000), Some("shot.png".to_string()), 5_000);
  assert_eq!(bound.capture_skew_ms, 0);
}
