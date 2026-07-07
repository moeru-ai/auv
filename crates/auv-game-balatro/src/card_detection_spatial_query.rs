use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use auv_file::{
  JsonFileReadError, JsonFileWriteError, JsonWriteOptions, read_json_file as read_json_file_helper,
  write_json_file as write_json_file_helper,
};
use auv_inference_common::Detection;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::card_detection_producer::load_detection_bundle;
use crate::card_detection_semantic::{CardDetectionSemanticManifest, CardDetectionSemanticStatus};
use crate::model::{ObjectZone, SlotId};

pub type CardDetectionSpatialQueryResult<T> = Result<T, String>;

pub const CARD_DETECTION_SPATIAL_QUERY_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const CARD_DETECTION_SPATIAL_QUERY_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;

const QUERY_MANIFEST_FILE: &str = "balatro-card-detection-spatial-query.json";
const QUERY_INSPECT_FILE: &str = "balatro-card-detection-spatial-query-inspect.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardDetectionSpatialQueryInputs {
  pub card_detection_semantic_manifest_path: PathBuf,
  pub target_slot: SlotId,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardDetectionSpatialQueryOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub manifest: CardDetectionSpatialQueryManifest,
  pub inspect_report: CardDetectionSpatialQueryInspectReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardDetectionSpatialQueryManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub card_detection_semantic_manifest_path: String,
  pub source_detection_bundle_dir: String,
  pub target_zone: String,
  pub target_index: u32,
  pub query_backend: CardDetectionSpatialQueryBackend,
  pub status: CardDetectionSpatialQueryStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<CardDetectionSpatialQueryReason>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub pixel_x: Option<f32>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub pixel_y: Option<f32>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub image_width: Option<u32>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub image_height: Option<u32>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardDetectionSpatialQueryInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub card_detection_spatial_query_manifest_path: String,
  pub card_detection_semantic_manifest_path: String,
  pub target_zone: String,
  pub target_index: u32,
  pub query_backend: CardDetectionSpatialQueryBackend,
  pub status: CardDetectionSpatialQueryStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<CardDetectionSpatialQueryReason>,
  pub semantic_status: CardDetectionSemanticStatus,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardDetectionSpatialQueryBackend {
  DetectionBundleReference,
}

impl CardDetectionSpatialQueryBackend {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::DetectionBundleReference => "detection_bundle_reference",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardDetectionSpatialQueryStatus {
  Answered,
  Blocked,
  Failed,
}

impl CardDetectionSpatialQueryStatus {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Answered => "answered",
      Self::Blocked => "blocked",
      Self::Failed => "failed",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardDetectionSpatialQueryReason {
  SemanticNotReady,
  TargetSlotNotFound,
  SlotOutOfBounds,
  BundleUnavailable,
}

impl CardDetectionSpatialQueryReason {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::SemanticNotReady => "semantic_not_ready",
      Self::TargetSlotNotFound => "target_slot_not_found",
      Self::SlotOutOfBounds => "slot_out_of_bounds",
      Self::BundleUnavailable => "bundle_unavailable",
    }
  }
}

pub fn query_card_detection_spatial(
  inputs: CardDetectionSpatialQueryInputs,
) -> CardDetectionSpatialQueryResult<CardDetectionSpatialQueryOutput> {
  fs::create_dir_all(&inputs.output_dir).map_err(|error| format!("failed to create output dir {}: {error}", inputs.output_dir.display()))?;

  let generated_at_millis = auv_tracing_driver::now_millis();
  let semantic_manifest = read_json_file::<CardDetectionSemanticManifest>(
    &inputs.card_detection_semantic_manifest_path,
    "balatro card detection semantic manifest",
  )?;

  let mut known_limits = BTreeSet::from([
    "balatro card detection spatial query v1 uses detection bundle reference only; dual-backend compare is deferred".to_string(),
    "pixel answers are source-image coordinates from committed detection bundle, not window-click authority".to_string(),
  ]);
  let mut warnings = BTreeSet::new();

  if semantic_manifest.semantic_status != CardDetectionSemanticStatus::Ready {
    return write_query_output(
      inputs,
      generated_at_millis,
      &semantic_manifest,
      QueryAnswer {
        status: CardDetectionSpatialQueryStatus::Blocked,
        reason: Some(CardDetectionSpatialQueryReason::SemanticNotReady),
        pixel_x: None,
        pixel_y: None,
        image_width: None,
        image_height: None,
      },
      &mut warnings,
      &mut known_limits,
    );
  }

  let bundle_dir = PathBuf::from(&semantic_manifest.source_detection_bundle_dir);
  let bundle = match load_detection_bundle(&bundle_dir) {
    Ok(bundle) => bundle,
    Err(message) => {
      warnings.insert(message);
      return write_query_output(
        inputs,
        generated_at_millis,
        &semantic_manifest,
        QueryAnswer {
          status: CardDetectionSpatialQueryStatus::Failed,
          reason: Some(CardDetectionSpatialQueryReason::BundleUnavailable),
          pixel_x: None,
          pixel_y: None,
          image_width: None,
          image_height: None,
        },
        &mut warnings,
        &mut known_limits,
      );
    }
  };

  let image_width = bundle.manifest.frame.image_size.width;
  let image_height = bundle.manifest.frame.image_size.height;
  let detections = slot_detections(&bundle, inputs.target_slot.zone);
  let Some(detection) = detections.get(inputs.target_slot.index as usize) else {
    return write_query_output(
      inputs,
      generated_at_millis,
      &semantic_manifest,
      QueryAnswer {
        status: CardDetectionSpatialQueryStatus::Blocked,
        reason: Some(CardDetectionSpatialQueryReason::TargetSlotNotFound),
        pixel_x: None,
        pixel_y: None,
        image_width: Some(image_width),
        image_height: Some(image_height),
      },
      &mut warnings,
      &mut known_limits,
    );
  };

  let (pixel_x, pixel_y) = bbox_center(detection);
  if pixel_x < 0.0 || pixel_y < 0.0 || pixel_x > image_width as f32 || pixel_y > image_height as f32 {
    return write_query_output(
      inputs,
      generated_at_millis,
      &semantic_manifest,
      QueryAnswer {
        status: CardDetectionSpatialQueryStatus::Blocked,
        reason: Some(CardDetectionSpatialQueryReason::SlotOutOfBounds),
        pixel_x: Some(pixel_x),
        pixel_y: Some(pixel_y),
        image_width: Some(image_width),
        image_height: Some(image_height),
      },
      &mut warnings,
      &mut known_limits,
    );
  }

  write_query_output(
    inputs,
    generated_at_millis,
    &semantic_manifest,
    QueryAnswer {
      status: CardDetectionSpatialQueryStatus::Answered,
      reason: None,
      pixel_x: Some(pixel_x),
      pixel_y: Some(pixel_y),
      image_width: Some(image_width),
      image_height: Some(image_height),
    },
    &mut warnings,
    &mut known_limits,
  )
}

struct QueryAnswer {
  status: CardDetectionSpatialQueryStatus,
  reason: Option<CardDetectionSpatialQueryReason>,
  pixel_x: Option<f32>,
  pixel_y: Option<f32>,
  image_width: Option<u32>,
  image_height: Option<u32>,
}

pub(crate) fn slot_detections(bundle: &crate::card_detection_producer::LoadedDetectionBundle, zone: ObjectZone) -> Vec<Detection> {
  let mut detections = match zone {
    ObjectZone::Hand => bundle
      .entities_detections
      .detections
      .iter()
      .filter(|detection| detection.label == "poker_card_front" || detection.label == "poker_card_back")
      .cloned()
      .collect(),
    ObjectZone::Joker => bundle.entities_detections.detections.iter().filter(|detection| detection.label == "joker_card").cloned().collect(),
    ObjectZone::Button => {
      bundle.ui_detections.detections.iter().filter(|detection| detection.label.starts_with("button_")).cloned().collect()
    }
    _ => Vec::new(),
  };
  detections.sort_by(|left, right| left.bbox.x1.partial_cmp(&right.bbox.x1).unwrap_or(std::cmp::Ordering::Equal));
  detections
}

fn bbox_center(detection: &Detection) -> (f32, f32) {
  let x = (detection.bbox.x1 + detection.bbox.x2) / 2.0;
  let y = (detection.bbox.y1 + detection.bbox.y2) / 2.0;
  (x, y)
}

fn write_query_output(
  inputs: CardDetectionSpatialQueryInputs,
  generated_at_millis: u64,
  semantic_manifest: &CardDetectionSemanticManifest,
  answer: QueryAnswer,
  warnings: &mut BTreeSet<String>,
  known_limits: &mut BTreeSet<String>,
) -> CardDetectionSpatialQueryResult<CardDetectionSpatialQueryOutput> {
  known_limits.extend(semantic_manifest.known_limits.iter().cloned());

  let manifest = CardDetectionSpatialQueryManifest {
    schema_version: CARD_DETECTION_SPATIAL_QUERY_MANIFEST_SCHEMA_VERSION,
    generated_at_millis,
    card_detection_semantic_manifest_path: inputs.card_detection_semantic_manifest_path.display().to_string(),
    source_detection_bundle_dir: semantic_manifest.source_detection_bundle_dir.clone(),
    target_zone: inputs.target_slot.zone.as_str().to_string(),
    target_index: inputs.target_slot.index,
    query_backend: CardDetectionSpatialQueryBackend::DetectionBundleReference,
    status: answer.status,
    reason: answer.reason,
    pixel_x: answer.pixel_x,
    pixel_y: answer.pixel_y,
    image_width: answer.image_width,
    image_height: answer.image_height,
    known_limits: known_limits.iter().cloned().collect(),
  };

  let manifest_path = inputs.output_dir.join(QUERY_MANIFEST_FILE);
  write_json_file(&manifest_path, &manifest)?;

  let inspect_report = CardDetectionSpatialQueryInspectReport {
    schema_version: CARD_DETECTION_SPATIAL_QUERY_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    card_detection_spatial_query_manifest_path: manifest_path.display().to_string(),
    card_detection_semantic_manifest_path: manifest.card_detection_semantic_manifest_path.clone(),
    target_zone: manifest.target_zone.clone(),
    target_index: manifest.target_index,
    query_backend: manifest.query_backend,
    status: manifest.status,
    reason: manifest.reason,
    semantic_status: semantic_manifest.semantic_status,
    warnings: warnings.iter().cloned().collect(),
    known_limits: manifest.known_limits.clone(),
  };

  let inspect_report_path = inputs.output_dir.join(QUERY_INSPECT_FILE);
  write_json_file(&inspect_report_path, &inspect_report)?;

  Ok(CardDetectionSpatialQueryOutput {
    output_dir: inputs.output_dir,
    manifest_path,
    inspect_report_path,
    manifest,
    inspect_report,
  })
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

  #[test]
  fn positive_hand_slot_query_yields_answered_with_center() {
    let temp = tempfile::tempdir().expect("tempdir");
    let semantic_path = semantic_manifest_for(fixture_root(), &temp);
    let output = query_card_detection_spatial(CardDetectionSpatialQueryInputs {
      card_detection_semantic_manifest_path: semantic_path,
      target_slot: SlotId::new(ObjectZone::Hand, 0),
      output_dir: temp.path().join("query"),
    })
    .expect("query");
    assert_eq!(output.manifest.status, CardDetectionSpatialQueryStatus::Answered);
    assert!(output.manifest.pixel_x.is_some());
    assert!(output.manifest.pixel_y.is_some());
  }

  #[test]
  fn missing_target_slot_yields_blocked() {
    let temp = tempfile::tempdir().expect("tempdir");
    let semantic_path = semantic_manifest_for(fixture_root().join("query/missing_target_slot"), &temp);
    let output = query_card_detection_spatial(CardDetectionSpatialQueryInputs {
      card_detection_semantic_manifest_path: semantic_path,
      target_slot: SlotId::new(ObjectZone::Hand, 99),
      output_dir: temp.path().join("query"),
    })
    .expect("query");
    assert_eq!(output.manifest.status, CardDetectionSpatialQueryStatus::Blocked);
    assert_eq!(output.manifest.reason, Some(CardDetectionSpatialQueryReason::TargetSlotNotFound));
  }

  #[test]
  fn out_of_bounds_slot_yields_blocked() {
    let temp = tempfile::tempdir().expect("tempdir");
    let semantic_path = semantic_manifest_for(fixture_root().join("query/out_of_bounds"), &temp);
    let output = query_card_detection_spatial(CardDetectionSpatialQueryInputs {
      card_detection_semantic_manifest_path: semantic_path,
      target_slot: SlotId::new(ObjectZone::Hand, 0),
      output_dir: temp.path().join("query"),
    })
    .expect("query");
    assert_eq!(output.manifest.status, CardDetectionSpatialQueryStatus::Blocked);
    assert_eq!(output.manifest.reason, Some(CardDetectionSpatialQueryReason::SlotOutOfBounds));
  }
}
