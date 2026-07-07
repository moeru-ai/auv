use serde::{Deserialize, Serialize};

use auv_driver::geometry::{Point, Rect, Size};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
  pub x: f64,
  pub y: f64,
  pub z: f64,
}

impl Vec3 {
  pub const fn new(x: f64, y: f64, z: f64) -> Self {
    Self { x, y, z }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockFace {
  #[default]
  Up,
  Down,
  North,
  South,
  East,
  West,
}

impl BlockFace {
  pub const fn face_center_offset(self) -> Vec3 {
    match self {
      Self::Up => Vec3::new(0.5, 1.0, 0.5),
      Self::Down => Vec3::new(0.5, 0.0, 0.5),
      Self::North => Vec3::new(0.5, 0.5, 0.0),
      Self::South => Vec3::new(0.5, 0.5, 1.0),
      Self::East => Vec3::new(1.0, 0.5, 0.5),
      Self::West => Vec3::new(0.0, 0.5, 0.5),
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockPosition {
  pub x: i32,
  pub y: i32,
  pub z: i32,
}

impl BlockPosition {
  pub const fn new(x: i32, y: i32, z: i32) -> Self {
    Self { x, y, z }
  }

  pub fn center(self) -> Vec3 {
    Vec3::new(f64::from(self.x) + 0.5, f64::from(self.y) + 0.5, f64::from(self.z) + 0.5)
  }

  pub fn face_center(self, face: BlockFace) -> Vec3 {
    let offset = face.face_center_offset();
    Vec3::new(f64::from(self.x) + offset.x, f64::from(self.y) + offset.y, f64::from(self.z) + offset.z)
  }

  pub fn aabb_corners(self) -> [Vec3; 8] {
    let min_x = f64::from(self.x);
    let min_y = f64::from(self.y);
    let min_z = f64::from(self.z);
    let max_x = min_x + 1.0;
    let max_y = min_y + 1.0;
    let max_z = min_z + 1.0;

    [
      Vec3::new(min_x, min_y, min_z),
      Vec3::new(max_x, min_y, min_z),
      Vec3::new(min_x, max_y, min_z),
      Vec3::new(max_x, max_y, min_z),
      Vec3::new(min_x, min_y, max_z),
      Vec3::new(max_x, min_y, max_z),
      Vec3::new(min_x, max_y, max_z),
      Vec3::new(max_x, max_y, max_z),
    ]
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
  pub width: u32,
  pub height: u32,
}

impl Viewport {
  pub const fn new(width: u32, height: u32) -> Self {
    Self { width, height }
  }

  pub fn size(self) -> Size {
    Size::new(f64::from(self.width), f64::from(self.height))
  }

  pub fn bounds(self) -> Rect {
    Rect {
      origin: Point::new(0.0, 0.0),
      size: self.size(),
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerPose {
  pub eye_position: Vec3,
  pub yaw: f64,
  pub pitch: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaycastHit {
  pub block_pos: BlockPosition,
  pub face: BlockFace,
  pub block_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NearbyBlock {
  pub block_pos: BlockPosition,
  pub block_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NearbyEntity {
  pub entity_id: String,
  pub entity_kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventorySummaryEntry {
  pub item_id: String,
  pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MinecraftSpatialFrame {
  pub spatial_frame_id: String,
  pub world_tick: u64,
  pub monotonic_timestamp_ms: u64,
  #[serde(default)]
  pub telemetry_session_id: Option<String>,
  pub viewport: Viewport,
  pub view_matrix: [f64; 16],
  pub projection_matrix: [f64; 16],
  pub player_pose: PlayerPose,
  #[serde(default)]
  pub raycast_hit: Option<RaycastHit>,
  #[serde(default)]
  pub nearby_blocks: Vec<NearbyBlock>,
  #[serde(default)]
  pub nearby_entities: Vec<NearbyEntity>,
  #[serde(default)]
  pub inventory_summary: Vec<InventorySummaryEntry>,
  #[serde(default)]
  pub screenshot_artifact_ref: Option<String>,
  // NOTICE(mc3-live-binding): populated only after real screenshot/frame binding lands.
  #[serde(default)]
  pub mc_capture_skew_ms: Option<i64>,
  #[serde(default)]
  pub screen_state: Option<String>,
  #[serde(default)]
  pub resource_pack_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MinecraftBlockTarget {
  pub block_pos: BlockPosition,
  pub face: Option<BlockFace>,
  // NOTICE(mc6-geometry-gate): MC-6 single-frame calibration must align the actual
  // crosshair hit location, not only the target face center. When live telemetry
  // gives a matching `raycast_hit`, we estimate the ray/face intersection and keep
  // it local to the MC-6 read path. TODO(mc6-hit-vector): replace this estimate with
  // an exact sidecar-provided hit point if the owner approves that telemetry surface.
  #[serde(default)]
  pub precise_point: Option<Vec3>,
}

impl MinecraftBlockTarget {
  pub const fn new(block_pos: BlockPosition) -> Self {
    Self {
      block_pos,
      face: None,
      precise_point: None,
    }
  }

  pub fn aim_point(&self) -> Vec3 {
    self.precise_point.unwrap_or_else(|| self.face.map(|face| self.block_pos.face_center(face)).unwrap_or_else(|| self.block_pos.center()))
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MinecraftTargetSemantics {
  #[default]
  HitFaceCenter,
  BlockCenter,
}

pub fn mc6_projection_target_for_frame(
  target_block: BlockPosition,
  frame: &MinecraftSpatialFrame,
  semantics: MinecraftTargetSemantics,
) -> MinecraftBlockTarget {
  match semantics {
    MinecraftTargetSemantics::BlockCenter => MinecraftBlockTarget::new(target_block),
    MinecraftTargetSemantics::HitFaceCenter => {
      if let Some(raycast_hit) = &frame.raycast_hit
        && raycast_hit.block_pos == target_block
      {
        let precise_point = estimate_raycast_hit_point(frame.player_pose, raycast_hit)
          .filter(|point| point_lies_on_target_face(*point, raycast_hit.block_pos, raycast_hit.face));
        return MinecraftBlockTarget {
          block_pos: target_block,
          face: Some(raycast_hit.face),
          precise_point,
        };
      }
      MinecraftBlockTarget::new(target_block)
    }
  }
}

fn estimate_raycast_hit_point(player_pose: PlayerPose, raycast_hit: &RaycastHit) -> Option<Vec3> {
  let direction = forward_direction(player_pose);
  let eye = player_pose.eye_position;
  let (plane_axis, plane_value) = match raycast_hit.face {
    BlockFace::North => (2usize, f64::from(raycast_hit.block_pos.z)),
    BlockFace::South => (2usize, f64::from(raycast_hit.block_pos.z) + 1.0),
    BlockFace::East => (0usize, f64::from(raycast_hit.block_pos.x) + 1.0),
    BlockFace::West => (0usize, f64::from(raycast_hit.block_pos.x)),
    BlockFace::Up => (1usize, f64::from(raycast_hit.block_pos.y) + 1.0),
    BlockFace::Down => (1usize, f64::from(raycast_hit.block_pos.y)),
  };
  let direction_components = [direction.x, direction.y, direction.z];
  let denominator = direction_components[plane_axis];
  if denominator.abs() <= 1e-9 {
    return None;
  }

  let eye_components = [eye.x, eye.y, eye.z];
  let t = (plane_value - eye_components[plane_axis]) / denominator;
  if !t.is_finite() || t <= 0.0 {
    return None;
  }

  Some(Vec3::new(eye.x + direction.x * t, eye.y + direction.y * t, eye.z + direction.z * t))
}

fn point_lies_on_target_face(point: Vec3, block_pos: BlockPosition, face: BlockFace) -> bool {
  const EPSILON: f64 = 1e-4;
  let min_x = f64::from(block_pos.x);
  let min_y = f64::from(block_pos.y);
  let min_z = f64::from(block_pos.z);
  let max_x = min_x + 1.0;
  let max_y = min_y + 1.0;
  let max_z = min_z + 1.0;

  match face {
    BlockFace::North => {
      (point.z - min_z).abs() <= EPSILON
        && (min_x - EPSILON..=max_x + EPSILON).contains(&point.x)
        && (min_y - EPSILON..=max_y + EPSILON).contains(&point.y)
    }
    BlockFace::South => {
      (point.z - max_z).abs() <= EPSILON
        && (min_x - EPSILON..=max_x + EPSILON).contains(&point.x)
        && (min_y - EPSILON..=max_y + EPSILON).contains(&point.y)
    }
    BlockFace::East => {
      (point.x - max_x).abs() <= EPSILON
        && (min_y - EPSILON..=max_y + EPSILON).contains(&point.y)
        && (min_z - EPSILON..=max_z + EPSILON).contains(&point.z)
    }
    BlockFace::West => {
      (point.x - min_x).abs() <= EPSILON
        && (min_y - EPSILON..=max_y + EPSILON).contains(&point.y)
        && (min_z - EPSILON..=max_z + EPSILON).contains(&point.z)
    }
    BlockFace::Up => {
      (point.y - max_y).abs() <= EPSILON
        && (min_x - EPSILON..=max_x + EPSILON).contains(&point.x)
        && (min_z - EPSILON..=max_z + EPSILON).contains(&point.z)
    }
    BlockFace::Down => {
      (point.y - min_y).abs() <= EPSILON
        && (min_x - EPSILON..=max_x + EPSILON).contains(&point.x)
        && (min_z - EPSILON..=max_z + EPSILON).contains(&point.z)
    }
  }
}

fn forward_direction(player_pose: PlayerPose) -> Vec3 {
  let yaw_radians = player_pose.yaw.to_radians();
  let pitch_radians = player_pose.pitch.to_radians();
  Vec3::new(-yaw_radians.sin() * pitch_radians.cos(), -pitch_radians.sin(), yaw_radians.cos() * pitch_radians.cos())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionVisibility {
  #[default]
  Visible,
  BehindCamera,
  OutOfFrustum,
  OutsideWindow,
  // NOTICE(mc3-occlusion): occlusion classification requires live raycast/frame corroboration.
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MinecraftProjectedPoint {
  pub screen_point: Option<Point>,
  pub visibility: ProjectionVisibility,
  pub match_radius_px: f64,
  pub basis_frame_id: String,
  pub confidence: f64,
}

#[cfg(test)]
mod tests {
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
}
