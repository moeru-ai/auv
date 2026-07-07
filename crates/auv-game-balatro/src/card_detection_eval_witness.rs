use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use auv_file::{
  JsonFileReadError, JsonFileWriteError, JsonWriteOptions, read_json_file as read_json_file_helper,
  write_json_file as write_json_file_helper,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::card_detection_producer::{ExpectedSlotEntry, LoadedDetectionBundle, load_detection_bundle, load_expected_slots};
use crate::card_detection_semantic::{CardDetectionSemanticManifest, CardDetectionSemanticStatus};
use crate::card_detection_spatial_query::CardDetectionSpatialQueryManifest;
use crate::card_detection_spatial_query::slot_detections;
use crate::model::ObjectZone;

pub type CardDetectionEvalWitnessResult<T> = Result<T, String>;

pub const CARD_DETECTION_EVAL_WITNESS_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const CARD_DETECTION_EVAL_WITNESS_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;
pub const BALATRO_X4_WITNESS_KNOWN_LIMIT: &str = "balatro X4 witness records slot-coverage eval payload and spatial-query manifest path for durable lineage; eval scores bundle slots directly and does not require spatial query answered; it is not action verification or gameplay success";

const WITNESS_MANIFEST_FILE: &str = "balatro-card-detection-eval-witness.json";
const WITNESS_INSPECT_FILE: &str = "balatro-card-detection-eval-witness-inspect.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardDetectionEvalWitnessInputs {
  pub card_detection_semantic_manifest_path: PathBuf,
  pub card_detection_spatial_query_manifest_path: PathBuf,
  pub expected_slots_path: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardDetectionEvalWitnessOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub manifest: CardDetectionEvalWitnessManifest,
  pub inspect_report: CardDetectionEvalWitnessInspectReport,
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
pub struct CardDetectionEvalWitnessManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub card_detection_semantic_manifest_path: String,
  pub card_detection_spatial_query_manifest_path: String,
  pub expected_slots_path: String,
  pub source_detection_bundle_dir: String,
  pub expected_slot_count: usize,
  pub scored_slot_count: usize,
  pub unscored_slot_count: usize,
  pub below_confidence_slot_count: usize,
  pub quality_backend: CardDetectionQualityBackend,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub detector_model_id: Option<String>,
  pub slot_scores: Vec<CardDetectionSlotScore>,
  pub status: CardDetectionEvalWitnessStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<CardDetectionEvalWitnessReason>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardDetectionEvalWitnessInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub card_detection_eval_witness_manifest_path: String,
  pub card_detection_semantic_manifest_path: String,
  pub card_detection_spatial_query_manifest_path: String,
  pub expected_slots_path: String,
  pub source_detection_bundle_dir: String,
  pub expected_slot_count: usize,
  pub scored_slot_count: usize,
  pub unscored_slot_count: usize,
  pub below_confidence_slot_count: usize,
  pub quality_backend: CardDetectionQualityBackend,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub detector_model_id: Option<String>,
  pub slot_score_count: usize,
  pub semantic_manifest_readable: bool,
  pub spatial_query_manifest_readable: bool,
  pub expected_slots_readable: bool,
  pub status: CardDetectionEvalWitnessStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<CardDetectionEvalWitnessReason>,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

pub type CardDetectionEvalWitnessStatus = auv_stage_status::StageStatus;

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
pub enum CardDetectionEvalWitnessReason {
  SemanticNotReady,
  SemanticFailed,
  MissingExpectedSlots,
  ExpectedSlotsParseFailed,
  BundleUnavailable,
  MissingQueryManifest,
  QueryManifestParseFailed,
  QueryLineageMismatch,
}

impl CardDetectionEvalWitnessReason {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::SemanticNotReady => "semantic_not_ready",
      Self::SemanticFailed => "semantic_failed",
      Self::MissingExpectedSlots => "missing_expected_slots",
      Self::ExpectedSlotsParseFailed => "expected_slots_parse_failed",
      Self::BundleUnavailable => "bundle_unavailable",
      Self::MissingQueryManifest => "missing_query_manifest",
      Self::QueryManifestParseFailed => "query_manifest_parse_failed",
      Self::QueryLineageMismatch => "query_lineage_mismatch",
    }
  }
}

pub fn build_card_detection_eval_witness(
  inputs: &CardDetectionEvalWitnessInputs,
) -> CardDetectionEvalWitnessResult<CardDetectionEvalWitnessOutput> {
  fs::create_dir_all(&inputs.output_dir)
    .map_err(|error| format!("failed to create card detection eval witness output dir {}: {error}", inputs.output_dir.display()))?;

  let generated_at_millis = auv_tracing_driver::now_millis();
  let known_limits = BTreeSet::from([BALATRO_X4_WITNESS_KNOWN_LIMIT.to_string()]);
  let mut warnings = BTreeSet::new();

  let gate = evaluate_witness_gate(
    &inputs.card_detection_semantic_manifest_path,
    &inputs.card_detection_spatial_query_manifest_path,
    &inputs.expected_slots_path,
    &mut warnings,
  );

  let eval = gate.eval_report.as_ref();
  let manifest = CardDetectionEvalWitnessManifest {
    schema_version: CARD_DETECTION_EVAL_WITNESS_MANIFEST_SCHEMA_VERSION,
    generated_at_millis,
    card_detection_semantic_manifest_path: inputs.card_detection_semantic_manifest_path.display().to_string(),
    card_detection_spatial_query_manifest_path: inputs.card_detection_spatial_query_manifest_path.display().to_string(),
    expected_slots_path: inputs.expected_slots_path.display().to_string(),
    source_detection_bundle_dir: eval
      .map(|report| report.source_detection_bundle_dir.clone())
      .unwrap_or_else(|| gate.source_detection_bundle_dir.clone()),
    expected_slot_count: eval.map(|report| report.expected_slot_count).unwrap_or(0),
    scored_slot_count: eval.map(|report| report.scored_slot_count).unwrap_or(0),
    unscored_slot_count: eval.map(|report| report.unscored_slot_count).unwrap_or(0),
    below_confidence_slot_count: eval.map(|report| report.below_confidence_slot_count).unwrap_or(0),
    quality_backend: eval.map(|report| report.quality_backend).unwrap_or(CardDetectionQualityBackend::UltralyticsOnnxEntities),
    detector_model_id: eval.and_then(|report| report.detector_model_id.clone()).or(gate.detector_model_id.clone()),
    slot_scores: eval.map(|report| report.slot_scores.clone()).unwrap_or_default(),
    status: gate.status,
    reason: gate.reason,
    known_limits: known_limits.into_iter().collect(),
  };

  let manifest_path = inputs.output_dir.join(WITNESS_MANIFEST_FILE);
  write_json_file(&manifest_path, &manifest)?;

  let inspect_report = CardDetectionEvalWitnessInspectReport {
    schema_version: CARD_DETECTION_EVAL_WITNESS_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    card_detection_eval_witness_manifest_path: manifest_path.display().to_string(),
    card_detection_semantic_manifest_path: manifest.card_detection_semantic_manifest_path.clone(),
    card_detection_spatial_query_manifest_path: manifest.card_detection_spatial_query_manifest_path.clone(),
    expected_slots_path: manifest.expected_slots_path.clone(),
    source_detection_bundle_dir: manifest.source_detection_bundle_dir.clone(),
    expected_slot_count: manifest.expected_slot_count,
    scored_slot_count: manifest.scored_slot_count,
    unscored_slot_count: manifest.unscored_slot_count,
    below_confidence_slot_count: manifest.below_confidence_slot_count,
    quality_backend: manifest.quality_backend,
    detector_model_id: manifest.detector_model_id.clone(),
    slot_score_count: manifest.slot_scores.len(),
    semantic_manifest_readable: gate.semantic_manifest.is_some(),
    spatial_query_manifest_readable: gate.spatial_query_manifest_readable,
    expected_slots_readable: gate.expected_slots_readable,
    status: manifest.status,
    reason: manifest.reason,
    warnings: warnings.into_iter().collect(),
    known_limits: manifest.known_limits.clone(),
  };

  let inspect_report_path = inputs.output_dir.join(WITNESS_INSPECT_FILE);
  write_json_file(&inspect_report_path, &inspect_report)?;

  Ok(CardDetectionEvalWitnessOutput {
    output_dir: inputs.output_dir.clone(),
    manifest_path,
    inspect_report_path,
    manifest,
    inspect_report,
  })
}

struct WitnessGateEvaluation {
  status: CardDetectionEvalWitnessStatus,
  reason: Option<CardDetectionEvalWitnessReason>,
  semantic_manifest: Option<CardDetectionSemanticManifest>,
  #[allow(dead_code)]
  spatial_query_manifest: Option<CardDetectionSpatialQueryManifest>,
  spatial_query_manifest_readable: bool,
  source_detection_bundle_dir: String,
  detector_model_id: Option<String>,
  expected_slots_readable: bool,
  eval_report: Option<CardDetectionEvalReport>,
}

fn spatial_query_manifest_readable(path: &Path) -> bool {
  path.is_file() && read_json_file::<CardDetectionSpatialQueryManifest>(path, "balatro card detection spatial query manifest").is_ok()
}

fn query_lineage_matches_semantic(
  semantic_manifest_path: &Path,
  semantic_manifest: &CardDetectionSemanticManifest,
  spatial_query_manifest: &CardDetectionSpatialQueryManifest,
) -> bool {
  spatial_query_manifest.card_detection_semantic_manifest_path == semantic_manifest_path.display().to_string()
    && spatial_query_manifest.source_detection_bundle_dir == semantic_manifest.source_detection_bundle_dir
}

fn evaluate_witness_gate(
  semantic_manifest_path: &Path,
  spatial_query_manifest_path: &Path,
  expected_slots_path: &Path,
  warnings: &mut BTreeSet<String>,
) -> WitnessGateEvaluation {
  let spatial_query_manifest_readable = spatial_query_manifest_readable(spatial_query_manifest_path);

  if !semantic_manifest_path.is_file() {
    return WitnessGateEvaluation {
      status: CardDetectionEvalWitnessStatus::Blocked,
      reason: Some(CardDetectionEvalWitnessReason::SemanticNotReady),
      semantic_manifest: None,
      spatial_query_manifest: None,
      spatial_query_manifest_readable,
      source_detection_bundle_dir: String::new(),
      detector_model_id: None,
      expected_slots_readable: false,
      eval_report: None,
    };
  }

  let semantic_manifest =
    match read_json_file::<CardDetectionSemanticManifest>(semantic_manifest_path, "balatro card detection semantic manifest") {
      Ok(manifest) => manifest,
      Err(error) => {
        warnings.insert(error);
        return WitnessGateEvaluation {
          status: CardDetectionEvalWitnessStatus::Failed,
          reason: Some(CardDetectionEvalWitnessReason::SemanticFailed),
          semantic_manifest: None,
          spatial_query_manifest: None,
          spatial_query_manifest_readable,
          source_detection_bundle_dir: String::new(),
          detector_model_id: None,
          expected_slots_readable: false,
          eval_report: None,
        };
      }
    };

  if semantic_manifest.semantic_status != CardDetectionSemanticStatus::Ready {
    let reason = match semantic_manifest.semantic_status {
      CardDetectionSemanticStatus::Failed => CardDetectionEvalWitnessReason::SemanticFailed,
      _ => CardDetectionEvalWitnessReason::SemanticNotReady,
    };
    let source_detection_bundle_dir = semantic_manifest.source_detection_bundle_dir.clone();
    return WitnessGateEvaluation {
      status: CardDetectionEvalWitnessStatus::Blocked,
      reason: Some(reason),
      semantic_manifest: Some(semantic_manifest),
      spatial_query_manifest: None,
      spatial_query_manifest_readable,
      source_detection_bundle_dir,
      detector_model_id: None,
      expected_slots_readable: false,
      eval_report: None,
    };
  }

  if !spatial_query_manifest_path.is_file() {
    let source_detection_bundle_dir = semantic_manifest.source_detection_bundle_dir.clone();
    return WitnessGateEvaluation {
      status: CardDetectionEvalWitnessStatus::Blocked,
      reason: Some(CardDetectionEvalWitnessReason::MissingQueryManifest),
      semantic_manifest: Some(semantic_manifest),
      spatial_query_manifest: None,
      spatial_query_manifest_readable,
      source_detection_bundle_dir,
      detector_model_id: None,
      expected_slots_readable: false,
      eval_report: None,
    };
  }

  let spatial_query_manifest = match read_json_file::<CardDetectionSpatialQueryManifest>(
    spatial_query_manifest_path,
    "balatro card detection spatial query manifest",
  ) {
    Ok(manifest) => manifest,
    Err(error) => {
      warnings.insert(error);
      let source_detection_bundle_dir = semantic_manifest.source_detection_bundle_dir.clone();
      return WitnessGateEvaluation {
        status: CardDetectionEvalWitnessStatus::Failed,
        reason: Some(CardDetectionEvalWitnessReason::QueryManifestParseFailed),
        semantic_manifest: Some(semantic_manifest),
        spatial_query_manifest: None,
        spatial_query_manifest_readable: false,
        source_detection_bundle_dir,
        detector_model_id: None,
        expected_slots_readable: false,
        eval_report: None,
      };
    }
  };

  if !query_lineage_matches_semantic(semantic_manifest_path, &semantic_manifest, &spatial_query_manifest) {
    let source_detection_bundle_dir = semantic_manifest.source_detection_bundle_dir.clone();
    return WitnessGateEvaluation {
      status: CardDetectionEvalWitnessStatus::Blocked,
      reason: Some(CardDetectionEvalWitnessReason::QueryLineageMismatch),
      semantic_manifest: Some(semantic_manifest),
      spatial_query_manifest: Some(spatial_query_manifest),
      spatial_query_manifest_readable: true,
      source_detection_bundle_dir,
      detector_model_id: None,
      expected_slots_readable: false,
      eval_report: None,
    };
  }

  if !expected_slots_path.is_file() {
    let source_detection_bundle_dir = semantic_manifest.source_detection_bundle_dir.clone();
    return WitnessGateEvaluation {
      status: CardDetectionEvalWitnessStatus::Blocked,
      reason: Some(CardDetectionEvalWitnessReason::MissingExpectedSlots),
      semantic_manifest: Some(semantic_manifest),
      spatial_query_manifest: Some(spatial_query_manifest),
      spatial_query_manifest_readable: true,
      source_detection_bundle_dir,
      detector_model_id: None,
      expected_slots_readable: false,
      eval_report: None,
    };
  }

  let expected_slots = match load_expected_slots(expected_slots_path) {
    Ok(slots) => slots,
    Err(error) => {
      warnings.insert(error);
      let source_detection_bundle_dir = semantic_manifest.source_detection_bundle_dir.clone();
      return WitnessGateEvaluation {
        status: CardDetectionEvalWitnessStatus::Failed,
        reason: Some(CardDetectionEvalWitnessReason::ExpectedSlotsParseFailed),
        semantic_manifest: Some(semantic_manifest),
        spatial_query_manifest: Some(spatial_query_manifest),
        spatial_query_manifest_readable: true,
        source_detection_bundle_dir,
        detector_model_id: None,
        expected_slots_readable: false,
        eval_report: None,
      };
    }
  };

  let bundle_dir = PathBuf::from(&semantic_manifest.source_detection_bundle_dir);
  let bundle = match load_detection_bundle(&bundle_dir) {
    Ok(bundle) => bundle,
    Err(error) => {
      warnings.insert(error);
      let source_detection_bundle_dir = semantic_manifest.source_detection_bundle_dir.clone();
      return WitnessGateEvaluation {
        status: CardDetectionEvalWitnessStatus::Failed,
        reason: Some(CardDetectionEvalWitnessReason::BundleUnavailable),
        semantic_manifest: Some(semantic_manifest),
        spatial_query_manifest: Some(spatial_query_manifest),
        spatial_query_manifest_readable: true,
        source_detection_bundle_dir,
        detector_model_id: None,
        expected_slots_readable: true,
        eval_report: None,
      };
    }
  };

  let eval_report = build_eval_report(&bundle, &expected_slots.slots);
  let source_detection_bundle_dir = semantic_manifest.source_detection_bundle_dir.clone();
  WitnessGateEvaluation {
    status: CardDetectionEvalWitnessStatus::Ready,
    reason: None,
    semantic_manifest: Some(semantic_manifest),
    spatial_query_manifest: Some(spatial_query_manifest),
    spatial_query_manifest_readable: true,
    source_detection_bundle_dir,
    detector_model_id: eval_report.detector_model_id.clone(),
    expected_slots_readable: true,
    eval_report: Some(eval_report),
  }
}

fn build_eval_report(bundle: &LoadedDetectionBundle, expected_slots: &[ExpectedSlotEntry]) -> CardDetectionEvalReport {
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
  let unscored_slot_count = expected_slot_count.saturating_sub(scored_slot_count).max(below_confidence_slot_count);

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
  use crate::card_detection_semantic::{CardDetectionSemanticValidationInputs, validate_card_detection_semantic};
  use crate::card_detection_spatial_query::{CardDetectionSpatialQueryInputs, query_card_detection_spatial};
  use crate::model::{ObjectZone, SlotId};
  use std::path::PathBuf;

  fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/balatro_consumption_probe")
  }

  fn semantic_manifest_for(bundle: PathBuf, temp: &tempfile::TempDir) -> PathBuf {
    validate_card_detection_semantic(CardDetectionSemanticValidationInputs {
      bundle_input: bundle,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic")
    .manifest_path
  }

  fn query_manifest_for(semantic_path: &Path, temp: &tempfile::TempDir) -> PathBuf {
    query_card_detection_spatial(CardDetectionSpatialQueryInputs {
      card_detection_semantic_manifest_path: semantic_path.to_path_buf(),
      target_slot: SlotId::new(ObjectZone::Hand, 0),
      output_dir: temp.path().join("query"),
    })
    .expect("query")
    .manifest_path
  }

  fn witness_inputs(bundle: PathBuf, expected_slots_path: PathBuf, temp: &tempfile::TempDir) -> CardDetectionEvalWitnessInputs {
    let semantic_path = semantic_manifest_for(bundle, temp);
    let query_path = query_manifest_for(&semantic_path, temp);
    CardDetectionEvalWitnessInputs {
      card_detection_semantic_manifest_path: semantic_path,
      card_detection_spatial_query_manifest_path: query_path,
      expected_slots_path,
      output_dir: temp.path().join("witness"),
    }
  }

  #[test]
  fn witness_ready_full_coverage() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output = build_card_detection_eval_witness(&witness_inputs(fixture_root(), fixture_root().join("expected_slots.json"), &temp))
      .expect("witness");

    assert_eq!(output.manifest.status, CardDetectionEvalWitnessStatus::Ready);
    assert_eq!(output.manifest.expected_slot_count, 3);
    assert_eq!(output.manifest.scored_slot_count, 3);
    assert_eq!(output.manifest.slot_scores.len(), 3);
    assert!(output.manifest.card_detection_spatial_query_manifest_path.contains("balatro-card-detection-spatial-query.json"));
    assert!(output.manifest_path.exists());
    assert!(output.inspect_report_path.exists());
  }

  #[test]
  fn witness_blocked_when_semantic_not_ready() {
    let temp = tempfile::tempdir().expect("tempdir");
    let semantic_path = semantic_manifest_for(fixture_root().join("broken/empty_detections"), &temp);
    let output = build_card_detection_eval_witness(&CardDetectionEvalWitnessInputs {
      card_detection_semantic_manifest_path: semantic_path,
      card_detection_spatial_query_manifest_path: temp.path().join("missing-query.json"),
      expected_slots_path: fixture_root().join("expected_slots.json"),
      output_dir: temp.path().join("witness"),
    })
    .expect("witness");

    assert_eq!(output.manifest.status, CardDetectionEvalWitnessStatus::Blocked);
    assert_eq!(output.manifest.reason, Some(CardDetectionEvalWitnessReason::SemanticNotReady));
    assert!(output.manifest.slot_scores.is_empty());
  }

  #[test]
  fn witness_failed_when_expected_slots_unreadable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output =
      build_card_detection_eval_witness(&witness_inputs(fixture_root(), fixture_root().join("witness/failed_expected_slots.json"), &temp))
        .expect("witness");

    assert_eq!(output.manifest.status, CardDetectionEvalWitnessStatus::Failed);
    assert_eq!(output.manifest.reason, Some(CardDetectionEvalWitnessReason::ExpectedSlotsParseFailed));
    assert!(output.manifest.slot_scores.is_empty());
  }

  #[test]
  fn witness_blocked_when_query_manifest_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let semantic_path = semantic_manifest_for(fixture_root(), &temp);
    let output = build_card_detection_eval_witness(&CardDetectionEvalWitnessInputs {
      card_detection_semantic_manifest_path: semantic_path,
      card_detection_spatial_query_manifest_path: temp.path().join("missing-query.json"),
      expected_slots_path: fixture_root().join("expected_slots.json"),
      output_dir: temp.path().join("witness"),
    })
    .expect("witness");

    assert_eq!(output.manifest.status, CardDetectionEvalWitnessStatus::Blocked);
    assert_eq!(output.manifest.reason, Some(CardDetectionEvalWitnessReason::MissingQueryManifest));
  }

  #[test]
  fn witness_failed_when_query_manifest_parse_failed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let semantic_path = semantic_manifest_for(fixture_root(), &temp);
    let bad_query = temp.path().join("bad-query.json");
    fs::write(&bad_query, "{not-json").expect("write bad query");
    let output = build_card_detection_eval_witness(&CardDetectionEvalWitnessInputs {
      card_detection_semantic_manifest_path: semantic_path,
      card_detection_spatial_query_manifest_path: bad_query,
      expected_slots_path: fixture_root().join("expected_slots.json"),
      output_dir: temp.path().join("witness"),
    })
    .expect("witness");

    assert_eq!(output.manifest.status, CardDetectionEvalWitnessStatus::Failed);
    assert_eq!(output.manifest.reason, Some(CardDetectionEvalWitnessReason::QueryManifestParseFailed));
  }

  #[test]
  fn witness_failed_when_semantic_manifest_parse_failed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bad_semantic = temp.path().join("bad-semantic.json");
    fs::write(&bad_semantic, "{not-json").expect("write bad semantic");
    let output = build_card_detection_eval_witness(&CardDetectionEvalWitnessInputs {
      card_detection_semantic_manifest_path: bad_semantic,
      card_detection_spatial_query_manifest_path: temp.path().join("missing-query.json"),
      expected_slots_path: fixture_root().join("expected_slots.json"),
      output_dir: temp.path().join("witness"),
    })
    .expect("witness");

    assert_eq!(output.manifest.status, CardDetectionEvalWitnessStatus::Failed);
    assert_eq!(output.manifest.reason, Some(CardDetectionEvalWitnessReason::SemanticFailed));
  }

  #[test]
  fn witness_failed_when_bundle_unavailable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let semantic_path = semantic_manifest_for(fixture_root(), &temp);
    let query_path = query_manifest_for(&semantic_path, &temp);
    let missing_bundle = temp.path().join("missing-bundle");
    let mut semantic_manifest = read_json_file::<CardDetectionSemanticManifest>(&semantic_path, "semantic").expect("read semantic");
    semantic_manifest.source_detection_bundle_dir = missing_bundle.display().to_string();
    let broken_semantic = temp.path().join("broken-semantic.json");
    write_json_file(&broken_semantic, &semantic_manifest).expect("write semantic");
    let mut query_manifest = read_json_file::<CardDetectionSpatialQueryManifest>(&query_path, "query").expect("read query");
    query_manifest.card_detection_semantic_manifest_path = broken_semantic.display().to_string();
    query_manifest.source_detection_bundle_dir = missing_bundle.display().to_string();
    let broken_query = temp.path().join("broken-query.json");
    write_json_file(&broken_query, &query_manifest).expect("write query");
    let output = build_card_detection_eval_witness(&CardDetectionEvalWitnessInputs {
      card_detection_semantic_manifest_path: broken_semantic,
      card_detection_spatial_query_manifest_path: broken_query,
      expected_slots_path: fixture_root().join("expected_slots.json"),
      output_dir: temp.path().join("witness"),
    })
    .expect("witness");

    assert_eq!(output.manifest.status, CardDetectionEvalWitnessStatus::Failed);
    assert_eq!(output.manifest.reason, Some(CardDetectionEvalWitnessReason::BundleUnavailable));
  }

  #[test]
  fn witness_blocked_when_query_lineage_mismatch() {
    let temp = tempfile::tempdir().expect("tempdir");
    let semantic_path = semantic_manifest_for(fixture_root(), &temp);
    let query_path = query_manifest_for(&semantic_path, &temp);
    let mut query_manifest = read_json_file::<CardDetectionSpatialQueryManifest>(&query_path, "query").expect("read query");
    query_manifest.card_detection_semantic_manifest_path = temp.path().join("other-semantic.json").display().to_string();
    let mismatched_query = temp.path().join("mismatched-query.json");
    write_json_file(&mismatched_query, &query_manifest).expect("write query");
    let output = build_card_detection_eval_witness(&CardDetectionEvalWitnessInputs {
      card_detection_semantic_manifest_path: semantic_path,
      card_detection_spatial_query_manifest_path: mismatched_query,
      expected_slots_path: fixture_root().join("expected_slots.json"),
      output_dir: temp.path().join("witness"),
    })
    .expect("witness");

    assert_eq!(output.manifest.status, CardDetectionEvalWitnessStatus::Blocked);
    assert_eq!(output.manifest.reason, Some(CardDetectionEvalWitnessReason::QueryLineageMismatch));
  }

  #[test]
  fn witness_inspect_marks_query_readable_when_semantic_not_ready() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ready_semantic = semantic_manifest_for(fixture_root(), &temp);
    let query_path = query_manifest_for(&ready_semantic, &temp);
    let blocked_semantic = semantic_manifest_for(fixture_root().join("broken/empty_detections"), &temp);
    let output = build_card_detection_eval_witness(&CardDetectionEvalWitnessInputs {
      card_detection_semantic_manifest_path: blocked_semantic,
      card_detection_spatial_query_manifest_path: query_path,
      expected_slots_path: fixture_root().join("expected_slots.json"),
      output_dir: temp.path().join("witness"),
    })
    .expect("witness");

    assert_eq!(output.manifest.status, CardDetectionEvalWitnessStatus::Blocked);
    assert!(output.inspect_report.spatial_query_manifest_readable, "query file readability must be probed independently of semantic gate");
  }
}
