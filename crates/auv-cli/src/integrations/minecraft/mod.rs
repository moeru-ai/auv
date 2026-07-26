use std::path::PathBuf;

pub mod help;
pub mod projection_workflow;
pub mod query_live_action;

use auv_game_minecraft::{
  ScenePacketInputs, ScenePacketOutput, TextureSweepInputs, TextureSweepPreparationInputs, TextureSweepPreparationOutput,
  TextureSweepReport, TextureSweepSampleBuildInputs, TextureSweepSampleBuildOutput, TextureSweepThresholds,
  build_texture_sweep_samples_from_bundles, evaluate_texture_sweep, export_3dgs_scene_packet, prepare_texture_sweep_resource_packs,
};
use auv_runtime::model::AuvResult;
use auv_tracing::{ArtifactMetadata, Context, EventPayload};

#[derive(serde::Serialize)]
struct MinecraftArtifactPublicationFailed {
  artifact_purpose: &'static str,
  error: String,
}

impl EventPayload for MinecraftArtifactPublicationFailed {
  const NAME: &'static str = "auv.minecraft.artifact_publication_failed";
  const VERSION: u32 = 1;
}

fn keep_artifact_receipt<E: std::fmt::Display>(
  artifact_purpose: &'static str,
  result: Result<Option<ArtifactMetadata>, E>,
) -> Option<ArtifactMetadata> {
  match result {
    Ok(receipt) => receipt,
    Err(error) => {
      auv_tracing::emit_event!(MinecraftArtifactPublicationFailed {
        artifact_purpose,
        error: error.to_string(),
      });
      None
    }
  }
}

pub async fn run_minecraft_3dgs_scene_packet_export(
  bundle_manifest_paths: Vec<PathBuf>,
  output_dir: PathBuf,
) -> AuvResult<ScenePacketOutput> {
  let result = export_3dgs_scene_packet(ScenePacketInputs {
    bundle_manifest_paths,
    output_dir,
  })?;
  let context = Context::current();
  keep_artifact_receipt(
    auv_game_minecraft::scene_packet::MINECRAFT_SCENE_PACKET_PURPOSE,
    auv_game_minecraft::scene_packet::publish_minecraft_scene_packet(Some(&context), &result.manifest).await,
  );
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

pub async fn run_minecraft_texture_sweep_sample_build(
  bundle_manifest_paths: Vec<PathBuf>,
  output_path: PathBuf,
) -> AuvResult<TextureSweepSampleBuildOutput> {
  build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
    bundle_manifest_paths,
    output_path,
  })
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

// TODO(auv-inspector): spatial-bundle export from a recorded run is omitted
// because the legacy RunStore read-side was retired. Reintroduce it only after
// an owner-approved inspector contract supplies typed artifact discovery.
