use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use auv_file::{
  JsonFileReadError, JsonFileWriteError, JsonWriteOptions, read_json_file as read_json_file_helper,
  write_json_file as write_json_file_helper,
};
use auv_stage_status::StageStatus;
use auv_tracing::{ArtifactMetadata, ArtifactUri, Context, RunSnapshot, RunStore};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::overlay::render_projection_overlay;
use crate::projection::MinecraftProjector;
use crate::scene_packet::{ScenePacketFramePayload, ScenePacketFrameRecord, ScenePacketManifest};
use crate::training_result_semantic::{TrainingResultSemanticManifest, collect_checkpoint_files};
use crate::types::{MinecraftSpatialFrame, MinecraftTargetSemantics, mc6_projection_target_for_frame};

pub type TrainingResultHoldoutPreviewResult<T> = Result<T, String>;

pub const TRAINING_RESULT_HOLDOUT_PREVIEW_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const TRAINING_RESULT_HOLDOUT_PREVIEW_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;
pub const MINECRAFT_TRAINING_HOLDOUT_PREVIEW_PURPOSE: &str = "auv.minecraft.training.holdout_preview";

pub async fn publish_minecraft_training_holdout_preview(
  context: Option<&Context>,
  preview: &TrainingResultHoldoutPreviewManifest,
) -> Result<Option<ArtifactMetadata>, crate::run_read::MinecraftArtifactPublishError> {
  crate::run_read::publish_json_artifact(
    context,
    MINECRAFT_TRAINING_HOLDOUT_PREVIEW_PURPOSE,
    preview,
    validate_training_holdout_preview_payload,
  )
  .await
}

pub async fn read_minecraft_training_holdout_preview(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<TrainingResultHoldoutPreviewManifest, crate::run_read::MinecraftArtifactReadError> {
  crate::run_read::read_json_artifact(
    store,
    snapshot,
    uri,
    MINECRAFT_TRAINING_HOLDOUT_PREVIEW_PURPOSE,
    validate_training_holdout_preview_payload,
  )
  .await
}

fn validate_training_holdout_preview_payload(preview: &TrainingResultHoldoutPreviewManifest) -> Result<(), String> {
  if preview.schema_version != TRAINING_RESULT_HOLDOUT_PREVIEW_MANIFEST_SCHEMA_VERSION {
    return Err(format!(
      "unsupported Minecraft training holdout preview schema_version {} (expected {TRAINING_RESULT_HOLDOUT_PREVIEW_MANIFEST_SCHEMA_VERSION})",
      preview.schema_version
    ));
  }
  // TODO(minecraft-holdout-preview-invariants): Add cross-field checks when
  // the owning manifest contract declares invariants beyond schema_version.
  Ok(())
}

pub const MC16_V1_HOLDOUT_PREVIEW_KNOWN_LIMIT: &str = "MC-16 v1 holdout preview records scene-packet holdout witness and checkpoint basis; trained splat holdout render and photometric quality judgment are deferred";

const HOLDOUT_PREVIEW_MANIFEST_FILE: &str = "minecraft-3dgs-training-result-holdout-preview.json";
const HOLDOUT_PREVIEW_INSPECT_FILE: &str = "minecraft-3dgs-training-result-holdout-preview-inspect.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrainingResultHoldoutPreviewInputs {
  pub training_result_semantic_manifest_path: PathBuf,
  pub holdout_frame_index: Option<usize>,
  pub holdout_render_command: Option<String>,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingResultHoldoutPreviewOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub manifest: TrainingResultHoldoutPreviewManifest,
  pub inspect_report: TrainingResultHoldoutPreviewInspectReport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldoutFrameWitness {
  pub frame_index: usize,
  pub spatial_frame_id: String,
  pub screenshot_path: String,
  pub frame_json_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldoutPreviewRequest {
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
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub holdout_frame_index: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldoutPreviewAnswer {
  pub status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<HoldoutPreviewReason>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub message: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub holdout_frame: Option<HoldoutFrameWitness>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub basis_checkpoint_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reference_overlay_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingResultHoldoutPreviewManifest {
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
  pub holdout_frame_index: usize,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub holdout_frame: Option<HoldoutFrameWitness>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub basis_checkpoint_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub holdout_screenshot_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reference_overlay_path: Option<String>,
  pub status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<HoldoutPreviewReason>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingResultHoldoutPreviewInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub training_result_holdout_preview_manifest_path: String,
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
  pub holdout_frame_index: usize,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub holdout_frame: Option<HoldoutFrameWitness>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub basis_checkpoint_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub holdout_screenshot_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reference_overlay_path: Option<String>,
  pub status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<HoldoutPreviewReason>,
  pub holdout_frame_selection: HoldoutFrameSelection,
  pub checkpoint_count: usize,
  pub scene_packet_frame_count: usize,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldoutPreviewReason {
  SemanticSourceNotReady,
  ScenePacketUnreadable,
  NormalizedPathsInvalid,
  NoInGameHoldoutFrame,
  InvalidHoldoutFrameIndex,
  CheckpointMissing,
  HoldoutRenderCommandFailed,
  HoldoutRenderOutputInvalid,
}

impl HoldoutPreviewReason {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::SemanticSourceNotReady => "semantic_source_not_ready",
      Self::ScenePacketUnreadable => "scene_packet_unreadable",
      Self::NormalizedPathsInvalid => "normalized_paths_invalid",
      Self::NoInGameHoldoutFrame => "no_in_game_holdout_frame",
      Self::InvalidHoldoutFrameIndex => "invalid_holdout_frame_index",
      Self::CheckpointMissing => "checkpoint_missing",
      Self::HoldoutRenderCommandFailed => "holdout_render_command_failed",
      Self::HoldoutRenderOutputInvalid => "holdout_render_output_invalid",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldoutFrameSelection {
  LastInGame,
  FrameIndexOverride,
  ExternalCommand,
}

impl HoldoutFrameSelection {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::LastInGame => "last_in_game",
      Self::FrameIndexOverride => "frame_index_override",
      Self::ExternalCommand => "external_command",
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HoldoutPreviewOutcome {
  status: StageStatus,
  reason: Option<HoldoutPreviewReason>,
  holdout_frame_index: usize,
  holdout_frame: Option<HoldoutFrameWitness>,
  basis_checkpoint_path: Option<String>,
  holdout_screenshot_path: Option<String>,
  reference_overlay_path: Option<String>,
  holdout_frame_selection: HoldoutFrameSelection,
  checkpoint_count: usize,
  warnings: BTreeSet<String>,
}

pub fn inspect_3dgs_training_result_holdout(
  inputs: TrainingResultHoldoutPreviewInputs,
) -> TrainingResultHoldoutPreviewResult<TrainingResultHoldoutPreviewOutput> {
  let semantic_manifest = read_json_file::<TrainingResultSemanticManifest>(
    &inputs.training_result_semantic_manifest_path,
    "MC-10 training result semantic manifest",
  )?;

  fs::create_dir_all(&inputs.output_dir).map_err(|error| format!("failed to create output dir {}: {error}", inputs.output_dir.display()))?;

  let mut known_limits = BTreeSet::new();
  known_limits.extend(semantic_manifest.known_limits.iter().cloned());
  known_limits.insert(MC16_V1_HOLDOUT_PREVIEW_KNOWN_LIMIT.to_string());
  known_limits.insert(
        "MC-16 closes holdout preview witness evidence over MC-10 semantic manifests; it does not grade splat quality or claim trained holdout render closed"
            .to_string(),
    );

  let generated_at_millis = crate::run_read::now_millis();
  let semantic_ready = semantic_manifest.semantic_status == StageStatus::Ready;

  let scene_packet_manifest_path = PathBuf::from(&semantic_manifest.source_scene_packet_manifest_path);
  let scene_packet_dir =
    scene_packet_manifest_path.parent().ok_or_else(|| "MC-7 scene packet manifest path has no parent directory".to_string())?;

  let scene_packet_manifest = if scene_packet_manifest_path.is_file() {
    Some(read_json_file::<ScenePacketManifest>(&scene_packet_manifest_path, "MC-7 scene packet manifest")?)
  } else {
    None
  };

  let scene_packet_frame_count = scene_packet_manifest.as_ref().map(|manifest| manifest.frames.len()).unwrap_or(0);

  let outcome = if !semantic_ready {
    HoldoutPreviewOutcome {
      status: StageStatus::Blocked,
      reason: Some(HoldoutPreviewReason::SemanticSourceNotReady),
      holdout_frame_index: inputs.holdout_frame_index.unwrap_or(0),
      holdout_frame: None,
      basis_checkpoint_path: None,
      holdout_screenshot_path: None,
      reference_overlay_path: None,
      holdout_frame_selection: holdout_frame_selection_for_inputs(&inputs),
      checkpoint_count: 0,
      warnings: BTreeSet::from(["MC-10 semantic_status is not ready; MC-16 records blocked holdout preview evidence only".to_string()]),
    }
  } else if scene_packet_manifest.is_none() {
    HoldoutPreviewOutcome {
      status: StageStatus::Blocked,
      reason: Some(HoldoutPreviewReason::ScenePacketUnreadable),
      holdout_frame_index: inputs.holdout_frame_index.unwrap_or(0),
      holdout_frame: None,
      basis_checkpoint_path: None,
      holdout_screenshot_path: None,
      reference_overlay_path: None,
      holdout_frame_selection: holdout_frame_selection_for_inputs(&inputs),
      checkpoint_count: 0,
      warnings: BTreeSet::new(),
    }
  } else if let Some(command) = inputs.holdout_render_command.as_deref() {
    run_external_holdout_render(
      command,
      &semantic_manifest,
      &inputs,
      scene_packet_manifest.as_ref().expect("scene packet present"),
      scene_packet_dir,
    )?
  } else {
    run_default_holdout_seam(&semantic_manifest, &inputs, scene_packet_manifest.as_ref().expect("scene packet present"), scene_packet_dir)?
  };

  let checkpoint_count = outcome.checkpoint_count;
  let warnings = outcome.warnings.iter().cloned().collect::<Vec<_>>();
  let known_limits = known_limits.into_iter().collect::<Vec<_>>();

  let manifest_path = inputs.output_dir.join(HOLDOUT_PREVIEW_MANIFEST_FILE);
  let inspect_report_path = inputs.output_dir.join(HOLDOUT_PREVIEW_INSPECT_FILE);

  let manifest = TrainingResultHoldoutPreviewManifest {
    schema_version: TRAINING_RESULT_HOLDOUT_PREVIEW_MANIFEST_SCHEMA_VERSION,
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
    holdout_frame_index: outcome.holdout_frame_index,
    holdout_frame: outcome.holdout_frame.clone(),
    basis_checkpoint_path: outcome.basis_checkpoint_path.clone(),
    holdout_screenshot_path: outcome.holdout_screenshot_path.clone(),
    reference_overlay_path: outcome.reference_overlay_path.clone(),
    status: outcome.status,
    reason: outcome.reason,
    known_limits: known_limits.clone(),
  };

  let inspect_report = TrainingResultHoldoutPreviewInspectReport {
    schema_version: TRAINING_RESULT_HOLDOUT_PREVIEW_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    training_result_holdout_preview_manifest_path: manifest_path.to_string_lossy().into_owned(),
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
    holdout_frame_index: outcome.holdout_frame_index,
    holdout_frame: outcome.holdout_frame,
    basis_checkpoint_path: outcome.basis_checkpoint_path,
    holdout_screenshot_path: outcome.holdout_screenshot_path,
    reference_overlay_path: outcome.reference_overlay_path,
    status: outcome.status,
    reason: outcome.reason,
    holdout_frame_selection: outcome.holdout_frame_selection,
    checkpoint_count,
    scene_packet_frame_count,
    warnings,
    known_limits,
  };

  write_json_file(&manifest_path, &manifest, "MC-16 holdout preview manifest")?;
  write_json_file(&inspect_report_path, &inspect_report, "MC-16 holdout preview inspect report")?;

  Ok(TrainingResultHoldoutPreviewOutput {
    output_dir: inputs.output_dir,
    manifest_path,
    inspect_report_path,
    manifest,
    inspect_report,
  })
}

fn holdout_frame_selection_for_inputs(inputs: &TrainingResultHoldoutPreviewInputs) -> HoldoutFrameSelection {
  if inputs.holdout_render_command.is_some() {
    HoldoutFrameSelection::ExternalCommand
  } else if inputs.holdout_frame_index.is_some() {
    HoldoutFrameSelection::FrameIndexOverride
  } else {
    HoldoutFrameSelection::LastInGame
  }
}

fn run_default_holdout_seam(
  semantic_manifest: &TrainingResultSemanticManifest,
  inputs: &TrainingResultHoldoutPreviewInputs,
  scene_packet_manifest: &ScenePacketManifest,
  scene_packet_dir: &Path,
) -> TrainingResultHoldoutPreviewResult<HoldoutPreviewOutcome> {
  let holdout_frame_selection = holdout_frame_selection_for_inputs(inputs);
  let models_dir_path = PathBuf::from(&semantic_manifest.models_dir_path);
  if !models_dir_path.is_dir() {
    return Ok(HoldoutPreviewOutcome {
      status: StageStatus::Blocked,
      reason: Some(HoldoutPreviewReason::NormalizedPathsInvalid),
      holdout_frame_index: inputs.holdout_frame_index.unwrap_or(0),
      holdout_frame: None,
      basis_checkpoint_path: None,
      holdout_screenshot_path: None,
      reference_overlay_path: None,
      holdout_frame_selection,
      checkpoint_count: 0,
      warnings: BTreeSet::new(),
    });
  }

  let checkpoint_files = match collect_checkpoint_files(&models_dir_path) {
    Ok(files) => files,
    Err(_) => {
      return Ok(HoldoutPreviewOutcome {
        status: StageStatus::Failed,
        reason: Some(HoldoutPreviewReason::CheckpointMissing),
        holdout_frame_index: inputs.holdout_frame_index.unwrap_or(0),
        holdout_frame: None,
        basis_checkpoint_path: None,
        holdout_screenshot_path: None,
        reference_overlay_path: None,
        holdout_frame_selection,
        checkpoint_count: 0,
        warnings: BTreeSet::new(),
      });
    }
  };

  if checkpoint_files.is_empty() {
    return Ok(HoldoutPreviewOutcome {
      status: StageStatus::Failed,
      reason: Some(HoldoutPreviewReason::CheckpointMissing),
      holdout_frame_index: inputs.holdout_frame_index.unwrap_or(0),
      holdout_frame: None,
      basis_checkpoint_path: None,
      holdout_screenshot_path: None,
      reference_overlay_path: None,
      holdout_frame_selection,
      checkpoint_count: 0,
      warnings: BTreeSet::new(),
    });
  }

  let basis_checkpoint_path = models_dir_path.join(checkpoint_files.last().expect("checkpoint present").relative_path.as_str());

  let selected_frame = select_holdout_frame(scene_packet_manifest, inputs.holdout_frame_index);
  let Some(frame_record) = selected_frame else {
    let reason = if inputs.holdout_frame_index.is_some() {
      HoldoutPreviewReason::InvalidHoldoutFrameIndex
    } else {
      HoldoutPreviewReason::NoInGameHoldoutFrame
    };
    return Ok(HoldoutPreviewOutcome {
      status: StageStatus::Failed,
      reason: Some(reason),
      holdout_frame_index: inputs.holdout_frame_index.unwrap_or(0),
      holdout_frame: None,
      basis_checkpoint_path: Some(basis_checkpoint_path.to_string_lossy().into_owned()),
      holdout_screenshot_path: None,
      reference_overlay_path: None,
      holdout_frame_selection,
      checkpoint_count: checkpoint_files.len(),
      warnings: BTreeSet::new(),
    });
  };

  let frame_json_path = scene_packet_dir.join(&frame_record.frame_json_path);
  let spatial_frame = match load_scene_packet_frame(scene_packet_dir, frame_record) {
    Ok(frame) => frame,
    Err(error) => {
      return Ok(HoldoutPreviewOutcome {
        status: StageStatus::Failed,
        reason: Some(HoldoutPreviewReason::ScenePacketUnreadable),
        holdout_frame_index: frame_record.frame_index,
        holdout_frame: None,
        basis_checkpoint_path: Some(basis_checkpoint_path.to_string_lossy().into_owned()),
        holdout_screenshot_path: None,
        reference_overlay_path: None,
        holdout_frame_selection,
        checkpoint_count: checkpoint_files.len(),
        warnings: BTreeSet::from([error]),
      });
    }
  };

  let screenshot_path = frame_record.screenshot_path.as_ref().map(|path| scene_packet_dir.join(path).to_string_lossy().into_owned());

  let holdout_frame = HoldoutFrameWitness {
    frame_index: frame_record.frame_index,
    spatial_frame_id: frame_record.spatial_frame_id.clone(),
    screenshot_path: screenshot_path.clone().unwrap_or_default(),
    frame_json_path: frame_json_path.to_string_lossy().into_owned(),
  };

  let mut warnings = BTreeSet::new();
  if screenshot_path.is_none() {
    warnings.insert("holdout frame has no scene-packet screenshot witness; MC-16 records frame json only".to_string());
  }

  let reference_overlay_path = try_build_reference_overlay(scene_packet_dir, frame_record, &spatial_frame, &inputs.output_dir)
    .map(|path| path.to_string_lossy().into_owned());

  if reference_overlay_path.is_none() && screenshot_path.is_some() {
    warnings.insert("reference overlay was not generated; screenshot or raycast witness was insufficient".to_string());
  }

  Ok(HoldoutPreviewOutcome {
    status: StageStatus::Ready,
    reason: None,
    holdout_frame_index: frame_record.frame_index,
    holdout_frame: Some(holdout_frame),
    basis_checkpoint_path: Some(basis_checkpoint_path.to_string_lossy().into_owned()),
    holdout_screenshot_path: screenshot_path,
    reference_overlay_path,
    holdout_frame_selection,
    checkpoint_count: checkpoint_files.len(),
    warnings,
  })
}

fn run_external_holdout_render(
  command_text: &str,
  semantic_manifest: &TrainingResultSemanticManifest,
  inputs: &TrainingResultHoldoutPreviewInputs,
  _scene_packet_manifest: &ScenePacketManifest,
  _scene_packet_dir: &Path,
) -> TrainingResultHoldoutPreviewResult<HoldoutPreviewOutcome> {
  let request = HoldoutPreviewRequest {
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
    holdout_frame_index: inputs.holdout_frame_index,
  };

  let mut command = Command::new("sh");
  command.arg("-lc").arg(command_text).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

  let mut child = command.spawn().map_err(|error| format!("failed to run MC-16 holdout render command `{command_text}`: {error}"))?;
  {
    let stdin = child.stdin.as_mut().ok_or_else(|| "failed to open stdin for MC-16 holdout render command".to_string())?;
    serde_json::to_writer(&mut *stdin, &request).map_err(|error| format!("failed to write MC-16 holdout preview request JSON: {error}"))?;
    stdin.write_all(b"\n").map_err(|error| format!("failed to finish MC-16 holdout preview request JSON: {error}"))?;
  }

  let output =
    child.wait_with_output().map_err(|error| format!("failed to wait for MC-16 holdout render command `{command_text}`: {error}"))?;

  let checkpoint_count = collect_checkpoint_files(Path::new(&semantic_manifest.models_dir_path)).map(|files| files.len()).unwrap_or(0);

  if !output.status.success() {
    return Ok(HoldoutPreviewOutcome {
      status: StageStatus::Failed,
      reason: Some(HoldoutPreviewReason::HoldoutRenderCommandFailed),
      holdout_frame_index: inputs.holdout_frame_index.unwrap_or(0),
      holdout_frame: None,
      basis_checkpoint_path: None,
      holdout_screenshot_path: None,
      reference_overlay_path: None,
      holdout_frame_selection: HoldoutFrameSelection::ExternalCommand,
      checkpoint_count,
      warnings: BTreeSet::from([format!(
        "MC-16 holdout render command exited with status {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
      )]),
    });
  }

  let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
  let answer = match serde_json::from_str::<HoldoutPreviewAnswer>(&stdout) {
    Ok(answer) => answer,
    Err(error) => {
      return Ok(HoldoutPreviewOutcome {
        status: StageStatus::Failed,
        reason: Some(HoldoutPreviewReason::HoldoutRenderOutputInvalid),
        holdout_frame_index: inputs.holdout_frame_index.unwrap_or(0),
        holdout_frame: None,
        basis_checkpoint_path: None,
        holdout_screenshot_path: None,
        reference_overlay_path: None,
        holdout_frame_selection: HoldoutFrameSelection::ExternalCommand,
        checkpoint_count,
        warnings: BTreeSet::from([format!(
          "failed to parse MC-16 holdout render command output: {error}"
        )]),
      });
    }
  };

  let holdout_frame_index = answer.holdout_frame.as_ref().map(|witness| witness.frame_index).or(inputs.holdout_frame_index).unwrap_or(0);

  let holdout_screenshot_path = answer.holdout_frame.as_ref().map(|witness| witness.screenshot_path.clone());

  let mut warnings = BTreeSet::new();
  if let Some(message) = answer.message {
    warnings.insert(message);
  }

  Ok(HoldoutPreviewOutcome {
    status: answer.status,
    reason: answer.reason,
    holdout_frame_index,
    holdout_frame: answer.holdout_frame,
    basis_checkpoint_path: answer.basis_checkpoint_path,
    holdout_screenshot_path,
    reference_overlay_path: answer.reference_overlay_path,
    holdout_frame_selection: HoldoutFrameSelection::ExternalCommand,
    checkpoint_count,
    warnings,
  })
}

fn select_holdout_frame(scene_packet_manifest: &ScenePacketManifest, holdout_frame_index: Option<usize>) -> Option<&ScenePacketFrameRecord> {
  let mut in_game_frames =
    scene_packet_manifest.frames.iter().filter(|frame| frame.screen_state.as_deref() == Some("in_game")).collect::<Vec<_>>();

  if let Some(index) = holdout_frame_index {
    return in_game_frames.into_iter().find(|frame| frame.frame_index == index);
  }

  in_game_frames.sort_by_key(|frame| frame.monotonic_timestamp_ms);
  in_game_frames.last().copied()
}

fn load_scene_packet_frame(
  scene_packet_dir: &Path,
  frame_record: &ScenePacketFrameRecord,
) -> TrainingResultHoldoutPreviewResult<MinecraftSpatialFrame> {
  let frame_path = scene_packet_dir.join(&frame_record.frame_json_path);
  if let Ok(payload) = read_json_file::<ScenePacketFramePayload>(&frame_path, "MC-7 scene packet frame") {
    return Ok(payload.spatial_frame);
  }
  read_json_file::<MinecraftSpatialFrame>(&frame_path, "MC-7 scene packet frame")
}

fn try_build_reference_overlay(
  scene_packet_dir: &Path,
  frame_record: &ScenePacketFrameRecord,
  spatial_frame: &MinecraftSpatialFrame,
  output_dir: &Path,
) -> Option<PathBuf> {
  let screenshot_rel = frame_record.screenshot_path.as_ref()?;
  let screenshot_path = scene_packet_dir.join(screenshot_rel);
  if !screenshot_path.is_file() {
    return None;
  }

  let raycast_hit = spatial_frame.raycast_hit.as_ref()?;
  let target = mc6_projection_target_for_frame(raycast_hit.block_pos, spatial_frame, MinecraftTargetSemantics::HitFaceCenter);
  let projector = MinecraftProjector::new(spatial_frame.clone()).ok()?;
  let projected = projector.project_block_target(&target).ok()?;

  let image = image::open(&screenshot_path).ok()?.into_rgb8();
  let overlay = render_projection_overlay(image, &projected, Some(raycast_hit));
  let overlay_path = output_dir.join(format!("holdout_overlay_frame_{:06}.png", frame_record.frame_index));
  overlay.save(&overlay_path).ok()?;
  Some(overlay_path)
}

fn read_json_file<T: DeserializeOwned>(path: &Path, label: &str) -> TrainingResultHoldoutPreviewResult<T> {
  read_json_file_helper(path).map_err(|error| match error {
    JsonFileReadError::Open(error) => {
      format!("failed to open {label} {}: {error}", path.display())
    }
    JsonFileReadError::Parse(error) => {
      format!("failed to parse {label} {}: {error}", path.display())
    }
  })
}

fn write_json_file<T: Serialize>(path: &Path, value: &T, label: &str) -> TrainingResultHoldoutPreviewResult<()> {
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
  use crate::types::{BlockFace, PlayerPose, RaycastHit, Vec3, Viewport};
  use tempfile::TempDir;

  fn identity_matrix() -> [f64; 16] {
    [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
  }

  fn write_semantic_manifest(
    temp: &TempDir,
    semantic_status: StageStatus,
    scene_packet_manifest_path: &Path,
    normalized_dir: &Path,
  ) -> PathBuf {
    let models_dir = normalized_dir.join("nerfstudio_models");
    fs::create_dir_all(&models_dir).expect("models dir");
    fs::write(normalized_dir.join("config.yml"), "trainer: nerfstudio.splatfacto\n").expect("config");

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
      normalized_result_dir: normalized_dir.to_string_lossy().into_owned(),
      semantic_status,
      semantic_reason: None,
      config_path: normalized_dir.join("config.yml").to_string_lossy().into_owned(),
      models_dir_path: models_dir.to_string_lossy().into_owned(),
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

  fn test_frame(view_matrix: [f64; 16], projection_matrix: [f64; 16]) -> MinecraftSpatialFrame {
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
        block_pos: crate::types::BlockPosition::new(0, 0, 0),
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

  fn write_scene_packet_fixture(temp: &TempDir, frames: Vec<(usize, u64, MinecraftSpatialFrame)>) -> PathBuf {
    let output_dir = temp.path().join("scene-packet");
    fs::create_dir_all(output_dir.join("frames")).expect("frames dir");

    let frame_records = frames
      .into_iter()
      .map(|(frame_index, timestamp_ms, frame)| {
        let frame_json_name = format!("frames/frame_{frame_index:06}.json");
        let frame_json_path = output_dir.join(&frame_json_name);
        write_json_file(&frame_json_path, &frame, "frame fixture").expect("frame fixture");
        ScenePacketFrameRecord {
          frame_index,
          spatial_frame_id: frame.spatial_frame_id.clone(),
          source_run_id: "run-1".to_string(),
          source_bundle_manifest_path: "/tmp/bundle.json".to_string(),
          source_frame_artifact_id: "artifact_0001".to_string(),
          source_frame_bundle_path: "spatial_frames/frame.json".to_string(),
          frame_json_path: frame_json_name,
          screenshot_artifact_id: None,
          screenshot_path: None,
          monotonic_timestamp_ms: timestamp_ms,
          viewport: frame.viewport,
          screen_state: frame.screen_state.clone(),
          resource_pack_ids: Vec::new(),
        }
      })
      .collect::<Vec<_>>();

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
    manifest_path
  }

  fn write_checkpoint(_temp: &TempDir, normalized_dir: &Path) {
    let models_dir = normalized_dir.join("nerfstudio_models");
    fs::create_dir_all(&models_dir).expect("models dir");
    fs::write(models_dir.join("step-000001.ckpt"), b"fixture-checkpoint-bytes").expect("ckpt");
  }

  #[test]
  fn blocked_semantic_records_holdout_preview_evidence() {
    let temp = TempDir::new().expect("tempdir");
    let normalized_dir = temp.path().join("normalized");
    let frame = test_frame(identity_matrix(), identity_matrix());
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, vec![(1, 100, frame)]);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Blocked, &scene_packet_manifest_path, &normalized_dir);

    let output = inspect_3dgs_training_result_holdout(TrainingResultHoldoutPreviewInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      holdout_frame_index: None,
      holdout_render_command: None,
      output_dir: temp.path().join("holdout-output"),
    })
    .expect("blocked semantic");

    assert_eq!(output.manifest.status, StageStatus::Blocked);
    assert_eq!(output.manifest.reason, Some(HoldoutPreviewReason::SemanticSourceNotReady));
    assert!(output.manifest.holdout_frame.is_none());
  }

  #[test]
  fn missing_checkpoint_records_failed_holdout_preview() {
    let temp = TempDir::new().expect("tempdir");
    let normalized_dir = temp.path().join("normalized");
    let frame = test_frame(identity_matrix(), identity_matrix());
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, vec![(1, 100, frame)]);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path, &normalized_dir);

    let output = inspect_3dgs_training_result_holdout(TrainingResultHoldoutPreviewInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      holdout_frame_index: None,
      holdout_render_command: None,
      output_dir: temp.path().join("holdout-output"),
    })
    .expect("missing checkpoint");

    assert_eq!(output.manifest.status, StageStatus::Failed);
    assert_eq!(output.manifest.reason, Some(HoldoutPreviewReason::CheckpointMissing));
  }

  #[test]
  fn happy_path_selects_last_in_game_holdout_frame() {
    let temp = TempDir::new().expect("tempdir");
    let normalized_dir = temp.path().join("normalized");
    write_checkpoint(&temp, &normalized_dir);

    let early_frame = test_frame(identity_matrix(), identity_matrix());
    let mut late_frame = test_frame(identity_matrix(), identity_matrix());
    late_frame.spatial_frame_id = "frame-late".to_string();
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, vec![(1, 100, early_frame), (2, 200, late_frame)]);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path, &normalized_dir);

    let output = inspect_3dgs_training_result_holdout(TrainingResultHoldoutPreviewInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      holdout_frame_index: None,
      holdout_render_command: None,
      output_dir: temp.path().join("holdout-output"),
    })
    .expect("happy path");

    assert_eq!(output.manifest.status, StageStatus::Ready);
    assert_eq!(output.manifest.holdout_frame_index, 2);
    assert_eq!(output.manifest.holdout_frame.as_ref().map(|witness| witness.spatial_frame_id.as_str()), Some("frame-late"));
    assert!(output.manifest.basis_checkpoint_path.as_deref().is_some_and(|path| path.ends_with("step-000001.ckpt")));
    assert_eq!(output.inspect_report.holdout_frame_selection, HoldoutFrameSelection::LastInGame);
  }

  #[test]
  fn invalid_frame_index_records_failed_holdout_preview() {
    let temp = TempDir::new().expect("tempdir");
    let normalized_dir = temp.path().join("normalized");
    write_checkpoint(&temp, &normalized_dir);
    let frame = test_frame(identity_matrix(), identity_matrix());
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, vec![(1, 100, frame)]);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path, &normalized_dir);

    let output = inspect_3dgs_training_result_holdout(TrainingResultHoldoutPreviewInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      holdout_frame_index: Some(99),
      holdout_render_command: None,
      output_dir: temp.path().join("holdout-output"),
    })
    .expect("invalid frame index");

    assert_eq!(output.manifest.status, StageStatus::Failed);
    assert_eq!(output.manifest.reason, Some(HoldoutPreviewReason::InvalidHoldoutFrameIndex));
  }

  #[test]
  fn external_holdout_render_command_parse_failure_records_failed() {
    let temp = TempDir::new().expect("tempdir");
    let normalized_dir = temp.path().join("normalized");
    write_checkpoint(&temp, &normalized_dir);
    let frame = test_frame(identity_matrix(), identity_matrix());
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, vec![(1, 100, frame)]);
    let semantic_manifest_path = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path, &normalized_dir);

    let output = inspect_3dgs_training_result_holdout(TrainingResultHoldoutPreviewInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      holdout_frame_index: None,
      holdout_render_command: Some("printf 'not-json'".to_string()),
      output_dir: temp.path().join("holdout-output"),
    })
    .expect("external command parse failure");

    assert_eq!(output.manifest.status, StageStatus::Failed);
    assert_eq!(output.manifest.reason, Some(HoldoutPreviewReason::HoldoutRenderOutputInvalid));
    assert_eq!(output.inspect_report.holdout_frame_selection, HoldoutFrameSelection::ExternalCommand);
  }
}
