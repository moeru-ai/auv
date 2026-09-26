//! 投影精度 vs 截图真值（独立测量，非自洽）。
//!
//! 真值依据：
//! raycast 命中的帧（如 v01、v02），在截图拍摄瞬间十字准星正指着目标方块，
//! 因此该目标在截图中的二维屏幕位置应高度接近屏幕中心 `(width / 2.0, height / 2.0)`。
//!
//! 固有偏差说明（必须明确记录）：
//! 1. 准星与射线的真实物理交点是方块表面的某个具体点（hit point）。
//! 2. 空间记忆中存储的 BlockPosition 结合 surface_face 只能解析到该面的几何中心（face_center）。
//! 3. 面中心与真实射线命中点之间存在物理空间偏置，最大可达半个面对角线（~0.707m），典型偏移约 0.2m–0.4m。
//! 4. 在相机距目标约 3m、FOV 约 70° 的典型观察条件下，此物理几何偏置对应屏幕投影位移约 30px–60px。
//! 5. 因此，本测试用于探测并阻断重大几何与数学错误（如坐标系轴向翻转、视角矩阵转置、角度与弧度误用等数以百计像素的系统性灾难），
//!    而不是要求 0 像素误差。
//! 6. 判定边界设置为 150.0px（宽松 bound，抓大错并为固有几何偏置留足物理余量，不得随意修改以迁就错误）。

use std::path::PathBuf;

use auv_game_minecraft::memory_action_wiring::{MemoryActionQuery, MockActionExecutor, wire_memory_query_to_action};
use auv_game_minecraft::projection::MinecraftProjector;
use auv_game_minecraft::spatial_memory_store::SpatialMemoryStore;
use auv_game_minecraft::types::{
  BlockFace, BlockPosition, MinecraftBlockTarget, MinecraftSpatialFrame, PlayerPose, RaycastHit, Vec3, Viewport,
};

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

fn load_frames() -> (MinecraftSpatialFrame, MinecraftSpatialFrame, MinecraftSpatialFrame) {
  let session_dir = PathBuf::from("F:/auv/.tmp/m2-session");
  if session_dir.is_dir() {
    let f1 = auv_game_minecraft::read_latest_spatial_frame_from_tail(&session_dir.join("v01/telemetry.jsonl")).ok().flatten();
    let f2 = auv_game_minecraft::read_latest_spatial_frame_from_tail(&session_dir.join("v02/telemetry.jsonl")).ok().flatten();
    let f3 = auv_game_minecraft::read_latest_spatial_frame_from_tail(&session_dir.join("v03/telemetry.jsonl")).ok().flatten();

    match (f1, f2, f3) {
      (Some(a), Some(b), Some(c)) => (a, b, c),
      _ => fallback_frames(),
    }
  } else {
    fallback_frames()
  }
}

#[test]
fn projection_of_raycast_landmark_lands_near_screen_center() {
  let (v01, v02, _v03) = load_frames();
  let test_frames = [("v01", v01), ("v02", v02)];

  const MAX_PERMISSIBLE_ERROR_PX: f64 = 150.0;

  println!("\n=== T1: PROJECTION ACCURACY VS SCREEN CENTER GROUND TRUTH ===");
  for (frame_id, frame) in test_frames {
    let hit = frame
      .raycast_hit
      .as_ref()
      .unwrap_or_else(|| panic!("frame {} must have raycast_hit to test against screen center ground truth", frame_id));

    let projector = MinecraftProjector::new(frame.clone()).unwrap_or_else(|e| panic!("failed to build projector for {}: {}", frame_id, e));

    let mut target = MinecraftBlockTarget::new(hit.block_pos);
    target.face = Some(hit.face);

    let projected =
      projector.project_block_target(&target).unwrap_or_else(|e| panic!("failed to project block target for {}: {}", frame_id, e));

    let screen_pt = projected.screen_point.unwrap_or_else(|| panic!("frame {} target projected as hidden/out-of-frustum", frame_id));

    let center_x = f64::from(frame.viewport.width) / 2.0;
    let center_y = f64::from(frame.viewport.height) / 2.0;

    let dx = screen_pt.x - center_x;
    let dy = screen_pt.y - center_y;
    let err = (dx * dx + dy * dy).sqrt();

    println!(
      "frame {}: projected=({:.1}, {:.1}) vs center=({:.1}, {:.1}) | offset=(dx={:.1}px, dy={:.1}px) | error = {:.1}px",
      frame_id, screen_pt.x, screen_pt.y, center_x, center_y, dx, dy, err
    );

    assert!(
      err < MAX_PERMISSIBLE_ERROR_PX,
      "gross projection error on {}, got {:.1}px (threshold: {:.1}px)",
      frame_id,
      err,
      MAX_PERMISSIBLE_ERROR_PX
    );
  }
  println!("============================================================\n");
}

#[test]
fn test_wire_memory_query_to_action_projection_accuracy_vs_screen_center() {
  let (_v01, v02, _v03) = load_frames();

  let tmp = tempfile::NamedTempFile::new().unwrap();
  let mut store = SpatialMemoryStore::open(tmp.path()).unwrap();

  // Ingest v02 to seed landmark with raycast hit
  let hit = v02.raycast_hit.as_ref().expect("v02 must have raycast_hit");
  let obs = auv_game_minecraft::spatial_memory_store::ObservationRef {
    observation_id: "v02-raycast".to_string(),
    captured_at_millis: v02.monotonic_timestamp_ms,
  };
  store.upsert_from_raycast(hit, &obs);

  let mock_executor = MockActionExecutor::new();
  let query = MemoryActionQuery::new("grass_block", v02.player_pose).with_frame(v02.clone()).with_viewport(v02.viewport);

  let outcome = wire_memory_query_to_action(&store, &query, &mock_executor);

  assert!(outcome.attempted, "memory query to action must be attempted");
  assert_eq!(outcome.refusal_reason, None, "unoccluded landmark must not be refused");

  let delivered_point = outcome.window_point.expect("window point must be present");
  let center_x = f64::from(v02.viewport.width) / 2.0;
  let center_y = f64::from(v02.viewport.height) / 2.0;

  let dx = delivered_point.0.x - center_x;
  let dy = delivered_point.0.y - center_y;
  let err = (dx * dx + dy * dy).sqrt();

  println!("\n=== T2: MEMORY WIRING TO ACTION VS SCREEN CENTER GROUND TRUTH ===");
  println!(
    "wiring full-pipeline (v02): delivered=({:.1}, {:.1}) vs center=({:.1}, {:.1}) | offset=(dx={:.1}px, dy={:.1}px) | error = {:.1}px",
    delivered_point.0.x, delivered_point.0.y, center_x, center_y, dx, dy, err
  );
  println!("=================================================================\n");

  const MAX_PERMISSIBLE_ERROR_PX: f64 = 150.0;
  assert!(
    err < MAX_PERMISSIBLE_ERROR_PX,
    "gross error in wiring full pipeline on v02, got {:.1}px (threshold: {:.1}px)",
    err,
    MAX_PERMISSIBLE_ERROR_PX
  );
}
