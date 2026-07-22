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

use crate::projection::ProjectionArtifact;
use crate::visual_truth::VisualTruthManifest;

pub type VisualTruthSemanticValidationResult<T> = Result<T, String>;

pub const VISUAL_TRUTH_SEMANTIC_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const VISUAL_TRUTH_SEMANTIC_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;
pub const OSU_VISUAL_TRUTH_SEMANTIC_PURPOSE: &str = "auv.osu.visual_truth.semantic";

const VISUAL_TRUTH_MANIFEST_FILE: &str = "visual_truth_manifest.json";
const PROJECTION_FILE: &str = "projection.json";
const SEMANTIC_MANIFEST_FILE: &str = "osu-visual-truth-semantic.json";
const SEMANTIC_INSPECT_FILE: &str = "osu-visual-truth-semantic-inspect.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualTruthSemanticValidationInputs {
  pub run_artifact_dir: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisualTruthSemanticValidationOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub manifest: VisualTruthSemanticManifest,
  pub inspect_report: VisualTruthSemanticInspectReport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisualTruthSemanticManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub source_run_artifact_dir: String,
  pub source_visual_truth_manifest_path: String,
  pub source_projection_path: String,
  pub beatmap_path: String,
  pub frame_count: usize,
  pub semantic_status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub semantic_reason: Option<VisualTruthSemanticReason>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisualTruthSemanticInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub visual_truth_semantic_manifest_path: String,
  pub source_run_artifact_dir: String,
  pub source_visual_truth_manifest_path: String,
  pub source_projection_path: String,
  pub beatmap_path: String,
  pub frame_count: usize,
  pub semantic_status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub semantic_reason: Option<VisualTruthSemanticReason>,
  pub visual_truth_manifest_readable: bool,
  pub projection_readable: bool,
  pub projection_eval_ready: bool,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualTruthSemanticReason {
  MissingVisualTruthManifest,
  MissingProjection,
  EmptyFrames,
  NormalizedPathsInvalid,
  VisualTruthManifestParseFailed,
  ProjectionParseFailed,
  ProjectionNonFinite,
}

impl VisualTruthSemanticReason {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::MissingVisualTruthManifest => "missing_visual_truth_manifest",
      Self::MissingProjection => "missing_projection",
      Self::EmptyFrames => "empty_frames",
      Self::NormalizedPathsInvalid => "normalized_paths_invalid",
      Self::VisualTruthManifestParseFailed => "visual_truth_manifest_parse_failed",
      Self::ProjectionParseFailed => "projection_parse_failed",
      Self::ProjectionNonFinite => "projection_non_finite",
    }
  }
}

pub async fn publish_osu_visual_truth_semantic(
  context: Option<&Context>,
  semantic: &VisualTruthSemanticManifest,
) -> Result<Option<ArtifactMetadata>, crate::run_read::OsuArtifactPublishError> {
  crate::run_read::publish_json_artifact(context, OSU_VISUAL_TRUTH_SEMANTIC_PURPOSE, semantic, validate_semantic_payload).await
}

pub async fn read_osu_visual_truth_semantic(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<VisualTruthSemanticManifest, crate::run_read::OsuArtifactReadError> {
  crate::run_read::read_json_artifact(store, snapshot, uri, OSU_VISUAL_TRUTH_SEMANTIC_PURPOSE, validate_semantic_payload).await
}

fn validate_semantic_payload(semantic: &VisualTruthSemanticManifest) -> Result<(), String> {
  if semantic.schema_version != VISUAL_TRUTH_SEMANTIC_MANIFEST_SCHEMA_VERSION {
    return Err(format!(
      "unsupported osu! visual truth semantic schema_version {} (expected {VISUAL_TRUTH_SEMANTIC_MANIFEST_SCHEMA_VERSION})",
      semantic.schema_version
    ));
  }
  if semantic.semantic_status == StageStatus::Ready {
    if semantic.frame_count == 0 {
      return Err("ready visual truth semantic payload must contain at least one frame".to_string());
    }
    if semantic.semantic_reason.is_some() {
      return Err("ready visual truth semantic payload must not include a failure reason".to_string());
    }
  }
  Ok(())
}

pub fn validate_visual_truth_semantic(
  inputs: VisualTruthSemanticValidationInputs,
) -> VisualTruthSemanticValidationResult<VisualTruthSemanticValidationOutput> {
  fs::create_dir_all(&inputs.output_dir).map_err(|error| format!("failed to create output dir {}: {error}", inputs.output_dir.display()))?;

  let generated_at_millis = crate::run_read::now_millis();
  let source_visual_truth_manifest_path = inputs.run_artifact_dir.join(VISUAL_TRUTH_MANIFEST_FILE);
  let source_projection_path = inputs.run_artifact_dir.join(PROJECTION_FILE);

  let known_limits = BTreeSet::from([
    "osu visual truth semantic gate closes benchmark artifact consumability only; it does not grade detection quality or claim window-click authority".to_string(),
    "coordinates remain source-image pixels from benchmark capture artifacts".to_string(),
  ]);
  let mut warnings = BTreeSet::new();

  let gate = evaluate_semantic_gate(&inputs.run_artifact_dir, &source_visual_truth_manifest_path, &source_projection_path, &mut warnings);

  let beatmap_path = gate.visual_truth_manifest.as_ref().map(|manifest| manifest.beatmap_path.clone()).unwrap_or_default();
  let frame_count = gate.visual_truth_manifest.as_ref().map(|manifest| manifest.frames.len()).unwrap_or(0);

  let manifest = VisualTruthSemanticManifest {
    schema_version: VISUAL_TRUTH_SEMANTIC_MANIFEST_SCHEMA_VERSION,
    generated_at_millis,
    source_run_artifact_dir: inputs.run_artifact_dir.display().to_string(),
    source_visual_truth_manifest_path: source_visual_truth_manifest_path.display().to_string(),
    source_projection_path: source_projection_path.display().to_string(),
    beatmap_path,
    frame_count,
    semantic_status: gate.semantic_status,
    semantic_reason: gate.semantic_reason,
    known_limits: known_limits.into_iter().collect(),
  };

  let manifest_path = inputs.output_dir.join(SEMANTIC_MANIFEST_FILE);
  write_json_file(&manifest_path, &manifest)?;

  let inspect_report = VisualTruthSemanticInspectReport {
    schema_version: VISUAL_TRUTH_SEMANTIC_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    visual_truth_semantic_manifest_path: manifest_path.display().to_string(),
    source_run_artifact_dir: manifest.source_run_artifact_dir.clone(),
    source_visual_truth_manifest_path: manifest.source_visual_truth_manifest_path.clone(),
    source_projection_path: manifest.source_projection_path.clone(),
    beatmap_path: manifest.beatmap_path.clone(),
    frame_count: manifest.frame_count,
    semantic_status: manifest.semantic_status,
    semantic_reason: manifest.semantic_reason,
    visual_truth_manifest_readable: gate.visual_truth_manifest.is_some(),
    projection_readable: gate.projection_artifact.is_some(),
    projection_eval_ready: gate.projection_eval_ready,
    warnings: warnings.into_iter().collect(),
    known_limits: manifest.known_limits.clone(),
  };

  let inspect_report_path = inputs.output_dir.join(SEMANTIC_INSPECT_FILE);
  write_json_file(&inspect_report_path, &inspect_report)?;

  Ok(VisualTruthSemanticValidationOutput {
    output_dir: inputs.output_dir,
    manifest_path,
    inspect_report_path,
    manifest,
    inspect_report,
  })
}

struct SemanticGateEvaluation {
  semantic_status: StageStatus,
  semantic_reason: Option<VisualTruthSemanticReason>,
  visual_truth_manifest: Option<VisualTruthManifest>,
  projection_artifact: Option<ProjectionArtifact>,
  projection_eval_ready: bool,
}

fn evaluate_semantic_gate(
  run_artifact_dir: &Path,
  visual_truth_manifest_path: &Path,
  projection_path: &Path,
  warnings: &mut BTreeSet<String>,
) -> SemanticGateEvaluation {
  if path_is_symlink(run_artifact_dir) || path_is_symlink(visual_truth_manifest_path) || path_is_symlink(projection_path) {
    return SemanticGateEvaluation {
      semantic_status: StageStatus::Blocked,
      semantic_reason: Some(VisualTruthSemanticReason::NormalizedPathsInvalid),
      visual_truth_manifest: None,
      projection_artifact: None,
      projection_eval_ready: false,
    };
  }

  if !visual_truth_manifest_path.is_file() {
    return SemanticGateEvaluation {
      semantic_status: StageStatus::Blocked,
      semantic_reason: Some(VisualTruthSemanticReason::MissingVisualTruthManifest),
      visual_truth_manifest: None,
      projection_artifact: None,
      projection_eval_ready: false,
    };
  }

  if !projection_path.is_file() {
    return SemanticGateEvaluation {
      semantic_status: StageStatus::Blocked,
      semantic_reason: Some(VisualTruthSemanticReason::MissingProjection),
      visual_truth_manifest: None,
      projection_artifact: None,
      projection_eval_ready: false,
    };
  }

  let visual_truth_manifest = match read_json_file::<VisualTruthManifest>(visual_truth_manifest_path, "osu visual truth manifest") {
    Ok(manifest) => Some(manifest),
    Err(error) => {
      warnings.insert(error);
      return SemanticGateEvaluation {
        semantic_status: StageStatus::Failed,
        semantic_reason: Some(VisualTruthSemanticReason::VisualTruthManifestParseFailed),
        visual_truth_manifest: None,
        projection_artifact: None,
        projection_eval_ready: false,
      };
    }
  };

  let projection_artifact = match read_json_file::<ProjectionArtifact>(projection_path, "osu projection artifact") {
    Ok(projection) => Some(projection),
    Err(error) => {
      warnings.insert(error);
      return SemanticGateEvaluation {
        semantic_status: StageStatus::Failed,
        semantic_reason: Some(VisualTruthSemanticReason::ProjectionParseFailed),
        visual_truth_manifest,
        projection_artifact: None,
        projection_eval_ready: false,
      };
    }
  };

  let projection_eval_ready = projection_artifact.as_ref().and_then(|projection| projection.to_eval_projection().ok()).is_some();

  if !projection_eval_ready {
    return SemanticGateEvaluation {
      semantic_status: StageStatus::Failed,
      semantic_reason: Some(VisualTruthSemanticReason::ProjectionNonFinite),
      visual_truth_manifest,
      projection_artifact,
      projection_eval_ready: false,
    };
  }

  let Some(manifest) = visual_truth_manifest.as_ref() else {
    return SemanticGateEvaluation {
      semantic_status: StageStatus::Failed,
      semantic_reason: Some(VisualTruthSemanticReason::VisualTruthManifestParseFailed),
      visual_truth_manifest,
      projection_artifact,
      projection_eval_ready: true,
    };
  };

  if manifest.frames.is_empty() {
    return SemanticGateEvaluation {
      semantic_status: StageStatus::Blocked,
      semantic_reason: Some(VisualTruthSemanticReason::EmptyFrames),
      visual_truth_manifest: Some(manifest.clone()),
      projection_artifact,
      projection_eval_ready: true,
    };
  }

  SemanticGateEvaluation {
    semantic_status: StageStatus::Ready,
    semantic_reason: None,
    visual_truth_manifest: Some(manifest.clone()),
    projection_artifact,
    projection_eval_ready: true,
  }
}

fn path_is_symlink(path: &Path) -> bool {
  fs::symlink_metadata(path).ok().is_some_and(|metadata| metadata.file_type().is_symlink())
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
  use std::path::PathBuf;

  use super::*;
  use crate::benchmark::{CapturePhase, MapSummary, ObjectKind};
  use crate::projection::{PlayfieldProjection, ProjectionArtifact, ProjectionDerivationMethod};
  use crate::visual_truth::{CaptureFrame, ExpectedObjectTruth, VisualTruthFrame};

  #[test]
  fn stage_status_preserves_wire_labels() {
    for (status, wire) in [
      (StageStatus::Ready, "\"ready\""),
      (StageStatus::Blocked, "\"blocked\""),
      (StageStatus::Failed, "\"failed\""),
    ] {
      assert_eq!(serde_json::to_string(&status).expect("serialize"), wire);
      let decoded: StageStatus = serde_json::from_str(wire).expect("deserialize");
      assert_eq!(decoded, status);
    }
  }

  fn write_probe_fixture(root: &Path) {
    let manifest = VisualTruthManifest {
      schema_version: 1,
      beatmap_path: "tests/fixtures/probe.osu".to_string(),
      map_summary: MapSummary {
        beatmap_path: "tests/fixtures/probe.osu".to_string(),
        mode: 0,
        total_objects: 1,
        circle_count: 1,
        slider_count: 0,
        spinner_count: 0,
        hold_count: 0,
        first_object_time_ms: Some(1000),
        last_object_time_ms: Some(1000),
        approach_rate: 8.0,
        overall_difficulty: 7.0,
        circle_size: 4.0,
        hp_drain_rate: 5.0,
      },
      frames: vec![VisualTruthFrame {
        object_index: 0,
        scheduled_time_ms: 1000,
        actual_dispatch_time_ms: 1001,
        dispatch_error_ms: 1,
        capture: CaptureFrame {
          phase: CapturePhase::BeforeDispatch,
          capture_time_ms: 990,
          relative_to_scheduled_ms: -10,
          relative_to_dispatch_ms: -11,
          file_name: "capture-object-0000-before-16ms.png".to_string(),
          width: 800,
          height: 600,
          backend: "fixture".to_string(),
          fallback_reason: None,
        },
        expected_object: ExpectedObjectTruth {
          object_kind: ObjectKind::Circle,
          expected_playfield_x: 256.0,
          expected_playfield_y: 192.0,
          circle_size: 4.0,
          approach_rate: 8.0,
          overall_difficulty: 7.0,
        },
      }],
    };
    let projection = PlayfieldProjection::for_capture(800.0, 600.0, 4.0).expect("projection");
    let projection_artifact = ProjectionArtifact {
      source_window_bounds: crate::projection::ProjectionBounds {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
      },
      capture_bounds: Some(crate::projection::ProjectionBounds {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
      }),
      capture_width: Some(800),
      capture_height: Some(600),
      capture_scale_factor: Some(1.0),
      scale_x: projection.scale_x,
      scale_y: projection.scale_y,
      offset_x: projection.offset_x,
      offset_y: projection.offset_y,
      match_radius_px: projection.match_radius_px,
      derivation_method: ProjectionDerivationMethod::LayoutRule,
      verification_reference: None,
    };

    write_json_file(&root.join(VISUAL_TRUTH_MANIFEST_FILE), &manifest).expect("manifest");
    write_json_file(&root.join(PROJECTION_FILE), &projection_artifact).expect("projection");
  }

  #[test]
  fn semantic_validation_ready_on_probe_fixture() {
    let root = tempfile::tempdir().expect("tempdir");
    write_probe_fixture(root.path());

    let output = validate_visual_truth_semantic(VisualTruthSemanticValidationInputs {
      run_artifact_dir: root.path().to_path_buf(),
      output_dir: root.path().join("semantic-out"),
    })
    .expect("semantic validation should succeed");

    assert_eq!(output.manifest.semantic_status, StageStatus::Ready);
    assert_eq!(output.manifest.frame_count, 1);
    assert!(output.manifest_path.exists());
    assert!(output.inspect_report_path.exists());
  }

  #[test]
  fn semantic_validation_blocked_when_projection_missing() {
    let root = tempfile::tempdir().expect("tempdir");
    write_probe_fixture(root.path());
    fs::remove_file(root.path().join(PROJECTION_FILE)).expect("remove projection");

    let output = validate_visual_truth_semantic(VisualTruthSemanticValidationInputs {
      run_artifact_dir: root.path().to_path_buf(),
      output_dir: root.path().join("semantic-out"),
    })
    .expect("semantic validation should still write artifacts");

    assert_eq!(output.manifest.semantic_status, StageStatus::Blocked);
    assert_eq!(output.manifest.semantic_reason, Some(VisualTruthSemanticReason::MissingProjection));
  }

  #[test]
  fn committed_fixture_path_is_available() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root.join("tests/fixtures/osu_visual_truth_probe");
    assert!(fixture_root.join(VISUAL_TRUTH_MANIFEST_FILE).is_file());
    assert!(fixture_root.join(PROJECTION_FILE).is_file());
  }
}
