use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use auv_file::{
  JsonFileReadError, JsonFileWriteError, JsonWriteOptions, read_json_file as read_json_file_helper,
  write_json_file as write_json_file_helper,
};
use auv_stage_status::StageStatus;
use auv_tracing::{ArtifactMetadata, ArtifactUri, Context, RunSnapshot, RunStore};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::benchmark::DetectionEvalManifest;
use crate::visual_eval::{EvalProjection, FrameEvaluation, FrameLabelOutcome, FrameSpatialOutcome, VisualEvalReport};

pub type DetectionEvalWitnessResult<T> = Result<T, String>;

pub const DETECTION_EVAL_WITNESS_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const DETECTION_EVAL_WITNESS_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;
pub const OSU_DETECTION_EVAL_WITNESS_PURPOSE: &str = "auv.osu.detection_eval.witness";

pub const OSU_WQ1_V1_WITNESS_KNOWN_LIMIT: &str = "osu WQ1 witness records per-frame detection-vs-truth alignment from visual_eval_report; it is not action verification or gameplay success";

const VISUAL_EVAL_REPORT_FILE: &str = "visual_eval_report.json";
const DETECTION_EVAL_MANIFEST_FILE: &str = "detection_eval_manifest.json";
const WITNESS_MANIFEST_FILE: &str = "osu-detection-eval-witness.json";
const WITNESS_INSPECT_FILE: &str = "osu-detection-eval-witness-inspect.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectionEvalWitnessInputs {
  pub detection_eval_output_dir: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionEvalWitnessOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub manifest: DetectionEvalWitnessManifest,
  pub inspect_report: DetectionEvalWitnessInspectReport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionEvalFrameWitness {
  pub object_index: usize,
  pub capture_phase: String,
  pub capture_file_name: String,
  pub object_kind: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub expected_label: Option<String>,
  pub label_outcome: String,
  pub spatial_outcome: String,
  pub spurious_detection_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionEvalWitnessManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub source_visual_eval_report_path: String,
  pub source_detection_eval_manifest_path: String,
  pub source_run_artifact_dir: String,
  pub source_visual_truth_manifest_path: String,
  pub source_projection_path: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub detector_model_id: Option<String>,
  pub total_frames: usize,
  pub label_matched_frames: usize,
  pub label_missing_frames: usize,
  pub label_unmapped_frames: usize,
  pub spatial_matched_frames: usize,
  pub spatial_missing_frames: usize,
  pub spatial_unscored_frames: usize,
  pub spurious_detection_count: usize,
  pub projection_kind: String,
  pub frame_witnesses: Vec<DetectionEvalFrameWitness>,
  pub status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<DetectionEvalWitnessReason>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionEvalWitnessInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub detection_eval_witness_manifest_path: String,
  pub source_visual_eval_report_path: String,
  pub source_detection_eval_manifest_path: String,
  pub source_run_artifact_dir: String,
  pub source_visual_truth_manifest_path: String,
  pub source_projection_path: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub detector_model_id: Option<String>,
  pub total_frames: usize,
  pub label_matched_frames: usize,
  pub label_missing_frames: usize,
  pub spatial_matched_frames: usize,
  pub spatial_missing_frames: usize,
  pub spatial_unscored_frames: usize,
  pub spurious_detection_count: usize,
  pub projection_kind: String,
  pub frame_witness_count: usize,
  pub visual_eval_report_readable: bool,
  pub detection_eval_manifest_readable: bool,
  pub status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<DetectionEvalWitnessReason>,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionEvalWitnessReason {
  MissingVisualEvalReport,
  MissingDetectionEvalManifest,
  VisualEvalReportParseFailed,
  DetectionEvalManifestParseFailed,
  EmptyFrames,
}

impl DetectionEvalWitnessReason {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::MissingVisualEvalReport => "missing_visual_eval_report",
      Self::MissingDetectionEvalManifest => "missing_detection_eval_manifest",
      Self::VisualEvalReportParseFailed => "visual_eval_report_parse_failed",
      Self::DetectionEvalManifestParseFailed => "detection_eval_manifest_parse_failed",
      Self::EmptyFrames => "empty_frames",
    }
  }

  fn status(self) -> StageStatus {
    match self {
      Self::MissingVisualEvalReport | Self::MissingDetectionEvalManifest | Self::EmptyFrames => StageStatus::Blocked,
      Self::VisualEvalReportParseFailed | Self::DetectionEvalManifestParseFailed => StageStatus::Failed,
    }
  }
}

pub async fn publish_osu_detection_eval_witness(
  context: Option<&Context>,
  witness: &DetectionEvalWitnessManifest,
) -> Result<Option<ArtifactMetadata>, crate::run_read::OsuArtifactPublishError> {
  crate::run_read::publish_json_artifact(context, OSU_DETECTION_EVAL_WITNESS_PURPOSE, witness, validate_witness_payload).await
}

pub async fn read_osu_detection_eval_witness(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<DetectionEvalWitnessManifest, crate::run_read::OsuArtifactReadError> {
  crate::run_read::read_json_artifact(store, snapshot, uri, OSU_DETECTION_EVAL_WITNESS_PURPOSE, validate_witness_payload).await
}

pub(crate) fn validate_witness_payload(witness: &DetectionEvalWitnessManifest) -> Result<(), String> {
  if witness.schema_version != DETECTION_EVAL_WITNESS_MANIFEST_SCHEMA_VERSION {
    return Err(format!(
      "unsupported osu! detection eval witness schema_version {} (expected {DETECTION_EVAL_WITNESS_MANIFEST_SCHEMA_VERSION})",
      witness.schema_version
    ));
  }
  let label_total = witness
    .label_matched_frames
    .checked_add(witness.label_missing_frames)
    .and_then(|total| total.checked_add(witness.label_unmapped_frames))
    .ok_or_else(|| "witness label counts overflow usize".to_string())?;
  if label_total != witness.total_frames {
    return Err(format!("witness label counts total {label_total}, expected {}", witness.total_frames));
  }
  let spatial_total = witness
    .spatial_matched_frames
    .checked_add(witness.spatial_missing_frames)
    .and_then(|total| total.checked_add(witness.spatial_unscored_frames))
    .ok_or_else(|| "witness spatial counts overflow usize".to_string())?;
  if spatial_total != witness.total_frames {
    return Err(format!("witness spatial counts total {spatial_total}, expected {}", witness.total_frames));
  }
  if witness.frame_witnesses.len() != witness.total_frames {
    return Err(format!("witness contains {} frame records, expected {}", witness.frame_witnesses.len(), witness.total_frames));
  }

  let frame_totals = frame_aggregates(&witness.frame_witnesses)?;
  for (name, actual, expected) in [
    ("label matched", frame_totals.label_matched, witness.label_matched_frames),
    ("label missing", frame_totals.label_missing, witness.label_missing_frames),
    ("label unmapped", frame_totals.label_unmapped, witness.label_unmapped_frames),
    ("spatial matched", frame_totals.spatial_matched, witness.spatial_matched_frames),
    ("spatial missing", frame_totals.spatial_missing, witness.spatial_missing_frames),
    ("spatial unscored", frame_totals.spatial_unscored, witness.spatial_unscored_frames),
    ("spurious", frame_totals.spurious, witness.spurious_detection_count),
  ] {
    if actual != expected {
      return Err(format!("witness frame {name} count totals {actual}, expected {expected}"));
    }
  }

  match (witness.status, witness.reason) {
    (StageStatus::Ready, None) if witness.total_frames > 0 => {}
    (StageStatus::Ready, None) => return Err("ready witness payload must contain at least one frame".to_string()),
    (StageStatus::Ready, Some(_)) => return Err("ready witness payload must not include a reason".to_string()),
    (_, None) => return Err(format!("{} witness payload must include a reason", witness.status)),
    (status, Some(reason)) if status != reason.status() => {
      return Err(format!("witness reason {} is inconsistent with status {status}", reason.as_str()));
    }
    _ => {}
  }
  if witness.status == StageStatus::Blocked && witness.total_frames != 0 {
    return Err("blocked witness payload must not include frame evidence".to_string());
  }
  if witness.reason == Some(DetectionEvalWitnessReason::VisualEvalReportParseFailed) && witness.total_frames != 0 {
    return Err("visual-eval parse failure witness must not include frame evidence".to_string());
  }
  Ok(())
}

#[derive(Default)]
struct FrameAggregates {
  label_matched: usize,
  label_missing: usize,
  label_unmapped: usize,
  spatial_matched: usize,
  spatial_missing: usize,
  spatial_unscored: usize,
  spurious: usize,
}

fn frame_aggregates(frames: &[DetectionEvalFrameWitness]) -> Result<FrameAggregates, String> {
  let mut totals = FrameAggregates::default();
  for frame in frames {
    let label_count = match frame.label_outcome.as_str() {
      "matched" => &mut totals.label_matched,
      "missing" => &mut totals.label_missing,
      "unmapped" => &mut totals.label_unmapped,
      other => return Err(format!("witness frame has unsupported label_outcome {other}")),
    };
    *label_count = label_count.checked_add(1).ok_or_else(|| "witness frame label counts overflow usize".to_string())?;

    let spatial_count = match frame.spatial_outcome.as_str() {
      "matched" => &mut totals.spatial_matched,
      "missing" => &mut totals.spatial_missing,
      "not_scored" => &mut totals.spatial_unscored,
      other => return Err(format!("witness frame has unsupported spatial_outcome {other}")),
    };
    *spatial_count = spatial_count.checked_add(1).ok_or_else(|| "witness frame spatial counts overflow usize".to_string())?;
    totals.spurious = totals
      .spurious
      .checked_add(frame.spurious_detection_count)
      .ok_or_else(|| "witness frame spurious counts overflow usize".to_string())?;
  }
  Ok(totals)
}

pub fn build_detection_eval_witness(inputs: &DetectionEvalWitnessInputs) -> DetectionEvalWitnessResult<DetectionEvalWitnessOutput> {
  fs::create_dir_all(&inputs.output_dir)
    .map_err(|error| format!("failed to create detection eval witness output dir {}: {error}", inputs.output_dir.display()))?;

  let generated_at_millis = crate::run_read::now_millis();
  let source_visual_eval_report_path = inputs.detection_eval_output_dir.join(VISUAL_EVAL_REPORT_FILE);
  let source_detection_eval_manifest_path = inputs.detection_eval_output_dir.join(DETECTION_EVAL_MANIFEST_FILE);

  let known_limits = BTreeSet::from([OSU_WQ1_V1_WITNESS_KNOWN_LIMIT.to_string()]);
  let mut warnings = BTreeSet::new();

  let gate = evaluate_witness_gate(&source_visual_eval_report_path, &source_detection_eval_manifest_path, &mut warnings);

  let (source_run_artifact_dir, source_visual_truth_manifest_path, source_projection_path) = gate
    .detection_eval_manifest
    .as_ref()
    .map(|manifest| {
      (
        manifest.source_run_artifact_dir.clone(),
        format!("{}/visual_truth_manifest.json", manifest.source_run_artifact_dir),
        format!("{}/projection.json", manifest.source_run_artifact_dir),
      )
    })
    .unwrap_or_default();

  let detector_model_id = gate.detection_eval_manifest.as_ref().map(|manifest| manifest.detector_model_id.clone());

  let report = gate.visual_eval_report.as_ref();
  let projection_kind = report.map(|report| projection_kind_label(&report.projection)).unwrap_or_else(|| "unknown".to_string());

  let frame_witnesses = report.map(frame_witnesses_from_report).unwrap_or_default();

  let manifest = DetectionEvalWitnessManifest {
    schema_version: DETECTION_EVAL_WITNESS_MANIFEST_SCHEMA_VERSION,
    generated_at_millis,
    source_visual_eval_report_path: source_visual_eval_report_path.display().to_string(),
    source_detection_eval_manifest_path: source_detection_eval_manifest_path.display().to_string(),
    source_run_artifact_dir,
    source_visual_truth_manifest_path,
    source_projection_path,
    detector_model_id: detector_model_id.clone(),
    total_frames: report.map(|r| r.total_frames).unwrap_or(0),
    label_matched_frames: report.map(|r| r.label_matched_frames).unwrap_or(0),
    label_missing_frames: report.map(|r| r.label_missing_frames).unwrap_or(0),
    label_unmapped_frames: report.map(|r| r.label_unmapped_frames).unwrap_or(0),
    spatial_matched_frames: report.map(|r| r.spatial_matched_frames).unwrap_or(0),
    spatial_missing_frames: report.map(|r| r.spatial_missing_frames).unwrap_or(0),
    spatial_unscored_frames: report.map(|r| r.spatial_unscored_frames).unwrap_or(0),
    spurious_detection_count: report.map(|r| r.spurious_detection_count).unwrap_or(0),
    projection_kind,
    frame_witnesses,
    status: gate.status,
    reason: gate.reason,
    known_limits: known_limits.into_iter().collect(),
  };

  validate_witness_payload(&manifest)?;

  let manifest_path = inputs.output_dir.join(WITNESS_MANIFEST_FILE);
  write_json_file(&manifest_path, &manifest)?;

  let inspect_report = DetectionEvalWitnessInspectReport {
    schema_version: DETECTION_EVAL_WITNESS_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    detection_eval_witness_manifest_path: manifest_path.display().to_string(),
    source_visual_eval_report_path: manifest.source_visual_eval_report_path.clone(),
    source_detection_eval_manifest_path: manifest.source_detection_eval_manifest_path.clone(),
    source_run_artifact_dir: manifest.source_run_artifact_dir.clone(),
    source_visual_truth_manifest_path: manifest.source_visual_truth_manifest_path.clone(),
    source_projection_path: manifest.source_projection_path.clone(),
    detector_model_id,
    total_frames: manifest.total_frames,
    label_matched_frames: manifest.label_matched_frames,
    label_missing_frames: manifest.label_missing_frames,
    spatial_matched_frames: manifest.spatial_matched_frames,
    spatial_missing_frames: manifest.spatial_missing_frames,
    spatial_unscored_frames: manifest.spatial_unscored_frames,
    spurious_detection_count: manifest.spurious_detection_count,
    projection_kind: manifest.projection_kind.clone(),
    frame_witness_count: manifest.frame_witnesses.len(),
    visual_eval_report_readable: gate.visual_eval_report.is_some(),
    detection_eval_manifest_readable: gate.detection_eval_manifest.is_some(),
    status: manifest.status,
    reason: manifest.reason,
    warnings: warnings.into_iter().collect(),
    known_limits: manifest.known_limits.clone(),
  };

  let inspect_report_path = inputs.output_dir.join(WITNESS_INSPECT_FILE);
  write_json_file(&inspect_report_path, &inspect_report)?;

  Ok(DetectionEvalWitnessOutput {
    output_dir: inputs.output_dir.clone(),
    manifest_path,
    inspect_report_path,
    manifest,
    inspect_report,
  })
}

struct WitnessGateEvaluation {
  status: StageStatus,
  reason: Option<DetectionEvalWitnessReason>,
  visual_eval_report: Option<VisualEvalReport>,
  detection_eval_manifest: Option<DetectionEvalManifest>,
}

fn evaluate_witness_gate(
  visual_eval_report_path: &Path,
  detection_eval_manifest_path: &Path,
  warnings: &mut BTreeSet<String>,
) -> WitnessGateEvaluation {
  if !visual_eval_report_path.is_file() {
    return WitnessGateEvaluation {
      status: StageStatus::Blocked,
      reason: Some(DetectionEvalWitnessReason::MissingVisualEvalReport),
      visual_eval_report: None,
      detection_eval_manifest: None,
    };
  }

  if !detection_eval_manifest_path.is_file() {
    return WitnessGateEvaluation {
      status: StageStatus::Blocked,
      reason: Some(DetectionEvalWitnessReason::MissingDetectionEvalManifest),
      visual_eval_report: None,
      detection_eval_manifest: None,
    };
  }

  let visual_eval_report = match read_visual_eval_report(visual_eval_report_path) {
    Ok(report) => Some(report),
    Err(error) => {
      warnings.insert(error);
      return WitnessGateEvaluation {
        status: StageStatus::Failed,
        reason: Some(DetectionEvalWitnessReason::VisualEvalReportParseFailed),
        visual_eval_report: None,
        detection_eval_manifest: None,
      };
    }
  };

  let detection_eval_manifest = match read_json_file::<DetectionEvalManifest>(detection_eval_manifest_path, "osu detection eval manifest") {
    Ok(manifest) => Some(manifest),
    Err(error) => {
      warnings.insert(error);
      return WitnessGateEvaluation {
        status: StageStatus::Failed,
        reason: Some(DetectionEvalWitnessReason::DetectionEvalManifestParseFailed),
        visual_eval_report,
        detection_eval_manifest: None,
      };
    }
  };

  let Some(report) = visual_eval_report.as_ref() else {
    return WitnessGateEvaluation {
      status: StageStatus::Failed,
      reason: Some(DetectionEvalWitnessReason::VisualEvalReportParseFailed),
      visual_eval_report,
      detection_eval_manifest,
    };
  };

  if report.total_frames == 0 {
    return WitnessGateEvaluation {
      status: StageStatus::Blocked,
      reason: Some(DetectionEvalWitnessReason::EmptyFrames),
      visual_eval_report,
      detection_eval_manifest,
    };
  }

  WitnessGateEvaluation {
    status: StageStatus::Ready,
    reason: None,
    visual_eval_report,
    detection_eval_manifest,
  }
}

fn read_visual_eval_report(path: &Path) -> Result<VisualEvalReport, String> {
  let mut value = read_json_file::<serde_json::Value>(path, "osu visual eval report")?;
  let projection_value = value
    .as_object_mut()
    .and_then(|report| report.get_mut("projection"))
    .map(serde_json::Value::take)
    .ok_or_else(|| format!("osu visual eval report {} is missing projection", path.display()))?;
  let projection = decode_eval_projection(&projection_value, path)?;

  // NOTICE: `auv-tracing` enables serde_json `arbitrary_precision`, whose
  // private number map is incompatible with Serde's internally tagged enum
  // buffer for `EvalProjection` f32 fields. Remove this split decode when that
  // combination can deserialize the owning enum directly.
  value["projection"] = serde_json::json!({"kind": "unavailable", "reason": "decoded separately"});
  let mut report: VisualEvalReport =
    serde_json::from_value(value).map_err(|error| format!("failed to parse osu visual eval report {}: {error}", path.display()))?;
  report.projection = projection;
  Ok(report)
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProjectionWireKind {
  Unavailable,
  PlayfieldToPixels,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnavailableProjectionWire {
  kind: ProjectionWireKind,
  reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayfieldProjectionWire {
  kind: ProjectionWireKind,
  scale_x: serde_json::Number,
  scale_y: serde_json::Number,
  offset_x: serde_json::Number,
  offset_y: serde_json::Number,
  match_radius_px: serde_json::Number,
}

fn decode_eval_projection(value: &serde_json::Value, path: &Path) -> Result<EvalProjection, String> {
  let object = value.as_object().ok_or_else(|| format!("osu visual eval report {} projection must be an object", path.display()))?;
  let kind = object
    .get("kind")
    .and_then(serde_json::Value::as_str)
    .ok_or_else(|| format!("osu visual eval report {} projection is missing kind", path.display()))?;
  match kind {
    "unavailable" => {
      let wire: UnavailableProjectionWire = serde_json::from_value(value.clone())
        .map_err(|error| format!("failed to parse osu visual eval report {} unavailable projection: {error}", path.display()))?;
      if !matches!(wire.kind, ProjectionWireKind::Unavailable) {
        return Err(format!("osu visual eval report {} unavailable projection has inconsistent kind", path.display()));
      }
      Ok(EvalProjection::Unavailable {
        reason: wire.reason,
      })
    }
    "playfield_to_pixels" => {
      let wire: PlayfieldProjectionWire = serde_json::from_value(value.clone())
        .map_err(|error| format!("failed to parse osu visual eval report {} playfield projection: {error}", path.display()))?;
      if !matches!(wire.kind, ProjectionWireKind::PlayfieldToPixels) {
        return Err(format!("osu visual eval report {} playfield projection has inconsistent kind", path.display()));
      }
      Ok(EvalProjection::PlayfieldToPixels {
        scale_x: projection_f32(&wire.scale_x, "scale_x", path, true)?,
        scale_y: projection_f32(&wire.scale_y, "scale_y", path, true)?,
        offset_x: projection_f32(&wire.offset_x, "offset_x", path, false)?,
        offset_y: projection_f32(&wire.offset_y, "offset_y", path, false)?,
        match_radius_px: projection_f32(&wire.match_radius_px, "match_radius_px", path, true)?,
      })
    }
    other => Err(format!("osu visual eval report {} has unsupported projection kind {other}", path.display())),
  }
}

fn projection_f32(number: &serde_json::Number, field: &str, path: &Path, positive: bool) -> Result<f32, String> {
  let value = number
    .as_f64()
    .ok_or_else(|| format!("osu visual eval report {} projection field {field} must be representable as a finite f64", path.display()))?;
  if !value.is_finite() {
    return Err(format!("osu visual eval report {} projection field {field} must be finite", path.display()));
  }
  if positive && value <= 0.0 {
    return Err(format!("osu visual eval report {} projection field {field} must be positive", path.display()));
  }
  let value = value as f32;
  if !value.is_finite() {
    return Err(format!("osu visual eval report {} projection field {field} must be representable as a finite f32", path.display()));
  }
  if positive && value <= 0.0 {
    return Err(format!("osu visual eval report {} projection field {field} must remain positive when represented as f32", path.display()));
  }
  Ok(value)
}

fn frame_witnesses_from_report(report: &VisualEvalReport) -> Vec<DetectionEvalFrameWitness> {
  report.frames.iter().map(frame_witness_from_evaluation).collect()
}

fn frame_witness_from_evaluation(frame: &FrameEvaluation) -> DetectionEvalFrameWitness {
  DetectionEvalFrameWitness {
    object_index: frame.frame.object_index,
    capture_phase: frame.frame.phase.clone(),
    capture_file_name: frame.frame.capture_file_name.clone(),
    object_kind: format!("{:?}", frame.object_kind).to_lowercase(),
    expected_label: frame.expected_label.clone(),
    label_outcome: label_outcome_label(frame.label_outcome).to_string(),
    spatial_outcome: spatial_outcome_label(frame.spatial_outcome).to_string(),
    spurious_detection_count: frame.spurious_detection_count,
  }
}

fn label_outcome_label(outcome: FrameLabelOutcome) -> &'static str {
  match outcome {
    FrameLabelOutcome::Matched => "matched",
    FrameLabelOutcome::Missing => "missing",
    FrameLabelOutcome::Unmapped => "unmapped",
  }
}

fn spatial_outcome_label(outcome: FrameSpatialOutcome) -> &'static str {
  match outcome {
    FrameSpatialOutcome::Matched => "matched",
    FrameSpatialOutcome::Missing => "missing",
    FrameSpatialOutcome::NotScored => "not_scored",
  }
}

fn projection_kind_label(projection: &EvalProjection) -> String {
  match projection {
    EvalProjection::Unavailable { .. } => "unavailable".to_string(),
    EvalProjection::PlayfieldToPixels { .. } => "playfield_to_pixels".to_string(),
  }
}

fn read_json_file<T: DeserializeOwned>(path: &Path, label: &str) -> Result<T, String> {
  read_json_file_helper(path).map_err(|error| match error {
    JsonFileReadError::Open(error) => {
      format!("failed to open {label} {}: {error}", path.display())
    }
    JsonFileReadError::Parse(error) => {
      format!("failed to parse {label} {}: {error}", path.display())
    }
  })
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
  write_json_file_helper(path, value, JsonWriteOptions::default()).map_err(|error| match error {
    JsonFileWriteError::CreateParent(error) | JsonFileWriteError::Write(error) => {
      format!("failed to write {}: {error}", path.display())
    }
    JsonFileWriteError::Serialize(error) => {
      format!("failed to serialize {}: {error}", path.display())
    }
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::visual_eval::{FrameEvaluation, FrameKey, FrameLabelOutcome, FrameSpatialOutcome};
  use crate::{CapturePhase, ObjectKind};
  use std::path::PathBuf;

  fn fixture_eval_dir() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/osu_eval_run_artifacts");
    let detections_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/osu_eval_detection");
    let eval_output = temp.path().join("eval");
    crate::evaluate_detection_fixture(&crate::DetectionEvalInputs {
      run_artifact_dir: manifest_dir,
      detections_path,
      output_dir: eval_output.clone(),
    })
    .expect("eval");
    (temp, eval_output)
  }

  #[test]
  fn witness_from_fixture_eval_records_frame_outcomes_and_lineage() {
    let (temp, eval_output) = fixture_eval_dir();
    let witness_output = temp.path().join("witness");
    let output = build_detection_eval_witness(&DetectionEvalWitnessInputs {
      detection_eval_output_dir: eval_output.clone(),
      output_dir: witness_output,
    })
    .expect("witness");

    assert_eq!(output.manifest.status, StageStatus::Ready, "warnings: {:?}", output.inspect_report.warnings);
    assert_eq!(output.manifest.total_frames, 3);
    assert_eq!(output.manifest.label_matched_frames, 1);
    assert_eq!(output.manifest.spatial_matched_frames, 1);
    assert_eq!(output.manifest.frame_witnesses.len(), 3);
    assert!(output.manifest.source_visual_eval_report_path.contains("visual_eval_report.json"));
    assert_eq!(output.manifest.detector_model_id.as_deref(), Some("direct_detection_result"));
    assert!(output.manifest_path.exists());
    assert!(output.inspect_report_path.exists());
  }

  #[test]
  fn witness_blocked_when_visual_eval_report_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output = build_detection_eval_witness(&DetectionEvalWitnessInputs {
      detection_eval_output_dir: temp.path().to_path_buf(),
      output_dir: temp.path().join("witness"),
    })
    .expect("witness");

    assert_eq!(output.manifest.status, StageStatus::Blocked);
    assert_eq!(output.manifest.reason, Some(DetectionEvalWitnessReason::MissingVisualEvalReport));
    assert_eq!(output.manifest.frame_witnesses.len(), 0);
  }

  #[test]
  fn witness_maps_label_and_spatial_outcomes_from_report_frames() {
    use crate::visual_eval::{EvalProjection, VisualEvalReport};

    let report = VisualEvalReport {
      total_frames: 1,
      label_matched_frames: 1,
      label_missing_frames: 0,
      label_unmapped_frames: 0,
      spatial_matched_frames: 0,
      spatial_missing_frames: 0,
      spatial_unscored_frames: 1,
      spurious_detection_count: 0,
      projection: EvalProjection::Unavailable {
        reason: "test".to_string(),
      },
      frames: vec![FrameEvaluation {
        frame: FrameKey::from_parts(0, CapturePhase::AfterDispatch, "frame.png"),
        object_kind: ObjectKind::Circle,
        expected_label: Some("hit_circle".to_string()),
        label_outcome: FrameLabelOutcome::Matched,
        spatial_outcome: FrameSpatialOutcome::NotScored,
        spurious_detection_count: 0,
      }],
      known_limits: vec![],
      detector_provenance: None,
    };
    let witnesses = frame_witnesses_from_report(&report);
    assert_eq!(witnesses.len(), 1);
    assert_eq!(witnesses[0].label_outcome, "matched");
    assert_eq!(witnesses[0].spatial_outcome, "not_scored");
  }

  fn valid_projection_json() -> serde_json::Value {
    serde_json::json!({
      "kind": "playfield_to_pixels",
      "scale_x": 1.0,
      "scale_y": 1.0,
      "offset_x": 0.0,
      "offset_y": 0.0,
      "match_radius_px": 20.0
    })
  }

  #[test]
  fn split_projection_decoder_rejects_inconsistent_tagged_union_fields() {
    let path = Path::new("visual-eval.json");
    let cases = [
      serde_json::json!({"kind": "unavailable", "reason": "missing", "scale_x": 1.0}),
      {
        let mut value = valid_projection_json();
        value["reason"] = serde_json::json!("not unavailable");
        value
      },
      {
        let mut value = valid_projection_json();
        value["extra"] = serde_json::json!(true);
        value
      },
    ];

    for value in cases {
      assert!(decode_eval_projection(&value, path).is_err(), "accepted inconsistent projection {value}");
    }
  }

  #[test]
  fn split_projection_decoder_rejects_invalid_positive_fields() {
    let path = Path::new("visual-eval.json");
    for (field, value) in [
      ("scale_x", 0.0),
      ("scale_y", -1.0),
      ("match_radius_px", 0.0),
      ("match_radius_px", -1.0),
      ("scale_x", 1.0e-100),
      ("match_radius_px", 1.0e-100),
    ] {
      let mut projection = valid_projection_json();
      projection[field] = serde_json::json!(value);
      assert!(decode_eval_projection(&projection, path).is_err(), "accepted {field}={value}");
    }
  }

  #[test]
  fn split_projection_decoder_rejects_number_outside_f64_range() {
    let value: serde_json::Value =
      serde_json::from_str(r#"{"kind":"playfield_to_pixels","scale_x":1e400,"scale_y":1,"offset_x":0,"offset_y":0,"match_radius_px":20}"#)
        .expect("arbitrary-precision JSON number");

    assert!(decode_eval_projection(&value, Path::new("visual-eval.json")).is_err());
  }
}
