use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use auv_file::{JsonFileWriteError, JsonWriteOptions, write_json_file as write_json_file_helper};
use auv_stage_status::StageStatus;
use serde::{Deserialize, Serialize};

use crate::card_detection_producer::{LoadedDetectionBundle, load_detection_bundle, resolve_bundle_manifest_path, total_detection_count};

pub type CardDetectionSemanticValidationResult<T> = Result<T, String>;

pub const CARD_DETECTION_SEMANTIC_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const CARD_DETECTION_SEMANTIC_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;

const SEMANTIC_MANIFEST_FILE: &str = "balatro-card-detection-semantic.json";
const SEMANTIC_INSPECT_FILE: &str = "balatro-card-detection-semantic-inspect.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardDetectionSemanticValidationInputs {
  pub bundle_input: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardDetectionSemanticValidationOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub manifest: CardDetectionSemanticManifest,
  pub inspect_report: CardDetectionSemanticInspectReport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardDetectionSemanticManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub source_detection_bundle_path: String,
  pub source_detection_bundle_dir: String,
  pub frame_source: String,
  pub image_width: u32,
  pub image_height: u32,
  pub ui_detection_count: usize,
  pub entities_detection_count: usize,
  pub semantic_status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub semantic_reason: Option<CardDetectionSemanticReason>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardDetectionSemanticInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub card_detection_semantic_manifest_path: String,
  pub source_detection_bundle_path: String,
  pub source_detection_bundle_dir: String,
  pub semantic_status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub semantic_reason: Option<CardDetectionSemanticReason>,
  pub detection_bundle_readable: bool,
  pub detection_sets_non_empty: bool,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardDetectionSemanticReason {
  MissingDetectionBundle,
  BundleParseFailed,
  EmptyDetections,
  InvalidFrameRef,
  UnsupportedSchemaVersion,
}

impl CardDetectionSemanticReason {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::MissingDetectionBundle => "missing_detection_bundle",
      Self::BundleParseFailed => "bundle_parse_failed",
      Self::EmptyDetections => "empty_detections",
      Self::InvalidFrameRef => "invalid_frame_ref",
      Self::UnsupportedSchemaVersion => "unsupported_schema_version",
    }
  }
}

pub fn validate_card_detection_semantic(
  inputs: CardDetectionSemanticValidationInputs,
) -> CardDetectionSemanticValidationResult<CardDetectionSemanticValidationOutput> {
  fs::create_dir_all(&inputs.output_dir).map_err(|error| format!("failed to create output dir {}: {error}", inputs.output_dir.display()))?;

  let generated_at_millis = auv_tracing_driver::now_millis();
  let manifest_path = resolve_bundle_manifest_path(&inputs.bundle_input);
  let known_limits = BTreeSet::from([
    "balatro card detection semantic gate closes fixture consumability only; it does not grade detection quality or claim live gameplay authority".to_string(),
    "coordinates remain source-image pixels from committed detection bundle artifacts".to_string(),
  ]);
  let mut warnings = BTreeSet::new();

  let gate = evaluate_semantic_gate(&inputs.bundle_input, &mut warnings);

  let (frame_source, image_width, image_height, ui_count, entities_count) = gate
    .bundle
    .as_ref()
    .map(|bundle| {
      (
        bundle.manifest.frame.source.clone(),
        bundle.manifest.frame.image_size.width,
        bundle.manifest.frame.image_size.height,
        bundle.ui_detections.detections.len(),
        bundle.entities_detections.detections.len(),
      )
    })
    .unwrap_or((String::new(), 0, 0, 0, 0));

  let manifest = CardDetectionSemanticManifest {
    schema_version: CARD_DETECTION_SEMANTIC_MANIFEST_SCHEMA_VERSION,
    generated_at_millis,
    source_detection_bundle_path: manifest_path.display().to_string(),
    source_detection_bundle_dir: inputs.bundle_input.display().to_string(),
    frame_source,
    image_width,
    image_height,
    ui_detection_count: ui_count,
    entities_detection_count: entities_count,
    semantic_status: gate.semantic_status,
    semantic_reason: gate.semantic_reason,
    known_limits: known_limits.into_iter().collect(),
  };

  let manifest_out = inputs.output_dir.join(SEMANTIC_MANIFEST_FILE);
  write_json_file(&manifest_out, &manifest)?;

  let inspect_report = CardDetectionSemanticInspectReport {
    schema_version: CARD_DETECTION_SEMANTIC_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    card_detection_semantic_manifest_path: manifest_out.display().to_string(),
    source_detection_bundle_path: manifest.source_detection_bundle_path.clone(),
    source_detection_bundle_dir: manifest.source_detection_bundle_dir.clone(),
    semantic_status: manifest.semantic_status,
    semantic_reason: manifest.semantic_reason,
    detection_bundle_readable: gate.bundle.is_some(),
    detection_sets_non_empty: gate.detection_sets_non_empty,
    warnings: warnings.into_iter().collect(),
    known_limits: manifest.known_limits.clone(),
  };

  let inspect_report_path = inputs.output_dir.join(SEMANTIC_INSPECT_FILE);
  write_json_file(&inspect_report_path, &inspect_report)?;

  Ok(CardDetectionSemanticValidationOutput {
    output_dir: inputs.output_dir,
    manifest_path: manifest_out,
    inspect_report_path,
    manifest,
    inspect_report,
  })
}

struct SemanticGateEvaluation {
  semantic_status: StageStatus,
  semantic_reason: Option<CardDetectionSemanticReason>,
  bundle: Option<LoadedDetectionBundle>,
  detection_sets_non_empty: bool,
}

fn evaluate_semantic_gate(bundle_input: &Path, warnings: &mut BTreeSet<String>) -> SemanticGateEvaluation {
  let manifest_path = resolve_bundle_manifest_path(bundle_input);
  if !manifest_path.is_file() {
    return SemanticGateEvaluation {
      semantic_status: StageStatus::Blocked,
      semantic_reason: Some(CardDetectionSemanticReason::MissingDetectionBundle),
      bundle: None,
      detection_sets_non_empty: false,
    };
  }

  match load_detection_bundle(bundle_input) {
    Ok(bundle) => {
      if total_detection_count(&bundle) == 0 {
        return SemanticGateEvaluation {
          semantic_status: StageStatus::Blocked,
          semantic_reason: Some(CardDetectionSemanticReason::EmptyDetections),
          bundle: Some(bundle),
          detection_sets_non_empty: false,
        };
      }
      SemanticGateEvaluation {
        semantic_status: StageStatus::Ready,
        semantic_reason: None,
        detection_sets_non_empty: true,
        bundle: Some(bundle),
      }
    }
    Err(error) => {
      warnings.insert(error.clone());
      let reason = if error.contains("invalid frame image_size") {
        CardDetectionSemanticReason::InvalidFrameRef
      } else if error.contains("unsupported card detection bundle schema_version") {
        CardDetectionSemanticReason::UnsupportedSchemaVersion
      } else {
        CardDetectionSemanticReason::BundleParseFailed
      };
      SemanticGateEvaluation {
        semantic_status: StageStatus::Failed,
        semantic_reason: Some(reason),
        bundle: None,
        detection_sets_non_empty: false,
      }
    }
  }
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
  use std::path::PathBuf;

  fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/balatro_consumption_probe")
  }

  #[test]
  fn stage_status_preserves_wire_labels() {
    for (status, wire) in [
      (StageStatus::Ready, "\"ready\""),
      (StageStatus::Blocked, "\"blocked\""),
      (StageStatus::Failed, "\"failed\""),
    ] {
      assert_eq!(serde_json::to_string(&status).unwrap(), wire);
    }
  }

  #[test]
  fn positive_fixture_yields_ready() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output = validate_card_detection_semantic(CardDetectionSemanticValidationInputs {
      bundle_input: fixture_root(),
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic");
    assert_eq!(output.manifest.semantic_status, StageStatus::Ready);
    assert!(output.manifest_path.exists());
  }

  #[test]
  fn missing_bundle_yields_blocked() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output = validate_card_detection_semantic(CardDetectionSemanticValidationInputs {
      bundle_input: temp.path().join("missing"),
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic");
    assert_eq!(output.manifest.semantic_status, StageStatus::Blocked);
    assert_eq!(output.manifest.semantic_reason, Some(CardDetectionSemanticReason::MissingDetectionBundle));
  }

  #[test]
  fn empty_detections_yields_blocked() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bundle = fixture_root().join("broken/empty_detections");
    let output = validate_card_detection_semantic(CardDetectionSemanticValidationInputs {
      bundle_input: bundle,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic");
    assert_eq!(output.manifest.semantic_status, StageStatus::Blocked);
    assert_eq!(output.manifest.semantic_reason, Some(CardDetectionSemanticReason::EmptyDetections));
  }

  #[test]
  fn bad_schema_yields_failed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bundle = fixture_root().join("broken/bad_schema");
    let output = validate_card_detection_semantic(CardDetectionSemanticValidationInputs {
      bundle_input: bundle,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic");
    assert_eq!(output.manifest.semantic_status, StageStatus::Failed);
    assert_eq!(output.manifest.semantic_reason, Some(CardDetectionSemanticReason::BundleParseFailed));
  }
}
