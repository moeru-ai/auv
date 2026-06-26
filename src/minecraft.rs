use std::fs;
use std::path::PathBuf;

use auv_game_minecraft::{
  ScenePacketInputs, ScenePacketOutput, SourceRunSummary, SpatialBundleInputs, SpatialBundleOutput,
  SpatialBundleSourceArtifact, TextureSweepInputs, TextureSweepPreparationInputs,
  TextureSweepPreparationOutput, TextureSweepReport, TextureSweepSampleBuildInputs,
  TextureSweepSampleBuildOutput, TextureSweepThresholds, TrainingLaunchJobInputs,
  TrainingLaunchPreparationInputs, TrainingLaunchPreparationOutput, TrainingPackageInputs,
  TrainingPackageOutput, TrainingResultArtifactFetchInputs, TrainingResultArtifactFetchOutput,
  TrainingResultInputs, TrainingResultOutput, build_texture_sweep_samples_from_bundles,
  collect_3dgs_training_job_result, collect_3dgs_training_job_result_with_environment,
  evaluate_texture_sweep, export_3dgs_scene_packet, export_3dgs_training_package,
  export_spatial_bundle, fetch_3dgs_training_result_artifacts_with_command,
  launch_3dgs_training_job, launch_3dgs_training_job_with_environment,
  prepare_3dgs_training_launch, prepare_texture_sweep_resource_packs,
};

use auv_tracing_driver::RecordingHandle;
use auv_tracing_driver::recorded_operation::RecordedOperationOutput;
use auv_tracing_driver::run_builder::RunSpec;
use auv_tracing_driver::store::CanonicalRun;
use auv_tracing_driver::trace::{RunType, TraceStatusCode};

use crate::model::AuvResult;

pub const MINECRAFT_SPATIAL_FRAME_ARTIFACT_ROLE: &str = "minecraft-spatial-frame";
pub const MINECRAFT_SPATIAL_BUNDLE_ARTIFACT_ROLE: &str = "minecraft-spatial-bundle";
pub const MINECRAFT_TEXTURE_SWEEP_SAMPLES_ARTIFACT_ROLE: &str = "minecraft-texture-sweep-samples";
pub const MINECRAFT_TEXTURE_SWEEP_ARTIFACT_ROLE: &str = "minecraft-texture-sweep";
pub const MINECRAFT_TEXTURE_SWEEP_PREP_ARTIFACT_ROLE: &str = "minecraft-texture-sweep-prep";
pub const MINECRAFT_TEXTURE_SWEEP_RUNBOOK_ARTIFACT_ROLE: &str = "minecraft-texture-sweep-runbook";
pub const MINECRAFT_3DGS_SCENE_PACKET_ARTIFACT_ROLE: &str = "minecraft-3dgs-scene-packet";
pub const MINECRAFT_3DGS_SCENE_PACKET_INSPECT_ARTIFACT_ROLE: &str =
  "minecraft-3dgs-scene-packet-inspect";
pub const MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE: &str = "minecraft-3dgs-training-package";
pub const MINECRAFT_3DGS_TRAINING_PACKAGE_INSPECT_ARTIFACT_ROLE: &str =
  "minecraft-3dgs-training-package-inspect";
pub const MINECRAFT_3DGS_TRAINING_LAUNCH_PLAN_ARTIFACT_ROLE: &str =
  "minecraft-3dgs-training-launch-plan";
pub const MINECRAFT_3DGS_TRAINING_LAUNCH_INSPECT_ARTIFACT_ROLE: &str =
  "minecraft-3dgs-training-launch-inspect";
pub const MINECRAFT_3DGS_TRAINING_LAUNCH_RUNBOOK_ARTIFACT_ROLE: &str =
  "minecraft-3dgs-training-launch-runbook";
pub const MINECRAFT_3DGS_TRAINING_JOB_ARTIFACT_ROLE: &str = "minecraft-3dgs-training-job";
pub const MINECRAFT_3DGS_TRAINING_JOB_INSPECT_ARTIFACT_ROLE: &str =
  "minecraft-3dgs-training-job-inspect";
pub const MINECRAFT_3DGS_TRAINING_JOB_RUNBOOK_ARTIFACT_ROLE: &str =
  "minecraft-3dgs-training-job-runbook";
pub const MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_ROLE: &str = "minecraft-3dgs-training-result";
pub const MINECRAFT_3DGS_TRAINING_RESULT_INSPECT_ARTIFACT_ROLE: &str =
  "minecraft-3dgs-training-result-inspect";
pub const MINECRAFT_3DGS_TRAINING_RESULT_RUNBOOK_ARTIFACT_ROLE: &str =
  "minecraft-3dgs-training-result-runbook";
pub const MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_MANIFEST_ROLE: &str =
  "minecraft-3dgs-training-result-artifact-manifest";
pub const MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_INSPECT_ROLE: &str =
  "minecraft-3dgs-training-result-artifact-inspect";
pub const MINECRAFT_PROJECTION_CALIBRATION_ARTIFACT_ROLE: &str = "minecraft-projection-calibration";

pub fn run_minecraft_3dgs_scene_packet_export(
  recording: &RecordingHandle,
  bundle_manifest_paths: Vec<PathBuf>,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<ScenePacketOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.minecraft.export_3dgs_scene_packet"),
    "Minecraft export MC-7 3DGS scene packet",
    |context| {
      context.record_event(
        "minecraft.export_3dgs_scene_packet.inputs",
        Some(format!(
          "bundle_manifests={} output_dir={} trained_3dgs=false action_path=false",
          bundle_manifest_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(","),
          output_dir.display()
        )),
      );
      let result = export_3dgs_scene_packet(ScenePacketInputs {
        bundle_manifest_paths: bundle_manifest_paths.clone(),
        output_dir: output_dir.clone(),
      })?;
      context.in_span("minecraft.export_3dgs_scene_packet.artifacts", |context| {
        context.stage_artifact_file(
          MINECRAFT_3DGS_SCENE_PACKET_ARTIFACT_ROLE,
          &result.manifest_path,
          "minecraft-3dgs-scene-packet-run.json",
          Some("MC-7 3DGS input scene packet manifest; offline inspect artifact only".to_string()),
        )?;
        context.stage_artifact_file(
          MINECRAFT_3DGS_SCENE_PACKET_INSPECT_ARTIFACT_ROLE,
          &result.inspect_report_path,
          "minecraft-3dgs-scene-packet-inspect.json",
          Some(
            "MC-7 accepted-only scene packet inspect report; offline inspect artifact only"
              .to_string(),
          ),
        )?;
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_minecraft_3dgs_training_package_export(
  recording: &RecordingHandle,
  scene_packet_manifest_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<TrainingPackageOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(
      RunType::Execute,
      "auv.minecraft.export_3dgs_training_package",
    ),
    "Minecraft export MC-7 D3 training-prep package",
    |context| {
      context.record_event(
        "minecraft.export_3dgs_training_package.inputs",
        Some(format!(
          "scene_packet_manifest={} output_dir={} trained_3dgs=false trainer_backend=false",
          scene_packet_manifest_path.display(),
          output_dir.display()
        )),
      );
      let result = export_3dgs_training_package(TrainingPackageInputs {
        scene_packet_manifest_path: scene_packet_manifest_path.clone(),
        output_dir: output_dir.clone(),
      })?;
      context.in_span(
        "minecraft.export_3dgs_training_package.artifacts",
        |context| {
          context.stage_artifact_file(
            MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE,
            &result.manifest_path,
            "minecraft-3dgs-training-package-run.json",
            Some(
              "MC-7 D3 canonical training-prep package manifest; offline inspect artifact only"
                .to_string(),
            ),
          )?;
          context.stage_artifact_file(
            MINECRAFT_3DGS_TRAINING_PACKAGE_INSPECT_ARTIFACT_ROLE,
            &result.inspect_report_path,
            "minecraft-3dgs-training-package-inspect.json",
            Some(
              "MC-7 D3 training-prep inspect report plus Nerfstudio compatibility view status"
                .to_string(),
            ),
          )?;
          Ok::<_, String>(())
        },
      )?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_minecraft_texture_sweep_preparation(
  recording: &RecordingHandle,
  sidecar_run_dir: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<TextureSweepPreparationOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.minecraft.prepare_texture_sweep"),
    "Minecraft prepare MC-6 texture sweep inputs",
    |context| {
      context.record_event(
        "minecraft.prepare_texture_sweep.inputs",
        Some(format!(
          "sidecar_run_dir={} output_dir={} live_chain=false",
          sidecar_run_dir.display(),
          output_dir.display()
        )),
      );
      let result = prepare_texture_sweep_resource_packs(TextureSweepPreparationInputs {
        sidecar_run_dir: sidecar_run_dir.clone(),
        output_dir: output_dir.clone(),
      })?;
      context.in_span("minecraft.prepare_texture_sweep.artifacts", |context| {
        context.stage_artifact_file(
          MINECRAFT_TEXTURE_SWEEP_PREP_ARTIFACT_ROLE,
          &result.manifest_path,
          "mc6-texture-sweep-prep.json",
          Some("MC-6 texture sweep preparation manifest".to_string()),
        )?;
        context.stage_artifact_file(
          MINECRAFT_TEXTURE_SWEEP_RUNBOOK_ARTIFACT_ROLE,
          &result.runbook_path,
          "mc6-texture-sweep-runbook.md",
          Some("MC-6 texture sweep manual runbook".to_string()),
        )?;
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_minecraft_3dgs_training_launch_preparation(
  recording: &RecordingHandle,
  training_package_manifest_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<TrainingLaunchPreparationOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(
      RunType::Execute,
      "auv.minecraft.prepare_3dgs_training",
    ),
    "Minecraft prepare MC-7 D5 training launch inputs",
    |context| {
      context.record_event(
        "minecraft.prepare_3dgs_training.inputs",
        Some(format!(
          "training_package_manifest={} output_dir={} trained_3dgs=false trainer_started=false trainer_backend=nerfstudio.splatfacto",
          training_package_manifest_path.display(),
          output_dir.display()
        )),
      );
      let result = prepare_3dgs_training_launch(TrainingLaunchPreparationInputs {
        training_package_manifest_path: training_package_manifest_path.clone(),
        output_dir: output_dir.clone(),
      })?;
      context.in_span("minecraft.prepare_3dgs_training.artifacts", |context| {
        context.stage_artifact_file(
          MINECRAFT_3DGS_TRAINING_LAUNCH_PLAN_ARTIFACT_ROLE,
          &result.manifest_path,
          "minecraft-3dgs-training-launch-plan.json",
          Some(
            "MC-7 D5 training launch preparation plan; offline inspect artifact only".to_string(),
          ),
        )?;
        context.stage_artifact_file(
          MINECRAFT_3DGS_TRAINING_LAUNCH_INSPECT_ARTIFACT_ROLE,
          &result.inspect_report_path,
          "minecraft-3dgs-training-launch-inspect.json",
          Some(
            "MC-7 D5 training launch readiness report; offline inspect artifact only".to_string(),
          ),
        )?;
        context.stage_artifact_file(
          MINECRAFT_3DGS_TRAINING_LAUNCH_RUNBOOK_ARTIFACT_ROLE,
          &result.runbook_path,
          "mc7-training-launch-runbook.md",
          Some("MC-7 D5 training launch manual runbook".to_string()),
        )?;
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_minecraft_3dgs_training_job_launch(
  recording: &RecordingHandle,
  training_launch_plan_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<auv_game_minecraft::TrainingLaunchJobOutput>> {
  run_minecraft_3dgs_training_job_launch_with_environment(
    recording,
    training_launch_plan_path,
    output_dir,
    None,
    None,
    None,
  )
}

pub fn run_minecraft_3dgs_training_job_launch_with_environment(
  recording: &RecordingHandle,
  training_launch_plan_path: PathBuf,
  output_dir: PathBuf,
  training_job_endpoint: Option<String>,
  training_job_token: Option<String>,
  training_job_submit_command: Option<String>,
) -> AuvResult<RecordedOperationOutput<auv_game_minecraft::TrainingLaunchJobOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.minecraft.launch_3dgs_training_job"),
    "Minecraft launch MC-9 D2 real provider submit",
    |context| {
      context.record_event(
        "minecraft.launch_3dgs_training_job.inputs",
        Some(format!(
          "training_launch_plan={} output_dir={} trained_3dgs=false trainer_started=false job_backend=remote provider_backend=remote-command-provider real_submit=true",
          training_launch_plan_path.display(),
          output_dir.display()
        )),
      );
      let result = if training_job_endpoint.is_some()
        || training_job_token.is_some()
        || training_job_submit_command.is_some()
      {
        launch_3dgs_training_job_with_environment(
          TrainingLaunchJobInputs {
            training_launch_plan_path: training_launch_plan_path.clone(),
            output_dir: output_dir.clone(),
          },
          auv_game_minecraft::TrainingJobEnvironment::with_values(
            training_job_endpoint.clone(),
            training_job_token.clone(),
            training_job_submit_command.clone(),
          ),
        )?
      } else {
        launch_3dgs_training_job(TrainingLaunchJobInputs {
          training_launch_plan_path: training_launch_plan_path.clone(),
          output_dir: output_dir.clone(),
        })?
      };
      context.in_span("minecraft.launch_3dgs_training_job.artifacts", |context| {
        context.stage_artifact_file(
          MINECRAFT_3DGS_TRAINING_JOB_ARTIFACT_ROLE,
          &result.manifest_path,
          "minecraft-3dgs-training-job.json",
          Some("MC-9 D2 real provider submit manifest".to_string()),
        )?;
        context.stage_artifact_file(
          MINECRAFT_3DGS_TRAINING_JOB_INSPECT_ARTIFACT_ROLE,
          &result.inspect_report_path,
          "minecraft-3dgs-training-job-inspect.json",
          Some("MC-9 D2 real provider submit inspect report".to_string()),
        )?;
        context.stage_artifact_file(
          MINECRAFT_3DGS_TRAINING_JOB_RUNBOOK_ARTIFACT_ROLE,
          &result.runbook_path,
          "mc7-training-job-runbook.md",
          Some("MC-9 D2 real provider submit runbook".to_string()),
        )?;
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_minecraft_3dgs_training_result_collection(
  recording: &RecordingHandle,
  training_job_manifest_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<TrainingResultOutput>> {
  run_minecraft_3dgs_training_result_collection_with_environment(
    recording,
    training_job_manifest_path,
    output_dir,
    None,
    None,
    None,
  )
}

pub fn run_minecraft_3dgs_training_result_collection_with_environment(
  recording: &RecordingHandle,
  training_job_manifest_path: PathBuf,
  output_dir: PathBuf,
  training_job_endpoint: Option<String>,
  training_job_token: Option<String>,
  training_job_status_command: Option<String>,
) -> AuvResult<RecordedOperationOutput<TrainingResultOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(
      RunType::Execute,
      "auv.minecraft.collect_3dgs_training_job_result",
    ),
    "Minecraft collect MC-9 D3 real provider training job result",
    |context| {
      context.record_event(
        "minecraft.collect_3dgs_training_job_result.inputs",
        Some(format!(
          "training_job_manifest={} output_dir={} trained_3dgs=false trainer_result_consumed=true real_provider_status=true job_backend=remote",
          training_job_manifest_path.display(),
          output_dir.display()
        )),
      );
      let result = if training_job_endpoint.is_some()
        || training_job_token.is_some()
        || training_job_status_command.is_some()
      {
        collect_3dgs_training_job_result_with_environment(
          TrainingResultInputs {
            training_job_manifest_path: training_job_manifest_path.clone(),
            output_dir: output_dir.clone(),
          },
          auv_game_minecraft::TrainingResultEnvironment::with_values(
            training_job_endpoint.clone(),
            training_job_token.clone(),
            training_job_status_command.clone(),
          ),
        )?
      } else {
        collect_3dgs_training_job_result(TrainingResultInputs {
          training_job_manifest_path: training_job_manifest_path.clone(),
          output_dir: output_dir.clone(),
        })?
      };
      context.in_span("minecraft.collect_3dgs_training_job_result.artifacts", |context| {
        context.stage_artifact_file(
          MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_ROLE,
          &result.manifest_path,
          "minecraft-3dgs-training-result.json",
          Some("MC-9 D3 real provider training result manifest".to_string()),
        )?;
        context.stage_artifact_file(
          MINECRAFT_3DGS_TRAINING_RESULT_INSPECT_ARTIFACT_ROLE,
          &result.inspect_report_path,
          "minecraft-3dgs-training-result-inspect.json",
          Some("MC-9 D3 real provider training result inspect report".to_string()),
        )?;
        context.stage_artifact_file(
          MINECRAFT_3DGS_TRAINING_RESULT_RUNBOOK_ARTIFACT_ROLE,
          &result.runbook_path,
          "mc7-training-result-runbook.md",
          Some("MC-9 D3 real provider training result runbook".to_string()),
        )?;
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_minecraft_3dgs_training_result_artifact_fetch(
  recording: &RecordingHandle,
  training_result_manifest_path: PathBuf,
  output_dir: PathBuf,
  artifact_fetch_command: Option<String>,
) -> AuvResult<RecordedOperationOutput<TrainingResultArtifactFetchOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(
      RunType::Execute,
      "auv.minecraft.fetch_3dgs_training_result_artifacts",
    ),
    "Minecraft fetch MC-7 D11 normalized training result artifacts",
    |context| {
      context.record_event(
        "minecraft.fetch_3dgs_training_result_artifacts.inputs",
        Some(format!(
          "training_result_manifest={} output_dir={} artifact_fetch_command={} trained_3dgs=false normalized_result_artifacts=true",
          training_result_manifest_path.display(),
          output_dir.display(),
          artifact_fetch_command.is_some()
        )),
      );
      let result = fetch_3dgs_training_result_artifacts_with_command(
        TrainingResultArtifactFetchInputs {
          training_result_manifest_path: training_result_manifest_path.clone(),
          output_dir: output_dir.clone(),
        },
        artifact_fetch_command.clone(),
      )?;
      context.in_span(
        "minecraft.fetch_3dgs_training_result_artifacts.artifacts",
        |context| {
          context.stage_artifact_file(
            MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_MANIFEST_ROLE,
            &result.manifest_path,
            "minecraft-3dgs-training-result-artifact-manifest.json",
            Some("MC-7 D11 normalized training result artifact manifest".to_string()),
          )?;
          context.stage_artifact_file(
            MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_INSPECT_ROLE,
            &result.inspect_report_path,
            "minecraft-3dgs-training-result-artifact-inspect.json",
            Some("MC-7 D11 normalized training result artifact inspect report".to_string()),
          )?;
          Ok::<_, String>(())
        },
      )?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_minecraft_texture_sweep_sample_build(
  recording: &RecordingHandle,
  bundle_manifest_paths: Vec<PathBuf>,
  output_path: PathBuf,
) -> AuvResult<RecordedOperationOutput<TextureSweepSampleBuildOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(
      RunType::Execute,
      "auv.minecraft.build_texture_sweep_samples",
    ),
    "Minecraft build MC-6 texture sweep samples",
    |context| {
      context.record_event(
        "minecraft.build_texture_sweep_samples.inputs",
        Some(format!(
          "bundle_manifests={} output={}",
          bundle_manifest_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(","),
          output_path.display()
        )),
      );
      let result = build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
        bundle_manifest_paths: bundle_manifest_paths.clone(),
        output_path: output_path.clone(),
      })?;
      context.in_span(
        "minecraft.build_texture_sweep_samples.artifacts",
        |context| {
          context.stage_artifact_file(
            MINECRAFT_TEXTURE_SWEEP_SAMPLES_ARTIFACT_ROLE,
            &result.output_path,
            "texture_sweep_samples.json",
            Some("MC-6 texture sweep samples built from spatial bundles".to_string()),
          )?;
          Ok::<_, String>(())
        },
      )?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_minecraft_spatial_bundle_export(
  recording: &RecordingHandle,
  source_run_id: String,
  output_dir: PathBuf,
  git_commit: Option<String>,
) -> AuvResult<RecordedOperationOutput<SpatialBundleOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.minecraft.export_spatial_bundle"),
    "Minecraft export spatial dataset bundle",
    |context| {
      context.record_event(
        "minecraft.export_spatial_bundle.inputs",
        Some(format!(
          "source_run_id={} output_dir={}",
          source_run_id,
          output_dir.display()
        )),
      );
      let source_run = context.recording().read_run(&source_run_id)?;
      let source_run_dir = context.recording().run_dir(&source_run_id)?;
      let result = export_spatial_bundle(SpatialBundleInputs {
        output_dir: output_dir.clone(),
        source: source_run_summary(&source_run, git_commit.clone()),
        artifacts: source_bundle_artifacts(source_run_dir, &source_run),
      })?;
      context.in_span("minecraft.export_spatial_bundle.artifacts", |context| {
        let manifest_path = result.output_dir.join("run.json");
        context.stage_artifact_file(
          MINECRAFT_SPATIAL_BUNDLE_ARTIFACT_ROLE,
          &manifest_path,
          "minecraft-spatial-bundle-run.json",
          Some("MC-6 spatial dataset bundle manifest".to_string()),
        )?;
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_minecraft_texture_sweep_eval(
  recording: &RecordingHandle,
  samples_path: PathBuf,
  output_dir: PathBuf,
  require_real_source: bool,
) -> AuvResult<RecordedOperationOutput<TextureSweepReport>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.minecraft.eval_texture_sweep"),
    "Minecraft evaluate 2.5D texture sweep",
    |context| {
      context.record_event(
        "minecraft.eval_texture_sweep.inputs",
        Some(format!(
          "samples={} output_dir={} thresholds=mc6_v0 require_real_source={}",
          samples_path.display(),
          output_dir.display(),
          require_real_source
        )),
      );
      let report = evaluate_texture_sweep(&TextureSweepInputs {
        samples_path: samples_path.clone(),
        output_dir: output_dir.clone(),
        thresholds: TextureSweepThresholds::mc6_v0(),
        require_real_source,
      })?;
      context.in_span("minecraft.eval_texture_sweep.artifacts", |context| {
        context.stage_artifact_file(
          MINECRAFT_TEXTURE_SWEEP_SAMPLES_ARTIFACT_ROLE,
          &samples_path,
          "texture_sweep_samples.json",
          Some("MC-6 texture sweep input samples".to_string()),
        )?;
        let report_path = output_dir.join("texture_sweep_report.json");
        context.stage_artifact_file(
          MINECRAFT_TEXTURE_SWEEP_ARTIFACT_ROLE,
          &report_path,
          "texture_sweep_report.json",
          Some("MC-6 texture sweep p95/IoU report".to_string()),
        )?;
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(report)
    },
  )
}

pub fn current_git_commit() -> Option<String> {
  let output = std::process::Command::new("git")
    .args(["rev-parse", "HEAD"])
    .output()
    .ok()?;
  if !output.status.success() {
    return None;
  }
  let commit = String::from_utf8(output.stdout).ok()?.trim().to_string();
  (!commit.is_empty()).then_some(commit)
}

fn source_run_summary(source_run: &CanonicalRun, git_commit: Option<String>) -> SourceRunSummary {
  SourceRunSummary {
    source_run_id: source_run.run.run_id.as_str().to_string(),
    source_operation: source_run
      .spans
      .iter()
      .find(|span| span.span_id == source_run.run.root_span_id)
      .map(|span| span.name.clone())
      .unwrap_or_else(|| "unknown".to_string()),
    source_run_type: source_run.run.run_type.as_str().to_string(),
    source_status: source_run.run.status_code.as_str().to_string(),
    generated_at_millis: auv_tracing_driver::now_millis(),
    auv_git_commit: git_commit.clone(),
    exporter_git_commit: git_commit,
  }
}

fn source_bundle_artifacts(
  source_run_dir: PathBuf,
  source_run: &CanonicalRun,
) -> Vec<SpatialBundleSourceArtifact> {
  source_run
    .artifacts
    .iter()
    .map(|artifact| SpatialBundleSourceArtifact {
      artifact_id: artifact.artifact_id.as_str().to_string(),
      role: artifact.role.clone(),
      source_path: source_run_dir.join(&artifact.path),
      source_run_path: artifact.path.clone(),
      summary: artifact.summary.clone(),
    })
    .collect()
}

pub fn read_spatial_bundle_manifest(
  path: PathBuf,
) -> AuvResult<auv_game_minecraft::SpatialBundleManifest> {
  let bytes = fs::read(&path).map_err(|error| {
    format!(
      "failed to read minecraft spatial bundle manifest {}: {error}",
      path.display()
    )
  })?;
  serde_json::from_slice::<auv_game_minecraft::SpatialBundleManifest>(&bytes).map_err(|error| {
    format!(
      "failed to parse minecraft spatial bundle manifest {}: {error}",
      path.display()
    )
  })
}

pub fn texture_sweep_status(report: &TextureSweepReport) -> TraceStatusCode {
  if report.passed {
    TraceStatusCode::Ok
  } else {
    TraceStatusCode::Error
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_tracing_driver::RunRecordingBackend;
  use auv_tracing_driver::recording::NoopRunRecorder;
  use auv_tracing_driver::run_builder::RunSpec;
  use auv_tracing_driver::store::LocalStore;
  use auv_tracing_driver::trace::RunType;
  use std::sync::Arc;

  fn temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("auv-{}-{}", label, crate::model::now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be created");
    path
  }

  fn identity_matrix() -> [f64; 16] {
    [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
  }

  fn write_sample_bundle(temp: &std::path::Path) -> PathBuf {
    let bundle_dir = temp.join("bundle");
    let screenshots_dir = bundle_dir.join("screenshots");
    let frames_dir = bundle_dir.join("spatial_frames");
    fs::create_dir_all(&screenshots_dir).expect("screenshots dir");
    fs::create_dir_all(&frames_dir).expect("frames dir");
    fs::write(screenshots_dir.join("artifact_0001-frame.png"), b"png").expect("screenshot");
    let frame = auv_game_minecraft::MinecraftSpatialFrame {
      spatial_frame_id: "frame-rich".to_string(),
      world_tick: 1,
      monotonic_timestamp_ms: 1_000,
      telemetry_session_id: None,
      viewport: auv_game_minecraft::Viewport::new(800, 600),
      view_matrix: identity_matrix(),
      projection_matrix: identity_matrix(),
      player_pose: auv_game_minecraft::PlayerPose {
        eye_position: auv_game_minecraft::Vec3::new(0.0, 0.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: Some(auv_game_minecraft::RaycastHit {
        block_pos: auv_game_minecraft::BlockPosition::new(0, 0, 0),
        face: auv_game_minecraft::BlockFace::North,
        block_id: "minecraft:stone".to_string(),
      }),
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: Some("artifact://artifact_0001".to_string()),
      mc_capture_skew_ms: Some(10),
      screen_state: Some("in_game".to_string()),
      resource_pack_ids: vec!["fabric".to_string(), "file/auv-mc6-rich".to_string()],
    };
    fs::write(
      frames_dir.join("artifact_0001-frame-rich.json"),
      serde_json::to_vec_pretty(&frame).expect("frame json"),
    )
    .expect("frame write");
    let manifest = auv_game_minecraft::SpatialBundleManifest {
      schema_version: auv_game_minecraft::SPATIAL_BUNDLE_SCHEMA_VERSION,
      source_run: auv_game_minecraft::SourceRunSummary {
        source_run_id: "run_1".to_string(),
        source_operation: "auv.minecraft.bridge".to_string(),
        source_run_type: "execute".to_string(),
        source_status: "ok".to_string(),
        generated_at_millis: 1,
        auv_git_commit: None,
        exporter_git_commit: None,
      },
      counts: auv_game_minecraft::SpatialBundleCounts {
        screenshots: 1,
        spatial_frames: 1,
        ..auv_game_minecraft::SpatialBundleCounts::default()
      },
      artifacts: vec![
        auv_game_minecraft::SpatialBundleArtifactRecord {
          artifact_id: "artifact_0001".to_string(),
          role: "minecraft-screenshot".to_string(),
          source_path: "artifacts/frame.png".to_string(),
          bundle_path: "screenshots/artifact_0001-frame.png".to_string(),
          directory: auv_game_minecraft::SpatialBundleDirectory::Screenshots,
          summary: None,
        },
        auv_game_minecraft::SpatialBundleArtifactRecord {
          artifact_id: "artifact_0002".to_string(),
          role: "minecraft-spatial-frame".to_string(),
          source_path: "artifacts/frame-rich.json".to_string(),
          bundle_path: "spatial_frames/artifact_0001-frame-rich.json".to_string(),
          directory: auv_game_minecraft::SpatialBundleDirectory::SpatialFrames,
          summary: None,
        },
      ],
      known_limits: Vec::new(),
    };
    let manifest_path = bundle_dir.join("run.json");
    fs::write(
      &manifest_path,
      serde_json::to_vec_pretty(&manifest).expect("manifest json"),
    )
    .expect("manifest write");
    manifest_path
  }

  #[test]
  fn three_dgs_scene_packet_export_records_manifest_artifact() {
    let temp = temp_dir("mc7-scene-packet");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();
    let manifest_path = write_sample_bundle(&temp);

    let output = run_minecraft_3dgs_scene_packet_export(
      &recording,
      vec![manifest_path],
      temp.join("scene-packet"),
    )
    .expect("scene packet export");

    assert_eq!(output.value.manifest.counts.frames, 1);
    assert_eq!(output.value.manifest.counts.screenshots, 1);
    assert!(output.value.inspect_report_path.is_file());
    assert_eq!(output.value.inspect_report.counts.camera_records, 1);
    let run = recording
      .read_run(output.run_id.as_str())
      .expect("scene packet run should persist");
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_3DGS_SCENE_PACKET_ARTIFACT_ROLE)
    );
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_3DGS_SCENE_PACKET_INSPECT_ARTIFACT_ROLE)
    );

    let _ = fs::remove_dir_all(temp);
  }

  #[test]
  fn three_dgs_training_package_export_records_manifest_and_inspect_artifacts() {
    let temp = temp_dir("mc7-training-package");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();
    let manifest_path = write_sample_bundle(&temp);
    let scene_packet = run_minecraft_3dgs_scene_packet_export(
      &recording,
      vec![manifest_path],
      temp.join("scene-packet"),
    )
    .expect("scene packet export");

    let output = run_minecraft_3dgs_training_package_export(
      &recording,
      scene_packet.value.manifest_path.clone(),
      temp.join("training-package"),
    )
    .expect("training package export");

    assert_eq!(output.value.manifest.counts.frames, 1);
    assert!(output.value.manifest_path.is_file());
    assert!(output.value.inspect_report_path.is_file());
    let run = recording
      .read_run(output.run_id.as_str())
      .expect("training package run should persist");
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE)
    );
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_3DGS_TRAINING_PACKAGE_INSPECT_ARTIFACT_ROLE)
    );

    let _ = fs::remove_dir_all(temp);
  }

  #[test]
  fn three_dgs_training_launch_preparation_records_plan_inspect_and_runbook_artifacts() {
    let temp = temp_dir("mc7-training-launch");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();
    let training_manifest_path = write_blocked_training_package_fixture(&temp);

    let output = run_minecraft_3dgs_training_launch_preparation(
      &recording,
      training_manifest_path,
      temp.join("training-launch"),
    )
    .expect("training launch prep");

    assert!(output.value.manifest_path.is_file());
    assert!(output.value.inspect_report_path.is_file());
    assert!(output.value.runbook_path.is_file());
    let run = recording
      .read_run(output.run_id.as_str())
      .expect("training launch run should persist");
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_3DGS_TRAINING_LAUNCH_PLAN_ARTIFACT_ROLE)
    );
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_3DGS_TRAINING_LAUNCH_INSPECT_ARTIFACT_ROLE)
    );
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_3DGS_TRAINING_LAUNCH_RUNBOOK_ARTIFACT_ROLE)
    );

    let _ = fs::remove_dir_all(temp);
  }

  #[test]
  fn three_dgs_training_result_collection_records_manifest_inspect_and_runbook_artifacts() {
    let temp = temp_dir("mc7-training-result");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();
    let job_manifest_path = write_training_job_fixture(&temp);

    let output = run_minecraft_3dgs_training_result_collection(
      &recording,
      job_manifest_path,
      temp.join("training-result"),
    )
    .expect("training result collection");

    assert!(output.value.manifest_path.is_file());
    assert!(output.value.inspect_report_path.is_file());
    assert!(output.value.runbook_path.is_file());
    let run = recording
      .read_run(output.run_id.as_str())
      .expect("training result run should persist");
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_ROLE)
    );
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_3DGS_TRAINING_RESULT_INSPECT_ARTIFACT_ROLE)
    );
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_3DGS_TRAINING_RESULT_RUNBOOK_ARTIFACT_ROLE)
    );

    let _ = fs::remove_dir_all(temp);
  }

  #[test]
  fn spatial_bundle_export_reads_source_run_and_records_manifest() {
    let temp = temp_dir("mc6-spatial-bundle");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();
    let source_file = temp.join("frame.json");
    fs::write(&source_file, br#"{"spatial_frame_id":"frame-1"}"#).expect("source write");

    let source = recording
      .run_recorded_operation(
        RunSpec::new(RunType::Execute, "auv.minecraft.fixture"),
        "fixture source run",
        |context| {
          context.stage_artifact_file(
            MINECRAFT_SPATIAL_FRAME_ARTIFACT_ROLE,
            &source_file,
            "frame.json",
            Some("frame".to_string()),
          )?;
          Ok::<_, String>(())
        },
      )
      .expect("source run");

    let output_dir = temp.join("bundle");
    let output = run_minecraft_spatial_bundle_export(
      &recording,
      source.run_id.as_str().to_string(),
      output_dir.clone(),
      Some("abc123".to_string()),
    )
    .expect("bundle export");

    assert_eq!(output.value.manifest.counts.spatial_frames, 1);
    assert!(output_dir.join("run.json").is_file());
    let export_run = recording
      .read_run(output.run_id.as_str())
      .expect("export run should persist");
    assert!(
      export_run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_SPATIAL_BUNDLE_ARTIFACT_ROLE)
    );
    let manifests = crate::run_read::list_minecraft_spatial_bundle_manifests(
      recording.recording_backend().store(),
      output.run_id.as_str(),
    )
    .expect("spatial bundle manifests should list");
    assert_eq!(manifests.len(), 1);
    assert_eq!(
      manifests[0]
        .manifest
        .as_ref()
        .expect("manifest should parse")
        .source_run
        .source_run_id,
      source.run_id.as_str()
    );

    let _ = fs::remove_dir_all(temp);
  }

  #[test]
  fn texture_sweep_preparation_records_manifest_and_runbook() {
    let temp = temp_dir("mc6-texture-sweep-prep");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();

    let output = run_minecraft_texture_sweep_preparation(
      &recording,
      temp.join("sidecar-run"),
      temp.join("prep-output"),
    )
    .expect("texture sweep prep");

    assert_eq!(output.value.manifest.profiles.len(), 3);
    let run = recording
      .read_run(output.run_id.as_str())
      .expect("prep run should persist");
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_TEXTURE_SWEEP_PREP_ARTIFACT_ROLE)
    );
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_TEXTURE_SWEEP_RUNBOOK_ARTIFACT_ROLE)
    );

    let _ = fs::remove_dir_all(temp);
  }

  #[test]
  fn texture_sweep_sample_build_records_samples_artifact() {
    let temp = temp_dir("mc6-texture-sweep-samples");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();
    let manifest_path = write_sample_bundle(&temp);

    let output = run_minecraft_texture_sweep_sample_build(
      &recording,
      vec![manifest_path],
      temp.join("samples.json"),
    )
    .expect("sample build");

    assert_eq!(output.value.sample_set.samples.len(), 1);
    let run = recording
      .read_run(output.run_id.as_str())
      .expect("sample build run should persist");
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_TEXTURE_SWEEP_SAMPLES_ARTIFACT_ROLE)
    );

    let _ = fs::remove_dir_all(temp);
  }

  #[test]
  fn texture_sweep_eval_records_report_artifact() {
    let temp = temp_dir("mc6-texture-sweep");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();
    let samples_path = temp.join("samples.json");
    fs::write(
      &samples_path,
      serde_json::to_vec_pretty(&auv_game_minecraft::TextureSweepSampleSet {
        source: None,
        samples: vec![
          auv_game_minecraft::TextureSweepSample {
            resource_pack: "rich-pack".to_string(),
            texture_profile: "rich".to_string(),
            duration_seconds: 30.0,
            pose_error_px: 2.0,
            occlusion_iou: 0.95,
            refused_noise: false,
            refusal_reason: None,
          },
          auv_game_minecraft::TextureSweepSample {
            resource_pack: "flat-pack".to_string(),
            texture_profile: "flat_color".to_string(),
            duration_seconds: 30.0,
            pose_error_px: 4.0,
            occlusion_iou: 0.92,
            refused_noise: false,
            refusal_reason: None,
          },
          auv_game_minecraft::TextureSweepSample {
            resource_pack: "flat-pack".to_string(),
            texture_profile: "flat_color".to_string(),
            duration_seconds: 30.0,
            pose_error_px: 20.0,
            occlusion_iou: 0.10,
            refused_noise: true,
            refusal_reason: Some(auv_game_minecraft::MismatchRefusalReason::MenuLoadingScreen),
          },
          auv_game_minecraft::TextureSweepSample {
            resource_pack: "repeat-pack".to_string(),
            texture_profile: "repetitive".to_string(),
            duration_seconds: 30.0,
            pose_error_px: 3.0,
            occlusion_iou: 0.93,
            refused_noise: false,
            refusal_reason: None,
          },
        ],
      })
      .expect("samples json"),
    )
    .expect("samples write");

    let output =
      run_minecraft_texture_sweep_eval(&recording, samples_path, temp.join("sweep-output"), false)
        .expect("sweep eval");

    assert!(output.value.passed);
    let run = recording
      .read_run(output.run_id.as_str())
      .expect("run should persist");
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_TEXTURE_SWEEP_ARTIFACT_ROLE)
    );
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_TEXTURE_SWEEP_SAMPLES_ARTIFACT_ROLE)
    );

    let _ = fs::remove_dir_all(temp);
  }

  fn write_blocked_training_package_fixture(root: &std::path::Path) -> PathBuf {
    let training_dir = root.join("training-package");
    fs::create_dir_all(training_dir.join("frames")).expect("frames dir");
    fs::create_dir_all(training_dir.join("compat/nerfstudio")).expect("compat dir");
    fs::write(
      training_dir.join("known_limits.json"),
      serde_json::to_vec_pretty(&vec![
        "canonical package only; no trainer output".to_string(),
      ])
      .expect("known limits json"),
    )
    .expect("known limits write");

    let manifest = auv_game_minecraft::TrainingPackageManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/run-1/run.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      counts: auv_game_minecraft::TrainingPackageCounts {
        frames: 1,
        images: 1,
        compatibility_exported_frames: 0,
        compatibility_skipped_frames: 1,
      },
      frames: vec![auv_game_minecraft::TrainingPackageFrameRecord {
        frame_index: 1,
        spatial_frame_id: "frame-1".to_string(),
        source_run_id: "run-1".to_string(),
        source_bundle_manifest_path: "/tmp/run-1/run.json".to_string(),
        source_scene_packet_frame_json_path: "frames/frame_000001.json".to_string(),
        canonical_frame_json_path: "frames/frame_000001.json".to_string(),
        canonical_image_path: Some("images/frame_000001.png".to_string()),
        screen_state: Some("menu".to_string()),
        resource_pack_ids: vec!["fabric".to_string(), "file/auv-mc6-rich".to_string()],
        primary_file_resource_pack_id: Some("file/auv-mc6-rich".to_string()),
        compatibility_status: auv_game_minecraft::TrainingCompatibilityStatus::Blocked,
        compatibility_skip_reasons: vec![
          auv_game_minecraft::TrainingCompatibilitySkipReason::MissingScreenshot,
        ],
      }],
      compatibility_views: vec![auv_game_minecraft::TrainingCompatibilityViewReport {
        view_name: "nerfstudio".to_string(),
        status: auv_game_minecraft::TrainingCompatibilityStatus::Blocked,
        exported_frame_count: 0,
        skipped_frame_count: 1,
        transforms_path: None,
        export_report_path: "compat/nerfstudio/export_report.json".to_string(),
        exported_frame_indices: Vec::new(),
        frame_decisions: vec![auv_game_minecraft::TrainingCompatibilityFrameDecision {
          frame_index: 1,
          spatial_frame_id: "frame-1".to_string(),
          source_run_id: "run-1".to_string(),
          status: auv_game_minecraft::TrainingCompatibilityStatus::Blocked,
          skip_reasons: vec![
            auv_game_minecraft::TrainingCompatibilitySkipReason::MissingScreenshot,
          ],
        }],
        skip_reason_counts: vec![auv_game_minecraft::TrainingCompatibilitySkipReasonCount {
          reason: auv_game_minecraft::TrainingCompatibilitySkipReason::MissingScreenshot,
          count: 1,
        }],
        warnings: vec!["no compatible frames".to_string()],
        used_legacy_view_translation_fallback_frame_indices: Vec::new(),
        known_limits: Vec::new(),
      }],
      known_limits: vec!["canonical package only; no trainer output".to_string()],
    };
    fs::write(
      training_dir.join("run.json"),
      serde_json::to_vec_pretty(&manifest).expect("manifest json"),
    )
    .expect("manifest write");
    fs::write(
      training_dir.join("inspect_report.json"),
      serde_json::to_vec_pretty(&auv_game_minecraft::TrainingPackageInspectReport {
        schema_version: 1,
        generated_at_millis: 1,
        training_package_manifest_path: training_dir
          .join("run.json")
          .to_string_lossy()
          .into_owned(),
        scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
        source_bundle_manifest_paths: vec!["/tmp/run-1/run.json".to_string()],
        source_run_ids: vec!["run-1".to_string()],
        counts: manifest.counts.clone(),
        compatibility_views: manifest.compatibility_views.clone(),
        warnings: vec!["no compatible frames".to_string()],
        known_limits: vec!["canonical package only; no trainer output".to_string()],
      })
      .expect("inspect json"),
    )
    .expect("inspect write");
    fs::write(
      training_dir.join("compat/nerfstudio/export_report.json"),
      serde_json::to_vec_pretty(&serde_json::json!({
        "view_name": "nerfstudio",
        "status": "blocked",
        "exported_frame_count": 0,
        "skipped_frame_count": 1
      }))
      .expect("export report json"),
    )
    .expect("export report write");

    training_dir.join("run.json")
  }

  fn write_training_job_fixture(root: &std::path::Path) -> PathBuf {
    let result_dir = root.join("trainer-output/nerfstudio-splatfacto");
    fs::create_dir_all(result_dir.join("nerfstudio_models")).expect("models dir");
    fs::write(result_dir.join("config.yml"), b"trainer: splatfacto\n").expect("config");
    fs::write(
      result_dir.join("job_status.json"),
      serde_json::to_vec_pretty(&serde_json::json!({
        "status": "succeeded",
        "message": "remote result available"
      }))
      .expect("status json"),
    )
    .expect("status write");

    unsafe {
      std::env::set_var(
        "AUV_MINECRAFT_TRAINING_JOB_ENDPOINT",
        "https://jobs.example.test/v1",
      );
      std::env::set_var("AUV_MINECRAFT_TRAINING_JOB_TOKEN", "secret");
    }

    let job_manifest = auv_game_minecraft::TrainingLaunchJobManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_training_launch_plan_path:
        "/tmp/training-launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/training-package/run.json".to_string(),
      source_training_package_inspect_report_path: "/tmp/training-package/inspect_report.json"
        .to_string(),
      source_scene_packet_manifest_path: "/tmp/scene/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle/run.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      counts: auv_game_minecraft::TrainingLaunchJobCounts {
        frames: 2,
        images: 2,
        compatibility_exported_frames: 2,
        compatibility_skipped_frames: 0,
      },
      compatibility_view_name: "nerfstudio".to_string(),
      provider_backend: "remote-command-provider".to_string(),
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      job_submission_endpoint: "https://jobs.example.test/v1".to_string(),
      job_submission_command: "remote-submit --plan launch.json".to_string(),
      submission_recorded_at_millis: Some(1),
      accepted_by_provider: true,
      training_data_dir: "/tmp/training-package/compat/nerfstudio".to_string(),
      transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
      export_report_path: "/tmp/training-package/compat/nerfstudio/export_report.json".to_string(),
      suggested_output_dir: result_dir.to_string_lossy().into_owned(),
      launch_command: "ns-train splatfacto --data compat/nerfstudio --output-dir out".to_string(),
      status: auv_game_minecraft::TrainingLaunchJobStatus::Submitted,
      job_id: Some("job-123".to_string()),
      job_url: Some("https://jobs.example.test/jobs/job-123".to_string()),
      readiness_blocker: None,
      known_limits: vec!["limit-a".to_string()],
    };
    let job_manifest_path = root.join("minecraft-3dgs-training-job.json");
    fs::write(
      &job_manifest_path,
      serde_json::to_vec_pretty(&job_manifest).expect("job manifest json"),
    )
    .expect("job manifest write");
    job_manifest_path
  }

  #[test]
  fn training_job_launch_with_environment_uses_explicit_remote_config() {
    let temp = temp_dir("mc7-d9-job-env");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();
    let training_package_manifest = write_blocked_training_package_fixture(&temp);
    let launch_output = run_minecraft_3dgs_training_launch_preparation(
      &recording,
      training_package_manifest,
      temp.join("launch"),
    )
    .expect("launch prep");

    let job_output = run_minecraft_3dgs_training_job_launch_with_environment(
      &recording,
      launch_output.value.manifest_path.clone(),
      temp.join("job"),
      Some("https://jobs.example.test/v1".to_string()),
      Some("secret-token".to_string()),
      Some(
        "python3 -c \"import json,sys; req=json.load(sys.stdin); json.dump({'status':'submitted','job_id':'job-from-runtime','job_url':req['endpoint'].rstrip('/') + '/jobs/job-from-runtime','blocker':None}, sys.stdout)\"".to_string(),
      ),
    )
    .expect("job launch with explicit environment");

    assert_eq!(
      job_output.value.manifest.job_submission_endpoint,
      "https://jobs.example.test/v1"
    );
    assert_eq!(
      job_output.value.manifest.job_submission_command,
      "python3 -c \"import json,sys; req=json.load(sys.stdin); json.dump({'status':'submitted','job_id':'job-from-runtime','job_url':req['endpoint'].rstrip('/') + '/jobs/job-from-runtime','blocker':None}, sys.stdout)\""
    );
    assert_eq!(
      job_output.value.inspect_report.status,
      auv_game_minecraft::TrainingLaunchJobStatus::Submitted
    );
    assert_eq!(
      job_output.value.inspect_report.job_id.as_deref(),
      Some("job-from-runtime")
    );
    let _ = fs::remove_dir_all(temp);
  }

  #[test]
  fn training_result_collection_with_environment_uses_explicit_remote_config() {
    let temp = temp_dir("mc7-d9-result-env");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();
    let job_manifest_path = write_training_job_fixture(&temp);

    let result_output = run_minecraft_3dgs_training_result_collection_with_environment(
      &recording,
      job_manifest_path,
      temp.join("result"),
      Some("https://jobs.example.test/v1".to_string()),
      Some("secret-token".to_string()),
      Some(
        "python3 -c \"import json,sys; json.dump({'status':'succeeded','message':'runtime-status-bridge'}, sys.stdout)\"".to_string(),
      ),
    )
    .expect("result collection with explicit environment");

    assert_eq!(
      result_output.value.manifest.job_submission_endpoint,
      "https://jobs.example.test/v1"
    );
    assert_eq!(
      result_output.value.inspect_report.status,
      auv_game_minecraft::TrainingResultStatus::Succeeded
    );
    let _ = fs::remove_dir_all(temp);
  }
}
