//! Balatro inspect composition over canonical run snapshots.

use auv_tracing::{RunSnapshot, RunStore};

use super::render::append_sections;
use crate::card_detection_eval_witness::{CARD_DETECTION_EVAL_WITNESS_PURPOSE, read_card_detection_witness};
use crate::card_detection_quality::{CARD_DETECTION_QUALITY_PURPOSE, read_card_detection_quality};
use crate::card_detection_semantic::{CARD_DETECTION_SEMANTIC_PURPOSE, read_card_detection_semantic};
use crate::card_detection_spatial_query::{CARD_DETECTION_SPATIAL_QUERY_PURPOSE, read_card_detection_spatial_query};
use crate::run_read::{BalatroArtifactReadError, artifact_uris_for_purpose, validate_snapshot_authority};

pub struct BalatroCardDetectionSection;

impl BalatroCardDetectionSection {
  pub const ID: &'static str = "balatro_card_detection";

  pub async fn collect(&self, store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<String, BalatroArtifactReadError> {
    render_balatro_card_detection_text(store, snapshot).await
  }
}

pub async fn render_balatro_card_detection_text(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<String, BalatroArtifactReadError> {
  validate_snapshot_authority(store, snapshot)?;

  let mut semantic_manifests = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, CARD_DETECTION_SEMANTIC_PURPOSE)? {
    let manifest = read_card_detection_semantic(store, snapshot, &uri).await?;
    semantic_manifests.push((uri, manifest));
  }

  let mut spatial_query_manifests = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, CARD_DETECTION_SPATIAL_QUERY_PURPOSE)? {
    let manifest = read_card_detection_spatial_query(store, snapshot, &uri).await?;
    spatial_query_manifests.push((uri, manifest));
  }

  let mut witness_manifests = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, CARD_DETECTION_EVAL_WITNESS_PURPOSE)? {
    let manifest = read_card_detection_witness(store, snapshot, &uri).await?;
    witness_manifests.push((uri, manifest));
  }

  let mut quality_manifests = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, CARD_DETECTION_QUALITY_PURPOSE)? {
    let manifest = read_card_detection_quality(store, snapshot, &uri).await?;
    quality_manifests.push((uri, manifest));
  }

  let mut output = String::new();
  append_sections(&mut output, &semantic_manifests, &spatial_query_manifests, &witness_manifests, &quality_manifests);
  Ok(output)
}

/// TODO: Remove this empty legacy factory in run-contract Task 22, when the
/// product composer moves from its synchronous local-store trait to canonical
/// async `RunStore` sections. New Balatro inspection uses
/// [`BalatroCardDetectionSection::collect`].
#[doc(hidden)]
pub fn inspect_sections<T>() -> Vec<T> {
  Vec::new()
}
