//! Balatro ordinary run_read helpers for inspect composition.
//!
//! Depends on `auv-inspect-model` only (no `auv-cli`). Query-wired adapters stay in product (S3b).

use auv_inspect_model::{ArtifactRefView, artifact_record_view, is_json_mime, read_artifact_json};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};

pub(crate) struct BalatroCardDetectionSemanticManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<BalatroCardDetectionSemanticManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct BalatroCardDetectionSemanticInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<BalatroCardDetectionSemanticInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct BalatroCardDetectionSpatialQueryManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<BalatroCardDetectionSpatialQueryManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct BalatroCardDetectionSpatialQueryInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<BalatroCardDetectionSpatialQueryInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct BalatroCardDetectionEvalWitnessManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<BalatroCardDetectionEvalWitnessManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct BalatroCardDetectionEvalWitnessInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<BalatroCardDetectionEvalWitnessInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct BalatroCardDetectionQualityManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<BalatroCardDetectionQualityManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct BalatroCardDetectionQualityInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<BalatroCardDetectionQualityInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BalatroCardDetectionSemanticManifestSummary {
  pub schema_version: u32,
  pub source_detection_bundle_dir: String,
  pub frame_source: String,
  pub image_width: u32,
  pub image_height: u32,
  pub ui_detection_count: usize,
  pub entities_detection_count: usize,
  pub semantic_status: String,
  pub semantic_reason: Option<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BalatroCardDetectionSemanticInspectReportSummary {
  pub schema_version: u32,
  pub card_detection_semantic_manifest_path: String,
  pub semantic_status: String,
  pub semantic_reason: Option<String>,
  pub detection_bundle_readable: bool,
  pub detection_sets_non_empty: bool,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BalatroCardDetectionSpatialQueryManifestSummary {
  pub schema_version: u32,
  pub card_detection_semantic_manifest_path: String,
  pub target_zone: String,
  pub target_index: u32,
  pub query_backend: String,
  pub status: String,
  pub reason: Option<String>,
  pub pixel_x: Option<f32>,
  pub pixel_y: Option<f32>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BalatroCardDetectionSpatialQueryInspectReportSummary {
  pub schema_version: u32,
  pub card_detection_spatial_query_manifest_path: String,
  pub target_zone: String,
  pub target_index: u32,
  pub query_backend: String,
  pub status: String,
  pub reason: Option<String>,
  pub semantic_status: String,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BalatroCardDetectionEvalWitnessManifestSummary {
  pub schema_version: u32,
  pub card_detection_semantic_manifest_path: String,
  pub card_detection_spatial_query_manifest_path: String,
  pub expected_slots_path: String,
  pub source_detection_bundle_dir: String,
  pub expected_slot_count: usize,
  pub scored_slot_count: usize,
  pub unscored_slot_count: usize,
  pub below_confidence_slot_count: usize,
  pub quality_backend: String,
  pub detector_model_id: Option<String>,
  pub slot_score_count: usize,
  pub status: String,
  pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BalatroCardDetectionEvalWitnessInspectReportSummary {
  pub schema_version: u32,
  pub card_detection_eval_witness_manifest_path: String,
  pub card_detection_semantic_manifest_path: String,
  pub card_detection_spatial_query_manifest_path: String,
  pub expected_slots_path: String,
  pub source_detection_bundle_dir: String,
  pub expected_slot_count: usize,
  pub scored_slot_count: usize,
  pub unscored_slot_count: usize,
  pub below_confidence_slot_count: usize,
  pub quality_backend: String,
  pub detector_model_id: Option<String>,
  pub slot_score_count: usize,
  pub semantic_manifest_readable: bool,
  pub spatial_query_manifest_readable: bool,
  pub expected_slots_readable: bool,
  pub status: String,
  pub reason: Option<String>,
  pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BalatroCardDetectionQualityManifestSummary {
  pub schema_version: u32,
  pub card_detection_eval_witness_manifest_path: String,
  pub witness_status: String,
  pub status: String,
  pub verdict: String,
  pub quality_backend: Option<String>,
  pub expected_slot_count: Option<usize>,
  pub scored_slot_count: Option<usize>,
  pub unscored_slot_count: Option<usize>,
  pub slot_coverage_ratio: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BalatroCardDetectionQualityInspectReportSummary {
  pub schema_version: u32,
  pub card_detection_quality_manifest_path: String,
  pub card_detection_eval_witness_manifest_path: String,
  pub witness_status: String,
  pub status: String,
  pub verdict: String,
  pub quality_backend: Option<String>,
  pub slot_coverage_ratio_available: bool,
}

pub(crate) fn extract_balatro_card_detection_semantic_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<BalatroCardDetectionSemanticManifestLineage>, String> {
  use crate::CardDetectionSemanticManifest;
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::BALATRO_CARD_DETECTION_SEMANTIC_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(BalatroCardDetectionSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("balatro card detection semantic manifest mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionSemanticManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::BALATRO_CARD_DETECTION_SEMANTIC_ROLE,
    )
    .map(|manifest| BalatroCardDetectionSemanticManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(BalatroCardDetectionSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(BalatroCardDetectionSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_balatro_card_detection_semantic_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<BalatroCardDetectionSemanticInspectReportLineage>, String> {
  use crate::CardDetectionSemanticInspectReport;
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::BALATRO_CARD_DETECTION_SEMANTIC_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(BalatroCardDetectionSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("balatro card detection semantic inspect mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionSemanticInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::BALATRO_CARD_DETECTION_SEMANTIC_INSPECT_ROLE,
    )
    .map(|report| BalatroCardDetectionSemanticInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(BalatroCardDetectionSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(BalatroCardDetectionSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_balatro_card_detection_spatial_query_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<BalatroCardDetectionSpatialQueryManifestLineage>, String> {
  use crate::CardDetectionSpatialQueryManifest;
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::BALATRO_CARD_DETECTION_SPATIAL_QUERY_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(BalatroCardDetectionSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("balatro card detection spatial query manifest mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionSpatialQueryManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::BALATRO_CARD_DETECTION_SPATIAL_QUERY_ROLE,
    )
    .map(|manifest| BalatroCardDetectionSpatialQueryManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(BalatroCardDetectionSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(BalatroCardDetectionSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_balatro_card_detection_spatial_query_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<BalatroCardDetectionSpatialQueryInspectReportLineage>, String> {
  use crate::CardDetectionSpatialQueryInspectReport;
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::BALATRO_CARD_DETECTION_SPATIAL_QUERY_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(BalatroCardDetectionSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("balatro card detection spatial query inspect mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionSpatialQueryInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::BALATRO_CARD_DETECTION_SPATIAL_QUERY_INSPECT_ROLE,
    )
    .map(|report| BalatroCardDetectionSpatialQueryInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(BalatroCardDetectionSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(BalatroCardDetectionSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_balatro_card_detection_eval_witness_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<BalatroCardDetectionEvalWitnessManifestLineage>, String> {
  use crate::CardDetectionEvalWitnessManifest;
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::BALATRO_CARD_DETECTION_EVAL_WITNESS_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(BalatroCardDetectionEvalWitnessManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("balatro card detection eval witness manifest mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionEvalWitnessManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::BALATRO_CARD_DETECTION_EVAL_WITNESS_ROLE,
    )
    .map(|manifest| BalatroCardDetectionEvalWitnessManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(BalatroCardDetectionEvalWitnessManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(BalatroCardDetectionEvalWitnessManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_balatro_card_detection_eval_witness_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<BalatroCardDetectionEvalWitnessInspectReportLineage>, String> {
  use crate::CardDetectionEvalWitnessInspectReport;
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::BALATRO_CARD_DETECTION_EVAL_WITNESS_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(BalatroCardDetectionEvalWitnessInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("balatro card detection eval witness inspect mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionEvalWitnessInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::BALATRO_CARD_DETECTION_EVAL_WITNESS_INSPECT_ROLE,
    )
    .map(|report| BalatroCardDetectionEvalWitnessInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(BalatroCardDetectionEvalWitnessInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(BalatroCardDetectionEvalWitnessInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_balatro_card_detection_quality_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<BalatroCardDetectionQualityManifestLineage>, String> {
  use crate::CardDetectionQualityManifest;
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::BALATRO_CARD_DETECTION_QUALITY_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(BalatroCardDetectionQualityManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("balatro card detection quality manifest mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionQualityManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::BALATRO_CARD_DETECTION_QUALITY_ROLE,
    )
    .map(|manifest| BalatroCardDetectionQualityManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(BalatroCardDetectionQualityManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(BalatroCardDetectionQualityManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_balatro_card_detection_quality_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<BalatroCardDetectionQualityInspectReportLineage>, String> {
  use crate::CardDetectionQualityInspectReport;
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::BALATRO_CARD_DETECTION_QUALITY_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(BalatroCardDetectionQualityInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("balatro card detection quality inspect mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionQualityInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::BALATRO_CARD_DETECTION_QUALITY_INSPECT_ROLE,
    )
    .map(|report| BalatroCardDetectionQualityInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(BalatroCardDetectionQualityInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(BalatroCardDetectionQualityInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

impl From<&crate::CardDetectionSemanticManifest> for BalatroCardDetectionSemanticManifestSummary {
  fn from(manifest: &crate::CardDetectionSemanticManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      source_detection_bundle_dir: manifest.source_detection_bundle_dir.clone(),
      frame_source: manifest.frame_source.clone(),
      image_width: manifest.image_width,
      image_height: manifest.image_height,
      ui_detection_count: manifest.ui_detection_count,
      entities_detection_count: manifest.entities_detection_count,
      semantic_status: manifest.semantic_status.as_str().to_string(),
      semantic_reason: manifest.semantic_reason.map(|reason| reason.as_str().to_string()),
      known_limits: manifest.known_limits.clone(),
    }
  }
}

impl From<&crate::CardDetectionSemanticInspectReport> for BalatroCardDetectionSemanticInspectReportSummary {
  fn from(report: &crate::CardDetectionSemanticInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      card_detection_semantic_manifest_path: report.card_detection_semantic_manifest_path.clone(),
      semantic_status: report.semantic_status.as_str().to_string(),
      semantic_reason: report.semantic_reason.map(|reason| reason.as_str().to_string()),
      detection_bundle_readable: report.detection_bundle_readable,
      detection_sets_non_empty: report.detection_sets_non_empty,
      warnings: report.warnings.clone(),
      known_limits: report.known_limits.clone(),
    }
  }
}

impl From<&crate::CardDetectionSpatialQueryManifest> for BalatroCardDetectionSpatialQueryManifestSummary {
  fn from(manifest: &crate::CardDetectionSpatialQueryManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      card_detection_semantic_manifest_path: manifest.card_detection_semantic_manifest_path.clone(),
      target_zone: manifest.target_zone.clone(),
      target_index: manifest.target_index,
      query_backend: manifest.query_backend.as_str().to_string(),
      status: manifest.status.as_str().to_string(),
      reason: manifest.reason.map(|reason| reason.as_str().to_string()),
      pixel_x: manifest.pixel_x,
      pixel_y: manifest.pixel_y,
      known_limits: manifest.known_limits.clone(),
    }
  }
}

impl From<&crate::CardDetectionSpatialQueryInspectReport> for BalatroCardDetectionSpatialQueryInspectReportSummary {
  fn from(report: &crate::CardDetectionSpatialQueryInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      card_detection_spatial_query_manifest_path: report.card_detection_spatial_query_manifest_path.clone(),
      target_zone: report.target_zone.clone(),
      target_index: report.target_index,
      query_backend: report.query_backend.as_str().to_string(),
      status: report.status.as_str().to_string(),
      reason: report.reason.map(|reason| reason.as_str().to_string()),
      semantic_status: report.semantic_status.as_str().to_string(),
      warnings: report.warnings.clone(),
      known_limits: report.known_limits.clone(),
    }
  }
}

impl From<&crate::CardDetectionEvalWitnessManifest> for BalatroCardDetectionEvalWitnessManifestSummary {
  fn from(manifest: &crate::CardDetectionEvalWitnessManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      card_detection_semantic_manifest_path: manifest.card_detection_semantic_manifest_path.clone(),
      card_detection_spatial_query_manifest_path: manifest.card_detection_spatial_query_manifest_path.clone(),
      expected_slots_path: manifest.expected_slots_path.clone(),
      source_detection_bundle_dir: manifest.source_detection_bundle_dir.clone(),
      expected_slot_count: manifest.expected_slot_count,
      scored_slot_count: manifest.scored_slot_count,
      unscored_slot_count: manifest.unscored_slot_count,
      below_confidence_slot_count: manifest.below_confidence_slot_count,
      quality_backend: manifest.quality_backend.as_str().to_string(),
      detector_model_id: manifest.detector_model_id.clone(),
      slot_score_count: manifest.slot_scores.len(),
      status: manifest.status.as_str().to_string(),
      reason: manifest.reason.map(|reason| reason.as_str().to_string()),
    }
  }
}

impl From<&crate::CardDetectionEvalWitnessInspectReport> for BalatroCardDetectionEvalWitnessInspectReportSummary {
  fn from(report: &crate::CardDetectionEvalWitnessInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      card_detection_eval_witness_manifest_path: report.card_detection_eval_witness_manifest_path.clone(),
      card_detection_semantic_manifest_path: report.card_detection_semantic_manifest_path.clone(),
      card_detection_spatial_query_manifest_path: report.card_detection_spatial_query_manifest_path.clone(),
      expected_slots_path: report.expected_slots_path.clone(),
      source_detection_bundle_dir: report.source_detection_bundle_dir.clone(),
      expected_slot_count: report.expected_slot_count,
      scored_slot_count: report.scored_slot_count,
      unscored_slot_count: report.unscored_slot_count,
      below_confidence_slot_count: report.below_confidence_slot_count,
      quality_backend: report.quality_backend.as_str().to_string(),
      detector_model_id: report.detector_model_id.clone(),
      slot_score_count: report.slot_score_count,
      semantic_manifest_readable: report.semantic_manifest_readable,
      spatial_query_manifest_readable: report.spatial_query_manifest_readable,
      expected_slots_readable: report.expected_slots_readable,
      status: report.status.as_str().to_string(),
      reason: report.reason.map(|reason| reason.as_str().to_string()),
      warnings: report.warnings.clone(),
    }
  }
}

impl From<&crate::CardDetectionQualityManifest> for BalatroCardDetectionQualityManifestSummary {
  fn from(manifest: &crate::CardDetectionQualityManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      card_detection_eval_witness_manifest_path: manifest.card_detection_eval_witness_manifest_path.clone(),
      witness_status: manifest.witness_status.as_str().to_string(),
      status: manifest.status.as_str().to_string(),
      verdict: manifest.verdict.as_str().to_string(),
      quality_backend: manifest.quality_backend.map(|backend| backend.as_str().to_string()),
      expected_slot_count: manifest.metrics.as_ref().map(|m| m.expected_slot_count),
      scored_slot_count: manifest.metrics.as_ref().map(|m| m.scored_slot_count),
      unscored_slot_count: manifest.metrics.as_ref().map(|m| m.unscored_slot_count),
      slot_coverage_ratio: manifest.metrics.as_ref().and_then(|m| m.slot_coverage_ratio),
    }
  }
}

impl From<&crate::CardDetectionQualityInspectReport> for BalatroCardDetectionQualityInspectReportSummary {
  fn from(report: &crate::CardDetectionQualityInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      card_detection_quality_manifest_path: report.card_detection_quality_manifest_path.clone(),
      card_detection_eval_witness_manifest_path: report.card_detection_eval_witness_manifest_path.clone(),
      witness_status: report.witness_status.as_str().to_string(),
      status: report.status.as_str().to_string(),
      verdict: report.verdict.as_str().to_string(),
      quality_backend: report.quality_backend.map(|backend| backend.as_str().to_string()),
      slot_coverage_ratio_available: report.slot_coverage_ratio_available,
    }
  }
}
