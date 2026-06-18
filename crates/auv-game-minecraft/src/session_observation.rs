use auv_driver::geometry::Rect;
use serde_json::json;

use crate::types::{MinecraftSpatialFrame, NearbyBlock, RaycastHit};

#[derive(Clone, Debug, PartialEq)]
pub struct MinecraftSessionObservationProvider {
  provider_id: String,
  frames: Vec<MinecraftSpatialFrame>,
  observe_count: usize,
}

impl MinecraftSessionObservationProvider {
  pub fn new(provider_id: impl Into<String>, frames: Vec<MinecraftSpatialFrame>) -> Self {
    Self {
      provider_id: provider_id.into(),
      frames,
      observe_count: 0,
    }
  }

  pub fn provider_id(&self) -> &str {
    &self.provider_id
  }

  pub fn observe(&mut self) -> Result<MinecraftSessionObservation, String> {
    let index = self.observe_count.min(self.frames.len().saturating_sub(1));
    self.observe_count += 1;
    self
      .frames
      .get(index)
      .map(|frame| {
        let mut observation = frame_to_session_observation(frame);
        observation.provider_id = self.provider_id.clone();
        observation
      })
      .ok_or_else(|| "minecraft session observation provider has no frames".to_string())
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MinecraftSessionObservation {
  pub provider_id: String,
  pub frame_id: String,
  pub monotonic_timestamp_ms: u64,
  pub nodes: Vec<MinecraftSessionNode>,
  pub screen_state: Option<String>,
  pub known_limits: Vec<String>,
  pub detail: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MinecraftSessionNode {
  pub node_id: String,
  pub kind: String,
  pub label: Option<String>,
  pub bounds: Rect,
  pub provider_score: Option<f64>,
  pub detail: serde_json::Value,
}

pub fn frame_to_session_observation(frame: &MinecraftSpatialFrame) -> MinecraftSessionObservation {
  let mut nodes = Vec::new();
  if let Some(hit) = &frame.raycast_hit {
    nodes.push(raycast_node(frame, hit));
  }
  nodes.extend(
    frame
      .nearby_blocks
      .iter()
      .enumerate()
      .map(|(index, block)| nearby_block_node(frame, index, block)),
  );

  let mut known_limits = vec![
    "minecraft session observation is telemetry truth, not a generic visual reconstruction"
      .to_string(),
    "minecraft block nodes use viewport placeholder bounds until a projected point or overlay artifact is attached".to_string(),
  ];
  if frame.screenshot_artifact_ref.is_none() {
    known_limits.push(
      "minecraft spatial frame has no screenshot artifact binding for this observation".to_string(),
    );
  }

  MinecraftSessionObservation {
    provider_id: "minecraft.spatial_frame".to_string(),
    frame_id: frame.spatial_frame_id.clone(),
    monotonic_timestamp_ms: frame.monotonic_timestamp_ms,
    nodes,
    screen_state: frame.screen_state.clone(),
    known_limits,
    detail: json!({
      "world_tick": frame.world_tick,
      "viewport": {
        "width": frame.viewport.width,
        "height": frame.viewport.height
      },
      "screen_state": frame.screen_state,
      "capture_skew_ms": frame.mc_capture_skew_ms,
      "resource_pack_ids": frame.resource_pack_ids,
      "screenshot_artifact_ref": frame.screenshot_artifact_ref
    }),
  }
}

fn raycast_node(frame: &MinecraftSpatialFrame, hit: &RaycastHit) -> MinecraftSessionNode {
  MinecraftSessionNode {
    node_id: format!(
      "minecraft_raycast_{}_{}_{}",
      hit.block_pos.x, hit.block_pos.y, hit.block_pos.z
    ),
    kind: "minecraft_raycast_block".to_string(),
    label: Some(hit.block_id.clone()),
    bounds: frame.viewport.bounds(),
    provider_score: Some(1.0),
    detail: json!({
      "block_pos": hit.block_pos,
      "face": hit.face,
      "source": "raycast_hit",
      "frame_id": frame.spatial_frame_id
    }),
  }
}

fn nearby_block_node(
  frame: &MinecraftSpatialFrame,
  index: usize,
  block: &NearbyBlock,
) -> MinecraftSessionNode {
  MinecraftSessionNode {
    node_id: format!(
      "minecraft_nearby_block_{}_{}_{}_{}",
      block.block_pos.x, block.block_pos.y, block.block_pos.z, index
    ),
    kind: "minecraft_nearby_block".to_string(),
    label: Some(block.block_id.clone()),
    bounds: frame.viewport.bounds(),
    provider_score: None,
    detail: json!({
      "block_pos": block.block_pos,
      "source": "nearby_blocks",
      "frame_id": frame.spatial_frame_id
    }),
  }
}

#[cfg(test)]
mod tests {
  use crate::types::{
    BlockFace, BlockPosition, NearbyBlock, PlayerPose, RaycastHit, Vec3, Viewport,
  };

  use super::*;

  fn frame() -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: "frame-session-1".to_string(),
      world_tick: 42,
      monotonic_timestamp_ms: 1_000,
      viewport: Viewport::new(800, 600),
      view_matrix: [0.0; 16],
      projection_matrix: [0.0; 16],
      player_pose: PlayerPose {
        eye_position: Vec3::new(0.0, 64.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: Some(RaycastHit {
        block_pos: BlockPosition::new(1, 2, 3),
        face: BlockFace::North,
        block_id: "minecraft:oak_button".to_string(),
      }),
      nearby_blocks: vec![NearbyBlock {
        block_pos: BlockPosition::new(2, 2, 3),
        block_id: "minecraft:stone".to_string(),
      }],
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: Some(0),
      screen_state: Some("in_game".to_string()),
      resource_pack_ids: vec!["vanilla".to_string()],
    }
  }

  #[test]
  fn frame_observation_projects_raycast_and_nearby_blocks() {
    let observation = frame_to_session_observation(&frame());

    assert_eq!(observation.frame_id, "frame-session-1");
    assert_eq!(observation.nodes.len(), 2);
    assert_eq!(
      observation.nodes[0].label.as_deref(),
      Some("minecraft:oak_button")
    );
    assert_eq!(observation.nodes[0].provider_score, Some(1.0));
    assert_eq!(
      observation.nodes[1].label.as_deref(),
      Some("minecraft:stone")
    );
    assert!(
      observation
        .known_limits
        .iter()
        .any(|limit| limit.contains("telemetry truth"))
    );
    assert_eq!(observation.detail["resource_pack_ids"][0], "vanilla");
  }

  #[test]
  fn buffered_minecraft_provider_reuses_latest_frame_when_exhausted() {
    let mut provider = MinecraftSessionObservationProvider::new("minecraft.live", vec![frame()]);

    let first = provider.observe().expect("first observation");
    let second = provider.observe().expect("second observation");

    assert_eq!(provider.provider_id(), "minecraft.live");
    assert_eq!(first.frame_id, "frame-session-1");
    assert_eq!(second.frame_id, "frame-session-1");
    assert_eq!(first.provider_id, "minecraft.live");
    assert_eq!(second.provider_id, "minecraft.live");
  }
}
