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
    Vec3::new(
      f64::from(self.x) + 0.5,
      f64::from(self.y) + 0.5,
      f64::from(self.z) + 0.5,
    )
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinecraftBlockTarget {
  pub block_pos: BlockPosition,
  pub face: Option<BlockFace>,
}

impl MinecraftBlockTarget {
  pub const fn new(block_pos: BlockPosition) -> Self {
    Self {
      block_pos,
      face: None,
    }
  }

  pub fn aim_point(&self) -> Vec3 {
    self.block_pos.center()
  }
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
