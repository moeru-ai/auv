//! Osu ordinary run_read helpers for inspect composition.
//!
//! Depends on `auv-inspect-model` only (no `auv-cli`). Query-wired adapters stay in product (S3b).

use crate::{
  DetectionEvalQualityInspectReport, DetectionEvalQualityManifest, DetectionEvalWitnessInspectReport, DetectionEvalWitnessManifest,
  VisualTruthSemanticInspectReport, VisualTruthSemanticManifest, VisualTruthSpatialQueryInspectReport, VisualTruthSpatialQueryManifest,
  derive_visual_truth_spatial_query_action_readiness,
};
use auv_inspect_model::legacy::{ArtifactRefView, artifact_record_view, is_json_mime, read_artifact_json};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};

pub(crate) struct OsuVisualTruthSemanticManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<OsuVisualTruthSemanticManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct OsuVisualTruthSemanticInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<OsuVisualTruthSemanticInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuVisualTruthSpatialQueryManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<OsuVisualTruthSpatialQueryManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct OsuVisualTruthSpatialQueryInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<OsuVisualTruthSpatialQueryInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuVisualTruthSpatialQueryActionReadinessSummary {
  pub action_eligibility: String,
  pub pixel_point: Option<String>,
  pub refusal_reason: Option<String>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OsuVisualTruthSemanticManifestSummary {
  pub schema_version: u32,
  pub source_run_artifact_dir: String,
  pub source_visual_truth_manifest_path: String,
  pub source_projection_path: String,
  pub beatmap_path: String,
  pub frame_count: usize,
  pub semantic_status: String,
  pub semantic_reason: Option<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OsuVisualTruthSemanticInspectReportSummary {
  pub schema_version: u32,
  pub visual_truth_semantic_manifest_path: String,
  pub source_run_artifact_dir: String,
  pub semantic_status: String,
  pub semantic_reason: Option<String>,
  pub visual_truth_manifest_readable: bool,
  pub projection_readable: bool,
  pub projection_eval_ready: bool,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OsuVisualTruthSpatialQueryManifestSummary {
  pub schema_version: u32,
  pub visual_truth_semantic_manifest_path: String,
  pub source_run_artifact_dir: String,
  pub object_index: usize,
  pub capture_phase: String,
  pub object_kind: Option<String>,
  pub query_backend: String,
  pub status: String,
  pub reason: Option<String>,
  pub pixel_visibility: Option<String>,
  pub pixel_x: Option<f32>,
  pub pixel_y: Option<f32>,
  pub match_radius_px: Option<f32>,
  pub capture_width: Option<u32>,
  pub capture_height: Option<u32>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OsuVisualTruthSpatialQueryInspectReportSummary {
  pub schema_version: u32,
  pub visual_truth_spatial_query_manifest_path: String,
  pub visual_truth_semantic_manifest_path: String,
  pub object_index: usize,
  pub capture_phase: String,
  pub query_backend: String,
  pub status: String,
  pub reason: Option<String>,
  pub pixel_visibility: Option<String>,
  pub semantic_status: String,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct OsuDetectionEvalWitnessManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<OsuDetectionEvalWitnessManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct OsuDetectionEvalWitnessInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<OsuDetectionEvalWitnessInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct OsuDetectionEvalQualityManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<OsuDetectionEvalQualityManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct OsuDetectionEvalQualityInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<OsuDetectionEvalQualityInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OsuDetectionEvalWitnessManifestSummary {
  pub schema_version: u32,
  pub source_visual_eval_report_path: String,
  pub source_run_artifact_dir: String,
  pub detector_model_id: Option<String>,
  pub total_frames: usize,
  pub label_matched_frames: usize,
  pub spatial_matched_frames: usize,
  pub spatial_unscored_frames: usize,
  pub spurious_detection_count: usize,
  pub projection_kind: String,
  pub frame_witness_count: usize,
  pub status: String,
  pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OsuDetectionEvalWitnessInspectReportSummary {
  pub schema_version: u32,
  pub detection_eval_witness_manifest_path: String,
  pub total_frames: usize,
  pub frame_witness_count: usize,
  pub status: String,
  pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OsuDetectionEvalQualityManifestSummary {
  pub schema_version: u32,
  pub detection_eval_witness_manifest_path: String,
  pub source_visual_eval_report_path: String,
  pub witness_status: String,
  pub status: String,
  pub verdict: String,
  pub label_recall: Option<f32>,
  pub spatial_recall: Option<f32>,
  pub spurious_detection_count: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OsuDetectionEvalQualityInspectReportSummary {
  pub schema_version: u32,
  pub detection_eval_quality_manifest_path: String,
  pub witness_status: String,
  pub status: String,
  pub verdict: String,
  pub label_recall_available: bool,
  pub spatial_recall_available: bool,
}

pub(crate) fn extract_osu_visual_truth_semantic_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<OsuVisualTruthSemanticManifestLineage>, String> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::OSU_VISUAL_TRUTH_SEMANTIC_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(OsuVisualTruthSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("osu visual truth semantic manifest mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed =
      read_artifact_json::<VisualTruthSemanticManifest>(store, run.run.run_id.as_str(), artifact, crate::OSU_VISUAL_TRUTH_SEMANTIC_ROLE)
        .map(|manifest| OsuVisualTruthSemanticManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(OsuVisualTruthSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(OsuVisualTruthSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_osu_visual_truth_semantic_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<OsuVisualTruthSemanticInspectReportLineage>, String> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::OSU_VISUAL_TRUTH_SEMANTIC_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(OsuVisualTruthSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("osu visual truth semantic inspect mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<VisualTruthSemanticInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::OSU_VISUAL_TRUTH_SEMANTIC_INSPECT_ROLE,
    )
    .map(|report| OsuVisualTruthSemanticInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(OsuVisualTruthSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(OsuVisualTruthSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub fn extract_osu_visual_truth_spatial_query_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<OsuVisualTruthSpatialQueryManifestLineage>, String> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(OsuVisualTruthSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("osu visual truth spatial query manifest mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<VisualTruthSpatialQueryManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE,
    )
    .map(|manifest| OsuVisualTruthSpatialQueryManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(OsuVisualTruthSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(OsuVisualTruthSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_osu_visual_truth_spatial_query_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<OsuVisualTruthSpatialQueryInspectReportLineage>, String> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::OSU_VISUAL_TRUTH_SPATIAL_QUERY_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(OsuVisualTruthSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("osu visual truth spatial query inspect mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<VisualTruthSpatialQueryInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::OSU_VISUAL_TRUTH_SPATIAL_QUERY_INSPECT_ROLE,
    )
    .map(|report| OsuVisualTruthSpatialQueryInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(OsuVisualTruthSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(OsuVisualTruthSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_osu_detection_eval_witness_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<OsuDetectionEvalWitnessManifestLineage>, String> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::OSU_DETECTION_EVAL_WITNESS_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(OsuDetectionEvalWitnessManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("osu detection eval witness manifest mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed =
      read_artifact_json::<DetectionEvalWitnessManifest>(store, run.run.run_id.as_str(), artifact, crate::OSU_DETECTION_EVAL_WITNESS_ROLE)
        .map(|manifest| OsuDetectionEvalWitnessManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(OsuDetectionEvalWitnessManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(OsuDetectionEvalWitnessManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_osu_detection_eval_witness_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<OsuDetectionEvalWitnessInspectReportLineage>, String> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::OSU_DETECTION_EVAL_WITNESS_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(OsuDetectionEvalWitnessInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("osu detection eval witness inspect mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<DetectionEvalWitnessInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::OSU_DETECTION_EVAL_WITNESS_INSPECT_ROLE,
    )
    .map(|report| OsuDetectionEvalWitnessInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(OsuDetectionEvalWitnessInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(OsuDetectionEvalWitnessInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_osu_detection_eval_quality_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<OsuDetectionEvalQualityManifestLineage>, String> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::OSU_DETECTION_EVAL_QUALITY_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(OsuDetectionEvalQualityManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("osu detection eval quality manifest mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed =
      read_artifact_json::<DetectionEvalQualityManifest>(store, run.run.run_id.as_str(), artifact, crate::OSU_DETECTION_EVAL_QUALITY_ROLE)
        .map(|manifest| OsuDetectionEvalQualityManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(OsuDetectionEvalQualityManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(OsuDetectionEvalQualityManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_osu_detection_eval_quality_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<OsuDetectionEvalQualityInspectReportLineage>, String> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::OSU_DETECTION_EVAL_QUALITY_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(OsuDetectionEvalQualityInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("osu detection eval quality inspect mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<DetectionEvalQualityInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::OSU_DETECTION_EVAL_QUALITY_INSPECT_ROLE,
    )
    .map(|report| OsuDetectionEvalQualityInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(OsuDetectionEvalQualityInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(OsuDetectionEvalQualityInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn derive_osu_detection_eval_quality_verdict_summary(lineage: &OsuDetectionEvalQualityManifestLineage) -> String {
  if lineage.issue.is_some() {
    return "n/a".to_string();
  }
  let Some(summary) = &lineage.manifest else {
    return "n/a".to_string();
  };
  summary.verdict.clone()
}

pub fn derive_osu_visual_truth_spatial_query_action_readiness(
  lineage: &OsuVisualTruthSpatialQueryManifestLineage,
) -> OsuVisualTruthSpatialQueryActionReadinessSummary {
  if let Some(issue) = &lineage.issue {
    return OsuVisualTruthSpatialQueryActionReadinessSummary {
      action_eligibility: "n/a".to_string(),
      pixel_point: None,
      refusal_reason: None,
      issue: Some(issue.clone()),
    };
  }
  let Some(summary) = &lineage.manifest else {
    return OsuVisualTruthSpatialQueryActionReadinessSummary {
      action_eligibility: "n/a".to_string(),
      pixel_point: None,
      refusal_reason: None,
      issue: Some("osu visual truth spatial query manifest summary missing".to_string()),
    };
  };
  let manifest = match osu_spatial_query_manifest_summary_for_action_readiness(summary) {
    Ok(manifest) => manifest,
    Err(error) => {
      return OsuVisualTruthSpatialQueryActionReadinessSummary {
        action_eligibility: "n/a".to_string(),
        pixel_point: None,
        refusal_reason: None,
        issue: Some(error),
      };
    }
  };
  let readiness = derive_visual_truth_spatial_query_action_readiness(&manifest);
  OsuVisualTruthSpatialQueryActionReadinessSummary {
    action_eligibility: readiness.eligibility.as_str().to_string(),
    pixel_point: readiness.pixel_point.map(|(x, y)| format!("{x},{y}")),
    refusal_reason: readiness.refusal_reason,
    issue: None,
  }
}

fn osu_spatial_query_manifest_summary_for_action_readiness(
  summary: &OsuVisualTruthSpatialQueryManifestSummary,
) -> Result<VisualTruthSpatialQueryManifest, String> {
  use crate::{
    CapturePhase, ObjectKind, VisualTruthPixelVisibility, VisualTruthSpatialQueryBackend, VisualTruthSpatialQueryReason,
    VisualTruthSpatialQueryStatus,
  };
  let capture_phase = match summary.capture_phase.as_str() {
    "before_dispatch" => CapturePhase::BeforeDispatch,
    "after_dispatch" => CapturePhase::AfterDispatch,
    other => return Err(format!("unknown capture_phase {other}")),
  };
  let object_kind = match summary.object_kind.as_deref() {
    None => None,
    Some(kind) => Some(match kind {
      "circle" => ObjectKind::Circle,
      "slider" => ObjectKind::Slider,
      "spinner" => ObjectKind::Spinner,
      "hold" => ObjectKind::Hold,
      other => return Err(format!("unknown object_kind {other}")),
    }),
  };
  let status = match summary.status.as_str() {
    "answered" => VisualTruthSpatialQueryStatus::Answered,
    "blocked" => VisualTruthSpatialQueryStatus::Blocked,
    "failed" => VisualTruthSpatialQueryStatus::Failed,
    other => return Err(format!("unknown query status {other}")),
  };
  let reason = match summary.reason.as_deref() {
    None => None,
    Some(reason) => Some(match reason {
      "semantic_source_not_ready" => VisualTruthSpatialQueryReason::SemanticSourceNotReady,
      "target_absent_from_visual_truth" => VisualTruthSpatialQueryReason::TargetAbsentFromVisualTruth,
      "projection_unavailable" => VisualTruthSpatialQueryReason::ProjectionUnavailable,
      other => return Err(format!("unknown query reason {other}")),
    }),
  };
  let pixel_visibility = match summary.pixel_visibility.as_deref() {
    None => None,
    Some(value) => Some(match value {
      "inside_capture" => VisualTruthPixelVisibility::InsideCapture,
      "outside_capture" => VisualTruthPixelVisibility::OutsideCapture,
      other => return Err(format!("unknown pixel_visibility {other}")),
    }),
  };
  let query_backend = match summary.query_backend.as_str() {
    "playfield_projection_reference" => VisualTruthSpatialQueryBackend::PlayfieldProjectionReference,
    other => return Err(format!("unknown query backend {other}")),
  };
  Ok(VisualTruthSpatialQueryManifest {
    schema_version: summary.schema_version,
    generated_at_millis: 0,
    visual_truth_semantic_manifest_path: summary.visual_truth_semantic_manifest_path.clone(),
    source_run_artifact_dir: summary.source_run_artifact_dir.clone(),
    source_visual_truth_manifest_path: String::new(),
    source_projection_path: String::new(),
    object_index: summary.object_index,
    capture_phase,
    object_kind,
    query_backend,
    status,
    reason,
    pixel_visibility,
    pixel_x: summary.pixel_x,
    pixel_y: summary.pixel_y,
    match_radius_px: summary.match_radius_px,
    capture_width: summary.capture_width,
    capture_height: summary.capture_height,
    known_limits: summary.known_limits.clone(),
  })
}

impl From<&DetectionEvalWitnessManifest> for OsuDetectionEvalWitnessManifestSummary {
  fn from(manifest: &DetectionEvalWitnessManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      source_visual_eval_report_path: manifest.source_visual_eval_report_path.clone(),
      source_run_artifact_dir: manifest.source_run_artifact_dir.clone(),
      detector_model_id: manifest.detector_model_id.clone(),
      total_frames: manifest.total_frames,
      label_matched_frames: manifest.label_matched_frames,
      spatial_matched_frames: manifest.spatial_matched_frames,
      spatial_unscored_frames: manifest.spatial_unscored_frames,
      spurious_detection_count: manifest.spurious_detection_count,
      projection_kind: manifest.projection_kind.clone(),
      frame_witness_count: manifest.frame_witnesses.len(),
      status: manifest.status.as_str().to_string(),
      reason: manifest.reason.map(|reason| reason.as_str().to_string()),
    }
  }
}

impl From<&DetectionEvalWitnessInspectReport> for OsuDetectionEvalWitnessInspectReportSummary {
  fn from(report: &DetectionEvalWitnessInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      detection_eval_witness_manifest_path: report.detection_eval_witness_manifest_path.clone(),
      total_frames: report.total_frames,
      frame_witness_count: report.frame_witness_count,
      status: report.status.as_str().to_string(),
      warnings: report.warnings.clone(),
    }
  }
}

impl From<&DetectionEvalQualityManifest> for OsuDetectionEvalQualityManifestSummary {
  fn from(manifest: &DetectionEvalQualityManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      detection_eval_witness_manifest_path: manifest.detection_eval_witness_manifest_path.clone(),
      source_visual_eval_report_path: manifest.source_visual_eval_report_path.clone(),
      witness_status: manifest.witness_status.as_str().to_string(),
      status: manifest.status.as_str().to_string(),
      verdict: manifest.verdict.as_str().to_string(),
      label_recall: manifest.metrics.as_ref().and_then(|m| m.label_recall),
      spatial_recall: manifest.metrics.as_ref().and_then(|m| m.spatial_recall),
      spurious_detection_count: manifest.metrics.as_ref().map(|m| m.spurious_detection_count),
    }
  }
}

impl From<&DetectionEvalQualityInspectReport> for OsuDetectionEvalQualityInspectReportSummary {
  fn from(report: &DetectionEvalQualityInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      detection_eval_quality_manifest_path: report.detection_eval_quality_manifest_path.clone(),
      witness_status: report.witness_status.as_str().to_string(),
      status: report.status.as_str().to_string(),
      verdict: report.verdict.as_str().to_string(),
      label_recall_available: report.label_recall_available,
      spatial_recall_available: report.spatial_recall_available,
    }
  }
}

impl From<&VisualTruthSemanticManifest> for OsuVisualTruthSemanticManifestSummary {
  fn from(manifest: &VisualTruthSemanticManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      source_run_artifact_dir: manifest.source_run_artifact_dir.clone(),
      source_visual_truth_manifest_path: manifest.source_visual_truth_manifest_path.clone(),
      source_projection_path: manifest.source_projection_path.clone(),
      beatmap_path: manifest.beatmap_path.clone(),
      frame_count: manifest.frame_count,
      semantic_status: manifest.semantic_status.as_str().to_string(),
      semantic_reason: manifest.semantic_reason.map(|reason| reason.as_str().to_string()),
      known_limits: manifest.known_limits.clone(),
    }
  }
}

impl From<&VisualTruthSemanticInspectReport> for OsuVisualTruthSemanticInspectReportSummary {
  fn from(report: &VisualTruthSemanticInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      visual_truth_semantic_manifest_path: report.visual_truth_semantic_manifest_path.clone(),
      source_run_artifact_dir: report.source_run_artifact_dir.clone(),
      semantic_status: report.semantic_status.as_str().to_string(),
      semantic_reason: report.semantic_reason.map(|reason| reason.as_str().to_string()),
      visual_truth_manifest_readable: report.visual_truth_manifest_readable,
      projection_readable: report.projection_readable,
      projection_eval_ready: report.projection_eval_ready,
      warnings: report.warnings.clone(),
      known_limits: report.known_limits.clone(),
    }
  }
}

impl From<&VisualTruthSpatialQueryManifest> for OsuVisualTruthSpatialQueryManifestSummary {
  fn from(manifest: &VisualTruthSpatialQueryManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      visual_truth_semantic_manifest_path: manifest.visual_truth_semantic_manifest_path.clone(),
      source_run_artifact_dir: manifest.source_run_artifact_dir.clone(),
      object_index: manifest.object_index,
      capture_phase: match manifest.capture_phase {
        crate::CapturePhase::BeforeDispatch => "before_dispatch".to_string(),
        crate::CapturePhase::AfterDispatch => "after_dispatch".to_string(),
      },
      object_kind: manifest.object_kind.as_ref().map(|kind| match kind {
        crate::ObjectKind::Circle => "circle".to_string(),
        crate::ObjectKind::Slider => "slider".to_string(),
        crate::ObjectKind::Spinner => "spinner".to_string(),
        crate::ObjectKind::Hold => "hold".to_string(),
      }),
      query_backend: manifest.query_backend.as_str().to_string(),
      status: manifest.status.as_str().to_string(),
      reason: manifest.reason.map(|reason| reason.as_str().to_string()),
      pixel_visibility: manifest.pixel_visibility.map(|visibility| visibility.as_str().to_string()),
      pixel_x: manifest.pixel_x,
      pixel_y: manifest.pixel_y,
      match_radius_px: manifest.match_radius_px,
      capture_width: manifest.capture_width,
      capture_height: manifest.capture_height,
      known_limits: manifest.known_limits.clone(),
    }
  }
}

impl From<&VisualTruthSpatialQueryInspectReport> for OsuVisualTruthSpatialQueryInspectReportSummary {
  fn from(report: &VisualTruthSpatialQueryInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      visual_truth_spatial_query_manifest_path: report.visual_truth_spatial_query_manifest_path.clone(),
      visual_truth_semantic_manifest_path: report.visual_truth_semantic_manifest_path.clone(),
      object_index: report.object_index,
      capture_phase: match report.capture_phase {
        crate::CapturePhase::BeforeDispatch => "before_dispatch".to_string(),
        crate::CapturePhase::AfterDispatch => "after_dispatch".to_string(),
      },
      query_backend: report.query_backend.as_str().to_string(),
      status: report.status.as_str().to_string(),
      reason: report.reason.map(|reason| reason.as_str().to_string()),
      pixel_visibility: report.pixel_visibility.map(|visibility| visibility.as_str().to_string()),
      semantic_status: report.semantic_status.as_str().to_string(),
      warnings: report.warnings.clone(),
      known_limits: report.known_limits.clone(),
    }
  }
}
