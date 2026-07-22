//! Minecraft inspect composition over canonical run snapshots.

use auv_tracing::{RunSnapshot, RunStore};

use super::render::{PrimaryArtifacts, append_primary_sections, append_quality_and_spatial_sections};
use crate::artifact::{MINECRAFT_PROJECTION_PURPOSE, read_minecraft_projection};
use crate::run_read::{MinecraftArtifactReadError, artifact_uris_for_purpose, validate_snapshot_authority};
use crate::scene_packet::{MINECRAFT_SCENE_PACKET_PURPOSE, read_minecraft_scene_packet};
use crate::training_job::{MINECRAFT_TRAINING_JOB_PURPOSE, read_minecraft_training_job};
use crate::training_package::{MINECRAFT_TRAINING_PACKAGE_PURPOSE, read_minecraft_training_package};
use crate::training_result::{MINECRAFT_TRAINING_RESULT_PURPOSE, read_minecraft_training_result};
use crate::training_result_holdout_preview::{MINECRAFT_TRAINING_HOLDOUT_PREVIEW_PURPOSE, read_minecraft_training_holdout_preview};
use crate::training_result_holdout_render_quality::{
  MINECRAFT_TRAINING_HOLDOUT_RENDER_QUALITY_PURPOSE, read_minecraft_training_holdout_render_quality,
};
use crate::training_result_semantic::{MINECRAFT_TRAINING_SEMANTIC_PURPOSE, read_minecraft_training_semantic};
use crate::training_result_spatial_query::{MINECRAFT_TRAINING_SPATIAL_QUERY_PURPOSE, read_minecraft_training_spatial_query};

pub struct MinecraftPrimarySection;

impl MinecraftPrimarySection {
  pub const ID: &'static str = "minecraft_primary";

  pub async fn collect(&self, store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<String, MinecraftArtifactReadError> {
    render_minecraft_primary_text(store, snapshot).await
  }
}

pub struct MinecraftQualitySpatialSection;

impl MinecraftQualitySpatialSection {
  pub const ID: &'static str = "minecraft_quality_spatial";

  pub async fn collect(&self, store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<String, MinecraftArtifactReadError> {
    render_minecraft_quality_spatial_text(store, snapshot).await
  }
}

pub async fn render_minecraft_primary_text(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<String, MinecraftArtifactReadError> {
  validate_snapshot_authority(store, snapshot)?;

  let mut projections = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_PROJECTION_PURPOSE)? {
    let value = read_minecraft_projection(store, snapshot, &uri).await?;
    projections.push((uri, value));
  }

  let mut scene_packets = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_SCENE_PACKET_PURPOSE)? {
    let value = read_minecraft_scene_packet(store, snapshot, &uri).await?;
    scene_packets.push((uri, value));
  }

  let mut training_packages = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_TRAINING_PACKAGE_PURPOSE)? {
    let value = read_minecraft_training_package(store, snapshot, &uri).await?;
    training_packages.push((uri, value));
  }

  let mut training_jobs = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_TRAINING_JOB_PURPOSE)? {
    let value = read_minecraft_training_job(store, snapshot, &uri).await?;
    training_jobs.push((uri, value));
  }

  let mut training_results = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_TRAINING_RESULT_PURPOSE)? {
    let value = read_minecraft_training_result(store, snapshot, &uri).await?;
    training_results.push((uri, value));
  }

  let mut semantics = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_TRAINING_SEMANTIC_PURPOSE)? {
    let value = read_minecraft_training_semantic(store, snapshot, &uri).await?;
    semantics.push((uri, value));
  }

  let holdout_previews = read_holdout_previews(store, snapshot).await?;
  let render_quality = read_render_quality(store, snapshot).await?;

  let mut output = String::new();
  append_primary_sections(
    &mut output,
    PrimaryArtifacts {
      projections: &projections,
      scene_packets: &scene_packets,
      training_packages: &training_packages,
      training_jobs: &training_jobs,
      training_results: &training_results,
      semantics: &semantics,
      holdout_previews: &holdout_previews,
      render_quality: &render_quality,
    },
  );
  Ok(output)
}

pub async fn render_minecraft_quality_spatial_text(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<String, MinecraftArtifactReadError> {
  validate_snapshot_authority(store, snapshot)?;

  let mut spatial_queries = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_TRAINING_SPATIAL_QUERY_PURPOSE)? {
    let value = read_minecraft_training_spatial_query(store, snapshot, &uri).await?;
    spatial_queries.push((uri, value));
  }
  let holdout_previews = read_holdout_previews(store, snapshot).await?;
  let render_quality = read_render_quality(store, snapshot).await?;

  let mut output = String::new();
  append_quality_and_spatial_sections(&mut output, &spatial_queries, &holdout_previews, &render_quality);
  Ok(output)
}

async fn read_holdout_previews(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<(auv_tracing::ArtifactUri, crate::TrainingResultHoldoutPreviewManifest)>, MinecraftArtifactReadError> {
  let mut values = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_TRAINING_HOLDOUT_PREVIEW_PURPOSE)? {
    let value = read_minecraft_training_holdout_preview(store, snapshot, &uri).await?;
    values.push((uri, value));
  }
  Ok(values)
}

async fn read_render_quality(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<(auv_tracing::ArtifactUri, crate::TrainingResultHoldoutRenderQualityManifest)>, MinecraftArtifactReadError> {
  let mut values = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_TRAINING_HOLDOUT_RENDER_QUALITY_PURPOSE)? {
    let value = read_minecraft_training_holdout_render_quality(store, snapshot, &uri).await?;
    values.push((uri, value));
  }
  Ok(values)
}

/// TODO: Remove this empty legacy factory in run-contract Task 22, when the
/// product composer accepts canonical async `RunStore` sections.
#[doc(hidden)]
pub fn inspect_sections_primary<T>() -> Vec<T> {
  Vec::new()
}

/// TODO: Remove this empty legacy factory in run-contract Task 22, when the
/// product composer accepts canonical async `RunStore` sections.
#[doc(hidden)]
pub fn inspect_sections_quality_spatial<T>() -> Vec<T> {
  Vec::new()
}
