use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use auv_file::{
  JsonFileReadError, JsonFileWriteError, JsonWriteOptions, read_json_file as read_json_file_helper,
  write_json_file as write_json_file_helper,
};
use auv_stage_status::StageStatus;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::card_detection_eval_witness::{CardDetectionEvalWitnessManifest, CardDetectionEvalWitnessReason, CardDetectionQualityBackend};

pub type CardDetectionQualityResult<T> = Result<T, String>;

pub const CARD_DETECTION_QUALITY_MANIFEST_SCHEMA_VERSION: u32 = 2;
pub const CARD_DETECTION_QUALITY_INSPECT_REPORT_SCHEMA_VERSION: u32 = 2;
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

pub fn build_card_detection_quality(inputs: &CardDetectionQualityInputs) -> CardDetectionQualityResult<CardDetectionQualityOutput> {
  fs::create_dir_all(&inputs.output_dir)
    .map_err(|error| format!("failed to create card detection quality output dir {}: {error}", inputs.output_dir.display()))?;

  let generated_at_millis = auv_tracing_driver::now_millis();
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::card_detection_eval_witness::{CardDetectionEvalWitnessInputs, build_card_detection_eval_witness};
  use crate::card_detection_semantic::{CardDetectionSemanticValidationInputs, validate_card_detection_semantic};
  use crate::card_detection_spatial_query::{CardDetectionSpatialQueryInputs, query_card_detection_spatial};
  use crate::model::{ObjectZone, SlotId};
  use std::path::PathBuf;

  fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/balatro_consumption_probe")
  }

  fn witness_manifest_for_bundle(bundle: PathBuf, expected_slots_path: PathBuf, temp: &tempfile::TempDir) -> PathBuf {
    let semantic_path = validate_card_detection_semantic(CardDetectionSemanticValidationInputs {
      bundle_input: bundle,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic")
    .manifest_path;
    let query_path = query_card_detection_spatial(CardDetectionSpatialQueryInputs {
      card_detection_semantic_manifest_path: semantic_path.clone(),
      target_slot: SlotId::new(ObjectZone::Hand, 0),
      output_dir: temp.path().join("query"),
    })
    .expect("query")
    .manifest_path;
    build_card_detection_eval_witness(&CardDetectionEvalWitnessInputs {
      card_detection_semantic_manifest_path: semantic_path,
      card_detection_spatial_query_manifest_path: query_path,
      expected_slots_path,
      output_dir: temp.path().join("witness"),
    })
    .expect("witness")
    .manifest_path
  }

  #[test]
  fn quality_full_coverage_yields_measured_only_with_backend() {
    let temp = tempfile::tempdir().expect("tempdir");
    let witness_path = witness_manifest_for_bundle(fixture_root(), fixture_root().join("expected_slots.json"), &temp);
    let output = build_card_detection_quality(&CardDetectionQualityInputs {
      witness_manifest_path: witness_path,
      output_dir: temp.path().join("quality"),
    })
    .expect("quality");

    assert_eq!(output.manifest.status, StageStatus::Ready);
    assert_eq!(output.manifest.verdict, CardDetectionQualityVerdict::MeasuredOnly);
    assert_eq!(output.manifest.quality_backend, Some(CardDetectionQualityBackend::UltralyticsOnnxEntities));
    let metrics = output.manifest.metrics.as_ref().expect("metrics");
    assert_eq!(metrics.expected_slot_count, 3);
    assert_eq!(metrics.unscored_slot_count, 0);
    assert!(metrics.slot_coverage_ratio.is_some());
  }

  #[test]
  fn quality_partial_slot_coverage_yields_metric_partial_with_metrics() {
    let temp = tempfile::tempdir().expect("tempdir");
    let witness_path =
      witness_manifest_for_bundle(fixture_root().join("partial_coverage"), fixture_root().join("partial_expected_slots.json"), &temp);
    let output = build_card_detection_quality(&CardDetectionQualityInputs {
      witness_manifest_path: witness_path,
      output_dir: temp.path().join("quality"),
    })
    .expect("quality");

    assert_eq!(output.manifest.verdict, CardDetectionQualityVerdict::MetricPartial);
    let metrics = output.manifest.metrics.as_ref().expect("metrics present");
    assert!(metrics.unscored_slot_count > 0);
    assert!(metrics.slot_coverage_ratio.is_some());
  }

  #[test]
  fn quality_blocked_when_witness_manifest_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output = build_card_detection_quality(&CardDetectionQualityInputs {
      witness_manifest_path: temp.path().join("missing.json"),
      output_dir: temp.path().join("quality"),
    })
    .expect("quality");

    assert_eq!(output.manifest.status, StageStatus::Blocked);
    assert_eq!(output.manifest.verdict, CardDetectionQualityVerdict::Blocked);
    assert!(output.manifest.metrics.is_none());
  }

  #[test]
  fn quality_blocked_when_witness_blocked() {
    let temp = tempfile::tempdir().expect("tempdir");
    let semantic_path = validate_card_detection_semantic(CardDetectionSemanticValidationInputs {
      bundle_input: fixture_root().join("broken/empty_detections"),
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic")
    .manifest_path;
    let witness_path = build_card_detection_eval_witness(&CardDetectionEvalWitnessInputs {
      card_detection_semantic_manifest_path: semantic_path,
      card_detection_spatial_query_manifest_path: temp.path().join("missing-query.json"),
      expected_slots_path: fixture_root().join("expected_slots.json"),
      output_dir: temp.path().join("witness"),
    })
    .expect("witness")
    .manifest_path;
    let output = build_card_detection_quality(&CardDetectionQualityInputs {
      witness_manifest_path: witness_path,
      output_dir: temp.path().join("quality"),
    })
    .expect("quality");

    assert_eq!(output.manifest.status, StageStatus::Blocked);
    assert_eq!(output.manifest.verdict, CardDetectionQualityVerdict::Blocked);
    assert!(output.manifest.metrics.is_none());
  }

  #[test]
  fn quality_metric_partial_from_witness_ready_partial_coverage() {
    let temp = tempfile::tempdir().expect("tempdir");
    let witness_path =
      witness_manifest_for_bundle(fixture_root().join("partial_coverage"), fixture_root().join("partial_expected_slots.json"), &temp);
    let witness = read_json_file::<CardDetectionEvalWitnessManifest>(&witness_path, "witness").expect("read witness");
    assert_eq!(witness.status, StageStatus::Ready);
    assert_eq!(derive_card_detection_quality_verdict(&witness), CardDetectionQualityVerdict::MetricPartial);
  }

  #[test]
  fn quality_does_not_reload_semantic_or_bundle_directly() {
    let temp = tempfile::tempdir().expect("tempdir");
    let witness_path = witness_manifest_for_bundle(fixture_root(), fixture_root().join("expected_slots.json"), &temp);
    let output = build_card_detection_quality(&CardDetectionQualityInputs {
      witness_manifest_path: witness_path,
      output_dir: temp.path().join("quality"),
    })
    .expect("quality");

    let manifest_json = serde_json::to_string(&output.manifest).expect("manifest json");
    assert!(!manifest_json.contains("card_detection_semantic_manifest_path"), "quality manifest must not carry direct semantic lineage");
    assert!(!manifest_json.contains("source_detection_bundle_dir"), "quality manifest must not reload bundle lineage directly");
    assert!(
      output.manifest.known_limits.iter().any(|limit| limit.contains("witness")),
      "known_limits must document witness-bound quality boundary"
    );
  }

  #[test]
  fn quality_does_not_persist_eval_report_sidecar_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let witness_path = witness_manifest_for_bundle(fixture_root(), fixture_root().join("expected_slots.json"), &temp);
    let _output = build_card_detection_quality(&CardDetectionQualityInputs {
      witness_manifest_path: witness_path,
      output_dir: temp.path().join("quality"),
    })
    .expect("quality");

    assert!(
      !temp.path().join("quality/balatro-card-detection-eval-report.json").exists(),
      "quality must not write a separate eval-report file"
    );
    assert!(temp.path().join("witness/balatro-card-detection-eval-witness.json").exists(), "witness must persist eval payload");
  }

  #[test]
  fn quality_manifest_schema_version_is_two_for_witness_bound_wire() {
    let temp = tempfile::tempdir().expect("tempdir");
    let witness_path = witness_manifest_for_bundle(fixture_root(), fixture_root().join("expected_slots.json"), &temp);
    let output = build_card_detection_quality(&CardDetectionQualityInputs {
      witness_manifest_path: witness_path,
      output_dir: temp.path().join("quality"),
    })
    .expect("quality");

    assert_eq!(output.manifest.schema_version, CARD_DETECTION_QUALITY_MANIFEST_SCHEMA_VERSION);
    assert_eq!(output.manifest.schema_version, 2);
    assert_eq!(output.inspect_report.schema_version, CARD_DETECTION_QUALITY_INSPECT_REPORT_SCHEMA_VERSION);
    assert_eq!(output.inspect_report.schema_version, 2);
  }

  #[test]
  fn quality_failed_when_witness_manifest_parse_failed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bad_witness = temp.path().join("bad-witness.json");
    fs::write(&bad_witness, "{not-json").expect("write bad witness");
    let output = build_card_detection_quality(&CardDetectionQualityInputs {
      witness_manifest_path: bad_witness,
      output_dir: temp.path().join("quality"),
    })
    .expect("quality");

    assert_eq!(output.manifest.status, StageStatus::Failed);
    assert_eq!(output.manifest.reason, Some(CardDetectionQualityReason::WitnessManifestParseFailed));
    assert_eq!(output.manifest.verdict, CardDetectionQualityVerdict::Failed);
    assert!(output.manifest.metrics.is_none());
  }

  #[test]
  fn quality_failed_when_witness_failed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let semantic_path = validate_card_detection_semantic(CardDetectionSemanticValidationInputs {
      bundle_input: fixture_root(),
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic")
    .manifest_path;
    let query_path = query_card_detection_spatial(CardDetectionSpatialQueryInputs {
      card_detection_semantic_manifest_path: semantic_path.clone(),
      target_slot: SlotId::new(ObjectZone::Hand, 0),
      output_dir: temp.path().join("query"),
    })
    .expect("query")
    .manifest_path;
    let witness_path = build_card_detection_eval_witness(&CardDetectionEvalWitnessInputs {
      card_detection_semantic_manifest_path: semantic_path,
      card_detection_spatial_query_manifest_path: query_path,
      expected_slots_path: fixture_root().join("witness/failed_expected_slots.json"),
      output_dir: temp.path().join("witness"),
    })
    .expect("witness")
    .manifest_path;
    let output = build_card_detection_quality(&CardDetectionQualityInputs {
      witness_manifest_path: witness_path,
      output_dir: temp.path().join("quality"),
    })
    .expect("quality");

    assert_eq!(output.manifest.status, StageStatus::Failed);
    assert_eq!(output.manifest.reason, Some(CardDetectionQualityReason::WitnessFailed));
    assert_eq!(output.manifest.verdict, CardDetectionQualityVerdict::Failed);
    assert!(output.manifest.metrics.is_none());
  }
}
