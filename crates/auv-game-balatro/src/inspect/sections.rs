//! Balatro InspectSection factory — loads store data and emits legacy fragments.

use std::sync::Arc;

use auv_inspect_model::{InspectError, InspectSection, InspectSectionOutput};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};

use super::render::append_sections;
use crate::run_read::{
  extract_balatro_card_detection_eval_witness_inspect_reports, extract_balatro_card_detection_eval_witness_manifests,
  extract_balatro_card_detection_quality_inspect_reports, extract_balatro_card_detection_quality_manifests,
  extract_balatro_card_detection_semantic_inspect_reports, extract_balatro_card_detection_semantic_manifests,
  extract_balatro_card_detection_spatial_query_inspect_reports, extract_balatro_card_detection_spatial_query_manifests,
};

/// Single balatro section covering all ordinary card-detection inspect headers.
pub struct BalatroCardDetectionSection;

impl InspectSection for BalatroCardDetectionSection {
  fn id(&self) -> &'static str {
    "balatro_card_detection"
  }

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError> {
    let text = render_balatro_card_detection_text(store, run)?;
    Ok(Some(InspectSectionOutput {
      id: self.id(),
      text,
      json: None,
    }))
  }
}

pub fn render_balatro_card_detection_text(store: &LocalStore, run: &CanonicalRun) -> Result<String, InspectError> {
  let semantic_manifests = extract_balatro_card_detection_semantic_manifests(store, run)?;
  let semantic_reports = extract_balatro_card_detection_semantic_inspect_reports(store, run)?;
  let spatial_manifests = extract_balatro_card_detection_spatial_query_manifests(store, run)?;
  let spatial_reports = extract_balatro_card_detection_spatial_query_inspect_reports(store, run)?;
  let witness_manifests = extract_balatro_card_detection_eval_witness_manifests(store, run)?;
  let witness_reports = extract_balatro_card_detection_eval_witness_inspect_reports(store, run)?;
  let quality_manifests = extract_balatro_card_detection_quality_manifests(store, run)?;
  let quality_reports = extract_balatro_card_detection_quality_inspect_reports(store, run)?;
  let mut output = String::new();
  append_sections(
    &mut output,
    &semantic_manifests,
    &semantic_reports,
    &spatial_manifests,
    &spatial_reports,
    &witness_manifests,
    &witness_reports,
    &quality_manifests,
    &quality_reports,
  );
  Ok(output)
}

pub fn inspect_sections() -> Vec<Arc<dyn InspectSection>> {
  vec![Arc::new(BalatroCardDetectionSection)]
}
