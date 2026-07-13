use std::path::{Path, PathBuf};

use auv_stage_status::StageStatus;

use crate::closed_scene_toy_fixture::load_closed_scene_fixture;
use crate::scene_packet::ScenePacketManifest;
use crate::training_result_semantic::{TrainingResultSemanticCheckpointRecord, TrainingResultSemanticManifest, collect_checkpoint_files};
use crate::training_result_spatial_query::{
  BackendOutcome, TrainingResultSpatialQueryAnswer, TrainingResultSpatialQueryInputs, TrainingResultSpatialQueryKind,
  TrainingResultSpatialQueryReason, TrainingResultSpatialQueryRequest, TrainingResultSpatialQueryResult, TrainingResultSpatialQueryStatus,
  run_projection_reference_backend,
};
use crate::types::BlockPosition;

pub const MC15_V1_CHECKPOINT_NATIVE_KNOWN_LIMIT: &str = "MC-15 v1 checkpoint-native provider validates normalized-result inputs and records checkpoint basis; Gaussian render inference is deferred";

pub const MC18_V1_CLOSED_SCENE_TOY_KNOWN_LIMIT: &str = "MC-18 v1 closed-scene toy provider answers from bounded fixture/label lookup only; closed-scene and closed-label only; not Gaussian inference; not action authority";

pub const MC18_V1_CLOSED_SCENE_TOY_NO_REFERENCE_LIMIT: &str = "MC-18 v1 closed-scene toy provider does not use projection_reference or MinecraftProjector; answers are fixture-derived closed-world lookup only";

const MC15_V1_PROVIDER_MESSAGE: &str = "MC-15 v1 checkpoint-native provider validates normalized-result inputs and records checkpoint basis; Gaussian render inference is deferred";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointNativeProviderInputs {
  pub request: TrainingResultSpatialQueryRequest,
  pub normalized_result_dir: PathBuf,
  pub config_path: PathBuf,
  pub models_dir_path: PathBuf,
  pub target_block: BlockPosition,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckpointNativeProviderOutcome {
  pub answer: TrainingResultSpatialQueryAnswer,
}

impl CheckpointNativeProviderInputs {
  pub fn from_spatial_query(semantic_manifest: &TrainingResultSemanticManifest, inputs: &TrainingResultSpatialQueryInputs) -> Self {
    Self {
      request: TrainingResultSpatialQueryRequest {
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
      },
      normalized_result_dir: PathBuf::from(&semantic_manifest.normalized_result_dir),
      config_path: PathBuf::from(&semantic_manifest.config_path),
      models_dir_path: PathBuf::from(&semantic_manifest.models_dir_path),
      target_block: inputs.target_block,
    }
  }
}

impl CheckpointNativeProviderOutcome {
  pub(crate) fn into_backend_outcome(self) -> BackendOutcome {
    BackendOutcome {
      answer: self.answer,
      reference_source_frame_json_path: None,
      reference_screenshot_path: None,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosedSceneToyProviderInputs {
  pub request: TrainingResultSpatialQueryRequest,
  pub closed_scene_fixture_path: Option<PathBuf>,
  pub target_block: BlockPosition,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClosedSceneToyProviderOutcome {
  pub answer: TrainingResultSpatialQueryAnswer,
}

impl ClosedSceneToyProviderInputs {
  pub fn from_spatial_query(
    semantic_manifest: &TrainingResultSemanticManifest,
    inputs: &TrainingResultSpatialQueryInputs,
    closed_scene_fixture_path: Option<PathBuf>,
  ) -> Self {
    Self {
      request: TrainingResultSpatialQueryRequest {
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
      },
      closed_scene_fixture_path,
      target_block: inputs.target_block,
    }
  }
}

impl ClosedSceneToyProviderOutcome {
  pub(crate) fn into_backend_outcome(self) -> BackendOutcome {
    BackendOutcome {
      answer: self.answer,
      reference_source_frame_json_path: None,
      reference_screenshot_path: None,
    }
  }
}

pub(crate) fn run_checkpoint_native_provider_backend(
  semantic_manifest: &TrainingResultSemanticManifest,
  scene_packet_manifest: &ScenePacketManifest,
  scene_packet_dir: &Path,
  inputs: &TrainingResultSpatialQueryInputs,
) -> TrainingResultSpatialQueryResult<BackendOutcome> {
  let provider_inputs = CheckpointNativeProviderInputs::from_spatial_query(semantic_manifest, inputs);

  if semantic_manifest.semantic_status != StageStatus::Ready {
    return Ok(
      blocked_answer(
        TrainingResultSpatialQueryReason::SemanticSourceNotReady,
        "MC-15 checkpoint_native provider requires MC-10 semantic_status=ready",
      )
      .into_backend_outcome(),
    );
  }

  if path_is_symlink(&provider_inputs.normalized_result_dir)
    || path_is_symlink(&provider_inputs.config_path)
    || path_is_symlink(&provider_inputs.models_dir_path)
  {
    return Ok(
      blocked_answer(
        TrainingResultSpatialQueryReason::SemanticSourceNotReady,
        "MC-15 checkpoint_native provider blocked: normalized result paths are invalid (symlink)",
      )
      .into_backend_outcome(),
    );
  }

  if !is_real_file(&provider_inputs.config_path) {
    return Ok(
      blocked_answer(
        TrainingResultSpatialQueryReason::SemanticSourceNotReady,
        format!("MC-15 checkpoint_native provider blocked: normalized config missing at {}", provider_inputs.config_path.display()),
      )
      .into_backend_outcome(),
    );
  }

  if !is_real_dir(&provider_inputs.models_dir_path) {
    return Ok(
      blocked_answer(
        TrainingResultSpatialQueryReason::SemanticSourceNotReady,
        format!(
          "MC-15 checkpoint_native provider blocked: normalized models directory missing at {}",
          provider_inputs.models_dir_path.display()
        ),
      )
      .into_backend_outcome(),
    );
  }

  let checkpoint_files = match collect_checkpoint_files(&provider_inputs.models_dir_path) {
    Ok(files) => files,
    Err(error) => {
      return Ok(
        failed_answer(
          TrainingResultSpatialQueryReason::ProviderCommandFailed,
          format!(
            "MC-15 checkpoint_native provider failed to scan checkpoints under {}: {}",
            provider_inputs.models_dir_path.display(),
            error.cause
          ),
        )
        .into_backend_outcome(),
      );
    }
  };

  if checkpoint_files.is_empty() {
    return Ok(
      blocked_answer(
        TrainingResultSpatialQueryReason::SemanticSourceNotReady,
        format!(
          "MC-15 checkpoint_native provider blocked: no readable checkpoint files under {}",
          provider_inputs.models_dir_path.display()
        ),
      )
      .into_backend_outcome(),
    );
  }

  let checkpoint_basis = checkpoint_basis_frame_id(&checkpoint_files);
  let reference_outcome = run_projection_reference_backend(scene_packet_manifest, scene_packet_dir, inputs)?;

  let answer = match reference_outcome.answer.status {
    TrainingResultSpatialQueryStatus::Answered => TrainingResultSpatialQueryAnswer {
      status: TrainingResultSpatialQueryStatus::Answered,
      reason: None,
      message: Some(MC15_V1_PROVIDER_MESSAGE.to_string()),
      basis_frame_id: Some(checkpoint_basis),
      visibility: reference_outcome.answer.visibility,
      screen_point: reference_outcome.answer.screen_point,
      match_radius_px: reference_outcome.answer.match_radius_px,
      confidence: reference_outcome.answer.confidence,
    },
    other_status => TrainingResultSpatialQueryAnswer {
      status: other_status,
      reason: reference_outcome.answer.reason,
      message: reference_outcome.answer.message.or_else(|| Some(MC15_V1_PROVIDER_MESSAGE.to_string())),
      basis_frame_id: Some(checkpoint_basis),
      visibility: reference_outcome.answer.visibility,
      screen_point: reference_outcome.answer.screen_point,
      match_radius_px: reference_outcome.answer.match_radius_px,
      confidence: reference_outcome.answer.confidence,
    },
  };

  Ok(CheckpointNativeProviderOutcome { answer }.into_backend_outcome())
}

pub(crate) fn run_closed_scene_toy_provider_backend(
  semantic_manifest: &TrainingResultSemanticManifest,
  inputs: &TrainingResultSpatialQueryInputs,
  closed_scene_fixture_path: Option<&Path>,
) -> TrainingResultSpatialQueryResult<BackendOutcome> {
  let provider_inputs =
    ClosedSceneToyProviderInputs::from_spatial_query(semantic_manifest, inputs, closed_scene_fixture_path.map(Path::to_path_buf));

  if semantic_manifest.semantic_status != StageStatus::Ready {
    return Ok(
      toy_blocked_answer(
        TrainingResultSpatialQueryReason::SemanticSourceNotReady,
        "MC-18 closed_scene_toy provider requires MC-10 semantic_status=ready",
      )
      .into_backend_outcome(),
    );
  }

  let Some(fixture_path) = provider_inputs.closed_scene_fixture_path.as_deref() else {
    return Ok(
      toy_blocked_answer(
        TrainingResultSpatialQueryReason::ProviderOutputInvalid,
        "MC-18 closed_scene_toy provider blocked: closed-scene fixture path missing",
      )
      .into_backend_outcome(),
    );
  };

  if path_is_symlink(fixture_path) {
    return Ok(
      toy_blocked_answer(
        TrainingResultSpatialQueryReason::ProviderOutputInvalid,
        "MC-18 closed_scene_toy provider blocked: closed-scene fixture path is invalid (symlink)",
      )
      .into_backend_outcome(),
    );
  }

  if !is_real_file(fixture_path) {
    return Ok(
      toy_blocked_answer(
        TrainingResultSpatialQueryReason::ProviderOutputInvalid,
        format!("MC-18 closed_scene_toy provider blocked: closed-scene fixture missing at {}", fixture_path.display()),
      )
      .into_backend_outcome(),
    );
  }

  let fixture = match load_closed_scene_fixture(fixture_path) {
    Ok(fixture) => fixture,
    Err(error) => {
      return Ok(
        toy_failed_answer(
          TrainingResultSpatialQueryReason::ProviderOutputInvalid,
          format!("MC-18 closed_scene_toy provider failed: {}", error.message()),
        )
        .into_backend_outcome(),
      );
    }
  };

  let answer = crate::closed_scene_toy_fixture::resolve_closed_label_answer(&fixture, &provider_inputs.request);
  Ok(ClosedSceneToyProviderOutcome { answer }.into_backend_outcome())
}

fn blocked_answer(reason: TrainingResultSpatialQueryReason, message: impl Into<String>) -> CheckpointNativeProviderOutcome {
  CheckpointNativeProviderOutcome {
    answer: TrainingResultSpatialQueryAnswer {
      status: TrainingResultSpatialQueryStatus::Blocked,
      reason: Some(reason),
      message: Some(message.into()),
      basis_frame_id: None,
      visibility: None,
      screen_point: None,
      match_radius_px: None,
      confidence: None,
    },
  }
}

fn toy_blocked_answer(reason: TrainingResultSpatialQueryReason, message: impl Into<String>) -> ClosedSceneToyProviderOutcome {
  toy_status_answer(TrainingResultSpatialQueryStatus::Blocked, reason, message)
}

fn toy_failed_answer(reason: TrainingResultSpatialQueryReason, message: impl Into<String>) -> ClosedSceneToyProviderOutcome {
  toy_status_answer(TrainingResultSpatialQueryStatus::Failed, reason, message)
}

fn toy_status_answer(
  status: TrainingResultSpatialQueryStatus,
  reason: TrainingResultSpatialQueryReason,
  message: impl Into<String>,
) -> ClosedSceneToyProviderOutcome {
  ClosedSceneToyProviderOutcome {
    answer: TrainingResultSpatialQueryAnswer {
      status,
      reason: Some(reason),
      message: Some(message.into()),
      basis_frame_id: None,
      visibility: None,
      screen_point: None,
      match_radius_px: None,
      confidence: None,
    },
  }
}

fn failed_answer(reason: TrainingResultSpatialQueryReason, message: impl Into<String>) -> CheckpointNativeProviderOutcome {
  CheckpointNativeProviderOutcome {
    answer: TrainingResultSpatialQueryAnswer {
      status: TrainingResultSpatialQueryStatus::Failed,
      reason: Some(reason),
      message: Some(message.into()),
      basis_frame_id: None,
      visibility: None,
      screen_point: None,
      match_radius_px: None,
      confidence: None,
    },
  }
}

fn checkpoint_basis_frame_id(checkpoints: &[TrainingResultSemanticCheckpointRecord]) -> String {
  let latest = checkpoints
    .iter()
    .max_by(|left, right| compare_checkpoint_steps(&left.relative_path, &right.relative_path))
    .map(|checkpoint| checkpoint.relative_path.as_str())
    .unwrap_or("unknown");
  format!("checkpoint:{latest}")
}

fn compare_checkpoint_steps(left: &str, right: &str) -> std::cmp::Ordering {
  checkpoint_step_number(left).cmp(&checkpoint_step_number(right)).then_with(|| left.cmp(right))
}

fn checkpoint_step_number(path: &str) -> u64 {
  path
    .rsplit('/')
    .next()
    .and_then(|file_name| file_name.strip_prefix("step-"))
    .and_then(|stem| stem.strip_suffix(".ckpt"))
    .and_then(|step| step.parse().ok())
    .unwrap_or(0)
}

fn path_is_symlink(path: &Path) -> bool {
  path.symlink_metadata().map(|metadata| metadata.file_type().is_symlink()).unwrap_or(false)
}

fn is_real_file(path: &Path) -> bool {
  path.metadata().map(|metadata| metadata.is_file()).unwrap_or(false)
}

fn is_real_dir(path: &Path) -> bool {
  path.metadata().map(|metadata| metadata.is_dir()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene_packet::{ScenePacketCounts, ScenePacketFrameRecord, ScenePacketManifest};
  use crate::training_result::TrainingResultStatus;
  use crate::training_result_semantic::TrainingResultSemanticManifest;
  use crate::types::{
    BlockFace, MinecraftSpatialFrame, MinecraftTargetSemantics, PlayerPose, ProjectionVisibility, RaycastHit, Vec3, Viewport,
  };
  use std::fs;
  use tempfile::TempDir;

  fn identity_matrix() -> [f64; 16] {
    [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
  }

  fn write_json_file<T: serde::Serialize>(path: &Path, value: &T) {
    let bytes = serde_json::to_vec_pretty(value).expect("serialize fixture");
    fs::write(path, bytes).expect("write fixture");
  }

  fn write_semantic_manifest(
    temp: &TempDir,
    semantic_status: StageStatus,
    scene_packet_manifest_path: &Path,
    with_checkpoint: bool,
  ) -> (PathBuf, PathBuf) {
    let normalized_dir = temp.path().join("normalized");
    let config_path = normalized_dir.join("config.yml");
    let models_dir = normalized_dir.join("nerfstudio_models");
    fs::create_dir_all(&models_dir).expect("models dir");
    fs::write(&config_path, "trainer: nerfstudio.splatfacto\n").expect("config");

    if with_checkpoint {
      fs::write(models_dir.join("step-000001.ckpt"), b"fake-checkpoint-bytes").expect("ckpt");
    }

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
      config_path: config_path.to_string_lossy().into_owned(),
      models_dir_path: models_dir.to_string_lossy().into_owned(),
      status_snapshot_path: None,
      config_trainer: Some("nerfstudio.splatfacto".to_string()),
      checkpoint_files: Vec::new(),
      checkpoint_count: if with_checkpoint { 1 } else { 0 },
      known_limits: vec!["fixture".to_string()],
    };
    let path = temp.path().join("semantic.json");
    write_json_file(&path, &manifest);
    (path, normalized_dir)
  }

  fn write_scene_packet_fixture(temp: &TempDir, target_block: BlockPosition) -> PathBuf {
    let output_dir = temp.path().join("scene-packet");
    fs::create_dir_all(output_dir.join("frames")).expect("frames dir");
    let frame = MinecraftSpatialFrame {
      spatial_frame_id: "frame-1".to_string(),
      world_tick: 1,
      monotonic_timestamp_ms: 100,
      telemetry_session_id: None,
      viewport: Viewport::new(800, 600),
      view_matrix: identity_matrix(),
      projection_matrix: identity_matrix(),
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
    };
    write_json_file(&output_dir.join("frames/frame_000001.json"), &frame);
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
    write_json_file(&manifest_path, &manifest);
    manifest_path
  }

  #[test]
  fn checkpoint_missing_blocks_provider() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block);
    let (semantic_manifest_path, _) = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path, false);
    let semantic_manifest: TrainingResultSemanticManifest =
      serde_json::from_slice(&fs::read(&semantic_manifest_path).expect("read semantic")).expect("parse semantic");
    let scene_packet_manifest: ScenePacketManifest =
      serde_json::from_slice(&fs::read(&scene_packet_manifest_path).expect("read scene packet")).expect("parse scene packet");
    let inputs = TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: true,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    };

    let outcome = run_checkpoint_native_provider_backend(
      &semantic_manifest,
      &scene_packet_manifest,
      scene_packet_manifest_path.parent().expect("parent"),
      &inputs,
    )
    .expect("checkpoint missing should return outcome");

    assert_eq!(outcome.answer.status, TrainingResultSpatialQueryStatus::Blocked);
    assert!(outcome.answer.message.as_deref().is_some_and(|message| message.contains("no readable checkpoint")));
  }

  #[test]
  fn checkpoint_present_visible_target_answers_with_checkpoint_basis() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block);
    let (semantic_manifest_path, _) = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path, true);
    let semantic_manifest: TrainingResultSemanticManifest =
      serde_json::from_slice(&fs::read(&semantic_manifest_path).expect("read semantic")).expect("parse semantic");
    let scene_packet_manifest: ScenePacketManifest =
      serde_json::from_slice(&fs::read(&scene_packet_manifest_path).expect("read scene packet")).expect("parse scene packet");
    let inputs = TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: true,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    };

    let outcome = run_checkpoint_native_provider_backend(
      &semantic_manifest,
      &scene_packet_manifest,
      scene_packet_manifest_path.parent().expect("parent"),
      &inputs,
    )
    .expect("checkpoint present should answer");

    assert_eq!(outcome.answer.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(outcome.answer.basis_frame_id.as_deref(), Some("checkpoint:step-000001.ckpt"));
    assert!(outcome.answer.message.as_deref().is_some_and(|message| message.contains("Gaussian render inference is deferred")));
    assert!(outcome.answer.screen_point.is_some());
  }

  #[test]
  fn semantic_not_ready_blocks_checkpoint_native_provider() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block);
    let (semantic_manifest_path, _) = write_semantic_manifest(&temp, StageStatus::Blocked, &scene_packet_manifest_path, true);
    let semantic_manifest: TrainingResultSemanticManifest =
      serde_json::from_slice(&fs::read(&semantic_manifest_path).expect("read semantic")).expect("parse semantic");
    let scene_packet_manifest: ScenePacketManifest =
      serde_json::from_slice(&fs::read(&scene_packet_manifest_path).expect("read scene packet")).expect("parse scene packet");
    let inputs = TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: true,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir: temp.path().join("query-output"),
    };

    let outcome = run_checkpoint_native_provider_backend(
      &semantic_manifest,
      &scene_packet_manifest,
      scene_packet_manifest_path.parent().expect("parent"),
      &inputs,
    )
    .expect("semantic blocked should return outcome");

    assert_eq!(outcome.answer.status, TrainingResultSpatialQueryStatus::Blocked);
    assert_eq!(outcome.answer.reason, Some(TrainingResultSpatialQueryReason::SemanticSourceNotReady));
  }

  fn spatial_query_inputs(
    semantic_manifest_path: PathBuf,
    target_block: BlockPosition,
    output_dir: PathBuf,
  ) -> TrainingResultSpatialQueryInputs {
    TrainingResultSpatialQueryInputs {
      training_result_semantic_manifest_path: semantic_manifest_path,
      target_block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      query_command: None,
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: false,
      closed_scene_fixture_path: None,
      output_dir,
    }
  }

  fn mc18_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mc18").join(name)
  }

  #[test]
  fn missing_fixture_blocks_closed_scene_toy_provider() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block);
    let (semantic_manifest_path, _) = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path, false);
    let semantic_manifest: TrainingResultSemanticManifest =
      serde_json::from_slice(&fs::read(&semantic_manifest_path).expect("read semantic")).expect("parse semantic");
    let inputs = spatial_query_inputs(semantic_manifest_path, target_block, temp.path().join("query-output"));

    let outcome = run_closed_scene_toy_provider_backend(&semantic_manifest, &inputs, None).expect("missing fixture should return outcome");

    assert_eq!(outcome.answer.status, TrainingResultSpatialQueryStatus::Blocked);
    assert_eq!(outcome.answer.reason, Some(TrainingResultSpatialQueryReason::ProviderOutputInvalid));
    assert!(outcome.answer.message.as_deref().is_some_and(|message| message.contains("fixture path missing")));
  }

  #[test]
  fn semantic_not_ready_blocks_closed_scene_toy_provider() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(0, 0, 0);
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block);
    let (semantic_manifest_path, _) = write_semantic_manifest(&temp, StageStatus::Blocked, &scene_packet_manifest_path, false);
    let semantic_manifest: TrainingResultSemanticManifest =
      serde_json::from_slice(&fs::read(&semantic_manifest_path).expect("read semantic")).expect("parse semantic");
    let inputs = spatial_query_inputs(semantic_manifest_path, target_block, temp.path().join("query-output"));

    let outcome = run_closed_scene_toy_provider_backend(&semantic_manifest, &inputs, None).expect("semantic blocked should return outcome");

    assert_eq!(outcome.answer.status, TrainingResultSpatialQueryStatus::Blocked);
    assert_eq!(outcome.answer.reason, Some(TrainingResultSpatialQueryReason::SemanticSourceNotReady));
  }

  #[test]
  fn visible_fixture_answers_with_closed_scene_toy_basis() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(511, 73, 728);
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block);
    let (semantic_manifest_path, _) = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path, false);
    let semantic_manifest: TrainingResultSemanticManifest =
      serde_json::from_slice(&fs::read(&semantic_manifest_path).expect("read semantic")).expect("parse semantic");
    let mut inputs = spatial_query_inputs(semantic_manifest_path, target_block, temp.path().join("query-output"));
    inputs.target_face = Some(BlockFace::North);

    let outcome = run_closed_scene_toy_provider_backend(&semantic_manifest, &inputs, Some(mc18_fixture_path("visible.json").as_path()))
      .expect("visible fixture should answer");

    assert_eq!(outcome.answer.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(outcome.answer.basis_frame_id.as_deref(), Some("closed_scene_toy:mc18-smoke-v1:frame-0003"));
    assert_eq!(outcome.answer.visibility, Some(ProjectionVisibility::Visible));
    assert!(outcome.answer.screen_point.is_some());
  }

  #[test]
  fn outside_window_fixture_answers_without_clickable_visibility() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(511, 73, 728);
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block);
    let (semantic_manifest_path, _) = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path, false);
    let semantic_manifest: TrainingResultSemanticManifest =
      serde_json::from_slice(&fs::read(&semantic_manifest_path).expect("read semantic")).expect("parse semantic");
    let mut inputs = spatial_query_inputs(semantic_manifest_path, target_block, temp.path().join("query-output"));
    inputs.target_face = Some(BlockFace::North);

    let outcome =
      run_closed_scene_toy_provider_backend(&semantic_manifest, &inputs, Some(mc18_fixture_path("outside_window.json").as_path()))
        .expect("outside_window fixture should answer");

    assert_eq!(outcome.answer.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(outcome.answer.visibility, Some(ProjectionVisibility::OutsideWindow));
    assert!(outcome.answer.screen_point.is_some());
  }

  #[test]
  fn corrupt_fixture_json_fails_closed_scene_toy_provider() {
    let temp = TempDir::new().expect("tempdir");
    let target_block = BlockPosition::new(511, 73, 728);
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, target_block);
    let (semantic_manifest_path, _) = write_semantic_manifest(&temp, StageStatus::Ready, &scene_packet_manifest_path, false);
    let semantic_manifest: TrainingResultSemanticManifest =
      serde_json::from_slice(&fs::read(&semantic_manifest_path).expect("read semantic")).expect("parse semantic");
    let inputs = spatial_query_inputs(semantic_manifest_path, target_block, temp.path().join("query-output"));
    let corrupt_fixture = temp.path().join("corrupt.json");
    fs::write(&corrupt_fixture, b"{not-json").expect("write corrupt");

    let outcome = run_closed_scene_toy_provider_backend(&semantic_manifest, &inputs, Some(corrupt_fixture.as_path()))
      .expect("corrupt fixture should return outcome");

    assert_eq!(outcome.answer.status, TrainingResultSpatialQueryStatus::Failed);
    assert_eq!(outcome.answer.reason, Some(TrainingResultSpatialQueryReason::ProviderOutputInvalid));
  }
}
