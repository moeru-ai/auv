use super::*;

fn frame_with_raycast_hit(block_pos: BlockPosition, face: BlockFace) -> MinecraftSpatialFrame {
  MinecraftSpatialFrame {
    spatial_frame_id: "frame-1".to_string(),
    world_tick: 1,
    monotonic_timestamp_ms: 1_000,
    telemetry_session_id: None,
    viewport: Viewport::new(800, 600),
    view_matrix: [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ],
    projection_matrix: [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ],
    player_pose: PlayerPose {
      eye_position: Vec3::new(0.0, 0.0, 0.0),
      yaw: 0.0,
      pitch: 0.0,
    },
    raycast_hit: Some(RaycastHit {
      block_pos,
      face,
      block_id: "minecraft:oak_button".to_string(),
    }),
    nearby_blocks: Vec::new(),
    nearby_entities: Vec::new(),
    inventory_summary: Vec::new(),
    screenshot_artifact_ref: None,
    mc_capture_skew_ms: None,
    screen_state: Some("in_game".to_string()),
    resource_pack_ids: Vec::new(),
  }
}

#[test]
fn block_face_center_offsets_match_expected_face_centers() {
  let block = BlockPosition::new(10, 20, 30);
  let cases = [
    (BlockFace::North, Vec3::new(10.5, 20.5, 30.0)),
    (BlockFace::South, Vec3::new(10.5, 20.5, 31.0)),
    (BlockFace::East, Vec3::new(11.0, 20.5, 30.5)),
    (BlockFace::West, Vec3::new(10.0, 20.5, 30.5)),
    (BlockFace::Up, Vec3::new(10.5, 21.0, 30.5)),
    (BlockFace::Down, Vec3::new(10.5, 20.0, 30.5)),
  ];

  for (face, expected) in cases {
    assert_eq!(block.face_center(face), expected);
  }
}

#[test]
fn mc6_projection_target_uses_hit_face_center_when_raycast_hits_target_block() {
  let target_block = BlockPosition::new(511, 73, 728);
  let cases = [
    (BlockFace::North, Vec3::new(511.5, 73.5, 728.0)),
    (BlockFace::South, Vec3::new(511.5, 73.5, 729.0)),
    (BlockFace::East, Vec3::new(512.0, 73.5, 728.5)),
    (BlockFace::West, Vec3::new(511.0, 73.5, 728.5)),
    (BlockFace::Up, Vec3::new(511.5, 74.0, 728.5)),
    (BlockFace::Down, Vec3::new(511.5, 73.0, 728.5)),
  ];

  for (face, expected_aim_point) in cases {
    let frame = frame_with_raycast_hit(target_block, face);
    let target = mc6_projection_target_for_frame(target_block, &frame, MinecraftTargetSemantics::HitFaceCenter);

    assert_eq!(target.block_pos, target_block);
    assert_eq!(target.face, Some(face));
    assert_eq!(target.precise_point, None);
    assert_eq!(target.aim_point(), expected_aim_point);
  }
}

#[test]
fn mc6_projection_target_falls_back_to_block_center_when_raycast_hits_other_block() {
  let target_block = BlockPosition::new(511, 73, 728);
  let frame = frame_with_raycast_hit(BlockPosition::new(512, 73, 728), BlockFace::East);

  let target = mc6_projection_target_for_frame(target_block, &frame, MinecraftTargetSemantics::HitFaceCenter);

  assert_eq!(target.block_pos, target_block);
  assert_eq!(target.face, None);
  assert_eq!(target.aim_point(), target_block.center());
}

#[test]
fn mc6_projection_target_prefers_estimated_raycast_hit_point_when_pose_supports_it() {
  let target_block = BlockPosition::new(511, 73, 728);
  let mut frame = frame_with_raycast_hit(target_block, BlockFace::North);
  frame.player_pose = PlayerPose {
    eye_position: Vec3::new(510.852669, 73.62, 727.2639),
    yaw: -379.246124,
    pitch: 4.349989,
  };

  let target = mc6_projection_target_for_frame(target_block, &frame, MinecraftTargetSemantics::HitFaceCenter);

  let precise_point = target.precise_point.expect("precise point");
  assert!((precise_point.x - 511.1096707599318).abs() < 1e-6);
  assert!((precise_point.y - 73.56069180584046).abs() < 1e-6);
  assert!((precise_point.z - 728.0).abs() < 1e-6);
  assert_eq!(target.aim_point(), precise_point);
}
