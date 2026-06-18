use auv_game_minecraft::{
  MinecraftSessionNode, MinecraftSessionObservation, MinecraftSpatialFrame,
  frame_to_session_observation,
};
use auv_tracing_driver::now_millis;
use auv_tracing_driver::trace::{new_run_id, new_span_id};

use crate::{
  contract::{
    NodeRef, OBSERVATION_SNAPSHOT_API_VERSION, ObservationSnapshot, ObservationSource,
    RecognitionBox, RecognitionScope, RecognitionSource, RecognitionSurface, SurfaceNode,
  },
  session::{BufferedObservationProvider, SessionObservationProvider},
};

/// Project Minecraft spatial telemetry into the core session observation
/// contract without teaching `session` about Minecraft-specific concepts.
pub fn minecraft_spatial_frame_session_provider(
  provider_id: impl Into<String>,
  frames: Vec<MinecraftSpatialFrame>,
) -> impl SessionObservationProvider {
  let provider_id = provider_id.into();
  let snapshots = frames
    .iter()
    .enumerate()
    .map(|(index, frame)| {
      let mut observation = frame_to_session_observation(frame);
      observation.provider_id = provider_id.clone();
      minecraft_session_observation_snapshot(index, observation)
    })
    .collect();
  BufferedObservationProvider::new(provider_id, snapshots)
}

fn minecraft_session_observation_snapshot(
  index: usize,
  observation: MinecraftSessionObservation,
) -> ObservationSnapshot {
  let run_id = new_run_id();
  let span_id = new_span_id();
  let source_artifacts = observation
    .detail
    .get("screenshot_artifact_ref")
    .and_then(|value| value.as_str())
    .map(|artifact| vec![artifact.to_string()])
    .unwrap_or_default();
  let nodes = observation
    .nodes
    .into_iter()
    .map(|node| minecraft_session_node_surface_node(&run_id, &span_id, &source_artifacts, node))
    .collect();

  ObservationSnapshot {
    api_version: OBSERVATION_SNAPSHOT_API_VERSION.to_string(),
    snapshot_id: format!("minecraft_session_observation_{index}"),
    run_id,
    span_id,
    captured_at_millis: now_millis(),
    source: ObservationSource::Visual,
    scope: RecognitionScope {
      surface: RecognitionSurface::Window,
      display_ref: None,
      native_display_id: None,
      app_bundle_id: Some("com.mojang.minecraft".to_string()),
      window_title: None,
      window_number: None,
      region_hint: None,
      capture_artifact: None,
      capture_contract_artifact: None,
    },
    capture_contract_ref: None,
    evidence: Vec::new(),
    nodes,
    detail: serde_json::json!({
      "producer": "minecraft_spatial_frame_session_provider",
      "provider_id": observation.provider_id,
      "frame_id": observation.frame_id,
      "frame_index": index,
      "monotonic_timestamp_ms": observation.monotonic_timestamp_ms,
      "screen_state": observation.screen_state,
      "source_detail": observation.detail
    }),
    known_limits: observation.known_limits,
  }
}

fn minecraft_session_node_surface_node(
  run_id: &auv_tracing_driver::trace::RunId,
  span_id: &auv_tracing_driver::trace::SpanId,
  source_artifacts: &[String],
  node: MinecraftSessionNode,
) -> SurfaceNode {
  SurfaceNode {
    node_ref: NodeRef {
      run_id: run_id.clone(),
      span_id: span_id.clone(),
      node_id: node.node_id.clone(),
    },
    kind: node.kind.clone(),
    label: node.label,
    box_: RecognitionBox {
      x: node.bounds.origin.x.round() as i64,
      y: node.bounds.origin.y.round() as i64,
      width: node.bounds.size.width.round().max(0.0) as i64,
      height: node.bounds.size.height.round().max(0.0) as i64,
    },
    source_artifacts: source_artifacts.to_vec(),
    recognition_id: Some("minecraft_spatial_frame".to_string()),
    recognition_source: Some(RecognitionSource::Custom),
    recognition_surface: Some(RecognitionSurface::Window),
    recognized_item_id: Some(node.node_id),
    recognized_item_kind: Some(node.kind),
    provider_score: node.provider_score,
    detail: node.detail,
  }
}

#[cfg(test)]
mod tests {
  use auv_game_minecraft::{
    BlockFace, BlockPosition, NearbyBlock, PlayerPose, RaycastHit, Vec3, Viewport,
  };

  use crate::session::{ObserveRequest, SessionOptions, SessionRuntime};

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
    }
  }

  #[test]
  fn minecraft_spatial_frame_session_provider_feeds_session_runtime() {
    let provider =
      minecraft_spatial_frame_session_provider("minecraft.fixture.spatial", vec![frame()]);

    let mut session = SessionRuntime::new(SessionOptions::default());
    let provider_id = session.register_provider(provider);
    let observation = session
      .observe(&provider_id, ObserveRequest::default())
      .expect("minecraft spatial frame observation should succeed");

    assert_eq!(observation.snapshot.nodes.len(), 2);
    assert_eq!(
      observation.snapshot.scope.app_bundle_id.as_deref(),
      Some("com.mojang.minecraft")
    );
    assert_eq!(
      observation.provider_id.as_str(),
      "minecraft.fixture.spatial"
    );
    assert_eq!(
      observation.snapshot.detail["provider_id"],
      serde_json::json!("minecraft.fixture.spatial")
    );
    assert!(
      observation
        .snapshot
        .known_limits
        .iter()
        .any(|limit| limit.contains("telemetry truth"))
    );

    let raycast = session
      .find_node_by_label("minecraft:oak_button")
      .expect("minecraft raycast block should be addressable by session lookup");
    assert_eq!(raycast.node.kind, "minecraft_raycast_block");
    assert_eq!(raycast.node.box_.width, 800);
    assert_eq!(raycast.node.box_.height, 600);
    assert_eq!(raycast.node.provider_score, Some(1.0));
  }
}
