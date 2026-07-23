use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::StreamExt;

pub mod help;
pub mod projection_workflow;
pub mod query_live_action;
pub mod session;
pub mod verification;

use auv_game_minecraft::{
  QueryActionWiringOutcome, QueryLiveClickExecutor, ScenePacketInputs, ScenePacketOutput, SourceRunSummary, SpatialBundleInputs,
  SpatialBundleSourceArtifact, TextureSweepInputs, TextureSweepPreparationInputs, TextureSweepPreparationOutput, TextureSweepReport,
  TextureSweepSampleBuildInputs, TextureSweepSampleBuildOutput, TextureSweepThresholds, TrainingLaunchJobInputs,
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
use auv_tracing::{Context, RunId, RunSnapshot, RunStore};

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
  let artifacts = materialize_spatial_bundle_artifacts(store.as_ref(), &snapshot, &staging_dir).await;
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

async fn materialize_spatial_bundle_artifacts(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  staging_dir: &Path,
) -> AuvResult<Vec<SpatialBundleSourceArtifact>> {
  let mut artifacts = Vec::new();
  for published in snapshot.artifacts().values() {
    let metadata = published.metadata();
    let Some((role, extension)) = spatial_bundle_role_for_purpose(metadata.purpose().as_str()) else {
      continue;
    };
    let uri = metadata.uri();
    let source_path = staging_dir.join(format!("{}.{}", uri.artifact_id(), extension));
    let mut reader =
      store.open_artifact(uri.clone()).await.map_err(|error| format!("failed to open Minecraft source artifact {uri}: {error}"))?;
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.next().await {
      let chunk = chunk.map_err(|error| format!("failed to read Minecraft source artifact {uri}: {error}"))?;
      bytes.extend_from_slice(&chunk);
    }
    fs::write(&source_path, bytes)
      .map_err(|error| format!("failed to materialize Minecraft source artifact {uri} at {}: {error}", source_path.display()))?;
    artifacts.push(SpatialBundleSourceArtifact {
      artifact_id: uri.artifact_id().to_string(),
      role: role.to_string(),
      source_path,
      source_run_path: uri.to_string(),
      summary: Some(format!("canonical purpose {}", metadata.purpose())),
    });
  }
  Ok(artifacts)
}

fn spatial_bundle_role_for_purpose(purpose: &str) -> Option<(&'static str, &'static str)> {
  match purpose {
    projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE => Some(("minecraft-screenshot", "png")),
    projection_workflow::MINECRAFT_SPATIAL_FRAME_PURPOSE => Some((MINECRAFT_SPATIAL_FRAME_ARTIFACT_ROLE, "json")),
    auv_game_minecraft::artifact::MINECRAFT_PROJECTION_PURPOSE => Some((MINECRAFT_PROJECTION_ARTIFACT_ROLE, "json")),
    projection_workflow::MINECRAFT_OVERLAY_PURPOSE => Some(("minecraft-overlay", "png")),
    _ => None,
  }
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
}
