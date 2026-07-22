use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use auv_compare::{
  DualBackendAnswer, DualBackendCompareVerdict, DualBackendSelectedSide, DualBackendStageStatus, compare_dual_backend_verdict,
  pick_blocked_or_failed_preferred, screen_points_match_with_tolerance, select_dual_backend_outcome,
};
use auv_driver::geometry::Point;
use auv_file::{
  JsonFileReadError, JsonFileWriteError, JsonWriteOptions, read_json_file as read_json_file_helper,
  write_json_file as write_json_file_helper,
};
use auv_stage_status::StageStatus;
use auv_tracing::{ArtifactMetadata, ArtifactUri, Context, RunSnapshot, RunStore};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::projection::MinecraftProjector;
use crate::scene_packet::{ScenePacketFramePayload, ScenePacketFrameRecord, ScenePacketManifest};
use crate::training_result_semantic::TrainingResultSemanticManifest;
use crate::training_result_spatial_query_provider::{
  MC15_V1_CHECKPOINT_NATIVE_KNOWN_LIMIT, MC18_V1_CLOSED_SCENE_TOY_KNOWN_LIMIT, MC18_V1_CLOSED_SCENE_TOY_NO_REFERENCE_LIMIT,
  run_checkpoint_native_provider_backend, run_closed_scene_toy_provider_backend,
};
use crate::types::{
  BlockFace, BlockPosition, MinecraftSpatialFrame, MinecraftTargetSemantics, ProjectionVisibility, mc6_projection_target_for_frame,
};

pub type TrainingResultSpatialQueryResult<T> = Result<T, String>;

pub const TRAINING_RESULT_SPATIAL_QUERY_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const TRAINING_RESULT_SPATIAL_QUERY_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;
pub const MINECRAFT_TRAINING_SPATIAL_QUERY_PURPOSE: &str = "auv.minecraft.training.spatial_query";

pub async fn publish_minecraft_training_spatial_query(
  context: Option<&Context>,
  query: &TrainingResultSpatialQueryManifest,
) -> Result<Option<ArtifactMetadata>, crate::run_read::MinecraftArtifactPublishError> {
  crate::run_read::publish_json_artifact(context, MINECRAFT_TRAINING_SPATIAL_QUERY_PURPOSE, query, |_| Ok(())).await
}

pub async fn read_minecraft_training_spatial_query(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<TrainingResultSpatialQueryManifest, crate::run_read::MinecraftArtifactReadError> {
  crate::run_read::read_json_artifact(store, snapshot, uri, MINECRAFT_TRAINING_SPATIAL_QUERY_PURPOSE, |_| Ok(())).await
}

const QUERY_MANIFEST_FILE: &str = "minecraft-3dgs-training-result-query.json";
const QUERY_INSPECT_FILE: &str = "minecraft-3dgs-training-result-query-inspect.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrainingResultSpatialQueryInputs {
  pub training_result_semantic_manifest_path: PathBuf,
  pub target_block: BlockPosition,
  pub target_face: Option<BlockFace>,
  pub target_semantics: MinecraftTargetSemantics,
  pub query_command: Option<String>,
  pub use_checkpoint_native_provider: bool,
  pub use_closed_scene_toy_provider: bool,
  pub closed_scene_fixture_path: Option<PathBuf>,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingResultSpatialQueryOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub manifest: TrainingResultSpatialQueryManifest,
  pub inspect_report: TrainingResultSpatialQueryInspectReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingResultSpatialQueryManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub training_result_semantic_manifest_path: String,
  pub source_training_result_artifact_manifest_path: String,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub normalized_result_dir: String,
  pub query_kind: TrainingResultSpatialQueryKind,
  pub target_block: BlockPosition,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub target_face: Option<BlockFace>,
  pub target_semantics: MinecraftTargetSemantics,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub selected_backend: Option<TrainingResultSpatialQueryBackend>,
  pub status: TrainingResultSpatialQueryStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<TrainingResultSpatialQueryReason>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub visibility: Option<ProjectionVisibility>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub screen_point: Option<Point>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub match_radius_px: Option<f64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub confidence: Option<f64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub basis_frame_id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub comparison_verdict: Option<TrainingResultSpatialQueryComparisonVerdict>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingResultSpatialQueryInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub training_result_spatial_query_manifest_path: String,
  pub training_result_semantic_manifest_path: String,
  pub source_training_result_artifact_manifest_path: String,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub normalized_result_dir: String,
  pub query_kind: TrainingResultSpatialQueryKind,
  pub target_block: BlockPosition,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub target_face: Option<BlockFace>,
  pub target_semantics: MinecraftTargetSemantics,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub selected_backend: Option<TrainingResultSpatialQueryBackend>,
  pub status: TrainingResultSpatialQueryStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<TrainingResultSpatialQueryReason>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub visibility: Option<ProjectionVisibility>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub screen_point: Option<Point>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub match_radius_px: Option<f64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub confidence: Option<f64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub basis_frame_id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub comparison_verdict: Option<TrainingResultSpatialQueryComparisonVerdict>,
  pub provider_status: TrainingResultSpatialQueryStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub provider_reason: Option<TrainingResultSpatialQueryReason>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub provider_message: Option<String>,
  pub reference_status: TrainingResultSpatialQueryStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reference_reason: Option<TrainingResultSpatialQueryReason>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reference_basis_frame_id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reference_source_frame_json_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reference_screenshot_path: Option<String>,
  pub scene_packet_frame_count: usize,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingResultSpatialQueryRequest {
  pub source_training_result_artifact_manifest_path: String,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub normalized_result_dir: String,
  pub query_kind: TrainingResultSpatialQueryKind,
  pub target_block: BlockPosition,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub target_face: Option<BlockFace>,
  pub target_semantics: MinecraftTargetSemantics,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingResultSpatialQueryAnswer {
  pub status: TrainingResultSpatialQueryStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<TrainingResultSpatialQueryReason>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub message: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub basis_frame_id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub visibility: Option<ProjectionVisibility>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub screen_point: Option<Point>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub match_radius_px: Option<f64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub confidence: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingResultSpatialQueryStatus {
  Answered,
  Blocked,
  Failed,
}

impl TrainingResultSpatialQueryStatus {
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
pub enum TrainingResultSpatialQueryBackend {
  CommandProvider,
  CheckpointNative,
  ClosedSceneToy,
  ProjectionReference,
}

impl TrainingResultSpatialQueryBackend {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::CommandProvider => "command_provider",
      Self::CheckpointNative => "checkpoint_native",
      Self::ClosedSceneToy => "closed_scene_toy",
      Self::ProjectionReference => "projection_reference",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingResultSpatialQueryKind {
  BlockProjection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingResultSpatialQueryReason {
  SemanticSourceNotReady,
  TargetBlockAbsentFromScenePacket,
  ReferenceProjectionFailed,
  ProviderCommandFailed,
  ProviderOutputInvalid,
}

impl TrainingResultSpatialQueryReason {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::SemanticSourceNotReady => "semantic_source_not_ready",
      Self::TargetBlockAbsentFromScenePacket => "target_block_absent_from_scene_packet",
      Self::ReferenceProjectionFailed => "reference_projection_failed",
      Self::ProviderCommandFailed => "provider_command_failed",
      Self::ProviderOutputInvalid => "provider_output_invalid",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingResultSpatialQueryComparisonVerdict {
  Match,
  Divergent,
  ProviderOnly,
  ReferenceOnly,
  NotComparable,
}

impl TrainingResultSpatialQueryComparisonVerdict {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Match => "match",
      Self::Divergent => "divergent",
      Self::ProviderOnly => "provider_only",
      Self::ReferenceOnly => "reference_only",
      Self::NotComparable => "not_comparable",
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BackendOutcome {
  pub(crate) answer: TrainingResultSpatialQueryAnswer,
  pub(crate) reference_source_frame_json_path: Option<String>,
  pub(crate) reference_screenshot_path: Option<String>,
}

pub fn query_3dgs_training_result(
  inputs: TrainingResultSpatialQueryInputs,
) -> TrainingResultSpatialQueryResult<TrainingResultSpatialQueryOutput> {
  let semantic_manifest = read_json_file::<TrainingResultSemanticManifest>(
    &inputs.training_result_semantic_manifest_path,
    "MC-10 training result semantic manifest",
  )?;

  let scene_packet_manifest_path = PathBuf::from(&semantic_manifest.source_scene_packet_manifest_path);
  if !scene_packet_manifest_path.is_file() {
    return Err(format!(
      "MC-12 requires readable source_scene_packet_manifest_path {}, but the file is missing",
      scene_packet_manifest_path.display()
    ));
  }

  fs::create_dir_all(&inputs.output_dir).map_err(|error| format!("failed to create output dir {}: {error}", inputs.output_dir.display()))?;

  let scene_packet_manifest = read_json_file::<ScenePacketManifest>(&scene_packet_manifest_path, "MC-7 scene packet manifest")?;
  let scene_packet_dir =
    scene_packet_manifest_path.parent().ok_or_else(|| "MC-7 scene packet manifest path has no parent directory".to_string())?;

  let generated_at_millis = crate::run_read::now_millis();
  let mut known_limits = BTreeSet::new();
  known_limits.extend(semantic_manifest.known_limits.iter().cloned());
  known_limits.insert(
    "MC-12 closes block-only spatial query evidence over MC-10 semantic manifests; it does not grade model quality or claim Gaussian-native inference"
      .to_string(),
  );
  known_limits
    .insert("projection_reference is a scene-packet fallback reference backend, not a checkpoint-native Gaussian query core".to_string());
  known_limits
    .insert("MC-12 does not add entity query, anchor/label query, render preview, or dedicated read-side viewer consumption".to_string());
  if inputs.use_checkpoint_native_provider {
    known_limits.insert(MC15_V1_CHECKPOINT_NATIVE_KNOWN_LIMIT.to_string());
  }
  if inputs.use_closed_scene_toy_provider {
    known_limits.insert(MC18_V1_CLOSED_SCENE_TOY_KNOWN_LIMIT.to_string());
    known_limits.insert(MC18_V1_CLOSED_SCENE_TOY_NO_REFERENCE_LIMIT.to_string());
  }

  let mut warnings = BTreeSet::new();
  let semantic_ready = semantic_manifest.semantic_status == StageStatus::Ready;

  let (provider_outcome, configured_provider_backend) = if inputs.use_checkpoint_native_provider {
    (
      Some(run_checkpoint_native_provider_backend(&semantic_manifest, &scene_packet_manifest, scene_packet_dir, &inputs)?),
      Some(TrainingResultSpatialQueryBackend::CheckpointNative),
    )
  } else if inputs.use_closed_scene_toy_provider {
    (
      Some(run_closed_scene_toy_provider_backend(&semantic_manifest, &inputs, inputs.closed_scene_fixture_path.as_deref())?),
      Some(TrainingResultSpatialQueryBackend::ClosedSceneToy),
    )
  } else if let Some(command) = inputs.query_command.as_deref() {
    (Some(run_command_provider_backend(command, &semantic_manifest, &inputs)?), Some(TrainingResultSpatialQueryBackend::CommandProvider))
  } else {
    (None, None)
  };

  let reference_outcome = if semantic_ready {
    run_projection_reference_backend(&scene_packet_manifest, scene_packet_dir, &inputs)?
  } else {
    BackendOutcome {
      answer: TrainingResultSpatialQueryAnswer {
        status: TrainingResultSpatialQueryStatus::Blocked,
        reason: Some(TrainingResultSpatialQueryReason::SemanticSourceNotReady),
        message: Some("MC-12 projection_reference requires MC-10 semantic_status=ready".to_string()),
        basis_frame_id: None,
        visibility: None,
        screen_point: None,
        match_radius_px: None,
        confidence: None,
      },
      reference_source_frame_json_path: None,
      reference_screenshot_path: None,
    }
  };

  if !semantic_ready {
    warnings.insert("MC-10 semantic_status is not ready; MC-12 records blocked spatial query evidence only".to_string());
  }

  let provider_answer = provider_outcome.as_ref().map(|outcome| outcome.answer.clone());
  let reference_answer = reference_outcome.answer.clone();

  let (selected_backend, selected_answer, comparison_verdict) =
    select_query_outcome(provider_answer.as_ref(), Some(&reference_answer), configured_provider_backend);

  let manifest_path = inputs.output_dir.join(QUERY_MANIFEST_FILE);
  let inspect_report_path = inputs.output_dir.join(QUERY_INSPECT_FILE);

  let manifest = TrainingResultSpatialQueryManifest {
    schema_version: TRAINING_RESULT_SPATIAL_QUERY_MANIFEST_SCHEMA_VERSION,
    generated_at_millis,
    training_result_semantic_manifest_path: inputs.training_result_semantic_manifest_path.to_string_lossy().into_owned(),
    source_training_result_artifact_manifest_path: semantic_manifest.source_training_result_artifact_manifest_path.clone(),
    source_training_result_manifest_path: semantic_manifest.source_training_result_manifest_path.clone(),
    source_training_job_manifest_path: semantic_manifest.source_training_job_manifest_path.clone(),
    source_training_launch_plan_path: semantic_manifest.source_training_launch_plan_path.clone(),
    source_training_package_manifest_path: semantic_manifest.source_training_package_manifest_path.clone(),
    source_scene_packet_manifest_path: semantic_manifest.source_scene_packet_manifest_path.clone(),
    source_bundle_manifest_paths: semantic_manifest.source_bundle_manifest_paths.clone(),
    source_run_ids: semantic_manifest.source_run_ids.clone(),
    trainer_backend: semantic_manifest.trainer_backend.clone(),
    job_backend: semantic_manifest.job_backend.clone(),
    normalized_result_dir: semantic_manifest.normalized_result_dir.clone(),
    query_kind: TrainingResultSpatialQueryKind::BlockProjection,
    target_block: inputs.target_block,
    target_face: inputs.target_face,
    target_semantics: inputs.target_semantics,
    selected_backend,
    status: selected_answer.status,
    reason: selected_answer.reason,
    visibility: selected_answer.visibility,
    screen_point: selected_answer.screen_point,
    match_radius_px: selected_answer.match_radius_px,
    confidence: selected_answer.confidence,
    basis_frame_id: selected_answer.basis_frame_id.clone(),
    comparison_verdict,
    known_limits: known_limits.into_iter().collect(),
  };

  let inspect_report = TrainingResultSpatialQueryInspectReport {
    schema_version: TRAINING_RESULT_SPATIAL_QUERY_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    training_result_spatial_query_manifest_path: manifest_path.to_string_lossy().into_owned(),
    training_result_semantic_manifest_path: manifest.training_result_semantic_manifest_path.clone(),
    source_training_result_artifact_manifest_path: manifest.source_training_result_artifact_manifest_path.clone(),
    source_training_result_manifest_path: manifest.source_training_result_manifest_path.clone(),
    source_training_job_manifest_path: manifest.source_training_job_manifest_path.clone(),
    source_training_launch_plan_path: manifest.source_training_launch_plan_path.clone(),
    source_training_package_manifest_path: manifest.source_training_package_manifest_path.clone(),
    source_scene_packet_manifest_path: manifest.source_scene_packet_manifest_path.clone(),
    source_bundle_manifest_paths: manifest.source_bundle_manifest_paths.clone(),
    source_run_ids: manifest.source_run_ids.clone(),
    trainer_backend: manifest.trainer_backend.clone(),
    job_backend: manifest.job_backend.clone(),
    normalized_result_dir: manifest.normalized_result_dir.clone(),
    query_kind: manifest.query_kind,
    target_block: manifest.target_block,
    target_face: manifest.target_face,
    target_semantics: manifest.target_semantics,
    selected_backend: manifest.selected_backend,
    status: manifest.status,
    reason: manifest.reason,
    visibility: manifest.visibility,
    screen_point: manifest.screen_point,
    match_radius_px: manifest.match_radius_px,
    confidence: manifest.confidence,
    basis_frame_id: manifest.basis_frame_id.clone(),
    comparison_verdict: manifest.comparison_verdict,
    provider_status: provider_answer.as_ref().map(|answer| answer.status).unwrap_or(TrainingResultSpatialQueryStatus::Blocked),
    provider_reason: provider_answer.as_ref().and_then(|answer| answer.reason),
    provider_message: provider_answer.as_ref().and_then(|answer| answer.message.clone()),
    reference_status: reference_answer.status,
    reference_reason: reference_answer.reason,
    reference_basis_frame_id: reference_answer.basis_frame_id.clone(),
    reference_source_frame_json_path: reference_outcome.reference_source_frame_json_path.clone(),
    reference_screenshot_path: reference_outcome.reference_screenshot_path.clone(),
    scene_packet_frame_count: scene_packet_manifest.frames.len(),
    warnings: warnings.into_iter().collect(),
    known_limits: manifest.known_limits.clone(),
  };

  write_json_file(&manifest_path, &manifest, "MC-12 training result spatial query manifest")?;
  write_json_file(&inspect_report_path, &inspect_report, "MC-12 training result spatial query inspect report")?;

  Ok(TrainingResultSpatialQueryOutput {
    output_dir: inputs.output_dir,
    manifest_path,
    inspect_report_path,
    manifest,
    inspect_report,
  })
}

impl DualBackendAnswer for TrainingResultSpatialQueryAnswer {
  type VisibilityKey = ProjectionVisibility;

  fn stage_status(&self) -> DualBackendStageStatus {
    match self.status {
      TrainingResultSpatialQueryStatus::Answered => DualBackendStageStatus::Answered,
      TrainingResultSpatialQueryStatus::Blocked => DualBackendStageStatus::Blocked,
      TrainingResultSpatialQueryStatus::Failed => DualBackendStageStatus::Failed,
    }
  }

  fn visibility_key(&self) -> Option<Self::VisibilityKey> {
    self.visibility
  }

  fn screen_point(&self) -> Option<auv_compare::ScreenPoint> {
    self.screen_point.map(|point| auv_compare::ScreenPoint {
      x: point.x,
      y: point.y,
    })
  }

  fn match_radius_px(&self) -> Option<f64> {
    self.match_radius_px
  }
}

fn map_comparison_verdict(verdict: DualBackendCompareVerdict) -> TrainingResultSpatialQueryComparisonVerdict {
  match verdict {
    DualBackendCompareVerdict::Match => TrainingResultSpatialQueryComparisonVerdict::Match,
    DualBackendCompareVerdict::Divergent => TrainingResultSpatialQueryComparisonVerdict::Divergent,
    DualBackendCompareVerdict::ProviderOnly => TrainingResultSpatialQueryComparisonVerdict::ProviderOnly,
    DualBackendCompareVerdict::ReferenceOnly => TrainingResultSpatialQueryComparisonVerdict::ReferenceOnly,
    DualBackendCompareVerdict::NotComparable => TrainingResultSpatialQueryComparisonVerdict::NotComparable,
  }
}

fn select_query_outcome(
  provider_answer: Option<&TrainingResultSpatialQueryAnswer>,
  reference_answer: Option<&TrainingResultSpatialQueryAnswer>,
  configured_provider_backend: Option<TrainingResultSpatialQueryBackend>,
) -> (Option<TrainingResultSpatialQueryBackend>, TrainingResultSpatialQueryAnswer, Option<TrainingResultSpatialQueryComparisonVerdict>) {
  let (selected_side, answer, comparison_verdict) =
    select_dual_backend_outcome(provider_answer, reference_answer, pick_blocked_or_failed_answer);
  let selected_backend = match selected_side {
    DualBackendSelectedSide::Provider => configured_provider_backend.or(Some(TrainingResultSpatialQueryBackend::CommandProvider)),
    DualBackendSelectedSide::Reference => Some(TrainingResultSpatialQueryBackend::ProjectionReference),
    DualBackendSelectedSide::Neither => None,
  };
  (selected_backend, answer, comparison_verdict.map(map_comparison_verdict))
}

fn pick_blocked_or_failed_answer(
  provider_answer: Option<&TrainingResultSpatialQueryAnswer>,
  reference_answer: Option<&TrainingResultSpatialQueryAnswer>,
) -> TrainingResultSpatialQueryAnswer {
  pick_blocked_or_failed_preferred([provider_answer, reference_answer], |answer| answer.status == TrainingResultSpatialQueryStatus::Blocked)
    .cloned()
    .unwrap_or(TrainingResultSpatialQueryAnswer {
      status: TrainingResultSpatialQueryStatus::Failed,
      reason: Some(TrainingResultSpatialQueryReason::TargetBlockAbsentFromScenePacket),
      message: Some("MC-12 spatial query produced no backend answer".to_string()),
      basis_frame_id: None,
      visibility: None,
      screen_point: None,
      match_radius_px: None,
      confidence: None,
    })
}

// Core-B2 adapter surface; selection path compares via select_dual_backend_outcome.
#[allow(dead_code)]
fn compare_answers(
  provider_answer: Option<&TrainingResultSpatialQueryAnswer>,
  reference_answer: Option<&TrainingResultSpatialQueryAnswer>,
) -> Option<TrainingResultSpatialQueryComparisonVerdict> {
  compare_dual_backend_verdict(provider_answer, reference_answer).map(map_comparison_verdict)
}

#[allow(dead_code)]
fn answers_match(provider: &TrainingResultSpatialQueryAnswer, reference: &TrainingResultSpatialQueryAnswer) -> bool {
  if provider.visibility != reference.visibility {
    return false;
  }
  match (provider.screen_point, reference.screen_point) {
    (Some(provider_point), Some(reference_point)) => screen_points_match_with_tolerance(
      auv_compare::ScreenPoint {
        x: provider_point.x,
        y: provider_point.y,
      },
      auv_compare::ScreenPoint {
        x: reference_point.x,
        y: reference_point.y,
      },
      provider.match_radius_px,
      reference.match_radius_px,
    ),
    (None, None) => true,
    _ => false,
  }
}

fn run_command_provider_backend(
  command_text: &str,
  semantic_manifest: &TrainingResultSemanticManifest,
  inputs: &TrainingResultSpatialQueryInputs,
) -> TrainingResultSpatialQueryResult<BackendOutcome> {
  let request = TrainingResultSpatialQueryRequest {
    source_training_result_artifact_manifest_path: semantic_manifest.source_training_result_artifact_manifest_path.clone(),
    source_training_result_manifest_path: semantic_manifest.source_training_result_manifest_path.clone(),
    source_training_job_manifest_path: semantic_manifest.source_training_job_manifest_path.clone(),
    source_training_launch_plan_path: semantic_manifest.source_training_launch_plan_path.clone(),
    source_training_package_manifest_path: semantic_manifest.source_training_package_manifest_path.clone(),
    source_scene_packet_manifest_path: semantic_manifest.source_scene_packet_manifest_path.clone(),
    source_bundle_manifest_paths: semantic_manifest.source_bundle_manifest_paths.clone(),
    source_run_ids: semantic_manifest.source_run_ids.clone(),
    trainer_backend: semantic_manifest.trainer_backend.clone(),
    job_backend: semantic_manifest.job_backend.clone(),
    normalized_result_dir: semantic_manifest.normalized_result_dir.clone(),
    query_kind: TrainingResultSpatialQueryKind::BlockProjection,
    target_block: inputs.target_block,
    target_face: inputs.target_face,
    target_semantics: inputs.target_semantics,
  };

  let mut command = Command::new("sh");
  command.arg("-lc").arg(command_text).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

  let mut child = command.spawn().map_err(|error| format!("failed to run MC-12 spatial query command `{command_text}`: {error}"))?;
  {
    let stdin = child.stdin.as_mut().ok_or_else(|| "failed to open stdin for MC-12 spatial query command".to_string())?;
    serde_json::to_writer(&mut *stdin, &request).map_err(|error| format!("failed to write MC-12 spatial query request JSON: {error}"))?;
    stdin.write_all(b"\n").map_err(|error| format!("failed to finish MC-12 spatial query request JSON: {error}"))?;
  }

  let output =
    child.wait_with_output().map_err(|error| format!("failed to wait for MC-12 spatial query command `{command_text}`: {error}"))?;
  if !output.status.success() {
    return Ok(BackendOutcome {
      answer: TrainingResultSpatialQueryAnswer {
        status: TrainingResultSpatialQueryStatus::Failed,
        reason: Some(TrainingResultSpatialQueryReason::ProviderCommandFailed),
        message: Some(format!(
          "MC-12 spatial query command exited with status {}: {}",
          output.status,
          String::from_utf8_lossy(&output.stderr).trim()
        )),
        basis_frame_id: None,
        visibility: None,
        screen_point: None,
        match_radius_px: None,
        confidence: None,
      },
      reference_source_frame_json_path: None,
      reference_screenshot_path: None,
    });
  }

  let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
  let answer = serde_json::from_str::<TrainingResultSpatialQueryAnswer>(&stdout).unwrap_or_else(|error| TrainingResultSpatialQueryAnswer {
    status: TrainingResultSpatialQueryStatus::Failed,
    reason: Some(TrainingResultSpatialQueryReason::ProviderOutputInvalid),
    message: Some(format!("failed to parse MC-12 spatial query command output: {error}")),
    basis_frame_id: None,
    visibility: None,
    screen_point: None,
    match_radius_px: None,
    confidence: None,
  });

  Ok(BackendOutcome {
    answer,
    reference_source_frame_json_path: None,
    reference_screenshot_path: None,
  })
}

pub(crate) fn run_projection_reference_backend(
  scene_packet_manifest: &ScenePacketManifest,
  scene_packet_dir: &Path,
  inputs: &TrainingResultSpatialQueryInputs,
) -> TrainingResultSpatialQueryResult<BackendOutcome> {
  let Some((frame_record, frame)) = select_reference_frame(scene_packet_manifest, scene_packet_dir, inputs.target_block) else {
    return Ok(BackendOutcome {
      answer: TrainingResultSpatialQueryAnswer {
        status: TrainingResultSpatialQueryStatus::Failed,
        reason: Some(TrainingResultSpatialQueryReason::TargetBlockAbsentFromScenePacket),
        message: Some(format!(
          "target block {},{},{} is absent from readable in_game scene packet frames",
          inputs.target_block.x, inputs.target_block.y, inputs.target_block.z
        )),
        basis_frame_id: None,
        visibility: None,
        screen_point: None,
        match_radius_px: None,
        confidence: None,
      },
      reference_source_frame_json_path: None,
      reference_screenshot_path: None,
    });
  };

  let frame_json_path = scene_packet_dir.join(&frame_record.frame_json_path);
  if !frame_json_path.is_file() {
    return Err(format!("MC-12 selected scene packet frame JSON is missing at {}", frame_json_path.display()));
  }

  let mut target = mc6_projection_target_for_frame(inputs.target_block, &frame, inputs.target_semantics);
  if let Some(face) = inputs.target_face {
    target.face = Some(face);
    if inputs.target_semantics == MinecraftTargetSemantics::BlockCenter {
      target.precise_point = None;
    }
  }

  let projected = match MinecraftProjector::new(frame.clone()).and_then(|projector| projector.project_block_target(&target)) {
    Ok(projected) => projected,
    Err(error) => {
      return Ok(BackendOutcome {
        answer: TrainingResultSpatialQueryAnswer {
          status: TrainingResultSpatialQueryStatus::Failed,
          reason: Some(TrainingResultSpatialQueryReason::ReferenceProjectionFailed),
          message: Some(error),
          basis_frame_id: Some(frame.spatial_frame_id.clone()),
          visibility: None,
          screen_point: None,
          match_radius_px: None,
          confidence: None,
        },
        reference_source_frame_json_path: Some(frame_json_path.to_string_lossy().into_owned()),
        reference_screenshot_path: frame_record
          .screenshot_path
          .as_ref()
          .map(|path| scene_packet_dir.join(path).to_string_lossy().into_owned()),
      });
    }
  };

  Ok(BackendOutcome {
    answer: TrainingResultSpatialQueryAnswer {
      status: TrainingResultSpatialQueryStatus::Answered,
      reason: None,
      message: None,
      basis_frame_id: Some(projected.basis_frame_id.clone()),
      visibility: Some(projected.visibility),
      screen_point: projected.screen_point,
      match_radius_px: Some(projected.match_radius_px),
      confidence: Some(projected.confidence),
    },
    reference_source_frame_json_path: Some(frame_json_path.to_string_lossy().into_owned()),
    reference_screenshot_path: frame_record.screenshot_path.as_ref().map(|path| scene_packet_dir.join(path).to_string_lossy().into_owned()),
  })
}

fn select_reference_frame(
  scene_packet_manifest: &ScenePacketManifest,
  scene_packet_dir: &Path,
  target_block: BlockPosition,
) -> Option<(ScenePacketFrameRecord, MinecraftSpatialFrame)> {
  let mut frames = scene_packet_manifest.frames.clone();
  frames.sort_by_key(|frame| frame.monotonic_timestamp_ms);
  frames.reverse();

  for frame_record in &frames {
    if !frame_is_query_candidate(frame_record) {
      continue;
    }
    let frame = load_scene_packet_frame(scene_packet_dir, frame_record).ok()?;
    if frame.raycast_hit.as_ref().is_some_and(|hit| hit.block_pos == target_block) {
      return Some((frame_record.clone(), frame));
    }
  }

  for frame_record in &frames {
    if !frame_is_query_candidate(frame_record) {
      continue;
    }
    let frame = load_scene_packet_frame(scene_packet_dir, frame_record).ok()?;
    if frame.nearby_blocks.iter().any(|block| block.block_pos == target_block) {
      return Some((frame_record.clone(), frame));
    }
  }

  None
}

fn frame_is_query_candidate(frame_record: &ScenePacketFrameRecord) -> bool {
  frame_record.screen_state.as_deref() == Some("in_game")
}

fn load_scene_packet_frame(
  scene_packet_dir: &Path,
  frame_record: &ScenePacketFrameRecord,
) -> TrainingResultSpatialQueryResult<MinecraftSpatialFrame> {
  let frame_path = scene_packet_dir.join(&frame_record.frame_json_path);
  if let Ok(payload) = read_json_file::<ScenePacketFramePayload>(&frame_path, "MC-7 scene packet frame") {
    return Ok(payload.spatial_frame);
  }
  read_json_file::<MinecraftSpatialFrame>(&frame_path, "MC-7 scene packet frame")
}

fn read_json_file<T: DeserializeOwned>(path: &Path, label: &str) -> TrainingResultSpatialQueryResult<T> {
  read_json_file_helper(path).map_err(|error| match error {
    JsonFileReadError::Open(error) => {
      format!("failed to open {label} {}: {error}", path.display())
    }
    JsonFileReadError::Parse(error) => {
      format!("failed to parse {label} {}: {error}", path.display())
    }
  })
}

fn write_json_file<T: Serialize>(path: &Path, value: &T, label: &str) -> TrainingResultSpatialQueryResult<()> {
  write_json_file_helper(path, value, JsonWriteOptions::default()).map_err(|error| match error {
    JsonFileWriteError::CreateParent(error) | JsonFileWriteError::Write(error) => {
      format!("failed to write {label} {}: {error}", path.display())
    }
    JsonFileWriteError::Serialize(error) => format!("failed to serialize {label}: {error}"),
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene_packet::{ScenePacketCounts, ScenePacketManifest};
  use crate::training_result::TrainingResultStatus;
  use crate::training_result_semantic::TrainingResultSemanticManifest;
  use crate::types::{PlayerPose, RaycastHit, Vec3, Viewport};
  use tempfile::TempDir;

  fn identity_matrix() -> [f64; 16] {
    [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
  }

  fn write_semantic_manifest(temp: &TempDir, semantic_status: StageStatus, scene_packet_manifest_path: &Path) -> PathBuf {
    let manifest = TrainingResultSemanticManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_training_result_artifact_manifest_path: "/tmp/d11.json".to_string(),
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch.json".to_string(),
      source_training_package_manifest_path: "/tmp/package.json".to_string(),
      source_scene_packet_manifest_path: scene_packet_manifest_path.to_string_lossy().into_owned(),
      source_bundle_manifest_paths: vec!["/tmp/bundle.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      source_result_status: TrainingResultStatus::Succeeded,
      normalized_result_dir: temp.path().join("normalized").to_string_lossy().into_owned(),
      semantic_status,
      semantic_reason: None,
      config_path: temp.path().join("normalized/config.yml").to_string_lossy().into_owned(),
      models_dir_path: temp.path().join("normalized/nerfstudio_models").to_string_lossy().into_owned(),
      status_snapshot_path: None,
      config_trainer: Some("nerfstudio.splatfacto".to_string()),
      checkpoint_files: Vec::new(),
      checkpoint_count: 0,
      known_limits: vec!["fixture".to_string()],
    };
    let path = temp.path().join("semantic.json");
    write_json_file(&path, &manifest, "semantic fixture").expect("semantic fixture");
    path
  }

  fn write_scene_packet_fixture(temp: &TempDir, target_block: BlockPosition, frame: MinecraftSpatialFrame) -> PathBuf {
    let output_dir = temp.path().join("scene-packet");
    fs::create_dir_all(output_dir.join("frames")).expect("frames dir");
    let frame_json_path = output_dir.join("frames/frame_000001.json");
    write_json_file(&frame_json_path, &frame, "frame fixture").expect("frame fixture");

    let manifest = ScenePacketManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_bundle_manifest_paths: vec!["/tmp/bundle.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      counts: ScenePacketCounts {
        frames: 1,
        screenshots: 0,
        missing_screenshots: 1,
      },
      frames: vec![ScenePacketFrameRecord {
        frame_index: 1,
        spatial_frame_id: frame.spatial_frame_id.clone(),
        source_run_id: "run-1".to_string(),
        source_bundle_manifest_path: "/tmp/bundle.json".to_string(),
        source_frame_artifact_id: "artifact_0001".to_string(),
        source_frame_bundle_path: "spatial_frames/frame.json".to_string(),
        frame_json_path: "frames/frame_000001.json".to_string(),
        screenshot_artifact_id: None,
        screenshot_path: None,
        monotonic_timestamp_ms: frame.monotonic_timestamp_ms,
        viewport: frame.viewport,
        screen_state: frame.screen_state.clone(),
        resource_pack_ids: Vec::new(),
      }],
      known_limits: Vec::new(),
    };
    let manifest_path = output_dir.join("scene-packet.json");
    write_json_file(&manifest_path, &manifest, "scene packet fixture").expect("scene packet");
    let _ = target_block;
    manifest_path
  }

  fn test_frame(target_block: BlockPosition, view_matrix: [f64; 16], projection_matrix: [f64; 16]) -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: "frame-1".to_string(),
      world_tick: 1,
      monotonic_timestamp_ms: 100,
      telemetry_session_id: None,
      viewport: Viewport::new(800, 600),
      view_matrix,
      projection_matrix,
      player_pose: PlayerPose {
        eye_position: Vec3::new(0.0, 0.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: Some(RaycastHit {
        block_pos: target_block,
        face: BlockFace::North,
        block_id: "minecraft:stone".to_string(),
      }),
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: Some("in_game".to_string()),
      resource_pack_ids: Vec::new(),
    }
  }

  #[test]
  fn reference_only_happy_path_answers_visible_target() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let frame = test_frame(target_block, identity_matrix(), identity_matrix());
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block, frame);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path);

    let output = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    })
    .expect("reference-only happy path");

    assert_eq!(output.manifest.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(output.manifest.selected_backend, Some(TrainingResultSpatialQueryBackend::ProjectionReference));
    assert_eq!(output.manifest.comparison_verdict, Some(TrainingResultSpatialQueryComparisonVerdict::ReferenceOnly));
    assert_eq!(output.inspect_report.reference_status, TrainingResultSpatialQueryStatus::Answered);
    assert!(output.manifest.screen_point.is_some());
  }

  #[test]
  fn blocked_when_semantic_source_not_ready() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let frame = test_frame(target_block, identity_matrix(), identity_matrix());
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block, frame);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Blocked, &scene_packet_manifest_path);

    let output = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    })
    .expect("blocked semantic should still write artifacts");

    assert_eq!(output.manifest.status, TrainingResultSpatialQueryStatus::Blocked);
    assert_eq!(output.manifest.reason, Some(TrainingResultSpatialQueryReason::SemanticSourceNotReady));
    assert!(output.manifest_path.is_file());
    assert!(output.inspect_report_path.is_file());
  }

  #[test]
  fn failed_when_target_block_absent_from_scene_packet() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(9, 9, 9);
    let frame = test_frame(BlockPosition::new(0, 0, 0), identity_matrix(), identity_matrix());
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block, frame);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path);

    let output = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    })
    .expect("missing target should still write artifacts");

    assert_eq!(output.manifest.status, TrainingResultSpatialQueryStatus::Failed);
    assert_eq!(output.manifest.reason, Some(TrainingResultSpatialQueryReason::TargetBlockAbsentFromScenePacket));
  }

  #[test]
  fn provider_and_reference_both_answered_selects_provider() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let frame = test_frame(target_block, identity_matrix(), identity_matrix());
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block, frame);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path);

    let provider_command = "printf '%s' '{\"status\":\"answered\",\"basis_frame_id\":\"provider-frame\",\"visibility\":\"visible\",\"screen_point\":{\"x\":600.0,\"y\":150.0},\"match_radius_px\":8.0,\"confidence\":0.9}'";

    let output = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: Some(provider_command.to_string()),
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    })
    .expect("provider + reference");

    assert_eq!(output.manifest.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(output.manifest.selected_backend, Some(TrainingResultSpatialQueryBackend::CommandProvider));
    assert!(matches!(
      output.manifest.comparison_verdict,
      Some(TrainingResultSpatialQueryComparisonVerdict::Match | TrainingResultSpatialQueryComparisonVerdict::Divergent)
    ));
  }

  #[test]
  fn provider_invalid_json_records_failed_provider_and_falls_back_to_reference() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let frame = test_frame(target_block, identity_matrix(), identity_matrix());
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block, frame);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path);
    let provider_command = "printf not-json".to_string();

    let output = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: Some(provider_command),
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    })
    .expect("invalid provider JSON should still write artifacts via reference fallback");

    assert_eq!(output.inspect_report.provider_status, TrainingResultSpatialQueryStatus::Failed);
    assert_eq!(output.inspect_report.provider_reason, Some(TrainingResultSpatialQueryReason::ProviderOutputInvalid));
    assert!(
      output
        .inspect_report
        .provider_message
        .as_deref()
        .is_some_and(|message| message.contains("failed to parse MC-12 spatial query command output"))
    );
    assert_eq!(output.manifest.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(output.manifest.selected_backend, Some(TrainingResultSpatialQueryBackend::ProjectionReference));
  }

  #[test]
  fn provider_non_zero_exit_records_failed_provider_status() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let frame = test_frame(target_block, identity_matrix(), identity_matrix());
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block, frame);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path);

    let output = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: Some("exit 17".to_string()),
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    })
    .expect("provider failure should still write artifacts via reference fallback");

    assert_eq!(output.inspect_report.provider_status, TrainingResultSpatialQueryStatus::Failed);
    assert_eq!(output.manifest.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(output.manifest.selected_backend, Some(TrainingResultSpatialQueryBackend::ProjectionReference));
  }

  #[test]
  fn visibility_behind_camera_is_answered_not_failed() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let frame = test_frame(
      target_block,
      identity_matrix(),
      [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0,
      ],
    );
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block, frame);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path);

    let output = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    })
    .expect("behind camera visibility");

    assert_eq!(output.manifest.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(output.manifest.visibility, Some(ProjectionVisibility::BehindCamera));
  }

  #[test]
  fn visibility_out_of_frustum_is_answered_not_failed() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let mut frame = test_frame(target_block, identity_matrix(), identity_matrix());
    frame.player_pose.eye_position = Vec3::new(5.0, 0.0, 0.0);
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block, frame);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path);

    let output = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    })
    .expect("out of frustum visibility");

    assert_eq!(output.manifest.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(output.manifest.visibility, Some(ProjectionVisibility::OutOfFrustum));
  }

  fn write_multi_frame_scene_packet_fixture(
    temp: &TempDir,
    target_block: BlockPosition,
    frames: Vec<(u64, &str, MinecraftSpatialFrame)>,
  ) -> PathBuf {
    let output_dir = temp.path().join("scene-packet");
    fs::create_dir_all(output_dir.join("frames")).expect("frames dir");
    let mut frame_records = Vec::new();
    for (index, (timestamp_ms, spatial_frame_id, frame)) in frames.into_iter().enumerate() {
      let frame_json_name = format!("frames/frame_{:06}.json", index + 1);
      let frame_json_path = output_dir.join(&frame_json_name);
      write_json_file(&frame_json_path, &frame, "frame fixture").expect("frame fixture");
      frame_records.push(ScenePacketFrameRecord {
        frame_index: index + 1,
        spatial_frame_id: spatial_frame_id.to_string(),
        source_run_id: "run-1".to_string(),
        source_bundle_manifest_path: "/tmp/bundle.json".to_string(),
        source_frame_artifact_id: format!("artifact_{index:04}"),
        source_frame_bundle_path: format!("spatial_frames/{spatial_frame_id}.json"),
        frame_json_path: frame_json_name,
        screenshot_artifact_id: None,
        screenshot_path: None,
        monotonic_timestamp_ms: timestamp_ms,
        viewport: frame.viewport,
        screen_state: frame.screen_state.clone(),
        resource_pack_ids: Vec::new(),
      });
    }

    let manifest = ScenePacketManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_bundle_manifest_paths: vec!["/tmp/bundle.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      counts: ScenePacketCounts {
        frames: frame_records.len(),
        screenshots: 0,
        missing_screenshots: frame_records.len(),
      },
      frames: frame_records,
      known_limits: Vec::new(),
    };
    let manifest_path = output_dir.join("scene-packet.json");
    write_json_file(&manifest_path, &manifest, "scene packet fixture").expect("scene packet");
    let _ = target_block;
    manifest_path
  }

  #[test]
  fn visibility_outside_window_is_answered_not_failed() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let frame = test_frame(target_block, identity_matrix(), identity_matrix());
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block, frame);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path);
    let provider_command = "printf '%s' '{\"status\":\"answered\",\"basis_frame_id\":\"provider-frame\",\"visibility\":\"outside_window\",\"match_radius_px\":8.0,\"confidence\":0.9}'";

    let output = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: Some(provider_command.to_string()),
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    })
    .expect("outside window visibility");

    assert_eq!(output.manifest.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(output.manifest.visibility, Some(ProjectionVisibility::OutsideWindow));
    assert!(output.manifest.screen_point.is_none());
  }

  #[test]
  fn reference_selects_newest_matching_frame() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let older_frame = test_frame(target_block, identity_matrix(), identity_matrix());
    let mut newer_frame = test_frame(target_block, identity_matrix(), identity_matrix());
    newer_frame.spatial_frame_id = "frame-newest".to_string();
    newer_frame.monotonic_timestamp_ms = 200;
    let scene_packet_manifest_path = write_multi_frame_scene_packet_fixture(
      &temp,
      target_block,
      vec![
        (100, "frame-older", older_frame),
        (200, "frame-newest", newer_frame),
      ],
    );
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path);

    let output = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    })
    .expect("newest matching frame");

    assert_eq!(output.manifest.basis_frame_id.as_deref(), Some("frame-newest"));
    assert_eq!(output.inspect_report.reference_basis_frame_id.as_deref(), Some("frame-newest"));
  }

  #[test]
  fn target_face_and_semantics_affect_reference_projection() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let mut frame = test_frame(target_block, identity_matrix(), identity_matrix());
    frame.raycast_hit = Some(RaycastHit {
      block_pos: target_block,
      face: BlockFace::North,
      block_id: "minecraft:stone".to_string(),
    });
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block, frame);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path);

    let hit_face_center = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path.clone(),
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-hit-face"),
    })
    .expect("hit face center semantics");

    let block_center = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path.clone(),
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::BlockCenter,
      query_command: None,
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-block-center"),
    })
    .expect("block center semantics");

    let explicit_face = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: Some(BlockFace::East),
      target_semantics: MinecraftTargetSemantics::BlockCenter,
      query_command: None,
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-explicit-face"),
    })
    .expect("explicit target face");

    assert_eq!(hit_face_center.manifest.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(block_center.manifest.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(explicit_face.manifest.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(hit_face_center.manifest.target_semantics, MinecraftTargetSemantics::HitFaceCenter);
    assert_eq!(block_center.manifest.target_semantics, MinecraftTargetSemantics::BlockCenter);
    assert_eq!(explicit_face.manifest.target_face, Some(BlockFace::East));
    assert_ne!(hit_face_center.manifest.screen_point, explicit_face.manifest.screen_point);
  }

  #[test]
  fn hard_fails_when_scene_packet_manifest_missing() {
    let temp = TempDir::new().expect("tempdir");
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, Path::new("/tmp/missing-scene-packet.json"));

    let error = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block: BlockPosition::new(0, 0, 0),
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    })
    .expect_err("missing scene packet manifest");

    assert!(error.contains("source_scene_packet_manifest_path"));
  }

  #[test]
  fn hard_fails_when_semantic_manifest_missing() {
    let temp = TempDir::new().expect("tempdir");
    let error = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: temp.path().join("missing.json"),
      target_block: BlockPosition::new(0, 0, 0),
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    })
    .expect_err("missing semantic manifest");

    assert!(error.contains("failed to open"));
  }
  fn write_normalized_result_fixture(temp: &TempDir, with_checkpoint: bool) {
    let normalized_dir = temp.path().join("normalized");
    let models_dir = normalized_dir.join("nerfstudio_models");
    fs::create_dir_all(&models_dir).expect("models dir");
    fs::write(normalized_dir.join("config.yml"), "trainer: nerfstudio.splatfacto\n").expect("config");
    if with_checkpoint {
      fs::write(models_dir.join("step-000001.ckpt"), b"fake-checkpoint").expect("checkpoint");
    }
  }

  #[test]
  fn checkpoint_native_provider_selects_checkpoint_backend() {
    let temp = TempDir::new().expect("tempdir");
    write_normalized_result_fixture(&temp, true);
    let target_block = BlockPosition::new(0, 0, 0);
    let frame = test_frame(target_block, identity_matrix(), identity_matrix());
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block, frame);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path);

    let output = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: true,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    })
    .expect("checkpoint native query");

    assert_eq!(output.manifest.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(output.manifest.selected_backend, Some(TrainingResultSpatialQueryBackend::CheckpointNative));
    assert!(output.manifest.basis_frame_id.as_deref().is_some_and(|basis| basis.starts_with("checkpoint:")));
    assert!(matches!(
      output.manifest.comparison_verdict,
      Some(TrainingResultSpatialQueryComparisonVerdict::Match | TrainingResultSpatialQueryComparisonVerdict::Divergent)
    ));
    assert!(output.manifest.known_limits.iter().any(|limit| limit.contains("Gaussian render inference is deferred")));
  }
  #[test]
  fn closed_scene_toy_provider_selects_closed_scene_toy_backend() {
    let temp = TempDir::new().expect("tempdir");
    write_normalized_result_fixture(&temp, true);
    let target_block = BlockPosition::new(511, 73, 728);
    let frame = test_frame(target_block, identity_matrix(), identity_matrix());
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block, frame);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path);
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mc18/visible.json");

    let output = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: Some(BlockFace::North),
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: true,
      closed_scene_fixture_path: Some(fixture_path),
      output_dir: temp.path().join("query-output"),
    })
    .expect("closed scene toy query");

    assert_eq!(output.manifest.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(output.manifest.selected_backend, Some(TrainingResultSpatialQueryBackend::ClosedSceneToy));
    assert!(output.manifest.basis_frame_id.as_deref().is_some_and(|basis| basis.starts_with("closed_scene_toy:")));
    assert!(output.manifest.known_limits.iter().any(|limit| limit.contains("not Gaussian inference")));
  }
}
