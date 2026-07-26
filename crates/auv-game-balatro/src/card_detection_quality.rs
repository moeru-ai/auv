use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use auv_file::{
  JsonFileReadError, JsonFileWriteError, JsonWriteOptions, read_json_file as read_json_file_helper,
  write_json_file as write_json_file_helper,
};
use auv_stage_status::StageStatus;
#[cfg(feature = "tracing")]
use auv_tracing::{ArtifactMetadata, Context};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::card_detection_eval_witness::{CardDetectionEvalWitnessManifest, CardDetectionEvalWitnessReason, CardDetectionQualityBackend};

pub type CardDetectionQualityResult<T> = Result<T, String>;

pub const CARD_DETECTION_QUALITY_MANIFEST_SCHEMA_VERSION: u32 = 2;
pub const CARD_DETECTION_QUALITY_INSPECT_REPORT_SCHEMA_VERSION: u32 = 2;
pub const CARD_DETECTION_QUALITY_PURPOSE: &str = "auv.balatro.card_detection.quality";
pub const BALATRO_SLOT_COVERAGE_QUALITY_KNOWN_LIMIT: &str = "balatro slot-coverage quality records measurement evidence only; it does not claim model usefulness, gameplay success, or pass/fail thresholds";
pub const BALATRO_X2_QUALITY_KNOWN_LIMIT: &str = BALATRO_SLOT_COVERAGE_QUALITY_KNOWN_LIMIT;
pub const BALATRO_X4_WITNESS_BOUND_QUALITY_KNOWN_LIMIT: &str = "balatro X4 quality derives metrics/verdict only from persisted card-detection-eval-witness manifest; it does not reload semantic bundle or expected_slots directly";

const WITNESS_MANIFEST_FILE: &str = "balatro-card-detection-eval-witness.json";
const QUALITY_MANIFEST_FILE: &str = "balatro-card-detection-quality.json";
const QUALITY_INSPECT_FILE: &str = "balatro-card-detection-quality-inspect.json";

#[derive(Clone, Debug, PartialEq)]
pub struct CardDetectionQualityInputs {
  pub witness_manifest_path: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardDetectionQualityOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub manifest: CardDetectionQualityManifest,
  pub inspect_report: CardDetectionQualityInspectReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardDetectionQualityMetrics {
  pub expected_slot_count: usize,
  pub scored_slot_count: usize,
  pub unscored_slot_count: usize,
  pub below_confidence_slot_count: usize,
  pub slot_coverage_ratio: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardDetectionQualityManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub card_detection_eval_witness_manifest_path: String,
  pub witness_status: StageStatus,
  pub status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<CardDetectionQualityReason>,
  pub verdict: CardDetectionQualityVerdict,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub quality_backend: Option<CardDetectionQualityBackend>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub detector_model_id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub metrics: Option<CardDetectionQualityMetrics>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardDetectionQualityInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub card_detection_quality_manifest_path: String,
  pub card_detection_eval_witness_manifest_path: String,
  pub witness_status: StageStatus,
  pub status: StageStatus,
  pub verdict: CardDetectionQualityVerdict,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub quality_backend: Option<CardDetectionQualityBackend>,
  pub slot_coverage_ratio_available: bool,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub metrics: Option<CardDetectionQualityMetrics>,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardDetectionQualityReason {
  MissingWitnessManifest,
  WitnessManifestParseFailed,
  WitnessNotReady,
  WitnessBlocked,
  WitnessFailed,
}

impl CardDetectionQualityReason {
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
pub enum CardDetectionQualityVerdict {
  MeasuredOnly,
  MetricPartial,
  Blocked,
  Failed,
}

impl CardDetectionQualityVerdict {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::MeasuredOnly => "measured_only",
      Self::MetricPartial => "metric_partial",
      Self::Blocked => "blocked",
      Self::Failed => "failed",
    }
  }
}

#[cfg(feature = "tracing")]
pub async fn publish_card_detection_quality(
  context: Option<&Context>,
  quality: &CardDetectionQualityManifest,
) -> Result<Option<ArtifactMetadata>, crate::BalatroArtifactPublishError> {
  crate::run_read::publish_json_artifact(context, CARD_DETECTION_QUALITY_PURPOSE, quality).await
}

pub fn build_card_detection_quality(inputs: &CardDetectionQualityInputs) -> CardDetectionQualityResult<CardDetectionQualityOutput> {
  fs::create_dir_all(&inputs.output_dir)
    .map_err(|error| format!("failed to create card detection quality output dir {}: {error}", inputs.output_dir.display()))?;

  let generated_at_millis = now_millis();
  let known_limits = BTreeSet::from([
    BALATRO_X2_QUALITY_KNOWN_LIMIT.to_string(),
    BALATRO_X4_WITNESS_BOUND_QUALITY_KNOWN_LIMIT.to_string(),
  ]);
  let mut warnings = BTreeSet::new();

  let gate = evaluate_quality_gate(&inputs.witness_manifest_path, &mut warnings);
  let witness = gate.witness_manifest.as_ref();

  let outcome = match gate.witness_manifest.as_ref() {
    Some(witness) if witness.status == StageStatus::Ready => derive_quality_outcome(witness),
    _ => QualityOutcome {
      status: gate.quality_status,
      reason: gate.quality_reason,
      verdict: gate.verdict,
      metrics: None,
      quality_backend: None,
      detector_model_id: None,
    },
  };

  let manifest = CardDetectionQualityManifest {
    schema_version: CARD_DETECTION_QUALITY_MANIFEST_SCHEMA_VERSION,
    generated_at_millis,
    card_detection_eval_witness_manifest_path: inputs.witness_manifest_path.display().to_string(),
    witness_status: witness.map(|w| w.status).unwrap_or(StageStatus::Blocked),
    status: outcome.status,
    reason: outcome.reason,
    verdict: outcome.verdict,
    quality_backend: outcome.quality_backend,
    detector_model_id: outcome.detector_model_id.clone(),
    metrics: outcome.metrics.clone(),
    known_limits: known_limits.into_iter().collect(),
  };

  let manifest_path = inputs.output_dir.join(QUALITY_MANIFEST_FILE);
  write_json_file(&manifest_path, &manifest)?;

  let inspect_report = CardDetectionQualityInspectReport {
    schema_version: CARD_DETECTION_QUALITY_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    card_detection_quality_manifest_path: manifest_path.display().to_string(),
    card_detection_eval_witness_manifest_path: manifest.card_detection_eval_witness_manifest_path.clone(),
    witness_status: manifest.witness_status,
    status: manifest.status,
    verdict: manifest.verdict,
    quality_backend: manifest.quality_backend,
    slot_coverage_ratio_available: manifest.metrics.as_ref().and_then(|metrics| metrics.slot_coverage_ratio).is_some(),
    metrics: manifest.metrics.clone(),
    warnings: warnings.into_iter().collect(),
    known_limits: manifest.known_limits.clone(),
  };

  let inspect_report_path = inputs.output_dir.join(QUALITY_INSPECT_FILE);
  write_json_file(&inspect_report_path, &inspect_report)?;

  Ok(CardDetectionQualityOutput {
    output_dir: inputs.output_dir.clone(),
    manifest_path,
    inspect_report_path,
    manifest,
    inspect_report,
  })
}

pub fn build_card_detection_quality_from_witness_dir(
  witness_output_dir: &Path,
  output_dir: PathBuf,
) -> CardDetectionQualityResult<CardDetectionQualityOutput> {
  build_card_detection_quality(&CardDetectionQualityInputs {
    witness_manifest_path: witness_output_dir.join(WITNESS_MANIFEST_FILE),
    output_dir,
  })
}

pub fn derive_card_detection_quality_verdict(witness: &CardDetectionEvalWitnessManifest) -> CardDetectionQualityVerdict {
  derive_quality_outcome(witness).verdict
}

struct QualityGateEvaluation {
  quality_status: StageStatus,
  quality_reason: Option<CardDetectionQualityReason>,
  verdict: CardDetectionQualityVerdict,
  witness_manifest: Option<CardDetectionEvalWitnessManifest>,
}

struct QualityOutcome {
  status: StageStatus,
  reason: Option<CardDetectionQualityReason>,
  verdict: CardDetectionQualityVerdict,
  metrics: Option<CardDetectionQualityMetrics>,
  quality_backend: Option<CardDetectionQualityBackend>,
  detector_model_id: Option<String>,
}

fn evaluate_quality_gate(witness_manifest_path: &Path, warnings: &mut BTreeSet<String>) -> QualityGateEvaluation {
  if !witness_manifest_path.is_file() {
    return QualityGateEvaluation {
      quality_status: StageStatus::Blocked,
      quality_reason: Some(CardDetectionQualityReason::MissingWitnessManifest),
      verdict: CardDetectionQualityVerdict::Blocked,
      witness_manifest: None,
    };
  }

  let witness_manifest =
    match read_json_file::<CardDetectionEvalWitnessManifest>(witness_manifest_path, "balatro card detection eval witness manifest") {
      Ok(manifest) => Some(manifest),
      Err(error) => {
        warnings.insert(error);
        return QualityGateEvaluation {
          quality_status: StageStatus::Failed,
          quality_reason: Some(CardDetectionQualityReason::WitnessManifestParseFailed),
          verdict: CardDetectionQualityVerdict::Failed,
          witness_manifest: None,
        };
      }
    };

  let Some(witness) = witness_manifest.as_ref() else {
    return QualityGateEvaluation {
      quality_status: StageStatus::Failed,
      quality_reason: Some(CardDetectionQualityReason::WitnessManifestParseFailed),
      verdict: CardDetectionQualityVerdict::Failed,
      witness_manifest,
    };
  };

  match witness.status {
    StageStatus::Blocked => QualityGateEvaluation {
      quality_status: StageStatus::Blocked,
      quality_reason: witness.reason.map(|reason| match reason {
        CardDetectionEvalWitnessReason::SemanticNotReady
        | CardDetectionEvalWitnessReason::MissingExpectedSlots
        | CardDetectionEvalWitnessReason::MissingQueryManifest
        | CardDetectionEvalWitnessReason::QueryLineageMismatch => CardDetectionQualityReason::WitnessBlocked,
        _ => CardDetectionQualityReason::WitnessNotReady,
      }),
      verdict: CardDetectionQualityVerdict::Blocked,
      witness_manifest,
    },
    StageStatus::Failed => QualityGateEvaluation {
      quality_status: StageStatus::Failed,
      quality_reason: Some(CardDetectionQualityReason::WitnessFailed),
      verdict: CardDetectionQualityVerdict::Failed,
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

fn derive_quality_outcome(witness: &CardDetectionEvalWitnessManifest) -> QualityOutcome {
  let metrics = metrics_from_witness(witness);
  let verdict = if witness.expected_slot_count == 0 {
    CardDetectionQualityVerdict::Blocked
  } else if witness.unscored_slot_count == 0 && witness.below_confidence_slot_count == 0 {
    CardDetectionQualityVerdict::MeasuredOnly
  } else if witness.expected_slot_count > 0 {
    CardDetectionQualityVerdict::MetricPartial
  } else {
    CardDetectionQualityVerdict::Blocked
  };

  QualityOutcome {
    status: StageStatus::Ready,
    reason: None,
    verdict,
    metrics: Some(metrics),
    quality_backend: Some(witness.quality_backend),
    detector_model_id: witness.detector_model_id.clone(),
  }
}

fn metrics_from_witness(witness: &CardDetectionEvalWitnessManifest) -> CardDetectionQualityMetrics {
  let slot_coverage_ratio = if witness.expected_slot_count == 0 {
    None
  } else {
    Some(witness.scored_slot_count as f32 / witness.expected_slot_count as f32)
  };

  CardDetectionQualityMetrics {
    expected_slot_count: witness.expected_slot_count,
    scored_slot_count: witness.scored_slot_count,
    unscored_slot_count: witness.unscored_slot_count,
    below_confidence_slot_count: witness.below_confidence_slot_count,
    slot_coverage_ratio,
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

fn now_millis() -> u64 {
  u64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()).unwrap_or(u64::MAX)
}
