//! Bounded field test scenario for agent memory integration.
//!
//! Replay-driven verification covering the full pipeline:
//! 1. Exploration & Mapping: Ingests v01 -> v02 sequence (simulating agent moving between views), then v03.
//! 2. Memory Query: Queries spatial memory for the block hit by raycast (ground truth `(-22, 81, 43)`).
//! 3. Action Delivery: Projects landmark to observer window coordinates and dispatches click to `ActionExecutor`.
//! 4. Verifications & Metrics:
//!    - `recall_success`: Memory successfully recalls target landmark.
//!    - `projection_error_px`: 3D->2D projection error relative to v02 ground truth.
//!    - `false_landmarks`: Landmarks created outside the static whitelist.
//!    - `visual_gated_ticks`: Ticks where visual perception was gated.
//!
//! Offline execution only; Minecraft client is not launched.

use std::path::PathBuf;

use auv_game_minecraft::agent_memory_loop::{AgentMemoryLoop, AgentMemoryLoopConfig, LiveCapture};
use auv_game_minecraft::memory_action_wiring::{MemoryActionQuery, MockActionExecutor, wire_memory_query_to_action};
use auv_game_minecraft::projection::MinecraftProjector;
use auv_game_minecraft::spatial_memory_store::SpatialMemoryStore;
use auv_game_minecraft::types::{
  BlockFace, BlockPosition, MinecraftBlockTarget, MinecraftSpatialFrame, PlayerPose, RaycastHit, Vec3, Viewport,
};
use auv_game_minecraft::visual_perception::{DepthEstimator, YoloWorldConfig, YoloWorldDetector};

fn fallback_frames() -> (MinecraftSpatialFrame, MinecraftSpatialFrame, MinecraftSpatialFrame) {
  let v01 = MinecraftSpatialFrame {
    spatial_frame_id: "fallback-v01".to_string(),
    world_tick: 144173,
    monotonic_timestamp_ms: 9194254,
    telemetry_session_id: Some("aa651e23-bb4d-4df0-8d65-0ca6be11791f".to_string()),
    viewport: Viewport::new(854, 480),
    view_matrix: [
      -0.98643, 0.034136, -0.160596, 0.0, 0.0, 0.978148, 0.207912, 0.0, 0.164184, 0.20509, -0.964874, 0.0, 0.0, 0.0, 0.0, 1.0,
    ],
    projection_matrix: [
      0.802706, 0.0, -0.0, -0.0, 0.0, 1.428148, -0.0, -0.0, 0.0, 0.0, -1.00013, -1.0, 0.0, -0.0, -0.100007, -0.0,
    ],
    player_pose: PlayerPose {
      eye_position: Vec3::new(-22.662026, 82.62, 39.552317),
      yaw: -9.449891,
      pitch: 12.000004,
    },
    raycast_hit: Some(RaycastHit {
      block_pos: BlockPosition::new(-22, 81, 43),
      face: BlockFace::West,
      block_id: "minecraft:grass_block".to_string(),
    }),
    nearby_blocks: vec![],
    nearby_entities: vec![],
    inventory_summary: vec![],
    resource_pack_ids: vec![],
    screen_state: Some("in_game".to_string()),
    screenshot_artifact_ref: None,
    mc_capture_skew_ms: None,
  };

  let v02 = MinecraftSpatialFrame {
    spatial_frame_id: "fallback-v02".to_string(),
    world_tick: 146938,
    monotonic_timestamp_ms: 9332505,
    telemetry_session_id: Some("aa651e23-bb4d-4df0-8d65-0ca6be11791f".to_string()),
    viewport: Viewport::new(854, 480),
    view_matrix: [
      -0.978689, 0.056774, -0.197343, 0.0, 0.0, 0.961021, 0.276476, 0.0, 0.205348, 0.270584, -0.940541, 0.0, 0.0, 0.0, 0.0, 1.0,
    ],
    projection_matrix: [
      0.802706, 0.001535, -0.0, -0.0, -0.000863, 1.428141, -0.002995, -0.002994, 3e-06, -0.004276, -1.000126, -0.999995, 0.00824, -0.055945,
      -0.100007, -0.0,
    ],
    player_pose: PlayerPose {
      eye_position: Vec3::new(-22.346885, 82.62, 41.446003),
      yaw: -11.099892,
      pitch: 15.000004,
    },
    raycast_hit: Some(RaycastHit {
      block_pos: BlockPosition::new(-22, 81, 43),
      face: BlockFace::Up,
      block_id: "minecraft:grass_block".to_string(),
    }),
    nearby_blocks: vec![],
    nearby_entities: vec![],
    inventory_summary: vec![],
    resource_pack_ids: vec![],
    screen_state: Some("in_game".to_string()),
    screenshot_artifact_ref: None,
    mc_capture_skew_ms: None,
  };

  let v03 = MinecraftSpatialFrame {
    spatial_frame_id: "fallback-v03".to_string(),
    world_tick: 147101,
    monotonic_timestamp_ms: 9340654,
    telemetry_session_id: Some("aa651e23-bb4d-4df0-8d65-0ca6be11791f".to_string()),
    viewport: Viewport::new(854, 480),
    view_matrix: [
      -0.995055, -0.00208, 0.099299, 0.0, 0.0, 0.999781, 0.020942, 0.0, -0.099321, 0.020839, -0.994837, 0.0, 0.0, 0.0, 0.0, 1.0,
    ],
    projection_matrix: [
      0.802706, 0.001355, -0.0, -0.0, -0.000762, 1.428137, -0.003858, -0.003858, 3e-06, -0.00551, -1.000123, -0.999993, 0.007274, -0.059173,
      -0.100007, -0.0,
    ],
    player_pose: PlayerPose {
      eye_position: Vec3::new(-22.089177, 82.62, 36.979274),
      yaw: 5.700115,
      pitch: 1.199992,
    },
    raycast_hit: None,
    nearby_blocks: vec![],
    nearby_entities: vec![],
    inventory_summary: vec![],
    resource_pack_ids: vec![],
    screen_state: Some("in_game".to_string()),
    screenshot_artifact_ref: None,
    mc_capture_skew_ms: None,
  };

  (v01, v02, v03)
}

#[test]
fn test_field_test_scenario_mapping_query_action() {
  let session_dir = PathBuf::from("F:/auv/.tmp/m2-session");
  let (v01_frame, v02_frame, v03_frame) = if session_dir.is_dir() {
    let f1 = auv_game_minecraft::read_latest_spatial_frame_from_tail(&session_dir.join("v01/telemetry.jsonl")).ok().flatten();
    let f2 = auv_game_minecraft::read_latest_spatial_frame_from_tail(&session_dir.join("v02/telemetry.jsonl")).ok().flatten();
    let f3 = auv_game_minecraft::read_latest_spatial_frame_from_tail(&session_dir.join("v03/telemetry.jsonl")).ok().flatten();

    match (f1, f2, f3) {
      (Some(a), Some(b), Some(c)) => (a, b, c),
      _ => fallback_frames(),
    }
  } else {
    fallback_frames()
  };

  let v01_img = session_dir.join("v01/screenshot.png");
  let v02_img = session_dir.join("v02/screenshot.png");
  let v03_img = session_dir.join("v03/screenshot.png");

  let img_1 = if v01_img.is_file() {
    image::open(&v01_img).ok()
  } else {
    None
  };
  let img_2 = if v02_img.is_file() {
    image::open(&v02_img).ok()
  } else {
    None
  };
  let img_3 = if v03_img.is_file() {
    image::open(&v03_img).ok()
  } else {
    None
  };

  // 1. Initialise AgentMemoryLoop with fresh temp store
  let tmp = tempfile::NamedTempFile::new().unwrap();
  let store = SpatialMemoryStore::open(tmp.path()).unwrap();
  let mut agent_loop = AgentMemoryLoop::new(store, AgentMemoryLoopConfig::default());

  // Attach real inference models if available on disk
  let yolo_path = PathBuf::from("F:/auv/.tmp/models/yolov8s-worldv2.onnx");
  let depth_path = PathBuf::from("F:/auv/.tmp/models/model-small.onnx");
  if yolo_path.is_file() && depth_path.is_file() {
    if let (Ok(detector), Ok(depth)) = (
      YoloWorldDetector::new(YoloWorldConfig {
        model_path: yolo_path,
        confidence_threshold: 0.50,
        iou_threshold: 0.45,
        input_size: 640,
        classes: auv_game_minecraft::visual_perception::DEFAULT_MINECRAFT_CLASSES.iter().map(|s| s.to_string()).collect(),
      }),
      DepthEstimator::new(&depth_path),
    ) {
      agent_loop = agent_loop.with_models(detector, depth);
    }
  }

  // -------------------------------------------------------------
  // STAGE 1: EXPLORATION & MAPPING
  // -------------------------------------------------------------
  let capture_1 =
    LiveCapture::new("v01-live-view", v01_frame.monotonic_timestamp_ms, Some(v01_frame.player_pose), v01_frame.raycast_hit.clone(), img_1)
      .with_viewport(v01_frame.viewport);

  let report_1 = agent_loop.tick(&capture_1).expect("tick 1 succeeds");
  assert_eq!(report_1.landmarks_created, 1, "tick 1 must create landmark from raycast");

  let capture_2 =
    LiveCapture::new("v02-live-view", v02_frame.monotonic_timestamp_ms, Some(v02_frame.player_pose), v02_frame.raycast_hit.clone(), img_2)
      .with_viewport(v02_frame.viewport);

  let report_2 = agent_loop.tick(&capture_2).expect("tick 2 succeeds");
  assert_eq!(report_2.landmarks_merged, 1, "tick 2 must merge into existing landmark at (-22, 81, 43)");

  let capture_3 =
    LiveCapture::new("v03-live-view", v03_frame.monotonic_timestamp_ms, Some(v03_frame.player_pose), v03_frame.raycast_hit.clone(), img_3)
      .with_viewport(v03_frame.viewport);

  let report_3 = agent_loop.tick(&capture_3).expect("tick 3 succeeds");
  let visual_gated_ticks = (if report_1.visual_skipped_reason.is_some() {
    1
  } else {
    0
  }) + (if report_2.visual_skipped_reason.is_some() {
    1
  } else {
    0
  }) + (if report_3.visual_skipped_reason.is_some() {
    1
  } else {
    0
  });

  // -------------------------------------------------------------
  // STAGE 2: MEMORY QUERY
  // -------------------------------------------------------------
  let target_block = BlockPosition::new(-22, 81, 43);
  let recalled_landmark = agent_loop.store().landmarks().values().find(|lm| lm.position == target_block);
  let recall_success = recalled_landmark.is_some();
  assert!(recall_success, "target landmark at (-22, 81, 43) must be recalled from memory");

  // -------------------------------------------------------------
  // STAGE 3: ACTION DELIVERY VIA PROJECTION & CLICK
  // -------------------------------------------------------------
  let mock_executor = MockActionExecutor::new();
  let query = MemoryActionQuery::new("grass_block", v02_frame.player_pose).with_frame(v02_frame.clone()).with_viewport(v02_frame.viewport);

  let outcome = wire_memory_query_to_action(agent_loop.store(), &query, &mock_executor);

  assert!(outcome.attempted, "memory query to action must be attempted");
  assert_eq!(outcome.refusal_reason, None, "unoccluded landmark must not be refused");
  let delivered_point = outcome.window_point.expect("window point must be present");

  let clicks = mock_executor.clicks();
  assert_eq!(clicks.len(), 1, "executor must receive exactly one click");
  assert_eq!(clicks[0], delivered_point, "clicked coordinates must match delivered window point");

  // -------------------------------------------------------------
  // STAGE 4: PROJECTION ERROR CALCULATION RELATIVE TO TRUTH
  // -------------------------------------------------------------
  let projector = MinecraftProjector::new(v02_frame.clone()).expect("projector creates");
  let mut block_target = MinecraftBlockTarget::new(target_block);
  block_target.face = Some(BlockFace::Up);
  let truth_projection = projector.project_block_target(&block_target).expect("truth projection");
  let truth_point = truth_projection.screen_point.expect("truth point exists");

  let dx = delivered_point.0.x - truth_point.x;
  let dy = delivered_point.0.y - truth_point.y;
  let projection_error_px = (dx * dx + dy * dy).sqrt();

  assert!(projection_error_px < 2.0, "projection error must be < 2px from v02 truth, got {:.4}px", projection_error_px);

  // False landmarks: count any landmarks whose descriptions are outside the static whitelist
  let false_landmarks = agent_loop
    .store()
    .landmarks()
    .values()
    .filter(|lm| {
      if let Some(desc) = &lm.description {
        let is_whitelisted = agent_loop.config().static_whitelist.iter().any(|w| desc.to_lowercase().contains(&w.to_lowercase()));
        !is_whitelisted
      } else {
        false
      }
    })
    .count();

  // -------------------------------------------------------------
  // METRICS REPORTING
  // -------------------------------------------------------------
  println!("\n================ FIELD TEST SCENARIO METRICS ================");
  println!("  recall_success:      {}", recall_success);
  println!("  projection_error_px: {:.4} px", projection_error_px);
  println!("  false_landmarks:     {}", false_landmarks);
  println!("  visual_gated_ticks:  {}", visual_gated_ticks);
  println!("=============================================================\n");
}
