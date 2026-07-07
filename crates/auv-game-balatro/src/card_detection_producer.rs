use std::path::{Path, PathBuf};

use auv_file::{JsonFileReadError, read_json_file as read_json_file_helper};
use auv_inference_common::DetectionSet;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::model::FrameRef;

pub const CARD_DETECTION_BUNDLE_SCHEMA_VERSION: u32 = 1;
pub const DETECTION_BUNDLE_FILE: &str = "detection_bundle.json";
pub const EXPECTED_SLOTS_FILE: &str = "expected_slots.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardDetectionBundleManifest {
  pub schema_version: u32,
  pub frame: FrameRef,
  pub ui_detection_set_path: String,
  pub entities_detection_set_path: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub setup_manifest_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub detector_model_id_ui: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub detector_model_id_entities: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpectedSlotEntry {
  pub zone: String,
  pub index: u32,
  #[serde(default = "default_min_confidence")]
  pub min_confidence: f32,
}

fn default_min_confidence() -> f32 {
  0.25
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpectedSlotsManifest {
  pub schema_version: u32,
  pub slots: Vec<ExpectedSlotEntry>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedDetectionBundle {
  pub bundle_dir: PathBuf,
  pub manifest: CardDetectionBundleManifest,
  pub ui_detections: DetectionSet,
  pub entities_detections: DetectionSet,
}

pub fn resolve_bundle_manifest_path(bundle_input: &Path) -> PathBuf {
  if bundle_input.is_dir() {
    bundle_input.join(DETECTION_BUNDLE_FILE)
  } else {
    bundle_input.to_path_buf()
  }
}

pub fn bundle_dir_from_input(bundle_input: &Path) -> PathBuf {
  if bundle_input.is_dir() {
    bundle_input.to_path_buf()
  } else {
    bundle_input.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
  }
}

pub fn load_detection_bundle(bundle_input: &Path) -> Result<LoadedDetectionBundle, String> {
  let manifest_path = resolve_bundle_manifest_path(bundle_input);
  let bundle_dir = bundle_dir_from_input(bundle_input);

  if !manifest_path.is_file() {
    return Err(format!("missing detection bundle manifest {}", manifest_path.display()));
  }

  let manifest = read_json_file::<CardDetectionBundleManifest>(&manifest_path, "balatro card detection bundle manifest")?;

  if manifest.schema_version != CARD_DETECTION_BUNDLE_SCHEMA_VERSION {
    return Err(format!(
      "unsupported card detection bundle schema_version {} (expected {CARD_DETECTION_BUNDLE_SCHEMA_VERSION})",
      manifest.schema_version
    ));
  }

  if manifest.frame.image_size.width == 0 || manifest.frame.image_size.height == 0 {
    return Err("invalid frame image_size: width and height must be positive".to_string());
  }

  let ui_path = bundle_dir.join(&manifest.ui_detection_set_path);
  let entities_path = bundle_dir.join(&manifest.entities_detection_set_path);
  let ui_detections = read_json_file::<DetectionSet>(&ui_path, "balatro ui detection set")?;
  let entities_detections = read_json_file::<DetectionSet>(&entities_path, "balatro entities detection set")?;

  Ok(LoadedDetectionBundle {
    bundle_dir,
    manifest,
    ui_detections,
    entities_detections,
  })
}

pub fn load_expected_slots(path: &Path) -> Result<ExpectedSlotsManifest, String> {
  read_json_file(path, "balatro expected slots manifest")
}

pub fn total_detection_count(bundle: &LoadedDetectionBundle) -> usize {
  bundle.ui_detections.detections.len() + bundle.entities_detections.detections.len()
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

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::PathBuf;

  fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/balatro_consumption_probe")
  }

  #[test]
  fn loads_positive_detection_bundle_fixture() {
    let bundle = load_detection_bundle(&fixture_root()).expect("bundle");
    assert_eq!(bundle.manifest.schema_version, CARD_DETECTION_BUNDLE_SCHEMA_VERSION);
    assert_eq!(bundle.manifest.frame.image_size.width, 1280);
    assert!(!bundle.ui_detections.detections.is_empty());
    assert!(!bundle.entities_detections.detections.is_empty());
  }

  #[test]
  fn missing_bundle_manifest_returns_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let error = load_detection_bundle(temp.path()).unwrap_err();
    assert!(error.contains("missing detection bundle manifest"));
  }

  #[test]
  fn loads_expected_slots_fixture() {
    let slots = load_expected_slots(&fixture_root().join(EXPECTED_SLOTS_FILE)).expect("slots");
    assert_eq!(slots.slots.len(), 3);
    assert_eq!(slots.slots[0].zone, "hand");
  }
}
