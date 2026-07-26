use auv_game_minecraft::{
  BlockPosition, InventorySummaryEntry, MinecraftBlockTarget, MinecraftProjector, MinecraftSpatialFrame, NearbyBlock, NearbyEntity,
  PlayerPose, ProjectionVisibility, Vec3, Viewport,
};

fn identity_matrix() -> [f64; 16] {
  [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
  ]
}

fn translated_view_matrix(z_offset: f64) -> [f64; 16] {
  [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, z_offset, 1.0,
  ]
}

fn test_frame(view_matrix: [f64; 16], projection_matrix: [f64; 16], viewport: Viewport) -> MinecraftSpatialFrame {
  MinecraftSpatialFrame {
    spatial_frame_id: "frame-1".to_string(),
    world_tick: 42,
    monotonic_timestamp_ms: 1000,
    telemetry_session_id: None,
    viewport,
    view_matrix,
    projection_matrix,
    player_pose: PlayerPose {
      eye_position: Vec3::new(0.0, 0.0, 0.0),
      yaw: 0.0,
      pitch: 0.0,
    },
    raycast_hit: None,
    nearby_blocks: vec![NearbyBlock {
      block_pos: BlockPosition::new(0, 0, 0),
      block_id: "minecraft:stone".to_string(),
    }],
    nearby_entities: vec![NearbyEntity {
      entity_id: "e-1".to_string(),
      entity_kind: "minecraft:pig".to_string(),
    }],
    inventory_summary: vec![InventorySummaryEntry {
      item_id: "minecraft:dirt".to_string(),
      count: 3,
    }],
    screenshot_artifact_ref: None,
    mc_capture_skew_ms: None,
    screen_state: None,
    resource_pack_ids: Vec::new(),
  }
}

fn test_frame_with_eye(
  view_matrix: [f64; 16],
  projection_matrix: [f64; 16],
  viewport: Viewport,
  eye_position: Vec3,
) -> MinecraftSpatialFrame {
  let mut frame = test_frame(view_matrix, projection_matrix, viewport);
  frame.player_pose.eye_position = eye_position;
  frame
}

#[test]
fn rejects_zero_projection_basis() {
  let frame = test_frame([0.0; 16], identity_matrix(), Viewport::new(854, 508));

  let projector = MinecraftProjector::new(frame).expect("projector");
  let error = projector.project_block_target(&MinecraftBlockTarget::new(BlockPosition::new(1, 2, 3))).expect_err("zero basis must fail");

  assert!(error.contains("all zero"));
}

#[test]
fn projects_center_point_into_center_pixel() {
  let projector = MinecraftProjector::new(test_frame(identity_matrix(), identity_matrix(), Viewport::new(800, 600))).expect("projector");
  let point = projector.project_block_target(&MinecraftBlockTarget::new(BlockPosition::new(0, 0, 0))).expect("projected point");

  assert_eq!(point.visibility, ProjectionVisibility::Visible);
  let screen_point = point.screen_point.expect("visible point");
  assert_eq!(screen_point.x, 600.0);
  assert_eq!(screen_point.y, 150.0);
  assert!(point.match_radius_px > 0.0);
}

#[test]
fn behind_camera_when_clip_w_is_non_positive() {
  let projector = MinecraftProjector::new(test_frame(
    identity_matrix(),
    [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0,
    ],
    Viewport::new(800, 600),
  ))
  .expect("projector");

  let point = projector.project_block_target(&MinecraftBlockTarget::new(BlockPosition::new(0, 0, 0))).expect("projected point");
  assert_eq!(point.visibility, ProjectionVisibility::BehindCamera);
  assert!(point.screen_point.is_none());
}

#[test]
fn out_of_frustum_when_ndc_exceeds_clip_range() {
  let projector =
    MinecraftProjector::new(test_frame(translated_view_matrix(5.0), identity_matrix(), Viewport::new(800, 600))).expect("projector");

  let point = projector.project_block_target(&MinecraftBlockTarget::new(BlockPosition::new(0, 0, 0))).expect("projected point");
  assert_eq!(point.visibility, ProjectionVisibility::OutOfFrustum);
  assert!(point.screen_point.is_none());
}

#[test]
fn rejects_zero_sized_viewport() {
  let error = MinecraftProjector::new(test_frame(identity_matrix(), identity_matrix(), Viewport::new(0, 600))).expect_err("must fail");
  assert!(error.contains("positive dimensions"));
}

#[test]
fn rejects_non_finite_matrix_values() {
  let mut matrix = identity_matrix();
  matrix[0] = f64::NAN;
  let error = MinecraftProjector::new(test_frame(matrix, identity_matrix(), Viewport::new(800, 600))).expect_err("must fail");
  assert!(error.contains("view_matrix contains non-finite values"));
}

#[test]
fn projects_live_rotation_only_matrix_with_eye_position_fallback() {
  let frame = test_frame_with_eye(
    [
      0.719950, 0.115742, -0.684307, 0.0, -0.0, 0.985996, 0.166769, 0.0, 0.694026, -0.120065, 0.709867, 0.0, 0.0, 0.0, 0.0, 1.0,
    ],
    [
      0.802706, 0.0, -0.0, -0.0, 0.0, 1.428148, -0.0, -0.0, 0.0, 0.0, -1.000130, -1.0, -0.0, -0.0, -0.100007, -0.0,
    ],
    Viewport::new(1708, 960),
    Vec3::new(511.028439, 73.62, 728.652906),
  );
  let projector = MinecraftProjector::new(frame).expect("projector");

  let point = projector.project_block_target(&MinecraftBlockTarget::new(BlockPosition::new(513, 72, 726))).expect("projected point");

  assert_eq!(point.visibility, ProjectionVisibility::Visible);
  let screen_point = point.screen_point.expect("visible point");
  assert!(screen_point.x > 0.0);
  assert!(screen_point.x < 1708.0);
  assert!(screen_point.y > 0.0);
  assert!(screen_point.y < 960.0);
}
