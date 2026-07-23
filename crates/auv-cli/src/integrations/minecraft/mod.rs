use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::StreamExt;
use image::{DynamicImage, ImageFormat};
use sha2::{Digest, Sha256};

pub mod help;
pub mod projection_workflow;
pub mod query_live_action;
pub mod session;
pub mod verification;

use auv_game_minecraft::{
  MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, MinecraftArtifactReadError, MinecraftProjectionArtifact, MinecraftProjector,
  MinecraftSpatialFrame, QueryActionWiringOutcome, QueryLiveClickExecutor, ScenePacketInputs, ScenePacketOutput, SourceRunSummary,
  SpatialBundleInputs, SpatialBundleSourceArtifact, TextureSweepInputs, TextureSweepPreparationInputs, TextureSweepPreparationOutput,
  TextureSweepReport, TextureSweepSampleBuildInputs, TextureSweepSampleBuildOutput, TextureSweepThresholds, TrainingLaunchJobInputs,
  TrainingLaunchPreparationInputs, TrainingLaunchPreparationOutput, TrainingPackageInputs, TrainingPackageOutput,
  TrainingResultArtifactFetchInputs, TrainingResultArtifactFetchOutput, TrainingResultHoldoutPreviewInputs,
  TrainingResultHoldoutPreviewOutput, TrainingResultHoldoutRenderQualityInputs, TrainingResultHoldoutRenderQualityOutput,
  TrainingResultInputs, TrainingResultOutput, TrainingResultSemanticValidationInputs, TrainingResultSemanticValidationOutput,
  TrainingResultSpatialQueryInputs, TrainingResultSpatialQueryManifest, TrainingResultSpatialQueryOutput,
  build_texture_sweep_samples_from_bundles, collect_3dgs_training_job_result, collect_3dgs_training_job_result_with_environment,
  evaluate_texture_sweep, export_3dgs_scene_packet, export_3dgs_training_package, export_spatial_bundle,
  fetch_3dgs_training_result_artifacts_with_environment, inspect_3dgs_training_result_holdout, launch_3dgs_training_job,
  launch_3dgs_training_job_with_environment, measure_3dgs_holdout_render_quality, prepare_3dgs_training_launch,
  prepare_texture_sweep_resource_packs, query_3dgs_training_result, query_action_wiring_lineage_from_manifest,
  validate_3dgs_training_result, wire_query_manifest_to_action,
};
use auv_runtime::contract::VerificationResult;
use auv_runtime::model::AuvResult;
use auv_tracing::{ArtifactPurpose, ArtifactUri, ContentType, Context, RunId, RunSnapshot, RunStore, Sha256Digest};

use self::query_live_action::DirectWindowPointClickExecutor;

pub use auv_game_minecraft::artifact_roles::*;

pub const MINECRAFT_SPATIAL_BUNDLE_PURPOSE: &str = "auv.minecraft.spatial_bundle";

pub async fn run_minecraft_3dgs_scene_packet_export(
  bundle_manifest_paths: Vec<PathBuf>,
  output_dir: PathBuf,
) -> AuvResult<ScenePacketOutput> {
  let result = export_3dgs_scene_packet(ScenePacketInputs {
    bundle_manifest_paths,
    output_dir,
  })?;
  let context = Context::current();
  let _ = auv_game_minecraft::scene_packet::publish_minecraft_scene_packet(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_minecraft_3dgs_training_package_export(
  scene_packet_manifest_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<TrainingPackageOutput> {
  let result = export_3dgs_training_package(TrainingPackageInputs {
    scene_packet_manifest_path,
    output_dir,
  })?;
  let context = Context::current();
  let _ = auv_game_minecraft::training_package::publish_minecraft_training_package(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_minecraft_texture_sweep_preparation(
  sidecar_run_dir: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<TextureSweepPreparationOutput> {
  prepare_texture_sweep_resource_packs(TextureSweepPreparationInputs {
    sidecar_run_dir,
    output_dir,
  })
}

pub async fn run_minecraft_3dgs_training_launch_preparation(
  training_package_manifest_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<TrainingLaunchPreparationOutput> {
  prepare_3dgs_training_launch(TrainingLaunchPreparationInputs {
    training_package_manifest_path,
    output_dir,
  })
}

pub async fn run_minecraft_3dgs_training_job_launch(
  training_launch_plan_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<auv_game_minecraft::TrainingLaunchJobOutput> {
  run_minecraft_3dgs_training_job_launch_with_environment(training_launch_plan_path, output_dir, None, None, None).await
}

pub async fn run_minecraft_3dgs_training_job_launch_with_environment(
  training_launch_plan_path: PathBuf,
  output_dir: PathBuf,
  training_job_endpoint: Option<String>,
  training_job_token: Option<String>,
  training_job_submit_command: Option<String>,
) -> AuvResult<auv_game_minecraft::TrainingLaunchJobOutput> {
  let inputs = TrainingLaunchJobInputs {
    training_launch_plan_path,
    output_dir,
  };
  let result = if training_job_endpoint.is_some() || training_job_token.is_some() || training_job_submit_command.is_some() {
    launch_3dgs_training_job_with_environment(
      inputs,
      auv_game_minecraft::TrainingJobEnvironment::with_values(training_job_endpoint, training_job_token, training_job_submit_command),
    )?
  } else {
    launch_3dgs_training_job(inputs)?
  };
  let context = Context::current();
  let _ = auv_game_minecraft::training_job::publish_minecraft_training_job(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_minecraft_3dgs_training_result_collection(
  training_job_manifest_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<TrainingResultOutput> {
  run_minecraft_3dgs_training_result_collection_with_environment(training_job_manifest_path, output_dir, None, None, None).await
}

pub async fn run_minecraft_3dgs_training_result_collection_with_environment(
  training_job_manifest_path: PathBuf,
  output_dir: PathBuf,
  training_job_endpoint: Option<String>,
  training_job_token: Option<String>,
  training_job_status_command: Option<String>,
) -> AuvResult<TrainingResultOutput> {
  let inputs = TrainingResultInputs {
    training_job_manifest_path,
    output_dir,
  };
  let result = if training_job_endpoint.is_some() || training_job_token.is_some() || training_job_status_command.is_some() {
    collect_3dgs_training_job_result_with_environment(
      inputs,
      auv_game_minecraft::TrainingResultEnvironment::with_values(training_job_endpoint, training_job_token, training_job_status_command),
    )?
  } else {
    collect_3dgs_training_job_result(inputs)?
  };
  let context = Context::current();
  let _ = auv_game_minecraft::training_result::publish_minecraft_training_result(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_minecraft_3dgs_training_result_artifact_fetch(
  training_result_manifest_path: PathBuf,
  output_dir: PathBuf,
  training_job_endpoint: Option<String>,
  training_job_token: Option<String>,
  artifact_fetch_command: Option<String>,
) -> AuvResult<TrainingResultArtifactFetchOutput> {
  fetch_3dgs_training_result_artifacts_with_environment(
    TrainingResultArtifactFetchInputs {
      training_result_manifest_path,
      output_dir,
    },
    auv_game_minecraft::TrainingResultArtifactFetchEnvironment::with_values(
      training_job_endpoint,
      training_job_token,
      artifact_fetch_command,
    ),
  )
}

pub async fn run_minecraft_3dgs_training_result_semantic_validation(
  training_result_artifact_manifest_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<TrainingResultSemanticValidationOutput> {
  let result = validate_3dgs_training_result(TrainingResultSemanticValidationInputs {
    training_result_artifact_manifest_path,
    output_dir,
  })?;
  let context = Context::current();
  let _ = auv_game_minecraft::training_result_semantic::publish_minecraft_training_semantic(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_minecraft_3dgs_training_result_holdout_preview(
  training_result_semantic_manifest_path: PathBuf,
  holdout_frame_index: Option<usize>,
  holdout_render_command: Option<String>,
  output_dir: PathBuf,
) -> AuvResult<TrainingResultHoldoutPreviewOutput> {
  let result = inspect_3dgs_training_result_holdout(TrainingResultHoldoutPreviewInputs {
    training_result_semantic_manifest_path,
    output_dir,
    holdout_frame_index,
    holdout_render_command,
  })?;
  let context = Context::current();
  let _ =
    auv_game_minecraft::training_result_holdout_preview::publish_minecraft_training_holdout_preview(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_minecraft_measure_3dgs_holdout_render_quality(
  training_result_semantic_manifest_path: PathBuf,
  holdout_preview_manifest_path: PathBuf,
  render_command: String,
  output_dir: PathBuf,
) -> AuvResult<TrainingResultHoldoutRenderQualityOutput> {
  let result = measure_3dgs_holdout_render_quality(TrainingResultHoldoutRenderQualityInputs {
    training_result_semantic_manifest_path,
    holdout_preview_manifest_path,
    render_command,
    output_dir,
  })?;
  let context = Context::current();
  let _ = auv_game_minecraft::training_result_holdout_render_quality::publish_minecraft_training_holdout_render_quality(
    Some(&context),
    &result.manifest,
  )
  .await;
  Ok(result)
}

#[allow(clippy::too_many_arguments)]
pub async fn run_minecraft_3dgs_training_result_spatial_query(
  training_result_semantic_manifest_path: PathBuf,
  target_block: auv_game_minecraft::BlockPosition,
  target_face: Option<auv_game_minecraft::BlockFace>,
  target_semantics: auv_game_minecraft::MinecraftTargetSemantics,
  query_command: Option<String>,
  use_checkpoint_native_provider: bool,
  use_closed_scene_toy_provider: bool,
  closed_scene_fixture_path: Option<PathBuf>,
  output_dir: PathBuf,
) -> AuvResult<TrainingResultSpatialQueryOutput> {
  let result = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
    training_result_semantic_manifest_path,
    target_block,
    target_face,
    target_semantics,
    query_command,
    use_checkpoint_native_provider,
    use_closed_scene_toy_provider,
    closed_scene_fixture_path,
    output_dir,
  })?;
  let context = Context::current();
  let _ =
    auv_game_minecraft::training_result_spatial_query::publish_minecraft_training_spatial_query(Some(&context), &result.manifest).await;
  Ok(result)
}

pub fn wire_spatial_query_manifest_to_action(
  manifest: &TrainingResultSpatialQueryManifest,
  manifest_path: &Path,
  executor: &impl QueryLiveClickExecutor,
) -> QueryActionWiringOutcome {
  let lineage = query_action_wiring_lineage_from_manifest(manifest, manifest_path);
  wire_query_manifest_to_action(manifest, &lineage, executor)
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryWiredLiveActionTelemetryWitness {
  pub pre_telemetry_sample: PathBuf,
  pub post_telemetry_sample: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryWiredLiveActionInputs {
  pub training_result_semantic_manifest_path: PathBuf,
  pub target_block: auv_game_minecraft::BlockPosition,
  pub target_face: Option<auv_game_minecraft::BlockFace>,
  pub target_semantics: auv_game_minecraft::MinecraftTargetSemantics,
  pub query_command: Option<String>,
  pub use_checkpoint_native_provider: bool,
  pub use_closed_scene_toy_provider: bool,
  pub closed_scene_fixture_path: Option<PathBuf>,
  pub output_dir: PathBuf,
  pub target_app: String,
  pub target_title: String,
  pub telemetry_witness: Option<QueryWiredLiveActionTelemetryWitness>,
  pub verification_expected_item_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryWiredLiveActionOutput {
  pub query: TrainingResultSpatialQueryOutput,
  pub wiring: QueryActionWiringOutcome,
  pub verifications: Vec<VerificationResult>,
  pub input_actions: Vec<auv_driver::InputActionResult>,
}

pub async fn run_minecraft_query_wired_live_action(inputs: QueryWiredLiveActionInputs) -> AuvResult<QueryWiredLiveActionOutput> {
  let executor = DirectWindowPointClickExecutor::new(inputs.target_app.clone(), inputs.target_title.clone());
  let mut output = run_query_wired_live_action_core(&inputs, &executor).await?;
  output.input_actions = executor.actions();
  let context = Context::current();
  for action in &output.input_actions {
    let _ = auv_runtime::run_read::publish_input_action_result(Some(&context), action).await;
  }
  Ok(output)
}

pub async fn run_minecraft_query_wired_live_action_with_executor<E: QueryLiveClickExecutor>(
  inputs: QueryWiredLiveActionInputs,
  executor: &E,
) -> AuvResult<QueryWiredLiveActionOutput> {
  run_query_wired_live_action_core(&inputs, executor).await
}

async fn run_query_wired_live_action_core<E: QueryLiveClickExecutor>(
  inputs: &QueryWiredLiveActionInputs,
  executor: &E,
) -> AuvResult<QueryWiredLiveActionOutput> {
  let query = run_minecraft_3dgs_training_result_spatial_query(
    inputs.training_result_semantic_manifest_path.clone(),
    inputs.target_block,
    inputs.target_face,
    inputs.target_semantics,
    inputs.query_command.clone(),
    inputs.use_checkpoint_native_provider,
    inputs.use_closed_scene_toy_provider,
    inputs.closed_scene_fixture_path.clone(),
    inputs.output_dir.clone(),
  )
  .await?;
  let pre_frame = if let Some(witness) = inputs.telemetry_witness.as_ref() {
    Some(
      auv_game_minecraft::read_latest_spatial_frame_from_tail(&witness.pre_telemetry_sample)?
        .ok_or_else(|| format!("no valid minecraft pre frame found in {}", witness.pre_telemetry_sample.display()))?,
    )
  } else {
    None
  };
  let mut wiring = wire_spatial_query_manifest_to_action(&query.manifest, &query.manifest_path, executor);
  let (verifications, witness_absent_limit_needed) = verification::build_query_wired_post_action_verifications(
    &wiring,
    verification::QueryWiredPostActionVerificationInput {
      telemetry_witness: inputs.telemetry_witness.as_ref(),
      input_target_block: inputs.target_block,
      manifest_target_block: query.manifest.target_block,
      pre_frame,
      verification_expected_item_id: inputs.verification_expected_item_id.clone(),
    },
  );
  if wiring.attempted && !verifications.is_empty() {
    wiring.known_limits.retain(|limit| limit != auv_game_minecraft::MC19_V1_D4_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT);
    if witness_absent_limit_needed {
      wiring.known_limits.push(auv_game_minecraft::MC20_V1_QUERY_WIRED_WITNESS_ABSENT_KNOWN_LIMIT.to_string());
    }
  }
  Ok(QueryWiredLiveActionOutput {
    query,
    wiring,
    verifications,
    input_actions: Vec::new(),
  })
}

pub async fn run_minecraft_texture_sweep_sample_build(
  bundle_manifest_paths: Vec<PathBuf>,
  output_path: PathBuf,
) -> AuvResult<TextureSweepSampleBuildOutput> {
  build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
    bundle_manifest_paths,
    output_path,
  })
}

pub async fn run_minecraft_spatial_bundle_export(
  store: Arc<dyn RunStore>,
  source_run_id: String,
  output_dir: PathBuf,
  git_commit: Option<String>,
) -> AuvResult<auv_game_minecraft::SpatialBundleOutput> {
  let source_run_id = source_run_id.parse::<RunId>().map_err(|error| format!("invalid Minecraft source run id: {error}"))?;
  let snapshot = store
    .load_snapshot(source_run_id)
    .await
    .map_err(|error| format!("failed to read Minecraft source run {source_run_id}: {error}"))?
    .ok_or_else(|| format!("Minecraft source run {source_run_id} was not found"))?;
  let staging_dir = std::env::temp_dir().join(format!("auv-minecraft-bundle-source-{}-{}", source_run_id, auv_runtime::model::now_millis()));
  fs::create_dir_all(&staging_dir)
    .map_err(|error| format!("failed to create Minecraft bundle source staging directory {}: {error}", staging_dir.display()))?;
  let artifacts = read_spatial_bundle_artifacts(store.as_ref(), &snapshot)
    .await
    .and_then(|artifacts| stage_spatial_bundle_artifacts(artifacts, &staging_dir));
  let result = match artifacts {
    Ok(artifacts) => export_spatial_bundle(SpatialBundleInputs {
      output_dir,
      source: source_run_summary(&snapshot, git_commit),
      artifacts,
    }),
    Err(error) => Err(error),
  };
  let _ = fs::remove_dir_all(&staging_dir);
  let result = result?;
  let _ = projection_workflow::publish_json_artifact(MINECRAFT_SPATIAL_BUNDLE_PURPOSE, &result.manifest).await;
  Ok(result)
}

enum ValidatedMinecraftBundleArtifact {
  Screenshot {
    artifact_id: String,
    image: DynamicImage,
  },
  SpatialFrame {
    artifact_id: String,
    frame: Box<MinecraftSpatialFrame>,
  },
  Projection {
    artifact_id: String,
    projection: Box<MinecraftProjectionArtifact>,
  },
  Overlay {
    artifact_id: String,
    image: DynamicImage,
  },
}

async fn read_spatial_bundle_artifacts(store: &dyn RunStore, snapshot: &RunSnapshot) -> AuvResult<Vec<ValidatedMinecraftBundleArtifact>> {
  validate_minecraft_bundle_snapshot_authority(store, snapshot)
    .map_err(|error| format!("failed to validate Minecraft bundle source snapshot: {}: {error}", error.code()))?;
  let mut artifacts = Vec::new();
  for published in snapshot.artifacts().values() {
    let metadata = published.metadata();
    let uri = metadata.uri();
    let artifact_id = uri.artifact_id().to_string();
    let artifact = match metadata.purpose().as_str() {
      projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE => ValidatedMinecraftBundleArtifact::Screenshot {
        artifact_id,
        image: read_minecraft_screenshot(store, snapshot, uri)
          .await
          .map_err(|error| minecraft_bundle_read_error("screenshot", uri, error))?,
      },
      projection_workflow::MINECRAFT_SPATIAL_FRAME_PURPOSE => ValidatedMinecraftBundleArtifact::SpatialFrame {
        artifact_id,
        frame: Box::new(
          read_minecraft_spatial_frame(store, snapshot, uri)
            .await
            .map_err(|error| minecraft_bundle_read_error("spatial-frame", uri, error))?,
        ),
      },
      auv_game_minecraft::artifact::MINECRAFT_PROJECTION_PURPOSE => ValidatedMinecraftBundleArtifact::Projection {
        artifact_id,
        projection: Box::new(
          auv_game_minecraft::artifact::read_minecraft_projection(store, snapshot, uri)
            .await
            .map_err(|error| minecraft_bundle_read_error("projection", uri, error))?,
        ),
      },
      projection_workflow::MINECRAFT_OVERLAY_PURPOSE => ValidatedMinecraftBundleArtifact::Overlay {
        artifact_id,
        image: read_minecraft_projection_overlay(store, snapshot, uri)
          .await
          .map_err(|error| minecraft_bundle_read_error("projection-overlay", uri, error))?,
      },
      _ => continue,
    };
    artifacts.push(artifact);
  }
  Ok(artifacts)
}

#[derive(Clone, Copy)]
struct SpatialBundleStagingSemantics {
  role: &'static str,
  directory: &'static str,
  file_name: &'static str,
  summary: &'static str,
}

fn stage_spatial_bundle_artifacts(
  artifacts: Vec<ValidatedMinecraftBundleArtifact>,
  staging_dir: &Path,
) -> AuvResult<Vec<SpatialBundleSourceArtifact>> {
  artifacts.into_iter().map(|artifact| stage_spatial_bundle_artifact(artifact, staging_dir)).collect()
}

fn stage_spatial_bundle_artifact(artifact: ValidatedMinecraftBundleArtifact, staging_dir: &Path) -> AuvResult<SpatialBundleSourceArtifact> {
  let (artifact_id, semantics, bytes) = match artifact {
    ValidatedMinecraftBundleArtifact::Screenshot { artifact_id, image } => (
      artifact_id,
      SpatialBundleStagingSemantics {
        role: "minecraft-screenshot",
        directory: "screenshots",
        file_name: "screenshot.png",
        summary: "validated Minecraft screenshot bundle input",
      },
      encode_bundle_png(image, "screenshot")?,
    ),
    ValidatedMinecraftBundleArtifact::SpatialFrame { artifact_id, frame } => (
      artifact_id,
      SpatialBundleStagingSemantics {
        role: MINECRAFT_SPATIAL_FRAME_ARTIFACT_ROLE,
        directory: "spatial_frames",
        file_name: "spatial-frame.json",
        summary: "validated Minecraft spatial-frame bundle input",
      },
      encode_bundle_json(frame.as_ref(), "spatial frame")?,
    ),
    ValidatedMinecraftBundleArtifact::Projection {
      artifact_id,
      projection,
    } => (
      artifact_id,
      SpatialBundleStagingSemantics {
        role: MINECRAFT_PROJECTION_ARTIFACT_ROLE,
        directory: "spatial_frames",
        file_name: "projection.json",
        summary: "validated Minecraft projection bundle input",
      },
      encode_bundle_json(projection.as_ref(), "projection")?,
    ),
    ValidatedMinecraftBundleArtifact::Overlay { artifact_id, image } => (
      artifact_id,
      SpatialBundleStagingSemantics {
        role: "minecraft-overlay",
        directory: "overlays",
        file_name: "projection-overlay.png",
        summary: "validated Minecraft projection-overlay bundle input",
      },
      encode_bundle_png(image, "projection overlay")?,
    ),
  };
  let source_path = staging_dir.join(&artifact_id).join(semantics.file_name);
  if let Some(parent) = source_path.parent() {
    fs::create_dir_all(parent)
      .map_err(|error| format!("failed to create Minecraft bundle staging directory {}: {error}", parent.display()))?;
  }
  fs::write(&source_path, bytes).map_err(|error| format!("failed to stage Minecraft bundle input at {}: {error}", source_path.display()))?;

  // NOTICE(minecraft-bundle-staging-semantics): the domain exporter's legacy
  // input names require `role` and `source_run_path`. These values are private
  // bundle routing semantics, not RunSnapshot storage metadata. Remove this
  // adapter if the domain exporter accepts decoded typed payloads directly.
  let bundle_input_path = Path::new("bundle-inputs").join(semantics.directory).join(format!("{}-{}", artifact_id, semantics.file_name));
  Ok(SpatialBundleSourceArtifact {
    artifact_id,
    role: semantics.role.to_string(),
    source_path,
    source_run_path: bundle_input_path.to_string_lossy().into_owned(),
    summary: Some(semantics.summary.to_string()),
  })
}

fn encode_bundle_json(value: &impl serde::Serialize, kind: &str) -> AuvResult<Vec<u8>> {
  serde_json::to_vec_pretty(value).map_err(|error| format!("failed to encode validated Minecraft {kind} bundle input: {error}"))
}

fn encode_bundle_png(image: DynamicImage, kind: &str) -> AuvResult<Vec<u8>> {
  let mut output = std::io::Cursor::new(Vec::new());
  image
    .write_to(&mut output, ImageFormat::Png)
    .map_err(|error| format!("failed to encode validated Minecraft {kind} bundle input: {error}"))?;
  Ok(output.into_inner())
}

async fn read_minecraft_screenshot(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<DynamicImage, MinecraftArtifactReadError> {
  let bytes =
    read_minecraft_bundle_artifact_bytes(store, snapshot, uri, projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE, "image/png", None).await?;
  decode_minecraft_png(uri, bytes, "screenshot")
}

async fn read_minecraft_spatial_frame(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<MinecraftSpatialFrame, MinecraftArtifactReadError> {
  let bytes = read_minecraft_bundle_artifact_bytes(
    store,
    snapshot,
    uri,
    projection_workflow::MINECRAFT_SPATIAL_FRAME_PURPOSE,
    "application/json",
    Some(MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT),
  )
  .await?;
  let frame: MinecraftSpatialFrame = serde_json::from_slice(&bytes).map_err(|source| MinecraftArtifactReadError::MalformedJson {
    uri: uri.clone(),
    source,
  })?;
  MinecraftProjector::new(frame.clone()).map_err(|message| MinecraftArtifactReadError::InvalidPayload {
    uri: uri.clone(),
    message,
  })?;
  Ok(frame)
}

async fn read_minecraft_projection_overlay(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<DynamicImage, MinecraftArtifactReadError> {
  let bytes =
    read_minecraft_bundle_artifact_bytes(store, snapshot, uri, projection_workflow::MINECRAFT_OVERLAY_PURPOSE, "image/png", None).await?;
  decode_minecraft_png(uri, bytes, "projection-overlay")
}

// The canonical Minecraft error enum carries rich typed transport failures;
// preserving that contract here is more useful than hiding it behind a local error.
#[allow(clippy::result_large_err)]
fn decode_minecraft_png(uri: &ArtifactUri, bytes: Vec<u8>, kind: &str) -> Result<DynamicImage, MinecraftArtifactReadError> {
  image::load_from_memory_with_format(&bytes, ImageFormat::Png).map_err(|error| MinecraftArtifactReadError::InvalidPayload {
    uri: uri.clone(),
    message: format!("{kind} PNG payload could not be decoded: {error}"),
  })
}

async fn read_minecraft_bundle_artifact_bytes(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  expected_purpose: &'static str,
  expected_content_type: &'static str,
  byte_limit: Option<u64>,
) -> Result<Vec<u8>, MinecraftArtifactReadError> {
  validate_minecraft_bundle_snapshot_authority(store, snapshot)?;
  if uri.run_id() != snapshot.run_id() {
    return Err(MinecraftArtifactReadError::WrongOwner {
      snapshot_run_id: snapshot.run_id(),
      artifact_run_id: uri.run_id(),
    });
  }
  let metadata = snapshot.artifacts().get(uri).ok_or_else(|| MinecraftArtifactReadError::DanglingUri { uri: uri.clone() })?.metadata();
  let expected_purpose = ArtifactPurpose::parse(expected_purpose).map_err(|source| MinecraftArtifactReadError::InvalidExpectedPurpose {
    value: expected_purpose,
    source,
  })?;
  if metadata.purpose() != &expected_purpose {
    return Err(MinecraftArtifactReadError::WrongPurpose {
      uri: Box::new(uri.clone()),
      expected: expected_purpose,
      actual: metadata.purpose().clone(),
    });
  }
  let expected_content_type =
    ContentType::parse(expected_content_type).map_err(|source| MinecraftArtifactReadError::InvalidExpectedContentType {
      value: expected_content_type,
      source,
    })?;
  if metadata.content_type() != &expected_content_type {
    return Err(MinecraftArtifactReadError::WrongContentType {
      uri: Box::new(uri.clone()),
      expected: Box::new(expected_content_type),
      actual: Box::new(metadata.content_type().clone()),
    });
  }

  let expected_length = metadata.byte_length().get();
  if let Some(limit) = byte_limit
    && expected_length > limit
  {
    return Err(MinecraftArtifactReadError::PayloadTooLarge {
      uri: uri.clone(),
      limit,
      actual: expected_length,
    });
  }
  let expected_capacity = usize::try_from(expected_length).map_err(|_| MinecraftArtifactReadError::LengthOutOfRange {
    uri: uri.clone(),
    actual: expected_length,
  })?;
  let mut bytes = Vec::new();
  bytes.try_reserve_exact(expected_capacity).map_err(|source| MinecraftArtifactReadError::Allocation {
    uri: uri.clone(),
    expected: expected_length,
    source,
  })?;
  let mut reader = store.open_artifact(uri.clone()).await.map_err(|source| MinecraftArtifactReadError::Open {
    uri: uri.clone(),
    source,
  })?;
  let mut actual_length = 0_u64;
  while let Some(chunk) = reader.next().await {
    let chunk = chunk.map_err(|source| MinecraftArtifactReadError::Stream {
      uri: uri.clone(),
      source,
    })?;
    actual_length = actual_length.saturating_add(chunk.len() as u64);
    if let Some(limit) = byte_limit
      && actual_length > limit
    {
      return Err(MinecraftArtifactReadError::PayloadTooLarge {
        uri: uri.clone(),
        limit,
        actual: actual_length,
      });
    }
    if actual_length > expected_length {
      return Err(MinecraftArtifactReadError::LengthMismatch {
        uri: uri.clone(),
        expected: expected_length,
        actual: actual_length,
      });
    }
    bytes.extend_from_slice(&chunk);
  }
  if actual_length != expected_length {
    return Err(MinecraftArtifactReadError::LengthMismatch {
      uri: uri.clone(),
      expected: expected_length,
      actual: actual_length,
    });
  }
  let actual_digest = Sha256Digest::new(Sha256::digest(&bytes).into());
  if actual_digest != metadata.sha256() {
    return Err(MinecraftArtifactReadError::DigestMismatch {
      uri: Box::new(uri.clone()),
      expected: metadata.sha256(),
      actual: actual_digest,
    });
  }
  Ok(bytes)
}

#[allow(clippy::result_large_err)]
fn validate_minecraft_bundle_snapshot_authority(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<(), MinecraftArtifactReadError> {
  let store_authority = store.authority_id();
  if snapshot.authority_id() != store_authority {
    return Err(MinecraftArtifactReadError::SnapshotAuthorityMismatch {
      snapshot_authority: snapshot.authority_id(),
      store_authority,
    });
  }
  Ok(())
}

fn minecraft_bundle_read_error(kind: &str, uri: &ArtifactUri, error: MinecraftArtifactReadError) -> String {
  format!("failed to read typed Minecraft {kind} artifact {uri}: {}: {error}", error.code())
}

fn source_run_summary(snapshot: &RunSnapshot, git_commit: Option<String>) -> SourceRunSummary {
  let source_operation = snapshot
    .spans()
    .values()
    .find(|span| span.started().parent_span_id().is_none())
    .map(|span| span.started().name().to_string())
    .unwrap_or_else(|| "unknown".to_string());
  SourceRunSummary {
    source_run_id: snapshot.run_id().to_string(),
    source_operation,
    source_run_type: "execute".to_string(),
    source_status: "recorded".to_string(),
    generated_at_millis: auv_runtime::model::now_millis(),
    auv_git_commit: git_commit.clone(),
    exporter_git_commit: git_commit,
  }
}

pub async fn run_minecraft_texture_sweep_eval(
  samples_path: PathBuf,
  output_dir: PathBuf,
  require_real_source: bool,
) -> AuvResult<TextureSweepReport> {
  evaluate_texture_sweep(&TextureSweepInputs {
    samples_path,
    output_dir,
    thresholds: TextureSweepThresholds::mc6_v0(),
    require_real_source,
  })
}

pub fn current_git_commit() -> Option<String> {
  let output = std::process::Command::new("git").args(["rev-parse", "HEAD"]).output().ok()?;
  if !output.status.success() {
    return None;
  }
  let commit = String::from_utf8(output.stdout).ok()?.trim().to_string();
  (!commit.is_empty()).then_some(commit)
}

pub fn read_spatial_bundle_manifest(path: PathBuf) -> AuvResult<auv_game_minecraft::SpatialBundleManifest> {
  let bytes = fs::read(&path).map_err(|error| format!("failed to read minecraft spatial bundle manifest {}: {error}", path.display()))?;
  serde_json::from_slice(&bytes).map_err(|error| format!("failed to parse minecraft spatial bundle manifest {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use auv_tracing::{
    ArtifactBody, ArtifactId, ArtifactPurpose, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, ByteLength,
    CommitError, CommitResult, ContentType, IdempotencyKey, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage,
    RunCommitRequest, RunRevision, RunStore, RunSubscription, Sha256Digest, StoreArtifactRequest,
  };
  use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
  use sha2::{Digest, Sha256};

  use super::*;

  #[tokio::test]
  async fn direct_texture_sweep_prep_returns_domain_output() {
    let root = std::env::temp_dir().join(format!("auv-minecraft-direct-{}", auv_tracing::RunId::new()));
    let sidecar_run_dir = root.join("sidecar");
    let output_dir = root.join("out");

    let result = run_minecraft_texture_sweep_preparation(sidecar_run_dir.clone(), output_dir.clone())
      .await
      .expect("direct preparation should return its domain output");

    assert_eq!(result.output_dir, output_dir);
    assert_eq!(result.manifest.sidecar_run_dir, sidecar_run_dir.to_string_lossy());
    assert!(result.manifest_path.is_file());
    assert!(result.runbook_path.is_file());
    fs::remove_dir_all(root).expect("remove direct preparation fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_rejects_malformed_spatial_frame_before_writing_bundle() {
    let root = bundle_test_root("malformed-frame");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    write_source_artifact(
      store.as_ref(),
      run_id,
      projection_workflow::MINECRAFT_SPATIAL_FRAME_PURPOSE,
      "application/json",
      b"{not-json".to_vec(),
    )
    .await;

    let error = run_minecraft_spatial_bundle_export(store, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect_err("malformed spatial frame must fail export");

    assert!(error.contains("malformed"), "unexpected malformed-frame error: {error}");
    assert!(!output_dir.join("run.json").exists());
    fs::remove_dir_all(root).expect("remove malformed-frame fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_rejects_corrupt_screenshot_before_writing_bundle() {
    let root = bundle_test_root("corrupt-screenshot");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    write_source_artifact(store.as_ref(), run_id, projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE, "image/png", b"not-a-png".to_vec())
      .await;

    let error = run_minecraft_spatial_bundle_export(store, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect_err("corrupt screenshot must fail export");

    assert!(error.contains("PNG payload"), "unexpected corrupt-screenshot error: {error}");
    assert!(!output_dir.join("run.json").exists());
    fs::remove_dir_all(root).expect("remove corrupt-screenshot fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_rejects_wrong_minecraft_content_type_before_writing_bundle() {
    let root = bundle_test_root("wrong-content-type");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    write_source_artifact(
      store.as_ref(),
      run_id,
      projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE,
      "application/json",
      png_bytes([8, 16, 32]),
    )
    .await;

    let error = run_minecraft_spatial_bundle_export(store, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect_err("wrong screenshot content type must fail export");

    assert!(error.contains("content type"), "unexpected wrong-content-type error: {error}");
    assert!(!output_dir.join("run.json").exists());
    fs::remove_dir_all(root).expect("remove wrong-content-type fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_rejects_digest_mismatch_before_writing_bundle() {
    let root = bundle_test_root("digest-mismatch");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    let projection = auv_game_minecraft::MinecraftProjectionArtifact::for_frame(&bundle_test_frame(), None, None);
    let body = serde_json::to_vec(&projection).expect("projection should encode");
    let uri = write_source_artifact(
      store.as_ref(),
      run_id,
      auv_game_minecraft::artifact::MINECRAFT_PROJECTION_PURPOSE,
      "application/json",
      body.clone(),
    )
    .await;
    let mut corrupt_body = body;
    corrupt_body[0] ^= 1;
    let controlled = Arc::new(ControlledArtifactStore::new(store, uri, corrupt_body));

    let error = run_minecraft_spatial_bundle_export(controlled, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect_err("digest mismatch must fail export");

    assert!(error.contains("digest mismatch"), "unexpected digest-mismatch error: {error}");
    assert!(!output_dir.join("run.json").exists());
    fs::remove_dir_all(root).expect("remove digest-mismatch fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_rejects_length_mismatch_before_writing_bundle() {
    let root = bundle_test_root("length-mismatch");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    let projection = auv_game_minecraft::MinecraftProjectionArtifact::for_frame(&bundle_test_frame(), None, None);
    let body = serde_json::to_vec(&projection).expect("projection should encode");
    let uri = write_source_artifact(
      store.as_ref(),
      run_id,
      auv_game_minecraft::artifact::MINECRAFT_PROJECTION_PURPOSE,
      "application/json",
      body.clone(),
    )
    .await;
    let mut short_body = body;
    short_body.pop().expect("projection body should be non-empty");
    let controlled = Arc::new(ControlledArtifactStore::new(store, uri, short_body));

    let error = run_minecraft_spatial_bundle_export(controlled, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect_err("length mismatch must fail export");

    assert!(error.contains("length mismatch"), "unexpected length-mismatch error: {error}");
    assert!(!output_dir.join("run.json").exists());
    fs::remove_dir_all(root).expect("remove length-mismatch fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_decodes_all_supported_artifacts_and_uses_bundle_local_semantics() {
    let root = bundle_test_root("multi-artifact");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    let frame = bundle_test_frame();
    let projection = auv_game_minecraft::MinecraftProjectionArtifact::for_frame(&frame, None, None);
    write_source_artifact(store.as_ref(), run_id, projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE, "image/png", png_bytes([8, 16, 32]))
      .await;
    write_source_artifact(
      store.as_ref(),
      run_id,
      projection_workflow::MINECRAFT_SPATIAL_FRAME_PURPOSE,
      "application/json",
      serde_json::to_vec(&frame).expect("spatial frame should encode"),
    )
    .await;
    write_source_artifact(
      store.as_ref(),
      run_id,
      auv_game_minecraft::artifact::MINECRAFT_PROJECTION_PURPOSE,
      "application/json",
      serde_json::to_vec(&projection).expect("projection should encode"),
    )
    .await;
    write_source_artifact(store.as_ref(), run_id, projection_workflow::MINECRAFT_OVERLAY_PURPOSE, "image/png", png_bytes([64, 32, 16]))
      .await;

    let output = run_minecraft_spatial_bundle_export(store, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect("valid typed artifacts should export");

    assert_eq!(output.manifest.counts.screenshots, 1);
    assert_eq!(output.manifest.counts.spatial_frames, 2);
    assert_eq!(output.manifest.counts.overlays, 1);
    assert_eq!(output.manifest.artifacts.len(), 4);
    for artifact in &output.manifest.artifacts {
      assert!(artifact.source_path.starts_with("bundle-inputs/"), "source path was not bundle-local: {artifact:?}");
      assert!(!artifact.source_path.contains(&run_id.to_string()), "source path exposed canonical run identity: {artifact:?}");
      assert!(output_dir.join(&artifact.bundle_path).is_file(), "bundle artifact was not written: {artifact:?}");
    }
    fs::remove_dir_all(root).expect("remove multi-artifact fixture");
  }

  fn bundle_test_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("auv-minecraft-bundle-{label}-{}", RunId::new()));
    fs::create_dir_all(&root).expect("bundle fixture root should write");
    root
  }

  fn bundle_test_frame() -> auv_game_minecraft::MinecraftSpatialFrame {
    auv_game_minecraft::MinecraftSpatialFrame {
      spatial_frame_id: "frame-bundle-test".to_string(),
      world_tick: 42,
      monotonic_timestamp_ms: 5_000,
      telemetry_session_id: None,
      viewport: auv_game_minecraft::Viewport::new(64, 64),
      view_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      projection_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      player_pose: auv_game_minecraft::PlayerPose {
        eye_position: auv_game_minecraft::Vec3::new(0.0, 64.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: None,
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: None,
      resource_pack_ids: Vec::new(),
    }
  }

  fn png_bytes(color: [u8; 3]) -> Vec<u8> {
    let image = RgbImage::from_pixel(8, 8, Rgb(color));
    let mut output = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image).write_to(&mut output, ImageFormat::Png).expect("PNG should encode");
    output.into_inner()
  }

  async fn write_source_artifact(store: &MemoryRunStore, run_id: RunId, purpose: &str, content_type: &str, body: Vec<u8>) -> ArtifactUri {
    let artifact_id = ArtifactId::new();
    let request = StoreArtifactRequest::new(
      store.authority_id(),
      run_id,
      IdempotencyKey::new(),
      artifact_id,
      None,
      ArtifactPurpose::parse(purpose).expect("artifact purpose"),
      ContentType::parse(content_type).expect("content type"),
      ByteLength::new(body.len() as u64).expect("byte length"),
      Sha256Digest::new(Sha256::digest(&body).into()),
      auv_tracing::Attributes::empty(),
    );
    store.write_artifact(request, Box::pin(futures_util::io::Cursor::new(body))).await.expect("source artifact should write");
    ArtifactUri::from_ids(run_id, artifact_id)
  }

  struct ControlledArtifactStore {
    inner: Arc<MemoryRunStore>,
    overridden_uri: ArtifactUri,
    body: Vec<u8>,
  }

  impl ControlledArtifactStore {
    fn new(inner: Arc<MemoryRunStore>, overridden_uri: ArtifactUri, body: Vec<u8>) -> Self {
      Self {
        inner,
        overridden_uri,
        body,
      }
    }
  }

  impl RunStore for ControlledArtifactStore {
    fn authority_id(&self) -> AuthorityId {
      self.inner.authority_id()
    }

    fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
      self.inner.commit(request)
    }

    fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
      self.inner.write_artifact(request, body)
    }

    fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
      self.inner.lookup_commit(run_id, key)
    }

    fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>> {
      self.inner.load_snapshot(run_id)
    }

    fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
      self.inner.commits_after(run_id, after, limit)
    }

    fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
      self.inner.subscribe(run_id, after)
    }

    fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
      if uri != self.overridden_uri {
        return self.inner.open_artifact(uri);
      }
      let body = self.body.clone();
      Box::pin(async move { Ok(Box::pin(futures_util::stream::once(async move { Ok(body.into()) })) as ArtifactReader) })
    }
  }
}
