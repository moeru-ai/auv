use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use auv_file::{
  JsonFileReadError, JsonFileWriteError, JsonWriteOptions, read_json_file as read_json_file_helper,
  write_json_file as write_json_file_helper,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::benchmark::{CapturePhase, ObjectKind};
use crate::projection::ProjectionArtifact;
use crate::visual_eval::{EvalProjection, pixel_point_inside_capture, project_playfield_point};
use crate::visual_truth::{VisualTruthFrame, VisualTruthManifest};
use crate::visual_truth_semantic::{VisualTruthSemanticManifest, VisualTruthSemanticStatus};

pub type VisualTruthSpatialQueryResult<T> = Result<T, String>;

pub const VISUAL_TRUTH_SPATIAL_QUERY_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const VISUAL_TRUTH_SPATIAL_QUERY_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;

const QUERY_MANIFEST_FILE: &str = "osu-visual-truth-spatial-query.json";
const QUERY_INSPECT_FILE: &str = "osu-visual-truth-spatial-query-inspect.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualTruthSpatialQueryInputs {
  pub visual_truth_semantic_manifest_path: PathBuf,
  pub object_index: usize,
  pub capture_phase: CapturePhase,
  pub object_kind: Option<ObjectKind>,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VisualTruthSpatialQueryOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub manifest: VisualTruthSpatialQueryManifest,
  pub inspect_report: VisualTruthSpatialQueryInspectReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VisualTruthSpatialQueryManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub visual_truth_semantic_manifest_path: String,
  pub source_run_artifact_dir: String,
  pub source_visual_truth_manifest_path: String,
  pub source_projection_path: String,
  pub object_index: usize,
  pub capture_phase: CapturePhase,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub object_kind: Option<ObjectKind>,
  pub query_backend: VisualTruthSpatialQueryBackend,
  pub status: VisualTruthSpatialQueryStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<VisualTruthSpatialQueryReason>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub pixel_visibility: Option<VisualTruthPixelVisibility>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub pixel_x: Option<f32>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub pixel_y: Option<f32>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub match_radius_px: Option<f32>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub capture_width: Option<u32>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub capture_height: Option<u32>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisualTruthSpatialQueryInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub visual_truth_spatial_query_manifest_path: String,
  pub visual_truth_semantic_manifest_path: String,
  pub source_run_artifact_dir: String,
  pub object_index: usize,
  pub capture_phase: CapturePhase,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub object_kind: Option<ObjectKind>,
  pub query_backend: VisualTruthSpatialQueryBackend,
  pub status: VisualTruthSpatialQueryStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<VisualTruthSpatialQueryReason>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub pixel_visibility: Option<VisualTruthPixelVisibility>,
  pub semantic_status: VisualTruthSemanticStatus,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualTruthSpatialQueryBackend {
  PlayfieldProjectionReference,
}

impl VisualTruthSpatialQueryBackend {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::PlayfieldProjectionReference => "playfield_projection_reference",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualTruthSpatialQueryStatus {
  Answered,
  Blocked,
  Failed,
}

impl VisualTruthSpatialQueryStatus {
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
pub enum VisualTruthSpatialQueryReason {
  SemanticSourceNotReady,
  TargetAbsentFromVisualTruth,
  ProjectionUnavailable,
}

impl VisualTruthSpatialQueryReason {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::SemanticSourceNotReady => "semantic_source_not_ready",
      Self::TargetAbsentFromVisualTruth => "target_absent_from_visual_truth",
      Self::ProjectionUnavailable => "projection_unavailable",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualTruthPixelVisibility {
  InsideCapture,
  OutsideCapture,
}

impl VisualTruthPixelVisibility {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::InsideCapture => "inside_capture",
      Self::OutsideCapture => "outside_capture",
    }
  }
}

pub fn query_visual_truth_spatial(
  inputs: VisualTruthSpatialQueryInputs,
) -> VisualTruthSpatialQueryResult<VisualTruthSpatialQueryOutput> {
  fs::create_dir_all(&inputs.output_dir).map_err(|error| {
    format!(
      "failed to create output dir {}: {error}",
      inputs.output_dir.display()
    )
  })?;

  let generated_at_millis = auv_tracing_driver::now_millis();
  let semantic_manifest = read_json_file::<VisualTruthSemanticManifest>(
    &inputs.visual_truth_semantic_manifest_path,
    "osu visual truth semantic manifest",
  )?;

  let mut known_limits = BTreeSet::from([
    "osu visual truth spatial query v1 uses playfield projection reference only; dual-backend compare is deferred".to_string(),
    "pixel answers are source-image coordinates from benchmark capture, not window-click authority".to_string(),
  ]);
  let mut warnings = BTreeSet::new();

  if semantic_manifest.semantic_status != VisualTruthSemanticStatus::Ready {
    return write_query_output(
      inputs,
      generated_at_millis,
      &semantic_manifest,
      QueryAnswer {
        status: VisualTruthSpatialQueryStatus::Blocked,
        reason: Some(VisualTruthSpatialQueryReason::SemanticSourceNotReady),
        pixel_visibility: None,
        pixel_x: None,
        pixel_y: None,
        match_radius_px: None,
        capture_width: None,
        capture_height: None,
      },
      &mut warnings,
      &mut known_limits,
    );
  }

  let visual_truth_manifest = read_json_file::<VisualTruthManifest>(
    Path::new(&semantic_manifest.source_visual_truth_manifest_path),
    "osu visual truth manifest",
  )?;
  let projection_artifact = read_json_file::<ProjectionArtifact>(
    Path::new(&semantic_manifest.source_projection_path),
    "osu projection artifact",
  )?;
  let projection = match projection_artifact.to_eval_projection() {
    Ok(projection) => projection,
    Err(message) => {
      warnings.insert(message.clone());
      return write_query_output(
        inputs,
        generated_at_millis,
        &semantic_manifest,
        QueryAnswer {
          status: VisualTruthSpatialQueryStatus::Failed,
          reason: Some(VisualTruthSpatialQueryReason::ProjectionUnavailable),
          pixel_visibility: None,
          pixel_x: None,
          pixel_y: None,
          match_radius_px: None,
          capture_width: None,
          capture_height: None,
        },
        &mut warnings,
        &mut known_limits,
      );
    }
  };

  let Some(frame) = find_target_frame(
    &visual_truth_manifest,
    inputs.object_index,
    &inputs.capture_phase,
    inputs.object_kind.as_ref(),
  ) else {
    return write_query_output(
      inputs,
      generated_at_millis,
      &semantic_manifest,
      QueryAnswer {
        status: VisualTruthSpatialQueryStatus::Failed,
        reason: Some(VisualTruthSpatialQueryReason::TargetAbsentFromVisualTruth),
        pixel_visibility: None,
        pixel_x: None,
        pixel_y: None,
        match_radius_px: None,
        capture_width: None,
        capture_height: None,
      },
      &mut warnings,
      &mut known_limits,
    );
  };

  let answer = answer_for_frame(frame, &projection);
  write_query_output(
    inputs,
    generated_at_millis,
    &semantic_manifest,
    answer,
    &mut warnings,
    &mut known_limits,
  )
}

struct QueryAnswer {
  status: VisualTruthSpatialQueryStatus,
  reason: Option<VisualTruthSpatialQueryReason>,
  pixel_visibility: Option<VisualTruthPixelVisibility>,
  pixel_x: Option<f32>,
  pixel_y: Option<f32>,
  match_radius_px: Option<f32>,
  capture_width: Option<u32>,
  capture_height: Option<u32>,
}

fn answer_for_frame(frame: &VisualTruthFrame, projection: &EvalProjection) -> QueryAnswer {
  let capture_width = frame.capture.width;
  let capture_height = frame.capture.height;
  let Some(point) = project_playfield_point(
    frame.expected_object.expected_playfield_x,
    frame.expected_object.expected_playfield_y,
    projection,
  ) else {
    return QueryAnswer {
      status: VisualTruthSpatialQueryStatus::Failed,
      reason: Some(VisualTruthSpatialQueryReason::ProjectionUnavailable),
      pixel_visibility: None,
      pixel_x: None,
      pixel_y: None,
      match_radius_px: None,
      capture_width: Some(capture_width),
      capture_height: Some(capture_height),
    };
  };

  let pixel_visibility = if pixel_point_inside_capture(&point, capture_width, capture_height) {
    VisualTruthPixelVisibility::InsideCapture
  } else {
    VisualTruthPixelVisibility::OutsideCapture
  };

  QueryAnswer {
    status: VisualTruthSpatialQueryStatus::Answered,
    reason: None,
    pixel_visibility: Some(pixel_visibility),
    pixel_x: Some(point.x),
    pixel_y: Some(point.y),
    match_radius_px: Some(point.match_radius_px),
    capture_width: Some(capture_width),
    capture_height: Some(capture_height),
  }
}

fn find_target_frame<'a>(
  manifest: &'a VisualTruthManifest,
  object_index: usize,
  capture_phase: &CapturePhase,
  object_kind: Option<&ObjectKind>,
) -> Option<&'a VisualTruthFrame> {
  manifest.frames.iter().find(|frame| {
    frame.object_index == object_index
      && frame.capture.phase == *capture_phase
      && object_kind.is_none_or(|kind| frame.expected_object.object_kind == *kind)
  })
}

fn write_query_output(
  inputs: VisualTruthSpatialQueryInputs,
  generated_at_millis: u64,
  semantic_manifest: &VisualTruthSemanticManifest,
  answer: QueryAnswer,
  warnings: &mut BTreeSet<String>,
  known_limits: &mut BTreeSet<String>,
) -> VisualTruthSpatialQueryResult<VisualTruthSpatialQueryOutput> {
  known_limits.extend(semantic_manifest.known_limits.iter().cloned());

  let manifest = VisualTruthSpatialQueryManifest {
    schema_version: VISUAL_TRUTH_SPATIAL_QUERY_MANIFEST_SCHEMA_VERSION,
    generated_at_millis,
    visual_truth_semantic_manifest_path: inputs
      .visual_truth_semantic_manifest_path
      .display()
      .to_string(),
    source_run_artifact_dir: semantic_manifest.source_run_artifact_dir.clone(),
    source_visual_truth_manifest_path: semantic_manifest.source_visual_truth_manifest_path.clone(),
    source_projection_path: semantic_manifest.source_projection_path.clone(),
    object_index: inputs.object_index,
    capture_phase: inputs.capture_phase.clone(),
    object_kind: inputs.object_kind.clone(),
    query_backend: VisualTruthSpatialQueryBackend::PlayfieldProjectionReference,
    status: answer.status,
    reason: answer.reason,
    pixel_visibility: answer.pixel_visibility,
    pixel_x: answer.pixel_x,
    pixel_y: answer.pixel_y,
    match_radius_px: answer.match_radius_px,
    capture_width: answer.capture_width,
    capture_height: answer.capture_height,
    known_limits: known_limits.iter().cloned().collect(),
  };

  let manifest_path = inputs.output_dir.join(QUERY_MANIFEST_FILE);
  write_json_file(&manifest_path, &manifest)?;

  let inspect_report = VisualTruthSpatialQueryInspectReport {
    schema_version: VISUAL_TRUTH_SPATIAL_QUERY_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    visual_truth_spatial_query_manifest_path: manifest_path.display().to_string(),
    visual_truth_semantic_manifest_path: manifest.visual_truth_semantic_manifest_path.clone(),
    source_run_artifact_dir: manifest.source_run_artifact_dir.clone(),
    object_index: manifest.object_index,
    capture_phase: manifest.capture_phase.clone(),
    object_kind: manifest.object_kind.clone(),
    query_backend: manifest.query_backend,
    status: manifest.status,
    reason: manifest.reason,
    pixel_visibility: manifest.pixel_visibility,
    semantic_status: semantic_manifest.semantic_status,
    warnings: warnings.iter().cloned().collect(),
    known_limits: manifest.known_limits.clone(),
  };

  let inspect_report_path = inputs.output_dir.join(QUERY_INSPECT_FILE);
  write_json_file(&inspect_report_path, &inspect_report)?;

  Ok(VisualTruthSpatialQueryOutput {
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
  use crate::benchmark::MapSummary;
  use crate::projection::{PlayfieldProjection, ProjectionArtifact, ProjectionDerivationMethod};
  use crate::visual_truth::{CaptureFrame, ExpectedObjectTruth, VisualTruthFrame};
  use crate::visual_truth_semantic::{
    VisualTruthSemanticValidationInputs, validate_visual_truth_semantic,
  };

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

    write_json_file(&root.join("visual_truth_manifest.json"), &manifest).expect("manifest");
    write_json_file(&root.join("projection.json"), &projection_artifact).expect("projection");
  }

  #[test]
  fn spatial_query_answered_for_probe_target() {
    let root = tempfile::tempdir().expect("tempdir");
    write_probe_fixture(root.path());
    let semantic_out = validate_visual_truth_semantic(VisualTruthSemanticValidationInputs {
      run_artifact_dir: root.path().to_path_buf(),
      output_dir: root.path().join("semantic-out"),
    })
    .expect("semantic validation");

    let output = query_visual_truth_spatial(VisualTruthSpatialQueryInputs {
      visual_truth_semantic_manifest_path: semantic_out.manifest_path,
      object_index: 0,
      capture_phase: CapturePhase::BeforeDispatch,
      object_kind: Some(ObjectKind::Circle),
      output_dir: root.path().join("query-out"),
    })
    .expect("query should succeed");

    assert_eq!(
      output.manifest.status,
      VisualTruthSpatialQueryStatus::Answered
    );
    assert_eq!(
      output.manifest.pixel_visibility,
      Some(VisualTruthPixelVisibility::InsideCapture)
    );
    assert!(output.manifest.pixel_x.is_some());
  }

  #[test]
  fn spatial_query_blocked_when_semantic_not_ready() {
    let root = tempfile::tempdir().expect("tempdir");
    let semantic_out = validate_visual_truth_semantic(VisualTruthSemanticValidationInputs {
      run_artifact_dir: root.path().to_path_buf(),
      output_dir: root.path().join("semantic-out"),
    })
    .expect("semantic blocked still writes");

    let output = query_visual_truth_spatial(VisualTruthSpatialQueryInputs {
      visual_truth_semantic_manifest_path: semantic_out.manifest_path,
      object_index: 0,
      capture_phase: CapturePhase::BeforeDispatch,
      object_kind: None,
      output_dir: root.path().join("query-out"),
    })
    .expect("query should write blocked manifest");

    assert_eq!(
      output.manifest.status,
      VisualTruthSpatialQueryStatus::Blocked
    );
    assert_eq!(
      output.manifest.reason,
      Some(VisualTruthSpatialQueryReason::SemanticSourceNotReady)
    );
  }

  #[test]
  fn spatial_query_failed_when_target_absent() {
    let root = tempfile::tempdir().expect("tempdir");
    write_probe_fixture(root.path());
    let semantic_out = validate_visual_truth_semantic(VisualTruthSemanticValidationInputs {
      run_artifact_dir: root.path().to_path_buf(),
      output_dir: root.path().join("semantic-out"),
    })
    .expect("semantic");

    let output = query_visual_truth_spatial(VisualTruthSpatialQueryInputs {
      visual_truth_semantic_manifest_path: semantic_out.manifest_path,
      object_index: 99,
      capture_phase: CapturePhase::BeforeDispatch,
      object_kind: None,
      output_dir: root.path().join("query-out"),
    })
    .expect("query should write failed manifest");

    assert_eq!(
      output.manifest.status,
      VisualTruthSpatialQueryStatus::Failed
    );
    assert_eq!(
      output.manifest.reason,
      Some(VisualTruthSpatialQueryReason::TargetAbsentFromVisualTruth)
    );
  }
}
