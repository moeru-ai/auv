//! Minecraft inspect composition over canonical run snapshots.

use auv_tracing::{ArtifactUri, RunSnapshot, RunStore};

use crate::artifact::{MINECRAFT_PROJECTION_PURPOSE, read_minecraft_projection};
use crate::run_read::{MinecraftArtifactReadError, artifact_uris_for_purpose, validate_snapshot_authority};
use crate::scene_packet::{MINECRAFT_SCENE_PACKET_PURPOSE, read_minecraft_scene_packet};
use crate::{MinecraftProjectionArtifact, ScenePacketManifest};

pub enum MinecraftInspectSection {
  Primary(String),
}

impl MinecraftInspectSection {
  pub fn id(&self) -> &'static str {
    "minecraft_primary"
  }

  pub fn text(&self) -> &str {
    match self {
      Self::Primary(text) => text,
    }
  }

  pub fn into_text(self) -> String {
    match self {
      Self::Primary(text) => text,
    }
  }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftInspectedArtifact<T> {
  pub uri: ArtifactUri,
  pub payload: T,
}

impl<T> MinecraftInspectedArtifact<T> {
  fn new(uri: ArtifactUri, payload: T) -> Self {
    Self { uri, payload }
  }
}

pub async fn inspect_sections_primary(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<MinecraftInspectSection>, MinecraftArtifactReadError> {
  validate_snapshot_authority(store, snapshot)?;

  let mut projections = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_PROJECTION_PURPOSE)? {
    let payload = read_minecraft_projection(store, snapshot, &uri).await?;
    projections.push(MinecraftInspectedArtifact::new(uri, payload));
  }

  let mut scene_packets = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_SCENE_PACKET_PURPOSE)? {
    let payload = read_minecraft_scene_packet(store, snapshot, &uri).await?;
    scene_packets.push(MinecraftInspectedArtifact::new(uri, payload));
  }

  Ok(vec![MinecraftInspectSection::Primary(render_primary(
    &projections,
    &scene_packets,
  ))])
}

fn render_primary(
  projections: &[MinecraftInspectedArtifact<MinecraftProjectionArtifact>],
  scene_packets: &[MinecraftInspectedArtifact<ScenePacketManifest>],
) -> String {
  let mut output = String::new();
  output.push_str("\nMC-2 Projection Artifacts:\n");
  if projections.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in projections {
      output.push_str(&format!("- artifact={} frame={}\n", artifact.uri, artifact.payload.spatial_frame_id));
    }
  }

  output.push_str("\nMC-8 Scene Packets:\n");
  if scene_packets.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in scene_packets {
      output.push_str(&format!("- artifact={} schema={}\n", artifact.uri, artifact.payload.schema_version));
    }
  }
  output
}
