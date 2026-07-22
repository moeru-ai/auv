//! Minecraft inspect composition over canonical run snapshots.

use auv_tracing::{RunSnapshot, RunStore};

use super::quality::{MinecraftInspectedArtifact, MinecraftQualityBaseline, derive_quality_baseline};
use super::render::{append_primary_sections, append_quality_and_spatial_sections};
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
use crate::{
  MinecraftProjectionArtifact, ScenePacketManifest, TrainingLaunchJobManifest, TrainingPackageManifest,
  TrainingResultHoldoutPreviewManifest, TrainingResultHoldoutRenderQualityManifest, TrainingResultManifest, TrainingResultSemanticManifest,
  TrainingResultSpatialQueryActionReadiness, TrainingResultSpatialQueryManifest, derive_action_readiness,
};

pub enum MinecraftInspectSection {
  Primary(String),
  QualitySpatial(String),
}

impl MinecraftInspectSection {
  pub fn id(&self) -> &'static str {
    match self {
      Self::Primary(_) => "minecraft_primary",
      Self::QualitySpatial(_) => "minecraft_quality_spatial",
    }
  }

  pub fn text(&self) -> &str {
    match self {
      Self::Primary(text) | Self::QualitySpatial(text) => text,
    }
  }

  pub fn into_text(self) -> String {
    match self {
      Self::Primary(text) | Self::QualitySpatial(text) => text,
    }
  }
}

pub(crate) struct MinecraftPrimaryInspection {
  pub projections: Vec<MinecraftInspectedArtifact<MinecraftProjectionArtifact>>,
  pub scene_packets: Vec<MinecraftInspectedArtifact<ScenePacketManifest>>,
  pub training_packages: Vec<MinecraftInspectedArtifact<TrainingPackageManifest>>,
  pub training_jobs: Vec<MinecraftInspectedArtifact<TrainingLaunchJobManifest>>,
  pub training_results: Vec<MinecraftInspectedArtifact<TrainingResultManifest>>,
  pub semantics: Vec<MinecraftInspectedArtifact<TrainingResultSemanticManifest>>,
  pub holdout_previews: Vec<MinecraftInspectedArtifact<TrainingResultHoldoutPreviewManifest>>,
  pub render_quality: Vec<MinecraftInspectedArtifact<TrainingResultHoldoutRenderQualityManifest>>,
}

pub(crate) struct MinecraftSpatialQueryInspection {
  pub artifact: MinecraftInspectedArtifact<TrainingResultSpatialQueryManifest>,
  pub readiness: TrainingResultSpatialQueryActionReadiness,
}

pub struct MinecraftQualitySpatialInspection {
  pub(crate) spatial_queries: Vec<MinecraftSpatialQueryInspection>,
  baseline: MinecraftQualityBaseline,
}

impl MinecraftQualitySpatialInspection {
  pub fn quality_baseline(&self) -> &MinecraftQualityBaseline {
    &self.baseline
  }
}

pub async fn inspect_sections_primary(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<MinecraftInspectSection>, MinecraftArtifactReadError> {
  let inspection = read_primary_inspection(store, snapshot).await?;
  let mut text = String::new();
  append_primary_sections(&mut text, &inspection);
  Ok(vec![MinecraftInspectSection::Primary(text)])
}

pub async fn inspect_sections_quality_spatial(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<MinecraftInspectSection>, MinecraftArtifactReadError> {
  let inspection = read_minecraft_quality_spatial_inspection(store, snapshot).await?;
  let mut text = String::new();
  append_quality_and_spatial_sections(&mut text, &inspection);
  Ok(vec![MinecraftInspectSection::QualitySpatial(text)])
}

pub async fn read_minecraft_quality_spatial_inspection(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<MinecraftQualitySpatialInspection, MinecraftArtifactReadError> {
  validate_snapshot_authority(store, snapshot)?;

  let mut spatial_queries = Vec::new();
  let mut baseline_spatial_queries = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_TRAINING_SPATIAL_QUERY_PURPOSE)? {
    let payload = read_minecraft_training_spatial_query(store, snapshot, &uri).await?;
    let artifact = MinecraftInspectedArtifact::new(uri, payload);
    let readiness = derive_action_readiness(&artifact.payload);
    baseline_spatial_queries.push(artifact.clone());
    spatial_queries.push(MinecraftSpatialQueryInspection {
      artifact,
      readiness,
    });
  }
  let holdout_previews = read_holdout_previews(store, snapshot).await?;
  let render_quality = read_render_quality(store, snapshot).await?;
  let baseline = derive_quality_baseline(&baseline_spatial_queries, &holdout_previews, &render_quality);

  Ok(MinecraftQualitySpatialInspection {
    spatial_queries,
    baseline,
  })
}

async fn read_primary_inspection(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<MinecraftPrimaryInspection, MinecraftArtifactReadError> {
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

  let mut training_packages = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_TRAINING_PACKAGE_PURPOSE)? {
    let payload = read_minecraft_training_package(store, snapshot, &uri).await?;
    training_packages.push(MinecraftInspectedArtifact::new(uri, payload));
  }

  let mut training_jobs = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_TRAINING_JOB_PURPOSE)? {
    let payload = read_minecraft_training_job(store, snapshot, &uri).await?;
    training_jobs.push(MinecraftInspectedArtifact::new(uri, payload));
  }

  let mut training_results = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_TRAINING_RESULT_PURPOSE)? {
    let payload = read_minecraft_training_result(store, snapshot, &uri).await?;
    training_results.push(MinecraftInspectedArtifact::new(uri, payload));
  }

  let mut semantics = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_TRAINING_SEMANTIC_PURPOSE)? {
    let payload = read_minecraft_training_semantic(store, snapshot, &uri).await?;
    semantics.push(MinecraftInspectedArtifact::new(uri, payload));
  }

  Ok(MinecraftPrimaryInspection {
    projections,
    scene_packets,
    training_packages,
    training_jobs,
    training_results,
    semantics,
    holdout_previews: read_holdout_previews(store, snapshot).await?,
    render_quality: read_render_quality(store, snapshot).await?,
  })
}

async fn read_holdout_previews(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<MinecraftInspectedArtifact<TrainingResultHoldoutPreviewManifest>>, MinecraftArtifactReadError> {
  let mut values = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_TRAINING_HOLDOUT_PREVIEW_PURPOSE)? {
    let payload = read_minecraft_training_holdout_preview(store, snapshot, &uri).await?;
    values.push(MinecraftInspectedArtifact::new(uri, payload));
  }
  Ok(values)
}

async fn read_render_quality(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<MinecraftInspectedArtifact<TrainingResultHoldoutRenderQualityManifest>>, MinecraftArtifactReadError> {
  let mut values = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, MINECRAFT_TRAINING_HOLDOUT_RENDER_QUALITY_PURPOSE)? {
    let payload = read_minecraft_training_holdout_render_quality(store, snapshot, &uri).await?;
    values.push(MinecraftInspectedArtifact::new(uri, payload));
  }
  Ok(values)
}
