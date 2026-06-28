use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use auv_file::{
  JsonFileReadError, JsonFileWriteError, JsonWriteOptions, read_json_file as read_json_file_helper,
  write_json_file as write_json_file_helper,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::card_detection_producer::{
  ExpectedSlotEntry, LoadedDetectionBundle, load_detection_bundle, load_expected_slots,
};
use crate::card_detection_semantic::{CardDetectionSemanticManifest, CardDetectionSemanticStatus};
use crate::card_detection_spatial_query::slot_detections;
use crate::model::ObjectZone;

pub type CardDetectionQualityResult<T> = Result<T, String>;

pub const CARD_DETECTION_QUALITY_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const CARD_DETECTION_QUALITY_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;
pub const BALATRO_X2_QUALITY_KNOWN_LIMIT: &str = "balatro X2 quality records slot-coverage measurement evidence only; it does not claim model usefulness, gameplay success, or pass/fail thresholds";

const QUALITY_MANIFEST_FILE: &str = "balatro-card-detection-quality.json";
const QUALITY_INSPECT_FILE: &str = "balatro-card-detection-quality-inspect.json";
const EVAL_REPORT_FILE: &str = "balatro-card-detection-eval-report.json";

#[derive(Clone, Debug, PartialEq)]
pub struct CardDetectionQualityInputs {
  pub card_detection_semantic_manifest_path: PathBuf,
  pub expected_slots_path: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardDetectionQualityOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub eval_report_path: PathBuf,
  pub manifest: CardDetectionQualityManifest,
  pub inspect_report: CardDetectionQualityInspectReport,
  pub eval_report: CardDetectionEvalReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardDetectionEvalReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub source_detection_bundle_dir: String,
  pub expected_slot_count: usize,
  pub scored_slot_count: usize,
  pub unscored_slot_count: usize,
  pub below_confidence_slot_count: usize,
  pub quality_backend: CardDetectionQualityBackend,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub detector_model_id: Option<String>,
  pub slot_scores: Vec<CardDetectionSlotScore>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardDetectionSlotScore {
  pub zone: String,
  pub index: u32,
  pub scored: bool,
  pub confidence: Option<f32>,
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
  pub card_detection_semantic_manifest_path: String,
  pub card_detection_eval_report_path: String,
  pub source_detection_bundle_dir: String,
  pub semantic_status: CardDetectionSemanticStatus,
  pub status: CardDetectionQualityStatus,
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
  pub card_detection_semantic_manifest_path: String,
  pub semantic_status: CardDetectionSemanticStatus,
  pub status: CardDetectionQualityStatus,
  pub verdict: CardDetectionQualityVerdict,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub quality_backend: Option<CardDetectionQualityBackend>,
  pub slot_coverage_ratio_available: bool,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub metrics: Option<CardDetectionQualityMetrics>,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

pub type CardDetectionQualityStatus = auv_stage_status::StageStatus;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardDetectionQualityBackend {
  UltralyticsOnnxUi,
  UltralyticsOnnxEntities,
}

impl CardDetectionQualityBackend {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::UltralyticsOnnxUi => "ultralytics_onnx_ui",
      Self::UltralyticsOnnxEntities => "ultralytics_onnx_entities",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardDetectionQualityReason {
  MissingSemanticManifest,
  SemanticManifestParseFailed,
  SemanticNotReady,
  MissingExpectedSlots,
  ExpectedSlotsParseFailed,
  BundleUnavailable,
}

impl CardDetectionQualityReason {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::MissingSemanticManifest => "missing_semantic_manifest",
      Self::SemanticManifestParseFailed => "semantic_manifest_parse_failed",
      Self::SemanticNotReady => "semantic_not_ready",
      Self::MissingExpectedSlots => "missing_expected_slots",
      Self::ExpectedSlotsParseFailed => "expected_slots_parse_failed",
      Self::BundleUnavailable => "bundle_unavailable",
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

pub fn build_card_detection_quality(
  inputs: &CardDetectionQualityInputs,
) -> CardDetectionQualityResult<CardDetectionQualityOutput> {
  fs::create_dir_all(&inputs.output_dir).map_err(|error| {
    format!(
      "failed to create card detection quality output dir {}: {error}",
      inputs.output_dir.display()
    )
  })?;

  let generated_at_millis = auv_tracing_driver::now_millis();
  let known_limits = BTreeSet::from([BALATRO_X2_QUALITY_KNOWN_LIMIT.to_string()]);
  let mut warnings = BTreeSet::new();

  let gate = evaluate_quality_gate(
    &inputs.card_detection_semantic_manifest_path,
    &inputs.expected_slots_path,
    &mut warnings,
  );

  let eval_report = gate.eval_report.clone();
  let outcome = gate
    .eval_report
    .as_ref()
    .map(|report| derive_quality_outcome(report))
    .unwrap_or(QualityOutcome {
      status: gate.quality_status,
      reason: gate.quality_reason,
      verdict: gate.verdict,
      metrics: None,
      quality_backend: None,
      detector_model_id: None,
    });

  let manifest = CardDetectionQualityManifest {
    schema_version: CARD_DETECTION_QUALITY_MANIFEST_SCHEMA_VERSION,
    generated_at_millis,
    card_detection_semantic_manifest_path: inputs
      .card_detection_semantic_manifest_path
      .display()
      .to_string(),
    card_detection_eval_report_path: String::new(),
    source_detection_bundle_dir: eval_report
      .as_ref()
      .map(|report| report.source_detection_bundle_dir.clone())
      .unwrap_or_default(),
    semantic_status: gate.semantic_status,
    status: outcome.status,
    reason: outcome.reason,
    verdict: outcome.verdict,
    quality_backend: outcome.quality_backend,
    detector_model_id: outcome.detector_model_id.clone(),
    metrics: outcome.metrics.clone(),
    known_limits: known_limits.into_iter().collect(),
  };

  let eval_report_path = inputs.output_dir.join(EVAL_REPORT_FILE);
  if let Some(report) = eval_report.as_ref() {
    write_json_file(&eval_report_path, report)?;
  }

  let mut manifest = manifest;
  manifest.card_detection_eval_report_path = eval_report_path.display().to_string();

  let manifest_path = inputs.output_dir.join(QUALITY_MANIFEST_FILE);
  write_json_file(&manifest_path, &manifest)?;

  let inspect_report = CardDetectionQualityInspectReport {
    schema_version: CARD_DETECTION_QUALITY_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    card_detection_quality_manifest_path: manifest_path.display().to_string(),
    card_detection_semantic_manifest_path: manifest.card_detection_semantic_manifest_path.clone(),
    semantic_status: manifest.semantic_status,
    status: manifest.status,
    verdict: manifest.verdict,
    quality_backend: manifest.quality_backend,
    slot_coverage_ratio_available: manifest
      .metrics
      .as_ref()
      .and_then(|metrics| metrics.slot_coverage_ratio)
      .is_some(),
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
    eval_report_path,
    manifest,
    inspect_report,
    eval_report: eval_report.unwrap_or(CardDetectionEvalReport {
      schema_version: 1,
      generated_at_millis,
      source_detection_bundle_dir: String::new(),
      expected_slot_count: 0,
      scored_slot_count: 0,
      unscored_slot_count: 0,
      below_confidence_slot_count: 0,
      quality_backend: CardDetectionQualityBackend::UltralyticsOnnxEntities,
      detector_model_id: None,
      slot_scores: vec![],
    }),
  })
}

pub fn derive_card_detection_quality_verdict(
  eval_report: &CardDetectionEvalReport,
) -> CardDetectionQualityVerdict {
  derive_quality_outcome(eval_report).verdict
}

struct QualityGateEvaluation {
  quality_status: CardDetectionQualityStatus,
  quality_reason: Option<CardDetectionQualityReason>,
  verdict: CardDetectionQualityVerdict,
  semantic_status: CardDetectionSemanticStatus,
  eval_report: Option<CardDetectionEvalReport>,
}

struct QualityOutcome {
  status: CardDetectionQualityStatus,
  reason: Option<CardDetectionQualityReason>,
  verdict: CardDetectionQualityVerdict,
  metrics: Option<CardDetectionQualityMetrics>,
  quality_backend: Option<CardDetectionQualityBackend>,
  detector_model_id: Option<String>,
}

fn evaluate_quality_gate(
  semantic_manifest_path: &Path,
  expected_slots_path: &Path,
  warnings: &mut BTreeSet<String>,
) -> QualityGateEvaluation {
  if !semantic_manifest_path.is_file() {
    return QualityGateEvaluation {
      quality_status: CardDetectionQualityStatus::Blocked,
      quality_reason: Some(CardDetectionQualityReason::MissingSemanticManifest),
      verdict: CardDetectionQualityVerdict::Blocked,
      semantic_status: CardDetectionSemanticStatus::Blocked,
      eval_report: None,
    };
  }

  let semantic_manifest = match read_json_file::<CardDetectionSemanticManifest>(
    semantic_manifest_path,
    "balatro card detection semantic manifest",
  ) {
    Ok(manifest) => manifest,
    Err(error) => {
      warnings.insert(error);
      return QualityGateEvaluation {
        quality_status: CardDetectionQualityStatus::Failed,
        quality_reason: Some(CardDetectionQualityReason::SemanticManifestParseFailed),
        verdict: CardDetectionQualityVerdict::Failed,
        semantic_status: CardDetectionSemanticStatus::Failed,
        eval_report: None,
      };
    }
  };

  if semantic_manifest.semantic_status != CardDetectionSemanticStatus::Ready {
    return QualityGateEvaluation {
      quality_status: CardDetectionQualityStatus::Blocked,
      quality_reason: Some(CardDetectionQualityReason::SemanticNotReady),
      verdict: CardDetectionQualityVerdict::Blocked,
      semantic_status: semantic_manifest.semantic_status,
      eval_report: None,
    };
  }

  if !expected_slots_path.is_file() {
    return QualityGateEvaluation {
      quality_status: CardDetectionQualityStatus::Blocked,
      quality_reason: Some(CardDetectionQualityReason::MissingExpectedSlots),
      verdict: CardDetectionQualityVerdict::Blocked,
      semantic_status: semantic_manifest.semantic_status,
      eval_report: None,
    };
  }

  let expected_slots = match load_expected_slots(expected_slots_path) {
    Ok(slots) => slots,
    Err(error) => {
      warnings.insert(error);
      return QualityGateEvaluation {
        quality_status: CardDetectionQualityStatus::Failed,
        quality_reason: Some(CardDetectionQualityReason::ExpectedSlotsParseFailed),
        verdict: CardDetectionQualityVerdict::Failed,
        semantic_status: semantic_manifest.semantic_status,
        eval_report: None,
      };
    }
  };

  let bundle_dir = PathBuf::from(&semantic_manifest.source_detection_bundle_dir);
  let bundle = match load_detection_bundle(&bundle_dir) {
    Ok(bundle) => bundle,
    Err(error) => {
      warnings.insert(error);
      return QualityGateEvaluation {
        quality_status: CardDetectionQualityStatus::Failed,
        quality_reason: Some(CardDetectionQualityReason::BundleUnavailable),
        verdict: CardDetectionQualityVerdict::Failed,
        semantic_status: semantic_manifest.semantic_status,
        eval_report: None,
      };
    }
  };

  let eval_report = build_eval_report(&bundle, &expected_slots.slots);
  let outcome = derive_quality_outcome(&eval_report);
  QualityGateEvaluation {
    quality_status: outcome.status,
    quality_reason: outcome.reason,
    verdict: outcome.verdict,
    semantic_status: semantic_manifest.semantic_status,
    eval_report: Some(eval_report),
  }
}

fn build_eval_report(
  bundle: &LoadedDetectionBundle,
  expected_slots: &[ExpectedSlotEntry],
) -> CardDetectionEvalReport {
  let mut slot_scores = Vec::new();
  let mut scored_slot_count = 0usize;
  let mut below_confidence_slot_count = 0usize;

  for entry in expected_slots {
    let zone = parse_zone(&entry.zone);
    let detections = slot_detections(bundle, zone);
    let detection = detections.get(entry.index as usize);
    let (scored, confidence) = match detection {
      Some(detection) if detection.confidence >= entry.min_confidence => {
        scored_slot_count += 1;
        (true, Some(detection.confidence))
      }
      Some(detection) => {
        below_confidence_slot_count += 1;
        (false, Some(detection.confidence))
      }
      None => (false, None),
    };
    slot_scores.push(CardDetectionSlotScore {
      zone: entry.zone.clone(),
      index: entry.index,
      scored,
      confidence,
    });
  }

  let expected_slot_count = expected_slots.len();
  let unscored_slot_count = expected_slot_count
    .saturating_sub(scored_slot_count)
    .max(below_confidence_slot_count);

  CardDetectionEvalReport {
    schema_version: 1,
    generated_at_millis: auv_tracing_driver::now_millis(),
    source_detection_bundle_dir: bundle.bundle_dir.display().to_string(),
    expected_slot_count,
    scored_slot_count,
    unscored_slot_count,
    below_confidence_slot_count,
    quality_backend: CardDetectionQualityBackend::UltralyticsOnnxEntities,
    detector_model_id: bundle.manifest.detector_model_id_entities.clone(),
    slot_scores,
  }
}

fn parse_zone(zone: &str) -> ObjectZone {
  match zone {
    "hand" => ObjectZone::Hand,
    "joker" => ObjectZone::Joker,
    "consumable" => ObjectZone::Consumable,
    "store" => ObjectZone::Store,
    "button" => ObjectZone::Button,
    _ => ObjectZone::Unknown,
  }
}

fn derive_quality_outcome(eval_report: &CardDetectionEvalReport) -> QualityOutcome {
  let metrics = metrics_from_eval_report(eval_report);
  let verdict = if eval_report.expected_slot_count == 0 {
    CardDetectionQualityVerdict::Blocked
  } else if eval_report.unscored_slot_count == 0 && eval_report.below_confidence_slot_count == 0 {
    CardDetectionQualityVerdict::MeasuredOnly
  } else if eval_report.expected_slot_count > 0 {
    CardDetectionQualityVerdict::MetricPartial
  } else {
    CardDetectionQualityVerdict::Blocked
  };

  QualityOutcome {
    status: CardDetectionQualityStatus::Ready,
    reason: None,
    verdict,
    metrics: Some(metrics),
    quality_backend: Some(eval_report.quality_backend),
    detector_model_id: eval_report.detector_model_id.clone(),
  }
}

fn metrics_from_eval_report(eval_report: &CardDetectionEvalReport) -> CardDetectionQualityMetrics {
  let slot_coverage_ratio = if eval_report.expected_slot_count == 0 {
    None
  } else {
    Some(eval_report.scored_slot_count as f32 / eval_report.expected_slot_count as f32)
  };

  CardDetectionQualityMetrics {
    expected_slot_count: eval_report.expected_slot_count,
    scored_slot_count: eval_report.scored_slot_count,
    unscored_slot_count: eval_report.unscored_slot_count,
    below_confidence_slot_count: eval_report.below_confidence_slot_count,
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
  use crate::card_detection_semantic::{
    CardDetectionSemanticValidationInputs, validate_card_detection_semantic,
  };
  use std::path::PathBuf;

  fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/balatro_consumption_probe")
  }

  fn semantic_for(bundle: PathBuf, temp: &tempfile::TempDir) -> PathBuf {
    validate_card_detection_semantic(CardDetectionSemanticValidationInputs {
      bundle_input: bundle,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic")
    .manifest_path
  }

  #[test]
  fn quality_full_coverage_yields_measured_only_with_backend() {
    let temp = tempfile::tempdir().expect("tempdir");
    let semantic_path = semantic_for(fixture_root(), &temp);
    let output = build_card_detection_quality(&CardDetectionQualityInputs {
      card_detection_semantic_manifest_path: semantic_path,
      expected_slots_path: fixture_root().join("expected_slots.json"),
      output_dir: temp.path().join("quality"),
    })
    .expect("quality");

    assert_eq!(output.manifest.status, CardDetectionQualityStatus::Ready);
    assert_eq!(
      output.manifest.verdict,
      CardDetectionQualityVerdict::MeasuredOnly
    );
    assert_eq!(
      output.manifest.quality_backend,
      Some(CardDetectionQualityBackend::UltralyticsOnnxEntities)
    );
    let metrics = output.manifest.metrics.as_ref().expect("metrics");
    assert_eq!(metrics.expected_slot_count, 3);
    assert_eq!(metrics.unscored_slot_count, 0);
    assert!(metrics.slot_coverage_ratio.is_some());
  }

  #[test]
  fn quality_partial_slot_coverage_yields_metric_partial_with_metrics() {
    let temp = tempfile::tempdir().expect("tempdir");
    let semantic_path = semantic_for(fixture_root().join("partial_coverage"), &temp);
    let output = build_card_detection_quality(&CardDetectionQualityInputs {
      card_detection_semantic_manifest_path: semantic_path,
      expected_slots_path: fixture_root().join("partial_expected_slots.json"),
      output_dir: temp.path().join("quality"),
    })
    .expect("quality");

    assert_eq!(
      output.manifest.verdict,
      CardDetectionQualityVerdict::MetricPartial
    );
    let metrics = output.manifest.metrics.as_ref().expect("metrics present");
    assert!(metrics.unscored_slot_count > 0);
    assert!(metrics.slot_coverage_ratio.is_some());
  }

  #[test]
  fn quality_blocked_when_semantic_manifest_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output = build_card_detection_quality(&CardDetectionQualityInputs {
      card_detection_semantic_manifest_path: temp.path().join("missing.json"),
      expected_slots_path: fixture_root().join("expected_slots.json"),
      output_dir: temp.path().join("quality"),
    })
    .expect("quality");

    assert_eq!(output.manifest.status, CardDetectionQualityStatus::Blocked);
    assert_eq!(
      output.manifest.verdict,
      CardDetectionQualityVerdict::Blocked
    );
    assert!(output.manifest.metrics.is_none());
  }
}
