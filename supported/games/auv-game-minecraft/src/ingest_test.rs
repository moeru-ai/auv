use std::fs;
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

// ROOT CAUSE:
//
// The Fabric mod hand-builds its JSONL line in TelemetrySample.toJsonLine()
// rather than through a serializer, and TelemetryRecorder had no
// populateNearbyBlocks, so every recorded line carried "nearby_blocks":[].
// The multi-entry branch of appendNearbyBlocks - element separators plus the
// nested block_pos object - had therefore never been exercised by real data,
// and no Rust test covered it either: the fixtures here build a frame through
// serde and round-trip Rust's own output, which cannot detect a mismatch with
// the Java writer's shape.
//
// Before the fix, that untested branch was unreachable in practice. Populating
// nearby_blocks makes it a live path, so this test pins the writer's exact byte
// shape against the reader.
#[test]
fn parses_mod_written_tail_line_with_populated_nearby_blocks() {
  let body = r#"{"spatial_frame_id":"frame-1234-5678","world_tick":1234,"monotonic_timestamp_ms":98765,"telemetry_session_id":"session-abc","viewport":{"width":1708,"height":960},"view_matrix":[1.000000,0.000000,0.000000,0.000000,0.000000,1.000000,0.000000,0.000000,0.000000,0.000000,1.000000,0.000000,0.000000,0.000000,0.000000,1.000000],"projection_matrix":[1.000000,0.000000,0.000000,0.000000,0.000000,1.000000,0.000000,0.000000,0.000000,0.000000,1.000000,0.000000,0.000000,0.000000,0.000000,1.000000],"player_pose":{"eye_position":{"x":511.028439,"y":73.620000,"z":728.652906},"yaw":-45.000000,"pitch":9.500000},"raycast_hit":{"block_pos":{"x":513,"y":72,"z":726},"face":"north","block_id":"minecraft:stone"},"nearby_blocks":[{"block_pos":{"x":513,"y":72,"z":726},"block_id":"minecraft:stone"},{"block_pos":{"x":511,"y":73,"z":727},"block_id":"minecraft:grass_block"},{"block_pos":{"x":-4,"y":-61,"z":-9},"block_id":"minecraft:deepslate"}],"inventory_summary":[{"item_id":"minecraft:stone","count":2}],"resource_pack_ids":["vanilla"],"screen_state":"in_game"}"#;
  let mut cursor = Cursor::new(format!("{body}\n").into_bytes());

  let frame = scan_latest_spatial_frame_from_tail(&mut cursor).expect("tail scan succeeds").expect("frame should parse");

  assert_eq!(
    frame.nearby_blocks,
    vec![
      NearbyBlock {
        block_pos: BlockPosition::new(513, 72, 726),
        block_id: "minecraft:stone".to_string(),
      },
      NearbyBlock {
        block_pos: BlockPosition::new(511, 73, 727),
        block_id: "minecraft:grass_block".to_string(),
      },
      NearbyBlock {
        block_pos: BlockPosition::new(-4, -61, -9),
        block_id: "minecraft:deepslate".to_string(),
      },
    ]
  );
  assert!(frame.nearby_blocks.iter().any(|block| block.block_pos == BlockPosition::new(511, 73, 727)));
  assert_eq!(frame.raycast_hit.expect("hit").block_pos, BlockPosition::new(513, 72, 726));
  assert!(frame.nearby_entities.is_empty());
}

// ROOT CAUSE:
//
// The timeout branch used to return the last observed frame even when it was
// still older than the caller's watermark. In the live-click path that let a
// stale pre-action frame flow into post-action verification.
// The fix keeps timeout as None so callers only get a truly newer frame.
#[test]
fn read_latest_spatial_frame_newer_than_times_out_without_stale_frame() {
  let temp = tempfile::tempdir().expect("temp dir");
  let path = temp.path().join("telemetry.jsonl");
  fs::write(&path, format!("{}\n", frame_line("stale", 1, 1000))).expect("write telemetry sample");

  let frame = read_latest_spatial_frame_newer_than(&path, 2000, TailFrameWaitConfig::new(0, 1)).expect("read succeeds");

  assert_eq!(frame, None);
}
