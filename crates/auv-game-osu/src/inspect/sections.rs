//! osu! inspect composition over canonical run snapshots.

use auv_tracing::{RunSnapshot, RunStore};

use super::render_a::append_sections_a;
use super::render_b::append_sections_b;
use crate::run_read::{
  OsuArtifactReadError, extract_osu_detection_eval_quality_manifests, extract_osu_detection_eval_witness_manifests,
  extract_osu_visual_truth_semantic_manifests, extract_osu_visual_truth_spatial_query_manifests, validate_snapshot_authority,
};

pub struct OsuVisualTruthPrimarySection;

impl OsuVisualTruthPrimarySection {
  pub const ID: &'static str = "osu_visual_truth_primary";

  pub async fn collect(&self, store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<String, OsuArtifactReadError> {
    render_osu_primary_text(store, snapshot).await
  }
}

pub struct OsuDetectionEvalSection;

impl OsuDetectionEvalSection {
  pub const ID: &'static str = "osu_detection_eval";

  pub async fn collect(&self, store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<String, OsuArtifactReadError> {
    render_osu_detection_eval_text(store, snapshot).await
  }
}

#[derive(Debug)]
pub enum OsuInspectSection {
  Primary(String),
  DetectionEval(String),
}

impl OsuInspectSection {
  pub fn id(&self) -> &'static str {
    match self {
      Self::Primary(_) => OsuVisualTruthPrimarySection::ID,
      Self::DetectionEval(_) => OsuDetectionEvalSection::ID,
    }
  }

  pub fn text(&self) -> &str {
    match self {
      Self::Primary(text) | Self::DetectionEval(text) => text,
    }
  }

  pub fn into_text(self) -> String {
    match self {
      Self::Primary(text) | Self::DetectionEval(text) => text,
    }
  }
}

pub async fn render_osu_primary_text(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<String, OsuArtifactReadError> {
  validate_snapshot_authority(store, snapshot)?;
  let semantic_manifests = extract_osu_visual_truth_semantic_manifests(store, snapshot).await?;
  let spatial_manifests = extract_osu_visual_truth_spatial_query_manifests(store, snapshot).await?;
  let mut output = String::new();
  append_sections_a(&mut output, &semantic_manifests, &spatial_manifests);
  Ok(output)
}

pub async fn render_osu_detection_eval_text(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<String, OsuArtifactReadError> {
  validate_snapshot_authority(store, snapshot)?;
  let witness_manifests = extract_osu_detection_eval_witness_manifests(store, snapshot).await?;
  let quality_manifests = extract_osu_detection_eval_quality_manifests(store, snapshot).await?;
  let mut output = String::new();
  append_sections_b(&mut output, &witness_manifests, &quality_manifests);
  Ok(output)
}

pub async fn inspect_sections_primary(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<Vec<OsuInspectSection>, OsuArtifactReadError> {
  Ok(vec![OsuInspectSection::Primary(
    render_osu_primary_text(store, snapshot).await?,
  )])
}

pub async fn inspect_sections_detection_eval(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<OsuInspectSection>, OsuArtifactReadError> {
  Ok(vec![OsuInspectSection::DetectionEval(
    render_osu_detection_eval_text(store, snapshot).await?,
  )])
}
