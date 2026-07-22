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

use crate::detection_eval_witness::{DetectionEvalWitnessManifest, DetectionEvalWitnessReason};

pub type DetectionEvalQualityResult<T> = Result<T, String>;

pub const DETECTION_EVAL_QUALITY_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const DETECTION_EVAL_QUALITY_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;
pub const OSU_DETECTION_EVAL_QUALITY_PURPOSE: &str = "auv.osu.detection_eval.quality";

pub const OSU_WQ1_V1_QUALITY_KNOWN_LIMIT: &str = "osu WQ1 quality records detection measurement evidence only; it does not claim model usefulness, gameplay success, or pass/fail thresholds";

const WITNESS_MANIFEST_FILE: &str = "osu-detection-eval-witness.json";
const QUALITY_MANIFEST_FILE: &str = "osu-detection-eval-quality.json";
const QUALITY_INSPECT_FILE: &str = "osu-detection-eval-quality-inspect.json";

#[derive(Clone, Debug, PartialEq)]
pub struct DetectionEvalQualityInputs {
  pub witness_manifest_path: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectionEvalQualityOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub manifest: DetectionEvalQualityManifest,
  pub inspect_report: DetectionEvalQualityInspectReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectionEvalQualityMetrics {
  pub total_frames: usize,
  pub label_matched_frames: usize,
  pub label_missing_frames: usize,
  pub label_unmapped_frames: usize,
  pub spatial_matched_frames: usize,
  pub spatial_missing_frames: usize,
  pub spatial_unscored_frames: usize,
  pub spurious_detection_count: usize,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub label_recall: Option<f32>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub spatial_recall: Option<f32>,
  pub projection_kind: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectionEvalQualityManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub detection_eval_witness_manifest_path: String,
  pub source_visual_eval_report_path: String,
  pub source_run_artifact_dir: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub detector_model_id: Option<String>,
  pub witness_status: StageStatus,
  pub status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<DetectionEvalQualityReason>,
  pub verdict: DetectionEvalQualityVerdict,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub metrics: Option<DetectionEvalQualityMetrics>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectionEvalQualityInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub detection_eval_quality_manifest_path: String,
  pub detection_eval_witness_manifest_path: String,
  pub source_visual_eval_report_path: String,
  pub witness_status: StageStatus,
  pub status: StageStatus,
  pub verdict: DetectionEvalQualityVerdict,
  pub label_recall_available: bool,
  pub spatial_recall_available: bool,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub metrics: Option<DetectionEvalQualityMetrics>,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionEvalQualityReason {
  MissingWitnessManifest,
  WitnessManifestParseFailed,
  WitnessNotReady,
  WitnessBlocked,
  WitnessFailed,
}

impl DetectionEvalQualityReason {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::MissingWitnessManifest => "missing_witness_manifest",
      Self::WitnessManifestParseFailed => "witness_manifest_parse_failed",
      Self::WitnessNotReady => "witness_not_ready",
      Self::WitnessBlocked => "witness_blocked",
      Self::WitnessFailed => "witness_failed",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionEvalQualityVerdict {
  MeasuredOnly,
  MetricPartial,
  Blocked,
  Failed,
}

impl DetectionEvalQualityVerdict {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::MeasuredOnly => "measured_only",
      Self::MetricPartial => "metric_partial",
      Self::Blocked => "blocked",
      Self::Failed => "failed",
    }
  }
}

pub async fn publish_osu_detection_eval_quality(
  context: Option<&Context>,
  quality: &DetectionEvalQualityManifest,
) -> Result<Option<ArtifactMetadata>, crate::run_read::OsuArtifactPublishError> {
  crate::run_read::publish_json_artifact(context, OSU_DETECTION_EVAL_QUALITY_PURPOSE, quality, validate_quality_payload).await
}

pub async fn read_osu_detection_eval_quality(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<DetectionEvalQualityManifest, crate::run_read::OsuArtifactReadError> {
  crate::run_read::read_json_artifact(store, snapshot, uri, OSU_DETECTION_EVAL_QUALITY_PURPOSE, validate_quality_payload).await
}

fn validate_quality_payload(quality: &DetectionEvalQualityManifest) -> Result<(), String> {
  if quality.schema_version != DETECTION_EVAL_QUALITY_MANIFEST_SCHEMA_VERSION {
    return Err(format!(
      "unsupported osu! detection eval quality schema_version {} (expected {DETECTION_EVAL_QUALITY_MANIFEST_SCHEMA_VERSION})",
      quality.schema_version
    ));
  }
  if let Some(metrics) = &quality.metrics {
    let label_total = metrics.label_matched_frames + metrics.label_missing_frames + metrics.label_unmapped_frames;
    if label_total != metrics.total_frames {
      return Err(format!("quality label counts total {label_total}, expected {}", metrics.total_frames));
    }
    let spatial_total = metrics.spatial_matched_frames + metrics.spatial_missing_frames + metrics.spatial_unscored_frames;
    if spatial_total != metrics.total_frames {
      return Err(format!("quality spatial counts total {spatial_total}, expected {}", metrics.total_frames));
    }
    for (name, recall) in [
      ("label_recall", metrics.label_recall),
      ("spatial_recall", metrics.spatial_recall),
    ] {
      if recall.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
        return Err(format!("quality {name} must be finite and between 0 and 1"));
      }
    }
  }
  if quality.status == StageStatus::Ready && quality.metrics.is_none() {
    return Err("ready quality payload must include metrics".to_string());
  }
  Ok(())
}

pub fn build_detection_eval_quality(inputs: &DetectionEvalQualityInputs) -> DetectionEvalQualityResult<DetectionEvalQualityOutput> {
  fs::create_dir_all(&inputs.output_dir)
    .map_err(|error| format!("failed to create detection eval quality output dir {}: {error}", inputs.output_dir.display()))?;

  let generated_at_millis = crate::run_read::now_millis();
  let known_limits = BTreeSet::from([OSU_WQ1_V1_QUALITY_KNOWN_LIMIT.to_string()]);
  let mut warnings = BTreeSet::new();

  let gate = evaluate_quality_gate(&inputs.witness_manifest_path, &mut warnings);
  let witness = gate.witness_manifest.as_ref();

  let outcome = witness.map(derive_quality_outcome).unwrap_or(QualityOutcome {
    status: gate.quality_status,
    reason: gate.quality_reason,
    verdict: gate.verdict,
    metrics: None,
  });

  let manifest = DetectionEvalQualityManifest {
    schema_version: DETECTION_EVAL_QUALITY_MANIFEST_SCHEMA_VERSION,
    generated_at_millis,
    detection_eval_witness_manifest_path: inputs.witness_manifest_path.display().to_string(),
    source_visual_eval_report_path: witness.map(|w| w.source_visual_eval_report_path.clone()).unwrap_or_default(),
    source_run_artifact_dir: witness.map(|w| w.source_run_artifact_dir.clone()).unwrap_or_default(),
    detector_model_id: witness.and_then(|w| w.detector_model_id.clone()),
    witness_status: witness.map(|w| w.status).unwrap_or(StageStatus::Blocked),
    status: outcome.status,
    reason: outcome.reason,
    verdict: outcome.verdict,
    metrics: outcome.metrics.clone(),
    known_limits: known_limits.into_iter().collect(),
  };

  let manifest_path = inputs.output_dir.join(QUALITY_MANIFEST_FILE);
  write_json_file(&manifest_path, &manifest)?;

  let inspect_report = DetectionEvalQualityInspectReport {
    schema_version: DETECTION_EVAL_QUALITY_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    detection_eval_quality_manifest_path: manifest_path.display().to_string(),
    detection_eval_witness_manifest_path: manifest.detection_eval_witness_manifest_path.clone(),
    source_visual_eval_report_path: manifest.source_visual_eval_report_path.clone(),
    witness_status: manifest.witness_status,
    status: manifest.status,
    verdict: manifest.verdict,
    label_recall_available: manifest.metrics.as_ref().and_then(|m| m.label_recall).is_some(),
    spatial_recall_available: manifest.metrics.as_ref().and_then(|m| m.spatial_recall).is_some(),
    metrics: manifest.metrics.clone(),
    warnings: warnings.into_iter().collect(),
    known_limits: manifest.known_limits.clone(),
  };

  let inspect_report_path = inputs.output_dir.join(QUALITY_INSPECT_FILE);
  write_json_file(&inspect_report_path, &inspect_report)?;

  Ok(DetectionEvalQualityOutput {
    output_dir: inputs.output_dir.clone(),
    manifest_path,
    inspect_report_path,
    manifest,
    inspect_report,
  })
}

pub fn build_detection_eval_quality_from_witness_dir(
  witness_output_dir: &Path,
  output_dir: PathBuf,
) -> DetectionEvalQualityResult<DetectionEvalQualityOutput> {
  build_detection_eval_quality(&DetectionEvalQualityInputs {
    witness_manifest_path: witness_output_dir.join(WITNESS_MANIFEST_FILE),
    output_dir,
  })
}

pub fn derive_detection_eval_quality_verdict(witness: &DetectionEvalWitnessManifest) -> DetectionEvalQualityVerdict {
  derive_quality_outcome(witness).verdict
}

struct QualityGateEvaluation {
  quality_status: StageStatus,
  quality_reason: Option<DetectionEvalQualityReason>,
  verdict: DetectionEvalQualityVerdict,
  witness_manifest: Option<DetectionEvalWitnessManifest>,
}

struct QualityOutcome {
  status: StageStatus,
  reason: Option<DetectionEvalQualityReason>,
  verdict: DetectionEvalQualityVerdict,
  metrics: Option<DetectionEvalQualityMetrics>,
}

fn evaluate_quality_gate(witness_manifest_path: &Path, warnings: &mut BTreeSet<String>) -> QualityGateEvaluation {
  if !witness_manifest_path.is_file() {
    return QualityGateEvaluation {
      quality_status: StageStatus::Blocked,
      quality_reason: Some(DetectionEvalQualityReason::MissingWitnessManifest),
      verdict: DetectionEvalQualityVerdict::Blocked,
      witness_manifest: None,
    };
  }

  let witness_manifest = match read_json_file::<DetectionEvalWitnessManifest>(witness_manifest_path, "osu detection eval witness manifest") {
    Ok(manifest) => Some(manifest),
    Err(error) => {
      warnings.insert(error);
      return QualityGateEvaluation {
        quality_status: StageStatus::Failed,
        quality_reason: Some(DetectionEvalQualityReason::WitnessManifestParseFailed),
        verdict: DetectionEvalQualityVerdict::Failed,
        witness_manifest: None,
      };
    }
  };

  let Some(witness) = witness_manifest.as_ref() else {
    return QualityGateEvaluation {
      quality_status: StageStatus::Failed,
      quality_reason: Some(DetectionEvalQualityReason::WitnessManifestParseFailed),
      verdict: DetectionEvalQualityVerdict::Failed,
      witness_manifest,
    };
  };

  match witness.status {
    StageStatus::Blocked => QualityGateEvaluation {
      quality_status: StageStatus::Blocked,
      quality_reason: witness.reason.map(|reason| match reason {
        DetectionEvalWitnessReason::MissingVisualEvalReport
        | DetectionEvalWitnessReason::MissingDetectionEvalManifest
        | DetectionEvalWitnessReason::EmptyFrames => DetectionEvalQualityReason::WitnessBlocked,
        _ => DetectionEvalQualityReason::WitnessNotReady,
      }),
      verdict: DetectionEvalQualityVerdict::Blocked,
      witness_manifest,
    },
    StageStatus::Failed => QualityGateEvaluation {
      quality_status: StageStatus::Failed,
      quality_reason: Some(DetectionEvalQualityReason::WitnessFailed),
      verdict: DetectionEvalQualityVerdict::Failed,
      witness_manifest,
    },
    StageStatus::Ready => {
      let outcome = derive_quality_outcome(witness);
      QualityGateEvaluation {
        quality_status: outcome.status,
        quality_reason: outcome.reason,
        verdict: outcome.verdict,
        witness_manifest,
      }
    }
  }
}

fn derive_quality_outcome(witness: &DetectionEvalWitnessManifest) -> QualityOutcome {
  let metrics = metrics_from_witness(witness);
  let verdict = if witness.projection_kind == "playfield_to_pixels" && witness.spatial_unscored_frames == 0 && witness.total_frames > 0 {
    DetectionEvalQualityVerdict::MeasuredOnly
  } else if witness.total_frames > 0 {
    DetectionEvalQualityVerdict::MetricPartial
  } else {
    DetectionEvalQualityVerdict::Blocked
  };

  QualityOutcome {
    status: StageStatus::Ready,
    reason: None,
    verdict,
    metrics: Some(metrics),
  }
}

fn metrics_from_witness(witness: &DetectionEvalWitnessManifest) -> DetectionEvalQualityMetrics {
  let label_scorable = witness.label_matched_frames + witness.label_missing_frames;
  let label_recall = if label_scorable == 0 {
    None
  } else {
    Some(witness.label_matched_frames as f32 / label_scorable as f32)
  };

  let spatial_scorable = witness.spatial_matched_frames + witness.spatial_missing_frames;
  let spatial_recall = if spatial_scorable == 0 {
    None
  } else {
    Some(witness.spatial_matched_frames as f32 / spatial_scorable as f32)
  };

  DetectionEvalQualityMetrics {
    total_frames: witness.total_frames,
    label_matched_frames: witness.label_matched_frames,
    label_missing_frames: witness.label_missing_frames,
    label_unmapped_frames: witness.label_unmapped_frames,
    spatial_matched_frames: witness.spatial_matched_frames,
    spatial_missing_frames: witness.spatial_missing_frames,
    spatial_unscored_frames: witness.spatial_unscored_frames,
    spurious_detection_count: witness.spurious_detection_count,
    label_recall,
    spatial_recall,
    projection_kind: witness.projection_kind.clone(),
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
  use crate::detection_eval_witness::{DetectionEvalWitnessInputs, DetectionEvalWitnessManifest, build_detection_eval_witness};
  use std::path::PathBuf;

  fn fixture_witness_manifest() -> (tempfile::TempDir, PathBuf) {
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
    let witness_output = temp.path().join("witness");
    let witness = build_detection_eval_witness(&DetectionEvalWitnessInputs {
      detection_eval_output_dir: eval_output,
      output_dir: witness_output.clone(),
    })
    .expect("witness");
    (temp, witness.manifest_path)
  }

  #[test]
  fn quality_from_fixture_witness_records_metrics_and_measured_only_verdict() {
    let (temp, witness_path) = fixture_witness_manifest();
    let quality_output = build_detection_eval_quality(&DetectionEvalQualityInputs {
      witness_manifest_path: witness_path,
      output_dir: temp.path().join("quality"),
    })
    .expect("quality");

    assert_eq!(quality_output.manifest.status, StageStatus::Ready);
    assert_eq!(
      quality_output.manifest.verdict,
      DetectionEvalQualityVerdict::MeasuredOnly,
      "warnings: {:?}",
      quality_output.inspect_report.warnings
    );
    let metrics = quality_output.manifest.metrics.as_ref().expect("metrics");
    assert_eq!(metrics.total_frames, 3);
    assert_eq!(metrics.label_matched_frames, 1);
    assert_eq!(metrics.spatial_matched_frames, 1);
    assert!(metrics.label_recall.is_some());
    assert!(metrics.spatial_recall.is_some());
    assert!(quality_output.manifest_path.exists());
    assert!(quality_output.inspect_report_path.exists());
  }

  #[test]
  fn quality_metric_partial_when_spatial_unscored() {
    let witness = DetectionEvalWitnessManifest {
      schema_version: 1,
      generated_at_millis: 0,
      source_visual_eval_report_path: "report.json".to_string(),
      source_detection_eval_manifest_path: "manifest.json".to_string(),
      source_run_artifact_dir: "run".to_string(),
      source_visual_truth_manifest_path: "truth.json".to_string(),
      source_projection_path: "projection.json".to_string(),
      detector_model_id: None,
      total_frames: 2,
      label_matched_frames: 1,
      label_missing_frames: 1,
      label_unmapped_frames: 0,
      spatial_matched_frames: 0,
      spatial_missing_frames: 0,
      spatial_unscored_frames: 2,
      spurious_detection_count: 0,
      projection_kind: "unavailable".to_string(),
      frame_witnesses: vec![],
      status: StageStatus::Ready,
      reason: None,
      known_limits: vec![],
    };
    assert_eq!(derive_detection_eval_quality_verdict(&witness), DetectionEvalQualityVerdict::MetricPartial);
  }

  #[test]
  fn quality_blocked_when_witness_manifest_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output = build_detection_eval_quality(&DetectionEvalQualityInputs {
      witness_manifest_path: temp.path().join("missing.json"),
      output_dir: temp.path().join("quality"),
    })
    .expect("quality");

    assert_eq!(output.manifest.status, StageStatus::Blocked);
    assert_eq!(output.manifest.verdict, DetectionEvalQualityVerdict::Blocked);
    assert!(output.manifest.metrics.is_none());
  }
}
