//! Osu InspectSection factories (A before query-wired, B after).

use std::sync::Arc;

use auv_inspect_model::{InspectError, InspectSection, InspectSectionOutput};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};

use super::render_a::append_sections_a;
use super::render_b::append_sections_b;
use crate::run_read::{
  extract_osu_detection_eval_quality_inspect_reports, extract_osu_detection_eval_quality_manifests,
  extract_osu_detection_eval_witness_inspect_reports, extract_osu_detection_eval_witness_manifests,
  extract_osu_visual_truth_semantic_inspect_reports, extract_osu_visual_truth_semantic_manifests,
  extract_osu_visual_truth_spatial_query_inspect_reports, extract_osu_visual_truth_spatial_query_manifests,
};

pub struct OsuVisualTruthPrimarySection;

impl InspectSection for OsuVisualTruthPrimarySection {
  fn id(&self) -> &'static str {
    "osu_visual_truth_primary"
  }

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError> {
    Ok(Some(InspectSectionOutput {
      id: self.id(),
      text: render_osu_primary_text(store, run)?,
      json: None,
    }))
  }
}

pub struct OsuDetectionEvalSection;

impl InspectSection for OsuDetectionEvalSection {
  fn id(&self) -> &'static str {
    "osu_detection_eval"
  }

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError> {
    Ok(Some(InspectSectionOutput {
      id: self.id(),
      text: render_osu_detection_eval_text(store, run)?,
      json: None,
    }))
  }
}

pub fn render_osu_primary_text(store: &LocalStore, run: &CanonicalRun) -> Result<String, InspectError> {
  let semantic_manifests = extract_osu_visual_truth_semantic_manifests(store, run)?;
  let semantic_reports = extract_osu_visual_truth_semantic_inspect_reports(store, run)?;
  let spatial_manifests = extract_osu_visual_truth_spatial_query_manifests(store, run)?;
  let spatial_reports = extract_osu_visual_truth_spatial_query_inspect_reports(store, run)?;
  let mut output = String::new();
  append_sections_a(&mut output, &semantic_manifests, &semantic_reports, &spatial_manifests, &spatial_reports);
  Ok(output)
}

pub fn render_osu_detection_eval_text(store: &LocalStore, run: &CanonicalRun) -> Result<String, InspectError> {
  let witness_manifests = extract_osu_detection_eval_witness_manifests(store, run)?;
  let witness_reports = extract_osu_detection_eval_witness_inspect_reports(store, run)?;
  let quality_manifests = extract_osu_detection_eval_quality_manifests(store, run)?;
  let quality_reports = extract_osu_detection_eval_quality_inspect_reports(store, run)?;
  let mut output = String::new();
  append_sections_b(&mut output, &witness_manifests, &witness_reports, &quality_manifests, &quality_reports);
  Ok(output)
}

/// Locked order for osu ordinary sections: primary (A), then later detection eval (B).
/// Product inserts query-wired between them.
pub fn inspect_sections_primary() -> Vec<Arc<dyn InspectSection>> {
  vec![Arc::new(OsuVisualTruthPrimarySection)]
}

pub fn inspect_sections_detection_eval() -> Vec<Arc<dyn InspectSection>> {
  vec![Arc::new(OsuDetectionEvalSection)]
}
