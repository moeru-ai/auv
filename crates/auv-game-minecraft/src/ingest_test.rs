use std::io::Cursor;

use super::*;
use crate::types::{BlockPosition, NearbyBlock, PlayerPose, Vec3, Viewport};

fn frame_line(id: &str, tick: u64, ts: u64) -> String {
  let frame = MinecraftSpatialFrame {
    spatial_frame_id: id.to_string(),
    world_tick: tick,
    monotonic_timestamp_ms: ts,
    telemetry_session_id: None,
    viewport: Viewport::new(1708, 960),
    view_matrix: [0.0; 16],
    projection_matrix: [0.0; 16],
    player_pose: PlayerPose {
      eye_position: Vec3::new(-3.5, 70.62, -9.5),
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
  };
  serde_json::to_string(&frame).expect("frame serializes")
}

fn oversized_frame_line(id: &str, tick: u64, ts: u64, block_count: usize) -> String {
  let mut frame = MinecraftSpatialFrame {
    spatial_frame_id: id.to_string(),
    world_tick: tick,
    monotonic_timestamp_ms: ts,
    telemetry_session_id: None,
    viewport: Viewport::new(1708, 960),
    view_matrix: [0.0; 16],
    projection_matrix: [0.0; 16],
    player_pose: PlayerPose {
      eye_position: Vec3::new(-3.5, 70.62, -9.5),
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
  };
  frame.nearby_blocks = (0..block_count)
    .map(|index| NearbyBlock {
      block_pos: BlockPosition::new(index as i32, 70, -9),
      block_id: "minecraft:stone".to_string(),
    })
    .collect();
  serde_json::to_string(&frame).expect("oversized frame serializes")
}

#[test]
fn tail_scan_skips_trailing_blank_and_malformed_lines() {
  let body = format!("{}\n{}\nnot json\n   \n", frame_line("valid-1", 1, 1000), frame_line("valid-2", 2, 2000),);
  let mut cursor = Cursor::new(body.into_bytes());

  let frame = scan_latest_spatial_frame_from_tail(&mut cursor).expect("tail scan succeeds").expect("frame is present");

  assert_eq!(frame.spatial_frame_id, "valid-2");
  assert_eq!(frame.world_tick, 2);
}

#[test]
fn tail_scan_handles_line_larger_than_chunk() {
  let big = oversized_frame_line("frame-big", 9, 9000, 2500);
  assert!(big.len() > 64 * 1024);
  let body = format!("{}\n{}\n", frame_line("frame-1", 1, 1000), big);
  let mut cursor = Cursor::new(body.into_bytes());

  let frame = scan_latest_spatial_frame_from_tail(&mut cursor).expect("tail scan succeeds").expect("frame is present");

  assert_eq!(frame.spatial_frame_id, "frame-big");
  assert_eq!(frame.world_tick, 9);
  assert_eq!(frame.monotonic_timestamp_ms, 9000);
}
