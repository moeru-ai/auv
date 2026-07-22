// Shared CLI frontend for root `auv` and donor bins (`auv-minecraft`, `auv-osu`, `auv-godot`).
//
// The root binary tombstones app-specific subcommands; dedicated app binaries
// own their live parse and dispatch paths. This product crate owns their shared
// frontend assembly.

use std::env;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode};
use std::sync::Arc;

use image::ImageReader;

use crate::cli::{CliCommand, InspectClientOptions, help_text, parse_cli, parse_donor_cli, root_donor_tombstone, version_text};
use crate::integrations::minecraft::verification::query_wired_verification_readable;
use crate::integrations::minecraft::{
  QueryWiredLiveActionInputs, QueryWiredLiveActionTelemetryWitness, run_minecraft_query_wired_live_action,
};
use auv_runtime::app::{analyze_app_probe, probe_app};
use auv_runtime::contract::{OPERATION_RESULT_API_VERSION, OperationOutput, OperationResult, OperationStatus, VerificationResult};
use auv_runtime::model::InvokeRequest;
use auv_runtime::{build_default_runtime, build_runtime_with_store_root};
use auv_tracing_driver::run_builder::RunSpec;

#[allow(dead_code)] // used by root bin; donor bins only call run_donor_bin
pub async fn run_root() -> Result<i32, String> {
  let arguments = env::args().skip(1).collect::<Vec<_>>();
  if let Some(message) = root_donor_tombstone(&arguments) {
    return Err(message);
  }
  let command = parse_cli(&arguments)?;
  dispatch(command).await
}

#[allow(dead_code)] // used by donor bins; root bin only calls run_root
pub async fn run_donor_bin(donor: &'static str) -> Result<i32, String> {
  let arguments = env::args().skip(1).collect::<Vec<_>>();
  let command = parse_donor_cli(donor, &arguments)?;
  dispatch(command).await
}

pub fn exit_status(result: Result<i32, String>) -> ExitCode {
  match result {
    Ok(0) => ExitCode::SUCCESS,
    Ok(exit_code @ 1..=255) => ExitCode::from(exit_code as u8),
    Ok(_) => ExitCode::FAILURE,
    Err(error) => {
      eprintln!("error: {error}");
      ExitCode::FAILURE
    }
  }
}

async fn dispatch(command: CliCommand) -> Result<i32, String> {
  if matches!(&command, CliCommand::Version) {
    print!("{}", version_text());
    return Ok(0);
  }

  let project_root = env::current_dir().map_err(|error| format!("failed to resolve current directory: {error}"))?;
  if let CliCommand::XtaskGenerateSwiftBridge = &command {
    let outputs = crate::xtask::generate_swift_bridge_for_ide(&project_root)?;
    println!("generated Swift bridge files for IDE indexing");
    for output in outputs {
      println!("output: {output}");
    }
    return Ok(0);
  }

  if let CliCommand::McpServe = &command {
    crate::mcp::serve_stdio(project_root.clone()).await?;
    return Ok(0);
  }

  if let CliCommand::PermissionCheck { json } = &command {
    run_permission_check(*json)?;
    return Ok(0);
  }

  if let CliCommand::InspectServe {
    host,
    port,
    store_root,
    write,
  } = &command
  {
    let store_root = resolve_store_root(&project_root, store_root.as_ref());
    // TODO(run-contract-task-16): Remove the retired parser fields when the
    // CLI contract migration reaches `cli.rs`; V1 never consumes them.
    if write != &crate::cli::InspectServeWriteOptions::default() {
      return Err(
        "inspect serve write-token options are retired; the V1 authority is loopback-only and has no credential contract".to_string(),
      );
    }
    let store = open_inspect_authority_store(&store_root)?;
    let config = auv_inspect_server::InspectServeConfig {
      host: host.clone(),
      port: *port,
    };
    auv_inspect_server::serve(store, config).await?;
    return Ok(0);
  }

  if let CliCommand::SessionServe {
    host,
    port,
    store_root,
  } = &command
  {
    let store_root = resolve_store_root(&project_root, store_root.as_ref());
    let config = auv_runtime::api::session_service::transport::SessionApiServeConfig {
      host: host.clone(),
      port: *port,
      store_root,
    };
    auv_runtime::api::session_service::transport::serve(config).await?;
    return Ok(0);
  }

  let mut exit_code = 0;
  match command {
    CliCommand::Help => {
      print!("{}", help_text());
    }
    CliCommand::Version => unreachable!("version is handled before runtime setup"),
    CliCommand::MinecraftHelp => {
      print!("{}", crate::integrations::minecraft::help::render_minecraft_help());
    }
    CliCommand::OsuHelp => {
      print!("{}", crate::integrations::osu::help::render_osu_help());
    }
    CliCommand::GodotHelp => {
      print!("{}", crate::integrations::godot::help::render_godot_help());
    }
    CliCommand::PermissionCheck { .. } => {
      unreachable!("permission check is handled before runtime setup")
    }
    CliCommand::MinecraftProjectionBridge {
      telemetry_sample,
      screenshot,
      capture_target_app,
      capture_target_title,
      target_block,
      capture_skew_ms,
      screenshot_is_minecraft_window,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = run_minecraft_projection_bridge(
        &runtime,
        PathBuf::from(telemetry_sample),
        screenshot.as_ref().map(PathBuf::from),
        capture_target_app.as_deref(),
        capture_target_title.as_deref(),
        &target_block,
        capture_skew_ms,
        screenshot_is_minecraft_window,
      )?;
      println!("runId: {}", output.run_id);
      println!("projectionArtifact: {}", output.value.projection_artifact_id);
      println!("screenshotArtifact: {}", output.value.screenshot_artifact_id);
      if let Some(overlay_artifact_id) = output.value.overlay_artifact_id.as_deref() {
        println!("overlayArtifact: {overlay_artifact_id}");
      }
      if let Some(refusal_reason) = output.value.refusal_reason.as_deref() {
        println!("refusalReason: {refusal_reason}");
      } else {
        println!("refusalReason: none");
      }
      for artifact in &output.value.artifact_paths {
        println!("artifact: {}", artifact.display());
      }
    }
    CliCommand::MinecraftCalibrateProjection {
      frame_path,
      screenshot,
      target_block,
      target_semantics,
      screenshot_is_minecraft_window,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = run_minecraft_calibrate_projection(
        &runtime,
        PathBuf::from(frame_path),
        PathBuf::from(screenshot),
        &target_block,
        &target_semantics,
        screenshot_is_minecraft_window,
      )?;
      println!("runId: {}", output.run_id);
      println!("projectionArtifact: {}", output.value.projection_artifact_id);
      println!("screenshotArtifact: {}", output.value.screenshot_artifact_id);
      println!("calibrationArtifact: {}", output.value.calibration_artifact_id);
      if let Some(overlay_artifact_id) = output.value.overlay_artifact_id.as_deref() {
        println!("overlayArtifact: {overlay_artifact_id}");
      }
      if let Some(refusal_reason) = output.value.refusal_reason.as_deref() {
        println!("refusalReason: {refusal_reason}");
      } else {
        println!("refusalReason: none");
      }
      for artifact in &output.value.artifact_paths {
        println!("artifact: {}", artifact.display());
      }
    }
    CliCommand::MinecraftLiveClick {
      telemetry_sample,
      screenshot,
      target_block,
      target_app,
      target_title,
      post_telemetry_sample,
      capture_skew_ms,
      screenshot_is_minecraft_window,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = run_minecraft_live_click(
        &runtime,
        PathBuf::from(telemetry_sample),
        post_telemetry_sample.as_ref().map(PathBuf::from),
        PathBuf::from(screenshot),
        &target_block,
        &target_app,
        &target_title,
        capture_skew_ms,
        screenshot_is_minecraft_window,
      )?;
      println!("runId: {}", output.run_id);
      println!("projectionArtifact: {}", output.value.projection_artifact_id);
      println!("screenshotArtifact: {}", output.value.screenshot_artifact_id);
      println!("operationResultArtifact: {}", output.value.operation_result_artifact_id);
      println!("inputSummary: {}", output.value.input_summary);
      for artifact in &output.value.artifact_paths {
        println!("artifact: {}", artifact.display());
      }
    }
    CliCommand::MinecraftExportSpatialBundle {
      run_id,
      output_dir,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = crate::integrations::minecraft::run_minecraft_spatial_bundle_export(
        &runtime.recording().handle(),
        run_id,
        PathBuf::from(output_dir),
        crate::integrations::minecraft::current_git_commit(),
      )?;
      println!("runId: {}", output.run_id);
      println!("status: completed");
      println!("sourceRunId: {}", output.value.manifest.source_run.source_run_id);
      println!("spatialFrames: {}", output.value.manifest.counts.spatial_frames);
      println!("screenshots: {}", output.value.manifest.counts.screenshots);
      println!("verification: {}", output.value.manifest.counts.verification);
      println!("overlays: {}", output.value.manifest.counts.overlays);
      println!("output: {}", output.value.output_dir.display());
    }
    CliCommand::MinecraftExport3dgsScenePacket {
      bundle_manifest_paths,
      output_dir,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = crate::integrations::minecraft::run_minecraft_3dgs_scene_packet_export(
        &runtime.recording().handle(),
        bundle_manifest_paths.into_iter().map(PathBuf::from).collect(),
        PathBuf::from(output_dir),
      )?;
      println!("runId: {}", output.run_id);
      println!("status: completed");
      println!("scenePacketSchema: {}", output.value.manifest.schema_version);
      println!("sourceRuns: {}", output.value.manifest.source_run_ids.join(","));
      println!("frames: {}", output.value.manifest.counts.frames);
      println!("screenshots: {}", output.value.manifest.counts.screenshots);
      println!("missingScreenshots: {}", output.value.manifest.counts.missing_screenshots);
      println!("manifest: {}", output.value.manifest_path.display());
      println!("cameras: {}", output.value.cameras_path.display());
      println!("output: {}", output.value.output_dir.display());
    }
    CliCommand::MinecraftExport3dgsTrainingPackage {
      scene_packet_manifest_path,
      output_dir,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = crate::integrations::minecraft::run_minecraft_3dgs_training_package_export(
        &runtime.recording().handle(),
        PathBuf::from(scene_packet_manifest_path),
        PathBuf::from(output_dir),
      )?;
      println!("runId: {}", output.run_id);
      println!("status: completed");
      println!("trainingPackageSchema: {}", output.value.manifest.schema_version);
      println!("sourceRuns: {}", output.value.manifest.source_run_ids.join(","));
      println!("frames: {}", output.value.manifest.counts.frames);
      println!("images: {}", output.value.manifest.counts.images);
      println!(
        "compatibilityStatus: {}",
        match output.value.inspect_report.compatibility_views[0].status {
          auv_game_minecraft::TrainingCompatibilityStatus::Ready => "ready",
          auv_game_minecraft::TrainingCompatibilityStatus::Partial => "partial",
          auv_game_minecraft::TrainingCompatibilityStatus::Blocked => "blocked",
        }
      );
      println!("compatibilityExportedFrames: {}", output.value.manifest.counts.compatibility_exported_frames);
      println!("manifest: {}", output.value.manifest_path.display());
      println!("inspectReport: {}", output.value.inspect_report_path.display());
      if let Some(transforms_path) = output.value.compatibility_transforms_path.as_ref() {
        println!("nerfstudioTransforms: {}", transforms_path.display());
      } else {
        println!("nerfstudioTransforms: none");
      }
      println!("output: {}", output.value.output_dir.display());
    }
    CliCommand::MinecraftPrepare3dgsTraining {
      training_package_manifest_path,
      output_dir,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = crate::integrations::minecraft::run_minecraft_3dgs_training_launch_preparation(
        &runtime.recording().handle(),
        PathBuf::from(training_package_manifest_path),
        PathBuf::from(output_dir),
      )?;
      println!("runId: {}", output.run_id);
      println!("status: completed");
      println!("trainerBackend: {}", output.value.manifest.trainer_backend);
      println!(
        "trainerReadiness: {}",
        match output.value.inspect_report.trainer_readiness {
          auv_game_minecraft::TrainingLaunchReadiness::Ready => "ready",
          auv_game_minecraft::TrainingLaunchReadiness::Blocked => "blocked",
        }
      );
      println!(
        "readinessBlocker: {}",
        match output.value.inspect_report.readiness_blocker {
          Some(auv_game_minecraft::TrainingLaunchReadinessBlocker::CompatibilityViewBlocked) => {
            "compatibility_view_blocked"
          }
          Some(auv_game_minecraft::TrainingLaunchReadinessBlocker::TransformsMissing) => {
            "transforms_missing"
          }
          Some(auv_game_minecraft::TrainingLaunchReadinessBlocker::TrainerCommandUnavailable) => {
            "trainer_command_unavailable"
          }
          None => "none",
        }
      );
      println!("launchCommand: {}", output.value.manifest.launch_command);
      println!("launchPlan: {}", output.value.manifest_path.display());
      println!("inspectReport: {}", output.value.inspect_report_path.display());
      println!("runbook: {}", output.value.runbook_path.display());
      println!("output: {}", output.value.output_dir.display());
    }
    CliCommand::MinecraftLaunch3dgsTrainingJob {
      training_launch_plan_path,
      output_dir,
      training_job_endpoint,
      training_job_token,
      training_job_submit_command,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = crate::integrations::minecraft::run_minecraft_3dgs_training_job_launch_with_environment(
        &runtime.recording().handle(),
        PathBuf::from(training_launch_plan_path),
        PathBuf::from(output_dir),
        training_job_endpoint,
        training_job_token,
        training_job_submit_command,
      )?;
      println!("runId: {}", output.run_id);
      println!("remoteJobStatus: {}", output.value.inspect_report.status.as_str());
      println!("trainerBackend: {}", output.value.manifest.trainer_backend);
      println!("providerBackend: {}", output.value.manifest.provider_backend);
      println!("jobBackend: {}", output.value.manifest.job_backend);
      println!(
        "submissionState: {}",
        match output.value.inspect_report.status {
          auv_game_minecraft::TrainingLaunchJobStatus::Blocked => "blocked_before_submission",
          auv_game_minecraft::TrainingLaunchJobStatus::Failed => "submission_failed",
          auv_game_minecraft::TrainingLaunchJobStatus::Queued
          | auv_game_minecraft::TrainingLaunchJobStatus::Submitted
          | auv_game_minecraft::TrainingLaunchJobStatus::Succeeded => {
            "submission_submitted_or_queued"
          }
        }
      );
      println!("acceptedByProvider: {}", output.value.inspect_report.accepted_by_provider);
      println!(
        "submissionRecordedAtMillis: {}",
        output.value.inspect_report.submission_recorded_at_millis.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string())
      );
      println!(
        "readinessBlocker: {}",
        match output.value.inspect_report.readiness_blocker {
          Some(auv_game_minecraft::TrainingLaunchJobBlocker::MissingConfiguration) => {
            "missing_configuration"
          }
          Some(auv_game_minecraft::TrainingLaunchJobBlocker::MissingAuthentication) => {
            "missing_authentication"
          }
          Some(auv_game_minecraft::TrainingLaunchJobBlocker::IncompleteLaunchPlan) => {
            "incomplete_launch_plan"
          }
          Some(auv_game_minecraft::TrainingLaunchJobBlocker::UnsupportedBackend) => {
            "unsupported_backend"
          }
          Some(auv_game_minecraft::TrainingLaunchJobBlocker::SubmissionFailed) => {
            "submission_failed"
          }
          None => "none",
        }
      );
      println!("launchCommand: {}", output.value.manifest.launch_command);
      println!("configuredJobSubmissionCommand: {}", output.value.manifest.job_submission_command);
      println!("launchPlan: {}", output.value.manifest.source_training_launch_plan_path);
      println!("manifest: {}", output.value.manifest_path.display());
      println!("inspectReport: {}", output.value.inspect_report_path.display());
      println!("runbook: {}", output.value.runbook_path.display());
      println!("output: {}", output.value.output_dir.display());
    }
    CliCommand::MinecraftCollect3dgsTrainingJobResult {
      training_job_manifest_path,
      output_dir,
      training_job_endpoint,
      training_job_token,
      training_job_status_command,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = crate::integrations::minecraft::run_minecraft_3dgs_training_result_collection_with_environment(
        &runtime.recording().handle(),
        PathBuf::from(training_job_manifest_path),
        PathBuf::from(output_dir),
        training_job_endpoint,
        training_job_token,
        training_job_status_command,
      )?;
      println!("runId: {}", output.run_id);
      println!("status: {}", output.value.inspect_report.status.as_str());
      println!("statusMessage: {}", output.value.inspect_report.status_message.as_deref().unwrap_or("none"));
      println!("remoteResultStatus: {}", output.value.inspect_report.status.as_str());
      println!("trainerBackend: {}", output.value.manifest.trainer_backend);
      println!("jobBackend: {}", output.value.manifest.job_backend);
      println!(
        "statusReason: {}",
        match output.value.inspect_report.status_reason {
          Some(auv_game_minecraft::TrainingResultReason::MissingConfiguration) => {
            "missing_configuration"
          }
          Some(auv_game_minecraft::TrainingResultReason::MissingAuthentication) => {
            "missing_authentication"
          }
          Some(auv_game_minecraft::TrainingResultReason::LaunchBlocked) => "launch_blocked",
          Some(auv_game_minecraft::TrainingResultReason::RemoteStatusUnavailable) => {
            "remote_status_unavailable"
          }
          Some(auv_game_minecraft::TrainingResultReason::ProviderReportedFailed) => {
            "provider_reported_failed"
          }
          Some(auv_game_minecraft::TrainingResultReason::ResultDirectoryMissing) => {
            "result_directory_missing"
          }
          Some(auv_game_minecraft::TrainingResultReason::ResultArtifactsMissing) => {
            "result_artifacts_missing"
          }
          None => "none",
        }
      );
      println!(
        "resultStateInterpretation: {}",
        match output.value.inspect_report.status_reason {
          Some(auv_game_minecraft::TrainingResultReason::MissingConfiguration) => {
            "blocked_without_remote_configuration"
          }
          Some(auv_game_minecraft::TrainingResultReason::MissingAuthentication) => {
            "blocked_without_remote_authentication"
          }
          Some(auv_game_minecraft::TrainingResultReason::LaunchBlocked) => {
            "upstream_job_never_submitted"
          }
          Some(auv_game_minecraft::TrainingResultReason::RemoteStatusUnavailable) => {
            "remote_job_state_not_yet_readable"
          }
          Some(auv_game_minecraft::TrainingResultReason::ProviderReportedFailed) => {
            "provider_reported_training_failed"
          }
          Some(auv_game_minecraft::TrainingResultReason::ResultDirectoryMissing) => {
            "legacy_adapter_result_dir_missing"
          }
          Some(auv_game_minecraft::TrainingResultReason::ResultArtifactsMissing) => {
            "legacy_adapter_key_result_artifacts_missing"
          }
          None => match output.value.inspect_report.status {
            auv_game_minecraft::TrainingResultStatus::Succeeded
            | auv_game_minecraft::TrainingResultStatus::Submitted
            | auv_game_minecraft::TrainingResultStatus::Queued => {
              if !output.value.inspect_report.result_dir_exists || !output.value.inspect_report.key_result_artifacts_present {
                "provider_status_recorded_local_results_not_yet_observed"
              } else {
                "provider_status_matches_local_result_observation"
              }
            }
            _ => "provider_status_recorded",
          },
        }
      );
      println!("jobId: {}", output.value.manifest.job_id);
      println!("jobUrl: {}", output.value.manifest.job_url.as_deref().unwrap_or("none"));
      println!("resultDir: {}", output.value.manifest.result_dir);
      println!("resultDirExists: {}", output.value.inspect_report.result_dir_exists);
      println!("keyResultArtifactsPresent: {}", output.value.inspect_report.key_result_artifacts_present);
      println!("manifest: {}", output.value.manifest_path.display());
      println!("inspectReport: {}", output.value.inspect_report_path.display());
      println!("runbook: {}", output.value.runbook_path.display());
      println!("output: {}", output.value.output_dir.display());
    }
    CliCommand::MinecraftFetch3dgsTrainingResultArtifacts {
      training_result_manifest_path,
      output_dir,
      training_job_endpoint,
      training_job_token,
      artifact_fetch_command,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = crate::integrations::minecraft::run_minecraft_3dgs_training_result_artifact_fetch(
        &runtime.recording().handle(),
        PathBuf::from(training_result_manifest_path),
        PathBuf::from(output_dir),
        training_job_endpoint,
        training_job_token,
        artifact_fetch_command,
      )?;
      println!("runId: {}", output.run_id);
      println!("fetchStatus: {}", output.value.inspect_report.fetch_status.as_str());
      println!("trainerBackend: {}", output.value.manifest.trainer_backend);
      println!("jobBackend: {}", output.value.manifest.job_backend);
      println!("sourceResultStatus: {}", output.value.manifest.source_result_status.as_str());
      println!("fetchReason: {}", output.value.inspect_report.fetch_reason.map(|reason| reason.as_str()).unwrap_or("none"));
      println!("sourceResultDir: {}", output.value.manifest.source_result_dir);
      println!("normalizedResultDir: {}", output.value.manifest.normalized_result_dir);
      println!("normalizedArtifactCount: {}", output.value.inspect_report.normalized_artifact_count);
      println!("requiredArtifactsPresent: {}", output.value.inspect_report.required_artifacts_present);
      println!("manifest: {}", output.value.manifest_path.display());
      println!("inspectReport: {}", output.value.inspect_report_path.display());
      println!("output: {}", output.value.output_dir.display());
    }

    CliCommand::MinecraftInspect3dgsTrainingResultHoldout {
      training_result_semantic_manifest_path,
      holdout_frame_index,
      holdout_render_command,
      output_dir,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = crate::integrations::minecraft::run_minecraft_3dgs_training_result_holdout_preview(
        &runtime.recording().handle(),
        PathBuf::from(training_result_semantic_manifest_path),
        holdout_frame_index,
        holdout_render_command,
        PathBuf::from(output_dir),
      )?;
      println!("runId: {}", output.run_id);
      println!("status: {}", output.value.manifest.status.as_str());
      println!("reason: {}", output.value.manifest.reason.map(|reason| reason.as_str()).unwrap_or("none"));
      println!("holdoutFrameIndex: {}", output.value.manifest.holdout_frame_index);
      println!(
        "spatialFrameId: {}",
        output.value.manifest.holdout_frame.as_ref().map(|witness| witness.spatial_frame_id.as_str()).unwrap_or("none")
      );
      println!("basisCheckpointPath: {}", output.value.manifest.basis_checkpoint_path.as_deref().unwrap_or("none"));
      println!("holdoutScreenshotPath: {}", output.value.manifest.holdout_screenshot_path.as_deref().unwrap_or("none"));
      println!("holdoutPreviewManifest: {}", output.value.manifest_path.display());
      println!("inspectReport: {}", output.value.inspect_report_path.display());
    }

    CliCommand::MinecraftMeasure3dgsHoldoutRenderQuality {
      training_result_semantic_manifest_path,
      holdout_preview_manifest_path,
      render_command,
      output_dir,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = crate::integrations::minecraft::run_minecraft_measure_3dgs_holdout_render_quality(
        &runtime.recording().handle(),
        PathBuf::from(training_result_semantic_manifest_path),
        PathBuf::from(holdout_preview_manifest_path),
        render_command,
        PathBuf::from(output_dir),
      )?;
      println!("runId: {}", output.run_id);
      println!("status: {}", output.value.manifest.status.as_str());
      println!("verdict: {}", output.value.manifest.verdict.as_str());
      println!("imageSizeMatch: {}", output.value.manifest.image_size_match);
      let metrics = output.value.manifest.metrics.as_ref();
      println!(
        "l1Mean: {}",
        metrics.and_then(|metrics| metrics.l1_mean).map(|value| value.to_string()).unwrap_or_else(|| "none".to_string())
      );
      println!("mse: {}", metrics.and_then(|metrics| metrics.mse).map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()));
      println!("psnr: {}", metrics.and_then(|metrics| metrics.psnr).map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()));
      println!("manifest: {}", output.value.manifest_path.display());
      println!("inspectReport: {}", output.value.inspect_report_path.display());
      println!("output: {}", output.value.output_dir.display());
    }
    CliCommand::MinecraftQuery3dgsTrainingResult {
      training_result_semantic_manifest_path,
      target_block,
      target_face,
      target_semantics,
      query_command,
      use_checkpoint_native_provider,
      use_closed_scene_toy_provider,
      closed_scene_fixture_path,
      output_dir,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let target_block = parse_block_position(&target_block)?;
      let target_face = target_face.as_deref().map(parse_block_face).transpose()?;
      let target_semantics = parse_target_semantics(&target_semantics)?;
      let output = crate::integrations::minecraft::run_minecraft_3dgs_training_result_spatial_query(
        &runtime.recording().handle(),
        PathBuf::from(training_result_semantic_manifest_path),
        target_block,
        target_face,
        target_semantics,
        query_command,
        use_checkpoint_native_provider,
        use_closed_scene_toy_provider,
        closed_scene_fixture_path.map(PathBuf::from),
        PathBuf::from(output_dir),
      )?;
      println!("runId: {}", output.run_id);
      println!("status: {}", output.value.manifest.status.as_str());
      if matches!(
        output.value.manifest.status,
        auv_game_minecraft::TrainingResultSpatialQueryStatus::Blocked | auv_game_minecraft::TrainingResultSpatialQueryStatus::Failed
      ) {
        println!("reason: {}", output.value.manifest.reason.map(|reason| reason.as_str()).unwrap_or("none"));
      }
      println!("selectedBackend: {}", output.value.manifest.selected_backend.map(|backend| backend.as_str()).unwrap_or("none"));
      println!(
        "visibility: {}",
        output.value.manifest.visibility.map(|visibility| format!("{visibility:?}")).unwrap_or_else(|| "none".to_string())
      );
      if let Some(screen_point) = output.value.manifest.screen_point {
        println!("screenPoint: {},{}", screen_point.x, screen_point.y);
      } else {
        println!("screenPoint: none");
      }
      println!("basisFrameId: {}", output.value.manifest.basis_frame_id.as_deref().unwrap_or("none"));
      println!("comparisonVerdict: {}", output.value.manifest.comparison_verdict.map(|verdict| verdict.as_str()).unwrap_or("none"));
      println!("queryManifest: {}", output.value.manifest_path.display());
      println!("inspectReport: {}", output.value.inspect_report_path.display());
    }
    CliCommand::MinecraftQueryWiredLiveClick {
      training_result_semantic_manifest_path,
      target_block,
      target_face,
      target_semantics,
      query_command,
      use_checkpoint_native_provider,
      use_closed_scene_toy_provider,
      closed_scene_fixture_path,
      output_dir,
      target_app,
      target_title,
      telemetry_sample,
      post_telemetry_sample,
      verification_expected_item_id,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let target_block = parse_block_position(&target_block)?;
      let target_face = target_face.as_deref().map(parse_block_face).transpose()?;
      let target_semantics = parse_target_semantics(&target_semantics)?;
      let telemetry_witness = telemetry_sample.map(|pre_sample| QueryWiredLiveActionTelemetryWitness {
        pre_telemetry_sample: PathBuf::from(pre_sample),
        post_telemetry_sample: post_telemetry_sample.map(PathBuf::from),
      });
      let inputs = QueryWiredLiveActionInputs {
        training_result_semantic_manifest_path: PathBuf::from(training_result_semantic_manifest_path),
        target_block,
        target_face,
        target_semantics,
        query_command,
        use_checkpoint_native_provider,
        use_closed_scene_toy_provider,
        closed_scene_fixture_path: closed_scene_fixture_path.map(PathBuf::from),
        output_dir: PathBuf::from(output_dir),
        target_app,
        target_title,
        telemetry_witness,
        verification_expected_item_id,
      };
      let output = run_minecraft_query_wired_live_action(&runtime.recording().handle(), inputs)?;
      println!("runId: {}", output.run_id);
      println!("queryStatus: {}", output.value.query.manifest.status.as_str());
      println!("wiringAttempted: {}", output.value.wiring.attempted);
      println!("actionEligibility: {}", output.value.wiring.action_eligibility.as_str());
      println!("operationResultArtifact: {}", output.value.operation_result_artifact_id);
      if query_wired_verification_readable(&output.value.wiring) && should_write_local(&inspect) {
        println!("{}", format_query_wired_inspect_hint(&output.run_id, &inspect));
      }
    }
    CliCommand::MinecraftValidate3dgsTrainingResult {
      training_result_artifact_manifest_path,
      output_dir,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = crate::integrations::minecraft::run_minecraft_3dgs_training_result_semantic_validation(
        &runtime.recording().handle(),
        PathBuf::from(training_result_artifact_manifest_path),
        PathBuf::from(output_dir),
      )?;
      println!("runId: {}", output.run_id);
      println!("status: {}", output.value.inspect_report.semantic_status.as_str());
      println!("reason: {}", output.value.inspect_report.semantic_reason.map(|reason| reason.as_str()).unwrap_or("none"));
      println!("trainerBackend: {}", output.value.manifest.trainer_backend);
      println!("checkpointCount: {}", output.value.inspect_report.checkpoint_count);
      println!("configTrainer: {}", output.value.inspect_report.config_trainer.as_deref().unwrap_or("none"));
      println!("semanticManifest: {}", output.value.manifest_path.display());
      println!("inspectReport: {}", output.value.inspect_report_path.display());
    }
    CliCommand::MinecraftPrepareTextureSweep {
      sidecar_run_dir,
      output_dir,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = crate::integrations::minecraft::run_minecraft_texture_sweep_preparation(
        &runtime.recording().handle(),
        PathBuf::from(sidecar_run_dir),
        PathBuf::from(output_dir),
      )?;
      println!("runId: {}", output.run_id);
      println!("status: prepared");
      println!("packFormat: {}", output.value.manifest.pack_format);
      println!("profiles: {}", output.value.manifest.profiles.len());
      for profile in &output.value.manifest.profiles {
        println!(
          "profile: {} pack={} expectedTelemetryId={} optionsResourcePacks={}",
          profile.texture_profile, profile.pack_dir, profile.expected_telemetry_resource_pack_id, profile.options_resource_packs_value
        );
      }
      println!("manifest: {}", output.value.manifest_path.display());
      println!("runbook: {}", output.value.runbook_path.display());
    }
    CliCommand::MinecraftBuildTextureSweepSamples {
      bundle_manifest_paths,
      output_path,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = crate::integrations::minecraft::run_minecraft_texture_sweep_sample_build(
        &runtime.recording().handle(),
        bundle_manifest_paths.into_iter().map(PathBuf::from).collect(),
        PathBuf::from(output_path),
      )?;
      println!("runId: {}", output.run_id);
      println!("status: completed");
      println!("samples: {}", output.value.sample_set.samples.len());
      if let Some(source) = &output.value.sample_set.source {
        println!("sampleSourceGenerator: {}", source.generator);
        println!("sampleSourceRuns: {}", source.source_run_ids.join(","));
        println!("bundleManifests: {}", source.bundle_manifest_paths.join(","));
      }
      println!("output: {}", output.value.output_path.display());
    }
    CliCommand::MinecraftEvalTextureSweep {
      samples_path,
      output_dir,
      require_real_source,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = crate::integrations::minecraft::run_minecraft_texture_sweep_eval(
        &runtime.recording().handle(),
        PathBuf::from(samples_path),
        PathBuf::from(output_dir),
        require_real_source,
      )?;
      println!("runId: {}", output.run_id);
      println!("status: completed");
      println!("requireRealSource: {require_real_source}");
      println!("passed: {}", output.value.passed);
      println!("resourcePacks: {}", output.value.actual_resource_pack_count);
      println!("noiseRefusalExercised: {}", output.value.noise_refusal_exercised);
      if let Some(source) = &output.value.source {
        println!("sampleSourceGenerator: {}", source.generator);
        if !source.source_run_ids.is_empty() {
          println!("sampleSourceRuns: {}", source.source_run_ids.join(","));
        }
      }
      for row in &output.value.rows {
        println!(
          "row: pack={} profile={} samples={} poseP95={} minIoU={} passed={}",
          row.resource_pack,
          row.texture_profile,
          row.sample_count,
          row.pose_error_p95_px.map(|value| format!("{value:.3}")).unwrap_or_else(|| "n/a".to_string()),
          row.min_occlusion_iou.map(|value| format!("{value:.3}")).unwrap_or_else(|| "n/a".to_string()),
          row.passed
        );
      }
    }
    CliCommand::XtaskGenerateSwiftBridge => unreachable!("xtask is handled before runtime setup"),
    CliCommand::ListCommandsTombstone => {
      return Err("`list-commands` has been removed; use `auv invoke --help` instead".to_string());
    }
    CliCommand::InvokeHelp { command_id } => {
      let registry = crate::product_registry();
      if let Some(command_id) = command_id {
        let command = registry
          .resolve(&command_id)
          .ok_or_else(|| format!("unknown command {command_id}; use `auv invoke --help` to inspect available entries"))?;
        print!("{}", auv_cli_invoke::render_command_help(command));
      } else {
        print!("{}", auv_cli_invoke::render_help_index(&registry));
      }
    }
    CliCommand::AppProbe {
      bundle_id,
      output_dir,
    } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let probe = probe_app(&project_root, &runtime, &bundle_id, output_dir.map(PathBuf::from))?;
      println!("app: {}", probe.app.bundle_id);
      println!("status: captured");
      println!("probe: {}", probe.output_dir.join("probe.json").display());
      println!("steps: {}", probe.steps.len());
    }
    CliCommand::AppAnalyze { query } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let output = analyze_app_probe(&runtime, &PathBuf::from(query))?;
      println!("app: {}", output.analysis.app_identity.bundle_id);
      println!("status: analyzed");
      println!("analysis: {}", output.analysis_path.display());
      println!("report: {}", output.report_path.display());
      println!("annotations: {}", output.analysis.annotation_candidates.len());
    }
    CliCommand::GodotCapabilityQuery { json } => {
      let capabilities = auv_godot::query_current_capabilities().map_err(|error| error.to_string())?;
      if json {
        println!(
          "{}",
          serde_json::to_string_pretty(&capabilities).map_err(|error| format!("failed to serialize Godot capabilities: {error}"))?
        );
      } else {
        println!("transport: {}", capabilities.transport);
        println!("pid: {}", capabilities.process.pid);
        println!("projectPath: {}", capabilities.process.project_path.display());
        println!("airiBridgeConnected: {}", capabilities.process.airi_bridge_connected);
        println!("features: {}", capabilities.features.join(", "));
        println!("renderStages: {}", capabilities.render_stages.join(", "));
        println!("cameraPresets: {}", capabilities.camera_presets.join(", "));
      }
    }
    CliCommand::GodotRenderObserve {
      output_dir,
      stages,
      json,
    } => {
      let artifact = auv_godot::export_current_render_observation(output_dir, stages).map_err(|error| error.to_string())?;
      if json {
        println!(
          "{}",
          serde_json::to_string_pretty(&artifact).map_err(|error| format!("failed to serialize Godot render observation: {error}"))?
        );
      } else {
        println!("status: exported");
        println!("outputDir: {}", artifact.output_dir.display());
        println!("manifest: {}", artifact.manifest_path.display());
        println!("finalScreenshot: {}", artifact.final_capture.path.display());
        println!("stages: {}", artifact.request.stages.join(", "));
        println!("files: {}", artifact.export.exported_files.len());
        if let Some(path) = &artifact.context_files.context {
          println!("context: {}", path.display());
        }
        if let Some(path) = &artifact.context_files.view_snapshot {
          println!("viewSnapshot: {}", path.display());
        }
        if let Some(path) = &artifact.context_files.scene {
          println!("scene: {}", path.display());
        }
      }
    }
    CliCommand::OsuBenchmark {
      beatmap_path,
      output_dir,
    } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let recording = runtime.recording().handle();
      let beatmap_path = PathBuf::from(beatmap_path);
      let output_dir = output_dir.map(PathBuf::from).unwrap_or_else(|| temp_runtime_store_root().join("osu-benchmark-output"));
      let output = crate::integrations::osu::run_osu_benchmark(&recording, beatmap_path, output_dir)?;
      println!("runId: {}", output.run_id);
      println!("status: completed");
      println!("beatmap: {}", output.value.map_summary.beatmap_path);
      println!("objects: {}", output.value.map_summary.total_objects);
      println!("latencyP95Ms: {}", output.value.latency_report.p95_error_ms);
      println!("jitterMs: {}", output.value.latency_report.jitter_ms);
      println!("output: {}", output.value.output_dir.display());
    }
    CliCommand::OsuBenchmarkDispatch {
      beatmap_path,
      target_app,
      output_dir,
      dispatch_limit,
      capture_verify,
    } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let recording = runtime.recording().handle();
      let beatmap_path = PathBuf::from(beatmap_path);
      let output_dir = output_dir.map(PathBuf::from).unwrap_or_else(|| temp_runtime_store_root().join("osu-dispatch-output"));
      let mut inputs = auv_game_osu::BenchmarkInputs::typed_dispatch(beatmap_path, output_dir, target_app);
      if let Some(dispatch_limit) = dispatch_limit {
        inputs.dispatch_limit = Some(dispatch_limit);
      }
      inputs.capture_verify = capture_verify;
      let output = crate::integrations::osu::run_osu_benchmark_with_inputs(
        &recording,
        inputs,
        if capture_verify {
          "osu benchmark typed dispatch with capture verification"
        } else {
          "osu benchmark typed dispatch"
        },
      )?;
      println!("runId: {}", output.run_id);
      println!("status: completed");
      println!("beatmap: {}", output.value.map_summary.beatmap_path);
      println!("objects: {}", output.value.map_summary.total_objects);
      println!("latencyP95Ms: {}", output.value.latency_report.p95_error_ms);
      println!("jitterMs: {}", output.value.latency_report.jitter_ms);
      if let Some(summary) = &output.value.verification_summary {
        println!("verificationCapturedActions: {}", summary.captured_action_count);
        println!("verificationMissingFrames: {}", summary.missing_frame_count);
      }
      println!("output: {}", output.value.output_dir.display());
    }
    CliCommand::OsuExportDataset {
      run_artifact_dir,
      output_dir,
    } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let recording = runtime.recording().handle();
      let output = crate::integrations::osu::run_osu_dataset_export(&recording, PathBuf::from(run_artifact_dir), PathBuf::from(output_dir))?;
      println!("runId: {}", output.run_id);
      println!("status: completed");
      println!("exportedFrames: {}", output.value.dataset_manifest.exported_frames.len());
      println!("skippedFrames: {}", output.value.dataset_manifest.skipped_frames.len());
      println!("output: {}", output.value.output_dir.display());
    }
    CliCommand::OsuEvalDetections {
      run_artifact_dir,
      detections_path,
      output_dir,
    } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let recording = runtime.recording().handle();
      let output = crate::integrations::osu::run_osu_detection_eval(
        &recording,
        PathBuf::from(run_artifact_dir),
        PathBuf::from(detections_path),
        output_dir.map(PathBuf::from).unwrap_or_else(|| temp_runtime_store_root().join("osu-eval-detections-output")),
      )?;
      println!("runId: {}", output.run_id);
      println!("status: completed");
      println!("totalFrames: {}", output.value.visual_eval_report.total_frames);
      println!("labelMatchedFrames: {}", output.value.visual_eval_report.label_matched_frames);
      println!("spatialMatchedFrames: {}", output.value.visual_eval_report.spatial_matched_frames);
      println!("output: {}", output.value.output_dir.display());
    }
    CliCommand::OsuVisionDemo {
      beatmap_path,
      target_app,
      output_dir,
      dispatch_limit,
      capture_verify,
    } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let recording = runtime.recording().handle();
      let output = crate::integrations::osu::run_osu_vision_demo(
        &recording,
        PathBuf::from(beatmap_path),
        target_app,
        output_dir.map(PathBuf::from).unwrap_or_else(|| temp_runtime_store_root().join("osu-vision-demo-output")),
        dispatch_limit,
        capture_verify,
      )?;
      println!("runId: {}", output.run_id);
      println!("status: completed");
      println!("beatmap: {}", output.value.map_summary.beatmap_path);
      println!("objects: {}", output.value.map_summary.total_objects);
      println!("latencyP95Ms: {}", output.value.latency_report.p95_error_ms);
      println!("jitterMs: {}", output.value.latency_report.jitter_ms);
      println!("dispatchSamples: {}", output.value.dispatch_trace.len());
      println!("captureArtifacts: {}", output.value.capture_trace.len());
      println!(
        "evidenceNotes: {}",
        if output.value.evidence_summary.evidence_notes.is_empty() {
          "none".to_string()
        } else {
          output.value.evidence_summary.evidence_notes.join(" | ")
        }
      );
      println!("hasEvidenceArtifact: {}", output.value.output_dir.join("evidence_summary.json").exists());
      println!("hasProjectionArtifact: {}", output.value.projection.as_ref().is_some());
      println!("hasVisualTruthManifest: {}", output.value.visual_truth_manifest.as_ref().is_some());
      if let Some(summary) = &output.value.verification_summary {
        println!("verificationCapturedActions: {}", summary.captured_action_count);
        println!("verificationMissingFrames: {}", summary.missing_frame_count);
      }
      println!("output: {}", output.value.output_dir.display());
    }
    CliCommand::Invoke {
      request,
      inspect,
      output,
    } => {
      let authority = build_invoke_dispatch(&project_root, &inspect).await?;
      let registry = crate::product_registry();
      let command =
        registry.resolve(&request.command_id).cloned().ok_or_else(|| format!("unknown invoke command: {}", request.command_id))?;
      let input = auv_cli_invoke::InvokeCommandInput {
        command_id: request.command_id,
        target_application_id: request.target.application_id,
        inputs: request.inputs,
        dry_run: request.dry_run,
        cancellation: auv_cli_invoke::InvokeCancellation::new(),
      };
      let invoked_command = command.clone();
      let execution = execute_invoke_frontend(&authority, move || invoked_command.invoke(input)).await?;
      if let Some(error) = execution.tracing_failure {
        eprintln!("warning: invoke tracing flush failed for run {}: {error}", execution.run_id);
      }
      let result = auv_cli_invoke::InvokeResult::from_command_result(execution.run_id.to_string(), &command, execution.direct_result)
        .with_canonical_artifacts(execution.canonical_artifacts);
      for failure in &result.artifact_failures {
        eprintln!("warning: artifact instrumentation failed for {}: {}", failure.purpose, failure.message);
      }
      let outcome = auv_cli_invoke::render_invoke_result(&result, output)?;
      exit_code = outcome.exit_code;
    }
    CliCommand::Inspect { run_id, store_root } => {
      let store_root = resolve_store_root(&project_root, store_root.as_ref());
      let store = open_inspect_authority_store(&store_root)?;
      let run_id = run_id.parse::<auv_tracing::RunId>().map_err(|error| format!("invalid run id: {error}"))?;
      let snapshot = store
        .load_snapshot(run_id)
        .await
        .map_err(|error| format!("failed to read run {run_id}: {error}"))?
        .ok_or_else(|| format!("run not found: {run_id}"))?;
      let document = auv_inspect_model::InspectDocument::from(&snapshot);
      println!("{}", serde_json::to_string_pretty(&document).map_err(|error| format!("failed to serialize run inspection: {error}"))?);
    }
    CliCommand::InspectServe { .. } => {
      unreachable!("inspect serve is handled before runtime setup")
    }
    CliCommand::McpServe => {
      unreachable!("mcp serve is handled before runtime setup")
    }
    CliCommand::SessionServe { .. } => {
      unreachable!("session serve is handled before runtime setup")
    }
  }

  Ok(exit_code)
}

#[derive(Debug)]
struct MinecraftBridgeOutput {
  screenshot_artifact_id: String,
  projection_artifact_id: String,
  overlay_artifact_id: Option<String>,
  refusal_reason: Option<String>,
  artifact_paths: Vec<PathBuf>,
}

#[derive(Debug)]
struct MinecraftCalibrationOutput {
  screenshot_artifact_id: String,
  projection_artifact_id: String,
  calibration_artifact_id: String,
  overlay_artifact_id: Option<String>,
  refusal_reason: Option<String>,
  artifact_paths: Vec<PathBuf>,
}

const MINECRAFT_LIVE_CLICK_POST_FRAME_WAIT: auv_game_minecraft::TailFrameWaitConfig = auv_game_minecraft::TailFrameWaitConfig::new(750, 25);

type MinecraftLiveClickDispatch = for<'a> fn(
  &auv_runtime::runtime::Runtime,
  &mut auv_tracing_driver::recorded_operation::RecordedOperationContext<'a>,
  &str,
  &str,
  auv_driver::geometry::WindowPoint,
) -> Result<String, String>;

#[derive(Debug)]
struct MinecraftLiveClickOutput {
  screenshot_artifact_id: String,
  projection_artifact_id: String,
  operation_result_artifact_id: String,
  input_summary: String,
  artifact_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct MinecraftProjectionCalibrationArtifact {
  frame_id: String,
  target_block: String,
  target_semantics: String,
  raycast_hit_block_pos: Option<String>,
  raycast_hit_face: Option<String>,
  refusal_reason: Option<String>,
  overlay_ref: Option<String>,
  known_limits: Vec<String>,
}

fn build_minecraft_operation_result(run_id: &auv_tracing_driver::trace::RunId, verification: VerificationResult) -> OperationResult {
  let evidence_artifacts = verification.evidence.clone();
  OperationResult {
    api_version: OPERATION_RESULT_API_VERSION.to_string(),
    run_id: run_id.clone(),
    status: OperationStatus::Completed,
    operation_id: "auv.minecraft.live_click".to_string(),
    evidence_artifacts,
    output: OperationOutput::Acknowledged {
      message: Some("minecraft live click completed".to_string()),
    },
    verifications: vec![verification],
    freshness_basis: None,
    known_limits: Vec::new(),
  }
}

fn stage_operation_result_artifact(
  context: &mut auv_tracing_driver::recorded_operation::RecordedOperationContext<'_>,
  operation_result: &OperationResult,
) -> Result<(PathBuf, auv_runtime::contract::ArtifactRef), String> {
  let artifact_json = serde_json::to_string_pretty(operation_result)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| format!("failed to serialize minecraft operation result: {error}"))?;
  let artifact_path =
    env::temp_dir().join(format!("auv-minecraft-operation-result-{}-{}.json", context.run_id(), auv_runtime::model::now_millis()));
  fs::write(&artifact_path, artifact_json.as_bytes())
    .map_err(|error| format!("failed to write minecraft operation result artifact: {error}"))?;
  let staged = context.stage_artifact_file_with_ref(
    "operation-result",
    &artifact_path,
    "operation-result.json",
    Some("minecraft live-click operation result with world diff verification".to_string()),
  );
  let _ = fs::remove_file(&artifact_path);
  staged.map_err(|error| error.to_string())
}

fn run_minecraft_live_click(
  runtime: &auv_runtime::runtime::Runtime,
  telemetry_sample: PathBuf,
  post_telemetry_sample: Option<PathBuf>,
  screenshot: PathBuf,
  target_block: &str,
  target_app: &str,
  target_title: &str,
  capture_skew_ms: Option<i64>,
  screenshot_is_minecraft_window: bool,
) -> Result<auv_tracing_driver::recorded_operation::RecordedOperationOutput<MinecraftLiveClickOutput>, String> {
  run_minecraft_live_click_with_dispatch(
    runtime,
    telemetry_sample,
    post_telemetry_sample,
    screenshot,
    target_block,
    target_app,
    target_title,
    capture_skew_ms,
    screenshot_is_minecraft_window,
    dispatch_minecraft_live_click,
  )
}

fn dispatch_minecraft_live_click(
  runtime: &auv_runtime::runtime::Runtime,
  context: &mut auv_tracing_driver::recorded_operation::RecordedOperationContext<'_>,
  target_app: &str,
  target_title: &str,
  window_point: auv_driver::geometry::WindowPoint,
) -> Result<String, String> {
  crate::integrations::minecraft::query_live_action::invoke_click_at_window_point(
    runtime.recording(),
    context,
    target_app,
    target_title,
    window_point,
  )
}

fn run_minecraft_live_click_with_dispatch(
  runtime: &auv_runtime::runtime::Runtime,
  telemetry_sample: PathBuf,
  post_telemetry_sample: Option<PathBuf>,
  screenshot: PathBuf,
  target_block: &str,
  target_app: &str,
  target_title: &str,
  capture_skew_ms: Option<i64>,
  screenshot_is_minecraft_window: bool,
  dispatch_click: MinecraftLiveClickDispatch,
) -> Result<auv_tracing_driver::recorded_operation::RecordedOperationOutput<MinecraftLiveClickOutput>, String> {
  let target_block = parse_block_position(target_block)?;
  let pre_frame = auv_game_minecraft::read_latest_spatial_frame_from_tail(&telemetry_sample)?
    .ok_or_else(|| format!("no valid minecraft frame found in {}", telemetry_sample.display()))?;
  let post_sample_path = post_telemetry_sample.unwrap_or_else(|| telemetry_sample.clone());
  let screenshot_dimensions = read_screenshot_dimensions(&screenshot)?;

  runtime.run_recorded_operation(
    RunSpec::new(auv_tracing_driver::trace::RunType::Execute, "auv.minecraft.live_click"),
    "Minecraft live click",
    |context| {
      let (staged_screenshot_path, screenshot_ref) = context.stage_artifact_file_with_ref(
        "minecraft-screenshot",
        &screenshot,
        screenshot.file_name().and_then(|name| name.to_str()).unwrap_or("minecraft-screenshot.png"),
        Some("minecraft screenshot bound to live telemetry frame".to_string()),
      )?;
      let screenshot_artifact_id = screenshot_ref.artifact_id.as_str().to_string();
      let (staged_frame_path, _frame_ref) =
        crate::integrations::minecraft::verification::stage_minecraft_spatial_frame_artifact(context, &pre_frame)?;
      let capture_timestamp_ms = if let Some(skew) = capture_skew_ms {
        if skew >= 0 {
          pre_frame.monotonic_timestamp_ms.saturating_sub(skew as u64)
        } else {
          pre_frame.monotonic_timestamp_ms.saturating_add((-skew) as u64)
        }
      } else {
        pre_frame.monotonic_timestamp_ms
      };

      let bound =
        auv_game_minecraft::bind_capture_to_frame(pre_frame.clone(), format!("artifact://{screenshot_artifact_id}"), capture_timestamp_ms);
      let assessment = auv_game_minecraft::evidence::assess_bound_projection(
        bound.frame,
        screenshot_dimensions,
        screenshot_is_minecraft_window,
        &auv_game_minecraft::MinecraftBlockTarget::new(target_block),
        Some(250),
      )?;

      let projection_artifact = match &assessment {
        auv_game_minecraft::evidence::ProjectionAssessment::Bound { artifact, .. } => artifact.clone(),
        auv_game_minecraft::evidence::ProjectionAssessment::Refused { artifact, .. } => artifact.clone(),
      };
      let (staged_projection_path, projection_ref) = stage_minecraft_projection_artifact(context, &projection_artifact)?;
      let projection_artifact_id = projection_ref.artifact_id.as_str().to_string();

      let projected_point = match &assessment {
        auv_game_minecraft::evidence::ProjectionAssessment::Bound { artifact, .. } => {
          artifact.projected_point.clone().ok_or_else(|| "minecraft projection evidence is bound but missing projected point".to_string())?
        }
        auv_game_minecraft::evidence::ProjectionAssessment::Refused { refusal, .. } => {
          return Err(format!("minecraft live click refused before input dispatch: {:?}", refusal.reason));
        }
      };

      let window_point = auv_game_minecraft::input_target::projected_window_point(&projected_point)
        .ok_or_else(|| "projected minecraft point is not window-clickable".to_string())?;

      let invoke_result_output_summary = dispatch_click(runtime, context, target_app, target_title, window_point)?;
      let post_frame = auv_game_minecraft::read_latest_spatial_frame_newer_than(
        &post_sample_path,
        pre_frame.monotonic_timestamp_ms,
        MINECRAFT_LIVE_CLICK_POST_FRAME_WAIT,
      )?
      .ok_or_else(|| format!("no valid minecraft post frame found in {}", post_sample_path.display()))?;

      let world_diff_request =
        auv_game_minecraft::verify::WorldDiffRequest::new(auv_game_minecraft::MinecraftBlockTarget::new(target_block))
          .allow_same_block_state_change();
      let verification = crate::integrations::minecraft::verification::map_world_diff_verdict_to_verification_result(
        &auv_game_minecraft::verify::evaluate_world_diff(&pre_frame, &post_frame, &world_diff_request),
        vec![screenshot_ref.clone(), projection_ref.clone()],
      );
      let operation_result = build_minecraft_operation_result(context.run_id(), verification);
      let (staged_operation_result_path, operation_result_ref) = stage_operation_result_artifact(context, &operation_result)?;

      Ok::<MinecraftLiveClickOutput, String>(MinecraftLiveClickOutput {
        screenshot_artifact_id,
        projection_artifact_id,
        operation_result_artifact_id: operation_result_ref.artifact_id.as_str().to_string(),
        input_summary: invoke_result_output_summary,
        artifact_paths: vec![
          staged_screenshot_path,
          staged_frame_path,
          staged_projection_path,
          staged_operation_result_path,
        ],
      })
    },
  )
}

fn run_minecraft_projection_bridge(
  runtime: &auv_runtime::runtime::Runtime,
  telemetry_sample: PathBuf,
  screenshot: Option<PathBuf>,
  capture_target_app: Option<&str>,
  capture_target_title: Option<&str>,
  target_block: &str,
  capture_skew_ms: Option<i64>,
  screenshot_is_minecraft_window: bool,
) -> Result<auv_tracing_driver::recorded_operation::RecordedOperationOutput<MinecraftBridgeOutput>, String> {
  let target_block = parse_block_position(target_block)?;
  let frame = auv_game_minecraft::read_latest_spatial_frame_from_tail(&telemetry_sample)?
    .ok_or_else(|| format!("no valid minecraft frame found in {}", telemetry_sample.display()))?;

  runtime.run_recorded_operation(
    RunSpec::new(auv_tracing_driver::trace::RunType::Execute, "auv.minecraft.bridge"),
    "Minecraft projection bridge",
    |context| {
      let captured = capture_or_stage_bridge_screenshot(runtime, context, screenshot.as_deref(), capture_target_app, capture_target_title)?;
      let screenshot_dimensions = read_screenshot_dimensions(&captured.staged_path)?;
      let screenshot_path = captured.staged_path.clone();
      let screenshot_ref = captured.artifact_ref.clone();
      let screenshot_artifact_id = screenshot_ref.artifact_id.as_str().to_string();

      let capture_timestamp_ms = if let Some(skew) = capture_skew_ms {
        if skew >= 0 {
          frame.monotonic_timestamp_ms.saturating_sub(skew as u64)
        } else {
          frame.monotonic_timestamp_ms.saturating_add((-skew) as u64)
        }
      } else {
        frame.monotonic_timestamp_ms
      };

      let bound = auv_game_minecraft::bind_capture_to_frame(frame, format!("artifact://{screenshot_artifact_id}"), capture_timestamp_ms);
      let (staged_frame_path, _frame_ref) =
        crate::integrations::minecraft::verification::stage_minecraft_spatial_frame_artifact(context, &bound.frame)?;

      let assessment = auv_game_minecraft::evidence::assess_bound_projection(
        bound.frame.clone(),
        screenshot_dimensions,
        screenshot_is_minecraft_window,
        &auv_game_minecraft::mc6_projection_target_for_frame(
          target_block,
          &bound.frame,
          auv_game_minecraft::MinecraftTargetSemantics::HitFaceCenter,
        ),
        Some(250),
      )?;

      let projection_artifact = match &assessment {
        auv_game_minecraft::evidence::ProjectionAssessment::Bound { artifact, .. } => artifact.clone(),
        auv_game_minecraft::evidence::ProjectionAssessment::Refused { artifact, .. } => artifact.clone(),
      };
      let (staged_projection_path, projection_ref) = stage_minecraft_projection_artifact(context, &projection_artifact)?;
      let projection_artifact_id = projection_ref.artifact_id.as_str().to_string();
      let mut artifact_paths = vec![
        staged_frame_path,
        screenshot_path.clone(),
        staged_projection_path,
      ];
      let mut overlay_artifact_id = None;
      let mut refusal_reason = None;

      if let auv_game_minecraft::evidence::ProjectionAssessment::Bound {
        artifact,
        raycast_hit,
      } = assessment
      {
        let screenshot_image = decode_screenshot_rgb(&screenshot_path)?;
        let projected =
          artifact.projected_point.clone().ok_or_else(|| "minecraft bridge bound projection is missing projected point".to_string())?;
        let overlay = auv_game_minecraft::render_projection_overlay(screenshot_image, &projected, raycast_hit.as_ref());
        let overlay_path =
          env::temp_dir().join(format!("auv-minecraft-overlay-{}-{}.png", context.run_id(), auv_runtime::model::now_millis()));
        overlay.save(&overlay_path).map_err(|error| format!("failed to save overlay image: {error}"))?;
        let (staged_overlay_path, overlay_ref) = context.stage_artifact_file_with_ref(
          "minecraft-overlay",
          &overlay_path,
          "minecraft-overlay.png",
          Some("projected minecraft overlay on real screenshot".to_string()),
        )?;
        let _ = fs::remove_file(&overlay_path);
        overlay_artifact_id = Some(overlay_ref.artifact_id.as_str().to_string());
        artifact_paths.push(staged_overlay_path);
      } else if let auv_game_minecraft::evidence::ProjectionAssessment::Refused { refusal, .. } = assessment {
        refusal_reason = refusal.reason.map(|reason| format!("{:?}", reason));
      }

      Ok::<MinecraftBridgeOutput, String>(MinecraftBridgeOutput {
        screenshot_artifact_id,
        projection_artifact_id,
        overlay_artifact_id,
        refusal_reason,
        artifact_paths,
      })
    },
  )
}

fn read_screenshot_dimensions(path: &Path) -> Result<(u32, u32), String> {
  ImageReader::open(path)
    .map_err(|error| format!("failed to open screenshot {}: {error}", path.display()))?
    .into_dimensions()
    .map_err(|error| format!("failed to read screenshot dimensions {}: {error}", path.display()))
}

fn parse_target_semantics(raw: &str) -> Result<auv_game_minecraft::MinecraftTargetSemantics, String> {
  match raw {
    "hit_face_center" => Ok(auv_game_minecraft::MinecraftTargetSemantics::HitFaceCenter),
    "block_center" => Ok(auv_game_minecraft::MinecraftTargetSemantics::BlockCenter),
    other => Err(format!("invalid target semantics {other:?}; expected hit_face_center or block_center")),
  }
}

#[derive(Clone, Debug)]
struct BridgeCapturedScreenshot {
  staged_path: PathBuf,
  artifact_ref: auv_runtime::contract::ArtifactRef,
}

fn capture_or_stage_bridge_screenshot(
  runtime: &auv_runtime::runtime::Runtime,
  context: &mut auv_tracing_driver::recorded_operation::RecordedOperationContext<'_>,
  screenshot: Option<&Path>,
  capture_target_app: Option<&str>,
  capture_target_title: Option<&str>,
) -> Result<BridgeCapturedScreenshot, String> {
  if let Some(screenshot) = screenshot {
    let (staged_screenshot_path, screenshot_ref) = context.stage_artifact_file_with_ref(
      "minecraft-screenshot",
      screenshot,
      screenshot.file_name().and_then(|name| name.to_str()).unwrap_or("minecraft-screenshot.png"),
      Some("minecraft screenshot bound to live telemetry frame".to_string()),
    )?;
    return Ok(BridgeCapturedScreenshot {
      staged_path: staged_screenshot_path,
      artifact_ref: screenshot_ref,
    });
  }

  let target_app = capture_target_app.ok_or_else(|| "bridge capture requires target app".to_string())?;
  let mut inputs = std::collections::BTreeMap::new();
  if let Some(title) = capture_target_title {
    inputs.insert("title".to_string(), title.to_string());
  }
  let registry = auv_cli_invoke::default_registry();
  let command = registry.resolve("window.capture").ok_or_else(|| "window.capture command is not registered".to_string())?;
  let parent = context.current_span().clone();
  let invoke_result = auv_cli_invoke::invoke_resolved_recorded_in_span(
    runtime.recording(),
    context.run_mut(),
    &parent,
    command,
    InvokeRequest {
      command_id: "window.capture".to_string(),
      target: auv_runtime::model::ExecutionTarget {
        application_id: Some(target_app.to_string()),
        target_label: None,
      },
      inputs,
      dry_run: false,
    },
  )?;
  let artifact = invoke_result
    .artifacts
    .iter()
    .find(|artifact| artifact.role == "window-capture")
    .ok_or_else(|| "window.capture produced no window-capture artifact".to_string())?;
  let run_dir = runtime.recording().run_dir(&invoke_result.run_id)?;
  let capture_path = run_dir.join(&artifact.path);
  let preferred_name = Path::new(&artifact.path).file_name().and_then(|name| name.to_str()).unwrap_or("minecraft-screenshot.png");
  let (staged_screenshot_path, screenshot_ref) = context.stage_artifact_file_with_ref(
    "minecraft-screenshot",
    &capture_path,
    preferred_name,
    Some("minecraft screenshot captured through window.capture".to_string()),
  )?;
  Ok(BridgeCapturedScreenshot {
    staged_path: staged_screenshot_path,
    artifact_ref: screenshot_ref,
  })
}

fn decode_screenshot_rgb(path: &Path) -> Result<image::RgbImage, String> {
  let image = ImageReader::open(path)
    .map_err(|error| format!("failed to open screenshot {}: {error}", path.display()))?
    .decode()
    .map_err(|error| format!("failed to decode screenshot {}: {error}", path.display()))?;
  Ok(image.to_rgb8())
}

fn stage_minecraft_projection_artifact(
  context: &mut auv_tracing_driver::recorded_operation::RecordedOperationContext<'_>,
  projection_artifact: &auv_game_minecraft::MinecraftProjectionArtifact,
) -> Result<(PathBuf, auv_runtime::contract::ArtifactRef), String> {
  projection_artifact.validate()?;
  let artifact_json = serde_json::to_string_pretty(projection_artifact)
    .map_err(|error| format!("failed to serialize minecraft projection artifact: {error}"))?;
  let artifact_path =
    env::temp_dir().join(format!("auv-minecraft-projection-{}-{}.json", context.run_id(), auv_runtime::model::now_millis()));
  fs::write(&artifact_path, artifact_json.as_bytes()).map_err(|error| format!("failed to write minecraft projection artifact: {error}"))?;
  let staged = context.stage_artifact_file_with_ref(
    crate::integrations::minecraft::MINECRAFT_PROJECTION_ARTIFACT_ROLE,
    &artifact_path,
    "projection-artifact.json",
    Some("durable minecraft projection artifact".to_string()),
  );
  let _ = fs::remove_file(&artifact_path);
  staged.map_err(|error| error.to_string())
}

fn stage_minecraft_projection_calibration_artifact(
  context: &mut auv_tracing_driver::recorded_operation::RecordedOperationContext<'_>,
  calibration_artifact: &MinecraftProjectionCalibrationArtifact,
) -> Result<(PathBuf, auv_runtime::contract::ArtifactRef), String> {
  let artifact_json = serde_json::to_string_pretty(calibration_artifact)
    .map_err(|error| format!("failed to serialize minecraft calibration artifact: {error}"))?;
  let artifact_path =
    env::temp_dir().join(format!("auv-minecraft-projection-calibration-{}-{}.json", context.run_id(), auv_runtime::model::now_millis()));
  fs::write(&artifact_path, artifact_json.as_bytes()).map_err(|error| format!("failed to write minecraft calibration artifact: {error}"))?;
  let staged = context.stage_artifact_file_with_ref(
    crate::integrations::minecraft::MINECRAFT_PROJECTION_CALIBRATION_ARTIFACT_ROLE,
    &artifact_path,
    "projection-calibration.json",
    Some("durable minecraft projection calibration summary".to_string()),
  );
  let _ = fs::remove_file(&artifact_path);
  staged.map_err(|error| error.to_string())
}

fn read_minecraft_spatial_frame_file(path: &Path) -> Result<auv_game_minecraft::MinecraftSpatialFrame, String> {
  let bytes = fs::read(path).map_err(|error| format!("failed to read minecraft spatial frame {}: {error}", path.display()))?;
  serde_json::from_slice(&bytes).map_err(|error| format!("failed to parse minecraft spatial frame {}: {error}", path.display()))
}

fn run_minecraft_calibrate_projection(
  runtime: &auv_runtime::runtime::Runtime,
  frame_path: PathBuf,
  screenshot: PathBuf,
  target_block: &str,
  target_semantics: &str,
  screenshot_is_minecraft_window: bool,
) -> Result<auv_tracing_driver::recorded_operation::RecordedOperationOutput<MinecraftCalibrationOutput>, String> {
  let frame = read_minecraft_spatial_frame_file(&frame_path)?;
  let target_block = parse_block_position(target_block)?;
  let semantics = parse_target_semantics(target_semantics)?;
  let screenshot_dimensions = read_screenshot_dimensions(&screenshot)?;

  runtime.run_recorded_operation(
    RunSpec::new(auv_tracing_driver::trace::RunType::Execute, "auv.minecraft.calibrate_projection"),
    "Minecraft projection calibration",
    |context| {
      let (staged_screenshot_path, screenshot_ref) = context.stage_artifact_file_with_ref(
        "minecraft-screenshot",
        &screenshot,
        screenshot.file_name().and_then(|name| name.to_str()).unwrap_or("minecraft-screenshot.png"),
        Some("minecraft screenshot used for calibration".to_string()),
      )?;
      let screenshot_artifact_id = screenshot_ref.artifact_id.as_str().to_string();
      let bound = auv_game_minecraft::bind_capture_to_frame(
        frame.clone(),
        format!("artifact://{screenshot_artifact_id}"),
        frame.monotonic_timestamp_ms,
      );
      let (staged_frame_path, _frame_ref) =
        crate::integrations::minecraft::verification::stage_minecraft_spatial_frame_artifact(context, &bound.frame)?;
      let target = auv_game_minecraft::mc6_projection_target_for_frame(target_block, &bound.frame, semantics);
      let assessment = auv_game_minecraft::evidence::assess_bound_projection(
        bound.frame.clone(),
        screenshot_dimensions,
        screenshot_is_minecraft_window,
        &target,
        Some(250),
      )?;
      let projection_artifact = match &assessment {
        auv_game_minecraft::evidence::ProjectionAssessment::Bound { artifact, .. } => artifact.clone(),
        auv_game_minecraft::evidence::ProjectionAssessment::Refused { artifact, .. } => artifact.clone(),
      };
      let (staged_projection_path, projection_ref) = stage_minecraft_projection_artifact(context, &projection_artifact)?;
      let projection_artifact_id = projection_ref.artifact_id.as_str().to_string();
      let mut artifact_paths = vec![
        staged_frame_path,
        staged_screenshot_path,
        staged_projection_path,
      ];
      let mut overlay_artifact_id = None;
      let refusal_reason = match &assessment {
        auv_game_minecraft::evidence::ProjectionAssessment::Bound {
          artifact,
          raycast_hit,
        } => {
          let screenshot_image = decode_screenshot_rgb(&screenshot)?;
          let projected = artifact
            .projected_point
            .clone()
            .ok_or_else(|| "minecraft calibration bound projection is missing projected point".to_string())?;
          let overlay = auv_game_minecraft::render_projection_overlay(screenshot_image, &projected, raycast_hit.as_ref());
          let overlay_path =
            env::temp_dir().join(format!("auv-minecraft-overlay-{}-{}.png", context.run_id(), auv_runtime::model::now_millis()));
          overlay.save(&overlay_path).map_err(|error| format!("failed to save overlay image: {error}"))?;
          let (staged_overlay_path, overlay_ref) = context.stage_artifact_file_with_ref(
            "minecraft-overlay",
            &overlay_path,
            "minecraft-overlay.png",
            Some("projected minecraft overlay for calibration".to_string()),
          )?;
          let _ = fs::remove_file(&overlay_path);
          overlay_artifact_id = Some(overlay_ref.artifact_id.as_str().to_string());
          artifact_paths.push(staged_overlay_path);
          None
        }
        auv_game_minecraft::evidence::ProjectionAssessment::Refused { refusal, .. } => refusal.reason.map(|reason| format!("{reason:?}")),
      };
      let calibration = MinecraftProjectionCalibrationArtifact {
        frame_id: bound.frame.spatial_frame_id.clone(),
        target_block: format!("{},{},{}", target_block.x, target_block.y, target_block.z),
        target_semantics: target_semantics.to_string(),
        raycast_hit_block_pos: bound
          .frame
          .raycast_hit
          .as_ref()
          .map(|hit| format!("{},{},{}", hit.block_pos.x, hit.block_pos.y, hit.block_pos.z)),
        raycast_hit_face: bound.frame.raycast_hit.as_ref().map(|hit| format!("{:?}", hit.face)),
        refusal_reason: refusal_reason.clone(),
        overlay_ref: overlay_artifact_id.as_ref().map(|artifact_id| format!("artifact://{artifact_id}")),
        known_limits: vec![
          "geometry gate is visual-review driven; this artifact does not assert numeric pass/fail".to_string(),
          "MC-6 hit-face-center applies only when raycast_hit.block_pos matches target_block".to_string(),
        ],
      };
      let (staged_calibration_path, calibration_ref) = stage_minecraft_projection_calibration_artifact(context, &calibration)?;
      artifact_paths.push(staged_calibration_path);

      Ok::<MinecraftCalibrationOutput, String>(MinecraftCalibrationOutput {
        screenshot_artifact_id,
        projection_artifact_id,
        calibration_artifact_id: calibration_ref.artifact_id.as_str().to_string(),
        overlay_artifact_id,
        refusal_reason,
        artifact_paths,
      })
    },
  )
}

fn parse_block_face(raw: &str) -> Result<auv_game_minecraft::BlockFace, String> {
  match raw {
    "up" => Ok(auv_game_minecraft::BlockFace::Up),
    "down" => Ok(auv_game_minecraft::BlockFace::Down),
    "north" => Ok(auv_game_minecraft::BlockFace::North),
    "south" => Ok(auv_game_minecraft::BlockFace::South),
    "east" => Ok(auv_game_minecraft::BlockFace::East),
    "west" => Ok(auv_game_minecraft::BlockFace::West),
    other => Err(format!("invalid --target-face {other:?}; expected up, down, north, south, east, or west")),
  }
}

fn parse_block_position(raw: &str) -> Result<auv_game_minecraft::BlockPosition, String> {
  let parts = raw.split(',').map(str::trim).collect::<Vec<_>>();
  if parts.len() != 3 {
    return Err(format!("invalid --target-block {raw:?}; expected x,y,z"));
  }
  let x = parts[0].parse::<i32>().map_err(|error| format!("invalid target block x: {error}"))?;
  let y = parts[1].parse::<i32>().map_err(|error| format!("invalid target block y: {error}"))?;
  let z = parts[2].parse::<i32>().map_err(|error| format!("invalid target block z: {error}"))?;
  Ok(auv_game_minecraft::BlockPosition::new(x, y, z))
}

#[derive(serde::Serialize)]
struct PermissionCheckReport {
  platform: &'static str,
  process_id: u32,
  executable: Option<String>,
  accessibility: &'static str,
  screen_recording_preflight: &'static str,
  screen_capture_kit: &'static str,
  all_ok: bool,
  warnings: Vec<String>,
  recommendation: String,
}

fn run_permission_check(json: bool) -> Result<(), String> {
  let report = collect_permission_check()?;

  if json {
    println!("{}", serde_json::to_string_pretty(&report).map_err(|error| format!("failed to encode permission report: {error}"))?);
  } else {
    print_permission_check_report(&report);
  }

  Ok(())
}

#[cfg(target_os = "macos")]
fn collect_permission_check() -> Result<PermissionCheckReport, String> {
  let native = auv_driver_macos::native::permission::probe_native_permissions()?;
  let all_ok = native.accessibility == "granted" && native.screen_capture_kit == "granted";
  let mut warnings = Vec::new();

  if native.screen_recording == "missing" && native.screen_capture_kit == "granted" {
    warnings.push(
      "CGPreflightScreenCaptureAccess reports missing, but the ScreenCaptureKit probe works; this can happen when the launch host owns TCC attribution."
        .to_string(),
    );
  }

  Ok(PermissionCheckReport {
    platform: "macos",
    process_id: process::id(),
    executable: env::current_exe().ok().map(|path| path.display().to_string()),
    accessibility: native.accessibility,
    screen_recording_preflight: native.screen_recording,
    screen_capture_kit: native.screen_capture_kit,
    all_ok,
    warnings,
    recommendation: permission_recommendation(native.accessibility, native.screen_capture_kit),
  })
}

#[cfg(not(target_os = "macos"))]
fn collect_permission_check() -> Result<PermissionCheckReport, String> {
  Err("permission check is currently implemented only for macOS".to_string())
}

fn permission_recommendation(accessibility: &str, screen_capture_kit: &str) -> String {
  match (accessibility, screen_capture_kit) {
    ("granted", "granted") => "AUV has the macOS permissions needed for capture and AX-backed automation.".to_string(),
    ("missing", "missing") => {
      "Grant Accessibility and Screen Recording to the terminal or app that launches auv, then rerun this check.".to_string()
    }
    ("missing", _) => "Grant Accessibility to the terminal or app that launches auv, then rerun this check.".to_string(),
    (_, "missing") => "Grant Screen Recording to the terminal or app that launches auv, then rerun this check.".to_string(),
    _ => "Review the permission statuses above before running desktop automation.".to_string(),
  }
}

fn print_permission_check_report(report: &PermissionCheckReport) {
  println!("AUV permission check");
  println!("platform: {}", report.platform);
  println!("process: {}", report.process_id);
  if let Some(executable) = &report.executable {
    println!("executable: {executable}");
  }
  println!("accessibility: {}", permission_status_line(report.accessibility));
  println!("screen recording preflight: {}", permission_status_line(report.screen_recording_preflight));
  println!("screen capture kit probe: {}", permission_status_line(report.screen_capture_kit));
  for warning in &report.warnings {
    println!("warning: {warning}");
  }
  println!("all ok: {}", report.all_ok);
  println!("recommendation: {}", report.recommendation);
}

fn permission_status_line(status: &str) -> String {
  match status {
    "granted" => "[ok] granted".to_string(),
    "missing" => "[missing] missing".to_string(),
    other => format!("[unknown] {other}"),
  }
}

fn shell_quote_hint_path(path: &str) -> String {
  if path.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-')) {
    path.to_string()
  } else {
    format!("'{}'", path.replace('\'', "'\"'\"'"))
  }
}

fn format_query_wired_inspect_hint(run_id: impl std::fmt::Display, inspect: &InspectClientOptions) -> String {
  if let Some(store_root) = inspect.store_root.as_deref() {
    let store_root = shell_quote_hint_path(store_root);
    format!("inspectHint: run `auv inspect {run_id} --store-root {store_root}` to view verification_outcome")
  } else {
    format!("inspectHint: run `auv inspect {run_id}` to view verification_outcome")
  }
}

fn resolve_store_root(project_root: &Path, explicit: Option<&String>) -> PathBuf {
  explicit.map(PathBuf::from).unwrap_or_else(|| auv_runtime::default_project_store_root(project_root.to_path_buf()))
}

fn open_inspect_authority_store(store_root: &Path) -> Result<Arc<dyn auv_tracing::RunStore>, String> {
  auv_tracing::FileRunStore::open(store_root)
    .map(|store| Arc::new(store) as Arc<dyn auv_tracing::RunStore>)
    .map_err(|error| format!("failed to open Inspect run authority {}: {error}", store_root.display()))
}

fn normalize_write_token(source: &str, token: String) -> Result<String, String> {
  if token.trim().is_empty() {
    Err(format!("{source} resolved to an empty write token"))
  } else {
    Ok(token)
  }
}

#[derive(Clone)]
struct InvokeFrontendAuthority {
  dispatch: auv_tracing::Dispatch,
}

async fn build_invoke_dispatch(project_root: &Path, inspect: &InspectClientOptions) -> Result<InvokeFrontendAuthority, String> {
  let server_target = if should_try_server_write(inspect) {
    if let Some((url, token)) = resolve_inspect_server_target(inspect)? {
      Some((url, token))
    } else if inspect.require_server_write {
      return Err("inspect server write is required but no inspect server is configured".to_string());
    } else if matches!(inspect.server_write, crate::cli::InspectWriteSetting::Enabled) {
      eprintln!("warning: inspect server write requested but no inspect server is configured");
      None
    } else {
      None
    }
  } else {
    None
  };

  let store: Option<Arc<dyn auv_tracing::RunStore>> = match server_target {
    // NOTICE(run-recording-v1): V1 Inspect authorities are loopback-only and
    // have no token contract. Parser compatibility fields retire in Task 22.
    Some((url, _legacy_token)) => {
      let parsed = reqwest::Url::parse(&url).map_err(|error| format!("invalid inspect authority URL {url}: {error}"))?;
      match auv_tracing_inspect::InspectRunStore::connect(parsed).await {
        Ok(store) => Some(Arc::new(store)),
        Err(error) if inspect.require_server_write => return Err(format!("failed to connect required inspect authority {url}: {error}")),
        Err(error) => {
          eprintln!("warning: failed to connect inspect authority {url}: {error}; using local tracing authority");
          None
        }
      }
    }
    None => None,
  };

  let store = match store {
    Some(store) => store,
    None if should_write_local(inspect) => open_inspect_authority_store(&resolve_store_root(project_root, inspect.store_root.as_ref()))?,
    None => return Err("invoke requires one configured V1 run authority".to_string()),
  };
  let dispatch =
    auv_tracing::configure().run_store(store.clone()).build().map_err(|error| format!("failed to configure invoke tracing: {error}"))?;
  Ok(InvokeFrontendAuthority { dispatch })
}

#[derive(Debug)]
struct InvokeFrontendExecution<T> {
  run_id: auv_tracing::RunId,
  direct_result: Result<T, String>,
  tracing_failure: Option<String>,
  canonical_artifacts: Vec<auv_tracing::ArtifactMetadata>,
}

#[derive(serde::Serialize)]
struct InvokeFrontendLifecycle {
  frontend: &'static str,
}

impl auv_tracing::EventPayload for InvokeFrontendLifecycle {
  const NAME: &'static str = "auv.frontend.lifecycle";
  const VERSION: u32 = 1;
}

async fn execute_invoke_frontend<T, F, Fut>(authority: &InvokeFrontendAuthority, call: F) -> Result<InvokeFrontendExecution<T>, String>
where
  T: Send + 'static,
  F: FnOnce() -> Fut + Send + 'static,
  Fut: Future<Output = Result<T, String>> + Send + 'static,
{
  let recorded = authority
    .dispatch
    .record(|| {
      auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
      call()
    })
    .await
    .map_err(|error| error.to_string())?;
  let (run_id, direct_result, recording) = recorded.into_parts();
  let (tracing_failure, canonical_artifacts) = match recording {
    auv_tracing::RecordingState::Committed(recording) => (
      recording.tracing_failure().map(ToString::to_string),
      recording.snapshot().artifacts().values().map(|artifact| artifact.metadata().clone()).collect(),
    ),
    auv_tracing::RecordingState::Failed(failure) => (Some(failure.to_string()), Vec::new()),
  };
  Ok(InvokeFrontendExecution {
    run_id,
    direct_result,
    tracing_failure,
    canonical_artifacts,
  })
}

fn build_runtime_for_inspect(project_root: &Path, inspect: &InspectClientOptions) -> Result<auv_runtime::runtime::Runtime, String> {
  let server_target = if should_try_server_write(inspect) {
    resolve_inspect_server_target(inspect)?
  } else {
    None
  };
  if inspect.require_server_write {
    return Err(match server_target {
      Some(_) => "inspect server write is required but only supported by the invoke composition root".to_string(),
      None => "inspect server write is required but no inspect server is configured".to_string(),
    });
  }
  if !should_write_local(inspect) {
    return Err("local recording can only be disabled for the invoke composition root".to_string());
  }
  build_runtime_with_store_root(project_root.to_path_buf(), resolve_store_root(project_root, inspect.store_root.as_ref()))
}

fn should_write_local(inspect: &InspectClientOptions) -> bool {
  !matches!(inspect.local_write, crate::cli::InspectWriteSetting::Disabled)
}

fn should_try_server_write(inspect: &InspectClientOptions) -> bool {
  inspect.require_server_write || !matches!(inspect.server_write, crate::cli::InspectWriteSetting::Disabled)
}

fn resolve_inspect_server_target(inspect: &InspectClientOptions) -> Result<Option<(String, Option<String>)>, String> {
  let explicit_token = resolve_client_token(inspect)?;
  if let Some(url) = &inspect.server_url {
    return Ok(Some((url.clone(), explicit_token)));
  }
  let Some(session) = read_discovered_inspect_session(inspect)? else {
    return Ok(None);
  };
  if !is_local_inspect_url(&session.url) {
    if inspect.require_server_write {
      return Err(format!("inspect server write is required but discovered inspect server URL is not local: {}", session.url));
    }
    eprintln!("warning: ignoring discovered inspect server with non-local URL: {}", session.url);
    return Ok(None);
  }
  Ok(Some((session.url, None)))
}

fn read_discovered_inspect_session(inspect: &InspectClientOptions) -> Result<Option<auv_inspect_server::InspectServerSession>, String> {
  match auv_inspect_server::read_inspect_session() {
    Ok(session) => Ok(session),
    Err(error) if inspect.require_server_write => Err(error),
    Err(error) => {
      eprintln!("warning: ignoring inspect server session descriptor: {error}");
      Ok(None)
    }
  }
}

fn is_local_inspect_url(raw: &str) -> bool {
  let Ok(url) = reqwest::Url::parse(raw) else {
    return false;
  };
  match url.host_str() {
    Some(host) if host.eq_ignore_ascii_case("localhost") => true,
    Some(host) => host.parse::<std::net::IpAddr>().is_ok_and(|address| address.is_loopback()),
    None => false,
  }
}

fn resolve_client_token(inspect: &InspectClientOptions) -> Result<Option<String>, String> {
  if let Some(token) = &inspect.server_token {
    return normalize_write_token("--inspect-server-token", token.clone()).map(Some);
  }
  if let Some(path) = &inspect.server_token_file {
    let token =
      fs::read_to_string(path).map_err(|error| format!("failed to read inspect server token file {path}: {error}"))?.trim().to_string();
    return normalize_write_token("--inspect-server-token-file", token).map(Some);
  }
  Ok(None)
}

fn temp_runtime_store_root() -> PathBuf {
  env::temp_dir().join(format!("auv-runtime-store-{}-{}", process::id(), auv_runtime::model::now_millis()))
}

#[cfg(test)]
mod tests {
  use std::future::Future;
  use std::sync::Arc;
  use std::sync::Mutex;
  use std::sync::atomic::{AtomicUsize, Ordering};

  use auv_tracing::{
    ArtifactBody, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, CommitError, CommitResult, ErrorCode,
    EventPayload, IdempotencyKey, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision,
    RunSnapshot, RunStore, RunSubscription, StoreArtifactRequest,
  };
  use axum::body::{Body, to_bytes};
  use axum::http::{Request, StatusCode};
  use futures_util::StreamExt;
  use image::{Rgb, RgbImage};
  use rmcp::{
    ClientHandler, ServiceExt as RmcpServiceExt,
    model::{CallToolRequestParam, ClientInfo},
  };
  use tower::ServiceExt;

  use super::*;

  static ENV_LOCK: Mutex<()> = Mutex::new(());

  #[derive(Debug, Clone, Default)]
  struct DummyClientHandler;

  impl ClientHandler for DummyClientHandler {
    fn get_info(&self) -> ClientInfo {
      ClientInfo::default()
    }
  }

  #[test]
  fn library_exit_status_returns_typed_codes_without_terminating_the_process() {
    assert_eq!(exit_status(Ok(0)), std::process::ExitCode::SUCCESS);
    assert_eq!(exit_status(Ok(7)), std::process::ExitCode::from(7));
    assert_eq!(exit_status(Err("failed".to_string())), std::process::ExitCode::FAILURE);
  }

  #[derive(Clone, Default)]
  struct CountingCall {
    calls: Arc<AtomicUsize>,
  }

  impl CountingCall {
    fn call_count(&self) -> usize {
      self.calls.load(Ordering::SeqCst)
    }

    fn call(&self) -> impl Future<Output = Result<u32, String>> + Send + 'static + use<> {
      auv_tracing::emit_event!(FrontendCallEvent {
        phase: "constructed"
      });
      let calls = self.calls.clone();
      async move {
        calls.fetch_add(1, Ordering::SeqCst);
        auv_tracing::emit_event!(FrontendCallEvent { phase: "polled" });
        Ok(7)
      }
    }
  }

  #[derive(serde::Serialize)]
  struct FrontendCallEvent {
    phase: &'static str,
  }

  impl EventPayload for FrontendCallEvent {
    const NAME: &'static str = "auv.test.cli_frontend_call";
    const VERSION: u32 = 1;
  }

  #[tokio::test]
  async fn cli_composition_scopes_construction_and_polling_without_changing_library_value() {
    let call = CountingCall::default();
    assert_eq!(call.call().await, Ok(7));

    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = auv_tracing::configure().run_store(store.clone()).build().expect("dispatch");
    let authority = InvokeFrontendAuthority { dispatch };
    let invoked_call = call.clone();
    let execution = execute_invoke_frontend(&authority, move || invoked_call.call()).await.expect("persisted execution");

    assert_eq!(execution.direct_result, Ok(7));
    assert_eq!(execution.tracing_failure, None);
    assert_eq!(call.call_count(), 2);
    let snapshot = store.load_snapshot(execution.run_id).await.expect("snapshot").expect("recorded run");
    assert_eq!(snapshot.run_id(), execution.run_id);
    assert_eq!(snapshot.events().len(), 3);
  }

  #[tokio::test]
  async fn cli_commit_unknown_preserves_direct_result_without_retry_or_canonical_advice() {
    let call = CountingCall::default();
    let store = Arc::new(CommitUnknownStore::new());
    let dispatch = auv_tracing::configure().run_store(store.clone()).build().expect("dispatch");
    let authority = InvokeFrontendAuthority { dispatch };

    let invoked_call = call.clone();
    let execution =
      execute_invoke_frontend(&authority, move || invoked_call.call()).await.expect("recording failure must preserve the direct result");

    assert_eq!(call.call_count(), 1);
    assert_eq!(execution.direct_result, Ok(7));
    assert_eq!(store.attempted_run_id(), Some(execution.run_id));
    assert!(execution.canonical_artifacts.is_empty());
    let failure = execution.tracing_failure.expect("recording failure");
    assert!(failure.contains("snapshot is missing"));
    assert_no_canonical_advice(&failure);
  }

  // ROOT CAUSE:
  //
  // MCP used the CLI InvokeCommand handler and treated every Ok value as
  // completed, so TextEdit semantic mismatch was silently discarded. The
  // direct frontends now map the same typed report independently and persist
  // evidence through their own run roots.
  // https://github.com/moeru-ai/auv/pull/102#issuecomment-4958351155
  #[tokio::test]
  async fn textedit_recorded_mismatch_keeps_cli_mcp_run_and_operation_in_sync() {
    let store_root = env::temp_dir().join(format!("auv-textedit-direct-parity-{}", auv_runtime::model::now_millis()));
    let _ = fs::remove_dir_all(&store_root);
    let store = Arc::new(auv_tracing::FileRunStore::open(&store_root).expect("file store"));
    let dispatch = auv_tracing::configure().run_store(store.clone()).build().expect("CLI dispatch");
    let authority = InvokeFrontendAuthority { dispatch };
    let command = crate::integrations::textedit::test_document_write_invoke_command();
    let input = auv_cli_invoke::InvokeCommandInput {
      command_id: crate::integrations::textedit::DOCUMENT_WRITE_COMMAND_ID.to_string(),
      target_application_id: Some("com.apple.TextEdit".to_string()),
      inputs: textedit_mismatch_inputs(),
      dry_run: false,
      cancellation: auv_cli_invoke::InvokeCancellation::new(),
    };

    let invoked_command = command.clone();
    let cli_execution = execute_invoke_frontend(&authority, move || invoked_command.invoke(input)).await.expect("persisted CLI execution");
    assert_eq!(cli_execution.tracing_failure, None);
    let cli_result =
      auv_cli_invoke::InvokeResult::from_command_result(cli_execution.run_id.to_string(), &command, cli_execution.direct_result);
    assert_eq!(cli_result.status, auv_cli_invoke::RunStatus::Failed);
    assert!(cli_result.failure_message.as_deref().is_some_and(|message| message.contains("semantic verification failed")));

    let server = crate::mcp::server_with_test_textedit(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).expect("test TextEdit MCP server");
    let (server_transport, client_transport) = tokio::io::duplex(16384);
    let server_handle = tokio::spawn(async move {
      let service = server.serve(server_transport).await.expect("MCP server start");
      service.waiting().await.expect("MCP server exit");
    });
    let client = DummyClientHandler.serve(client_transport).await.expect("MCP client");
    let response = client
      .call_tool(CallToolRequestParam {
        name: "invoke".into(),
        arguments: Some(
          serde_json::json!({
            "command_id": crate::integrations::textedit::DOCUMENT_WRITE_COMMAND_ID,
            "target": { "application_id": "com.apple.TextEdit" },
            "inputs": textedit_mismatch_inputs(),
            "inspect": { "store_root": store_root.display().to_string() }
          })
          .as_object()
          .expect("MCP arguments")
          .clone(),
        ),
      })
      .await
      .expect("MCP invoke");
    let mcp_value: serde_json::Value =
      serde_json::from_str(&response.content.first().and_then(|content| content.raw.as_text()).expect("MCP text response").text)
        .expect("MCP JSON response");
    assert_eq!(mcp_value["status"], "failed");
    assert!(mcp_value["failure_message"].as_str().is_some_and(|message| message.contains("semantic verification failed")));
    let mcp_run_id = mcp_value["run_id"].as_str().expect("MCP run id").parse::<auv_tracing::RunId>().expect("valid run id");
    assert_ne!(cli_execution.run_id, mcp_run_id);

    let cli_snapshot = store.load_snapshot(cli_execution.run_id).await.expect("CLI snapshot read").expect("CLI snapshot");
    let mcp_snapshot = store.load_snapshot(mcp_run_id).await.expect("MCP snapshot read").expect("MCP snapshot");
    let cli_purposes = artifact_purposes(&cli_snapshot);
    let mcp_purposes = artifact_purposes(&mcp_snapshot);
    assert_eq!(cli_purposes, mcp_purposes);
    assert_eq!(
      cli_purposes,
      [
        "auv.driver.input_action_result",
        "auv.driver.input_action_result",
        "auv.textedit.ax_text_observation",
        "auv.textedit.document_write_result",
      ]
    );
    for (snapshot, run_id) in [
      (&cli_snapshot, cli_execution.run_id),
      (&mcp_snapshot, mcp_run_id),
    ] {
      assert!(snapshot.artifacts().values().all(|artifact| artifact.metadata().uri().run_id() == run_id));
      assert_eq!(snapshot.events().iter().map(|event| event.schema().name().as_str()).collect::<Vec<_>>(), vec!["auv.frontend.lifecycle"]);
      assert!(!stored_document_write_semantic_match(store.as_ref(), snapshot).await);
    }

    client.cancel().await.expect("cancel MCP client");
    server_handle.await.expect("join MCP server");
    let _ = fs::remove_dir_all(store_root);
  }

  fn textedit_mismatch_inputs() -> std::collections::BTreeMap<String, String> {
    std::collections::BTreeMap::from([
      ("content".to_string(), "AUV_TEXTEDIT_EXPECTED_MARKER".to_string()),
      ("fixture_observed_text".to_string(), "observed-without-expected".to_string()),
    ])
  }

  fn artifact_purposes(snapshot: &auv_tracing::RunSnapshot) -> Vec<&str> {
    let mut purposes = snapshot.artifacts().values().map(|artifact| artifact.metadata().purpose().as_str()).collect::<Vec<_>>();
    purposes.sort();
    purposes
  }

  async fn stored_document_write_semantic_match(store: &dyn RunStore, snapshot: &auv_tracing::RunSnapshot) -> bool {
    let artifact = snapshot
      .artifacts()
      .values()
      .find(|artifact| artifact.metadata().purpose().as_str() == "auv.textedit.document_write_result")
      .expect("document-write result artifact");
    let mut reader = store.open_artifact(artifact.metadata().uri().clone()).await.expect("open document-write artifact");
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.next().await {
      bytes.extend_from_slice(&chunk.expect("read document-write artifact"));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("document-write artifact JSON");
    value["verification"]["semantic_matched"].as_bool().expect("semantic match field")
  }

  fn assert_no_canonical_advice(facts: &str) {
    for forbidden in [
      "operation-success",
      "verification",
      "retry",
      "recommended action",
    ] {
      assert!(!facts.contains(forbidden), "canonical facts contain {forbidden}: {facts}");
    }
  }

  struct CommitUnknownStore {
    inner: MemoryRunStore,
    attempted_run_id: Mutex<Option<RunId>>,
  }

  impl CommitUnknownStore {
    fn new() -> Self {
      Self {
        inner: MemoryRunStore::new(AuthorityId::new()),
        attempted_run_id: Mutex::new(None),
      }
    }

    fn attempted_run_id(&self) -> Option<RunId> {
      *self.attempted_run_id.lock().unwrap()
    }
  }

  impl RunStore for CommitUnknownStore {
    fn authority_id(&self) -> AuthorityId {
      self.inner.authority_id()
    }

    fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
      *self.attempted_run_id.lock().unwrap() = Some(request.run_id());
      Box::pin(async { Err(CommitError::CommitUnknown(ErrorCode::parse("auv.test.commit_unknown").unwrap())) })
    }

    fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
      self.inner.write_artifact(request, body)
    }

    fn lookup_commit(&self, _run_id: RunId, _key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
      Box::pin(async { Ok(None) })
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
      self.inner.open_artifact(uri)
    }
  }

  #[test]
  fn format_query_wired_inspect_hint_omits_store_root_when_default_store() {
    let inspect = InspectClientOptions {
      store_root: None,
      ..InspectClientOptions::default()
    };
    let hint = format_query_wired_inspect_hint("run_test_1", &inspect);
    assert_eq!(hint, "inspectHint: run `auv inspect run_test_1` to view verification_outcome");
  }

  #[test]
  fn format_query_wired_inspect_hint_echoes_custom_store_root() {
    let inspect = InspectClientOptions {
      store_root: Some("/tmp/mc20-store".to_string()),
      ..InspectClientOptions::default()
    };
    let hint = format_query_wired_inspect_hint("run_test_1", &inspect);
    assert_eq!(hint, "inspectHint: run `auv inspect run_test_1 --store-root /tmp/mc20-store` to view verification_outcome");
  }

  #[test]
  fn format_query_wired_inspect_hint_quotes_store_root_with_whitespace() {
    let inspect = InspectClientOptions {
      store_root: Some("/tmp/mc20 store".to_string()),
      ..InspectClientOptions::default()
    };
    let hint = format_query_wired_inspect_hint("run_test_1", &inspect);
    assert_eq!(hint, "inspectHint: run `auv inspect run_test_1 --store-root '/tmp/mc20 store'` to view verification_outcome");
  }

  #[test]
  fn format_query_wired_inspect_hint_quotes_store_root_with_single_quote() {
    let inspect = InspectClientOptions {
      store_root: Some("/tmp/mc20'store".to_string()),
      ..InspectClientOptions::default()
    };
    let hint = format_query_wired_inspect_hint("run_test_1", &inspect);
    assert_eq!(hint, "inspectHint: run `auv inspect run_test_1 --store-root '/tmp/mc20'\"'\"'store'` to view verification_outcome");
  }

  #[test]
  fn format_query_wired_inspect_hint_quotes_store_root_with_shell_metacharacters() {
    let inspect = InspectClientOptions {
      store_root: Some("/tmp/(mc20)[store]".to_string()),
      ..InspectClientOptions::default()
    };
    let hint = format_query_wired_inspect_hint("run_test_1", &inspect);
    assert_eq!(hint, "inspectHint: run `auv inspect run_test_1 --store-root '/tmp/(mc20)[store]'` to view verification_outcome");
  }

  #[test]
  fn inspect_server_target_prefers_explicit_url_and_token_file() {
    let path = env::temp_dir().join(format!("auv-client-write-token-{}.txt", auv_runtime::model::now_millis()));
    fs::write(&path, "secret\n").expect("token file should write");
    let inspect = InspectClientOptions {
      server_url: Some("http://127.0.0.1:9876/".to_string()),
      server_token_file: Some(path.display().to_string()),
      ..InspectClientOptions::default()
    };

    let target = resolve_inspect_server_target(&inspect).expect("explicit target should resolve");

    let _ = fs::remove_file(path);
    assert_eq!(target, Some(("http://127.0.0.1:9876/".to_string(), Some("secret".to_string()))));
  }

  #[test]
  fn required_inspect_server_write_rejects_missing_target() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = env::temp_dir().join(format!("auv-missing-inspect-session-{}", auv_runtime::model::now_millis()));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    unsafe {
      env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let inspect = InspectClientOptions {
      server_write: crate::cli::InspectWriteSetting::Enabled,
      require_server_write: true,
      ..InspectClientOptions::default()
    };

    let error = match build_runtime_for_inspect(&root, &inspect) {
      Ok(_) => panic!("required server write without target should fail"),
      Err(error) => error,
    };

    unsafe {
      env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert!(error.contains("inspect server write is required"));
  }

  #[test]
  fn required_missing_server_with_local_write_disabled_does_not_leave_temp_store() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = env::temp_dir().join(format!("auv-missing-required-server-no-local-{}", auv_runtime::model::now_millis()));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    unsafe {
      env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let prefix = format!("auv-runtime-store-{}-", process::id());
    let before = temp_runtime_store_entries(&prefix);
    let inspect = InspectClientOptions {
      local_write: crate::cli::InspectWriteSetting::Disabled,
      server_write: crate::cli::InspectWriteSetting::Enabled,
      require_server_write: true,
      ..InspectClientOptions::default()
    };

    let error = match build_runtime_for_inspect(&root, &inspect) {
      Ok(_) => panic!("required server write without target should fail"),
      Err(error) => error,
    };
    let after = temp_runtime_store_entries(&prefix);

    unsafe {
      env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert!(error.contains("inspect server write is required"));
    assert_eq!(after, before);
  }

  #[test]
  fn legacy_runtime_rejects_disabled_local_recording_without_allocating_temp_store() {
    let root = env::temp_dir().join(format!("auv-recording-no-local-{}", auv_runtime::model::now_millis()));
    let prefix = format!("auv-runtime-store-{}-", process::id());
    let before = temp_runtime_store_entries(&prefix);
    let inspect = InspectClientOptions {
      local_write: crate::cli::InspectWriteSetting::Disabled,
      ..InspectClientOptions::default()
    };

    let error = match build_runtime_for_inspect(&root, &inspect) {
      Ok(_) => panic!("legacy runtime should reject disabled local recording"),
      Err(error) => error,
    };

    assert!(error.contains("only be disabled for the invoke composition root"));
    assert_eq!(temp_runtime_store_entries(&prefix), before);
  }

  #[test]
  fn optional_inspect_server_write_ignores_malformed_discovered_session() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = env::temp_dir().join(format!("auv-malformed-inspect-session-{}", auv_runtime::model::now_millis()));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    fs::write(&session_path, "not json").expect("malformed session should write");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(&session_path, fs::Permissions::from_mode(0o600)).expect("session file permissions should change");
    }
    unsafe {
      env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let inspect = InspectClientOptions {
      server_write: crate::cli::InspectWriteSetting::Default,
      require_server_write: false,
      ..InspectClientOptions::default()
    };

    let runtime = build_runtime_for_inspect(&root, &inspect);

    unsafe {
      env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert!(runtime.is_ok());
  }

  #[test]
  fn required_inspect_server_write_rejects_malformed_discovered_session() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = env::temp_dir().join(format!("auv-required-malformed-inspect-session-{}", auv_runtime::model::now_millis()));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    fs::write(&session_path, "not json").expect("malformed session should write");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(&session_path, fs::Permissions::from_mode(0o600)).expect("session file permissions should change");
    }
    unsafe {
      env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let inspect = InspectClientOptions {
      server_write: crate::cli::InspectWriteSetting::Default,
      require_server_write: true,
      ..InspectClientOptions::default()
    };

    let error = match build_runtime_for_inspect(&root, &inspect) {
      Ok(_) => panic!("required server write should reject malformed session"),
      Err(error) => error,
    };

    unsafe {
      env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert!(error.contains("failed to parse inspect session"));
  }

  #[test]
  fn default_discovery_ignores_non_local_session_url() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = env::temp_dir().join(format!("auv-remote-inspect-session-{}", auv_runtime::model::now_millis()));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    fs::write(
      &session_path,
      serde_json::to_string(&auv_inspect_server::InspectServerSession {
        url: "http://203.0.113.7:8765".to_string(),
        authority_id: "019f8b1e-4b2d-7a00-8f00-0000000000aa".parse().expect("authority id"),
        pid: 123,
        started_at_millis: 456,
      })
      .expect("session should encode"),
    )
    .expect("session should write");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(&session_path, fs::Permissions::from_mode(0o600)).expect("session file permissions should change");
    }
    unsafe {
      env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let inspect = InspectClientOptions {
      server_write: crate::cli::InspectWriteSetting::Default,
      require_server_write: false,
      ..InspectClientOptions::default()
    };

    let target = resolve_inspect_server_target(&inspect).expect("optional discovery should ignore remote URL");

    unsafe {
      env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert_eq!(target, None);
  }

  #[tokio::test]
  async fn inspect_serve_adapter_uses_file_authority_and_v1_router() {
    let root = env::temp_dir().join(format!("auv-file-authority-adapter-{}", auv_runtime::model::now_millis()));
    let _ = fs::remove_dir_all(&root);
    let store = open_inspect_authority_store(&root).expect("file authority should open");
    let authority_id = store.authority_id();
    let app = auv_inspect_server::router(store);

    let response = app
      .clone()
      .oneshot(Request::builder().uri("/v1/authority").body(Body::empty()).expect("request should build"))
      .await
      .expect("authority route should respond");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.expect("body should read");
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&body).unwrap()["authority_id"], authority_id.to_string());

    let legacy = app
      .oneshot(Request::builder().uri("/runs").body(Body::empty()).expect("request should build"))
      .await
      .expect("legacy route should respond");
    assert_eq!(legacy.status(), StatusCode::NOT_FOUND);
    assert_eq!(open_inspect_authority_store(&root).unwrap().authority_id(), authority_id);
    let _ = fs::remove_dir_all(root);
  }

  fn mc2_temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("auv-{label}-{}", auv_runtime::model::now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }

  fn write_mc2_test_telemetry(path: &Path) {
    let frame = auv_game_minecraft::MinecraftSpatialFrame {
      spatial_frame_id: "frame-mc2".to_string(),
      world_tick: 42,
      monotonic_timestamp_ms: 5_000,
      telemetry_session_id: None,
      viewport: auv_game_minecraft::types::Viewport::new(64, 64),
      view_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      projection_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      player_pose: auv_game_minecraft::types::PlayerPose {
        eye_position: auv_game_minecraft::types::Vec3::new(0.0, 0.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: Some(auv_game_minecraft::types::RaycastHit {
        block_pos: auv_game_minecraft::BlockPosition::new(0, 0, 0),
        face: auv_game_minecraft::types::BlockFace::North,
        block_id: "minecraft:stone".to_string(),
      }),
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: None,
      resource_pack_ids: Vec::new(),
    };
    let body = serde_json::to_string(&frame).expect("frame should serialize");
    fs::write(path, format!("{body}\n")).expect("telemetry sample should write");
  }

  fn append_mc2_test_telemetry(path: &Path, frame: &auv_game_minecraft::MinecraftSpatialFrame) {
    use std::io::Write as _;

    let mut file = fs::OpenOptions::new().append(true).open(path).expect("telemetry sample should open for append");
    writeln!(file, "{}", serde_json::to_string(frame).expect("frame should serialize")).expect("telemetry sample should append");
  }

  fn write_mc2_test_screenshot(path: &Path) {
    RgbImage::from_pixel(64, 64, Rgb([0, 0, 0])).save(path).expect("screenshot should write");
  }

  #[test]
  fn minecraft_world_diff_verification_maps_success_verdict() {
    let verdict = auv_game_minecraft::verify::WorldDiffVerdict {
      executed: true,
      state_changed: true,
      semantic_matched: Some(true),
      failure: None,
      observed_block_id: Some("minecraft:air".to_string()),
      observed_item_delta: Some(1),
    };

    let verification = crate::integrations::minecraft::verification::map_world_diff_verdict_to_verification_result(&verdict, Vec::new());

    assert_eq!(verification.method, auv_runtime::contract::VerificationMethod::SemanticMatch);
    assert_eq!(verification.executed, true);
    assert_eq!(verification.state_changed, true);
    assert_eq!(verification.semantic_matched, Some(true));
    assert_eq!(verification.failure_layer, None);
    assert_eq!(verification.observed_label.as_deref(), Some("minecraft:air"));
  }

  #[test]
  fn minecraft_world_diff_verification_maps_failure_layers() {
    let cases = [
      (
        auv_game_minecraft::verify::WorldDiffFailure::VerificationUnreliable,
        Some(auv_runtime::contract::FailureLayer::VerificationUnreliable),
        None,
      ),
      (
        auv_game_minecraft::verify::WorldDiffFailure::StateChangedNoMatch,
        Some(auv_runtime::contract::FailureLayer::StateChangedNoMatch),
        Some(false),
      ),
      (
        auv_game_minecraft::verify::WorldDiffFailure::SemanticMismatch,
        Some(auv_runtime::contract::FailureLayer::SemanticMismatch),
        Some(false),
      ),
    ];

    for (failure, expected_layer, semantic_matched) in cases {
      let verdict = auv_game_minecraft::verify::WorldDiffVerdict {
        executed: true,
        state_changed: matches!(failure, auv_game_minecraft::verify::WorldDiffFailure::StateChangedNoMatch),
        semantic_matched,
        failure: Some(failure),
        observed_block_id: Some("minecraft:stone".to_string()),
        observed_item_delta: Some(0),
      };

      let verification = crate::integrations::minecraft::verification::map_world_diff_verdict_to_verification_result(&verdict, Vec::new());
      assert_eq!(verification.failure_layer, expected_layer);
      assert_eq!(verification.observed_label.as_deref(), Some("minecraft:stone"));
    }
  }

  #[test]
  fn minecraft_live_click_waits_for_fresher_post_frame() {
    let project_root = mc2_temp_dir("mc20-d3-1-live-click-project");
    let store_root = mc2_temp_dir("mc20-d3-1-live-click-store");
    let telemetry_path = project_root.join("pre.jsonl");
    let post_telemetry_path = project_root.join("post.jsonl");
    let screenshot_path = project_root.join("frame.png");
    write_mc2_test_telemetry(&telemetry_path);
    write_mc2_test_telemetry(&post_telemetry_path);
    write_mc2_test_screenshot(&screenshot_path);

    let appended_post = auv_game_minecraft::MinecraftSpatialFrame {
      spatial_frame_id: "frame-mc2-post".to_string(),
      world_tick: 43,
      monotonic_timestamp_ms: 5_050,
      telemetry_session_id: None,
      viewport: auv_game_minecraft::types::Viewport::new(64, 64),
      view_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      projection_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      player_pose: auv_game_minecraft::types::PlayerPose {
        eye_position: auv_game_minecraft::types::Vec3::new(0.0, 0.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: Some(auv_game_minecraft::types::RaycastHit {
        block_pos: auv_game_minecraft::BlockPosition::new(0, 0, 0),
        face: auv_game_minecraft::types::BlockFace::North,
        block_id: "minecraft:stone".to_string(),
      }),
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: None,
      resource_pack_ids: Vec::new(),
    };

    let writer_path = post_telemetry_path.clone();
    let writer_frame = appended_post.clone();
    let writer = std::thread::spawn(move || {
      std::thread::sleep(std::time::Duration::from_millis(25));
      append_mc2_test_telemetry(&writer_path, &writer_frame);
    });

    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone()).expect("runtime should build");
    let output = run_minecraft_live_click_with_dispatch(
      &runtime,
      telemetry_path,
      Some(post_telemetry_path),
      screenshot_path,
      "0,0,0",
      "FixtureApp",
      "Fixture Window",
      Some(0),
      true,
      fixture_minecraft_live_click_dispatch,
    )
    .expect("live click should record");

    writer.join().expect("writer thread should join");
    let verifications =
      auv_runtime::inspect::list_verifications(runtime.recording().store(), output.run_id.as_str()).expect("verifications should list");
    assert_eq!(verifications.len(), 1);
    assert_eq!(verifications[0].failure_layer, None);

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn minecraft_live_click_command_persists_operation_result_artifact() {
    let project_root = mc2_temp_dir("mc3-live-click-project");
    let store_root = mc2_temp_dir("mc3-live-click-store");
    let telemetry_path = project_root.join("pre.jsonl");
    let post_telemetry_path = project_root.join("post.jsonl");
    let screenshot_path = project_root.join("frame.png");
    write_mc2_test_telemetry(&telemetry_path);
    write_mc2_test_telemetry(&post_telemetry_path);
    write_mc2_test_screenshot(&screenshot_path);

    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone()).expect("runtime should build");
    let output = run_minecraft_live_click_with_dispatch(
      &runtime,
      telemetry_path,
      Some(post_telemetry_path),
      screenshot_path,
      "0,0,0",
      "FixtureApp",
      "Fixture Window",
      Some(0),
      true,
      fixture_minecraft_live_click_dispatch,
    )
    .expect("live click should record");

    let run = runtime.recording().read_run(output.run_id.as_str()).expect("run should persist");
    assert_eq!(run.artifacts.len(), 4);
    assert_eq!(run.artifacts[0].role, "minecraft-screenshot");
    assert_eq!(run.artifacts[1].role, crate::integrations::minecraft::MINECRAFT_SPATIAL_FRAME_ARTIFACT_ROLE);
    assert_eq!(run.artifacts[2].role, crate::integrations::minecraft::MINECRAFT_PROJECTION_ARTIFACT_ROLE);
    assert_eq!(run.artifacts[3].role, "operation-result");

    let verifications =
      auv_runtime::inspect::list_verifications(runtime.recording().store(), output.run_id.as_str()).expect("verifications should list");
    assert_eq!(verifications.len(), 1);
    assert_eq!(verifications[0].method, auv_runtime::contract::VerificationMethod::SemanticMatch);

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn fixture_minecraft_live_click_dispatch(
    _runtime: &auv_runtime::runtime::Runtime,
    _context: &mut auv_tracing_driver::recorded_operation::RecordedOperationContext<'_>,
    target_app: &str,
    target_title: &str,
    window_point: auv_driver::geometry::WindowPoint,
  ) -> Result<String, String> {
    assert_eq!(target_app, "FixtureApp");
    assert_eq!(target_title, "Fixture Window");
    Ok(format!("fixture clicked at ({:.3},{:.3})", window_point.point().x, window_point.point().y))
  }

  #[test]
  fn minecraft_bridge_run_persists_telemetry_and_projection_artifacts() {
    let project_root = mc2_temp_dir("mc2-bridge-project");
    let store_root = mc2_temp_dir("mc2-bridge-store");
    let telemetry_path = project_root.join("telemetry.jsonl");
    let screenshot_path = project_root.join("frame.png");
    write_mc2_test_telemetry(&telemetry_path);
    write_mc2_test_screenshot(&screenshot_path);

    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone()).expect("runtime should build");
    let output = run_minecraft_projection_bridge(&runtime, telemetry_path, Some(screenshot_path), None, None, "0,0,0", Some(0), true)
      .expect("bridge should succeed");

    let run = runtime.recording().read_run(output.run_id.as_str()).expect("run should persist");
    assert_eq!(run.artifacts.len(), 4);
    assert_eq!(run.artifacts[0].role, "minecraft-screenshot");
    assert_eq!(run.artifacts[1].role, crate::integrations::minecraft::MINECRAFT_SPATIAL_FRAME_ARTIFACT_ROLE);
    assert_eq!(run.artifacts[2].role, crate::integrations::minecraft::MINECRAFT_PROJECTION_ARTIFACT_ROLE);
    assert_eq!(run.artifacts[3].role, "minecraft-overlay");

    let inspect_text = crate::inspect::inspect_run_with(
      &crate::inspect::build_product_inspect_composer().expect("product composer"),
      runtime.recording().store(),
      output.run_id.as_str(),
    )
    .expect("inspect should render run");
    assert!(inspect_text.contains("MC-2 Projection Artifacts:"));
    assert!(inspect_text.contains("capture_skew_ms=0"));
    assert_eq!(output.value.overlay_artifact_id.is_some(), true);
    assert_eq!(output.value.refusal_reason, None);

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn minecraft_bridge_refusal_run_still_persists_telemetry_and_projection_artifacts() {
    let project_root = mc2_temp_dir("mc2-bridge-refusal-project");
    let store_root = mc2_temp_dir("mc2-bridge-refusal-store");
    let telemetry_path = project_root.join("telemetry.jsonl");
    let screenshot_path = project_root.join("frame.png");
    write_mc2_test_telemetry(&telemetry_path);
    write_mc2_test_screenshot(&screenshot_path);

    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone()).expect("runtime should build");
    let output = run_minecraft_projection_bridge(&runtime, telemetry_path, Some(screenshot_path), None, None, "0,0,0", Some(999), true)
      .expect("bridge refusal should still record");

    let run = runtime.recording().read_run(output.run_id.as_str()).expect("run should persist");
    assert_eq!(run.artifacts.len(), 3);
    assert_eq!(run.artifacts[0].role, "minecraft-screenshot");
    assert_eq!(run.artifacts[1].role, crate::integrations::minecraft::MINECRAFT_SPATIAL_FRAME_ARTIFACT_ROLE);
    assert_eq!(run.artifacts[2].role, crate::integrations::minecraft::MINECRAFT_PROJECTION_ARTIFACT_ROLE);

    let inspect_text = crate::inspect::inspect_run_with(
      &crate::inspect::build_product_inspect_composer().expect("product composer"),
      runtime.recording().store(),
      output.run_id.as_str(),
    )
    .expect("inspect should render run");
    assert!(inspect_text.contains("MC-2 Projection Artifacts:"));
    assert!(inspect_text.contains("capture_skew_ms=999"));
    assert_eq!(output.value.refusal_reason.as_deref(), Some("CaptureSkewUnreliable"));
    assert_eq!(output.value.overlay_artifact_id, None);

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn minecraft_calibrate_projection_persists_projection_overlay_and_calibration_artifacts() {
    let project_root = mc2_temp_dir("mc6-calibration-project");
    let store_root = mc2_temp_dir("mc6-calibration-store");
    let frame_path = project_root.join("frame.json");
    let screenshot_path = project_root.join("frame.png");
    write_mc2_test_telemetry(&frame_path);
    write_mc2_test_screenshot(&screenshot_path);

    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone()).expect("runtime should build");
    let output = run_minecraft_calibrate_projection(&runtime, frame_path, screenshot_path, "0,0,0", "hit_face_center", true)
      .expect("calibration should succeed");

    let run = runtime.recording().read_run(output.run_id.as_str()).expect("run should persist");
    assert_eq!(run.artifacts.len(), 5);
    assert_eq!(run.artifacts[0].role, "minecraft-screenshot");
    assert_eq!(run.artifacts[1].role, crate::integrations::minecraft::MINECRAFT_SPATIAL_FRAME_ARTIFACT_ROLE);
    assert_eq!(run.artifacts[2].role, crate::integrations::minecraft::MINECRAFT_PROJECTION_ARTIFACT_ROLE);
    assert_eq!(run.artifacts[3].role, "minecraft-overlay");
    assert_eq!(run.artifacts[4].role, crate::integrations::minecraft::MINECRAFT_PROJECTION_CALIBRATION_ARTIFACT_ROLE);
    assert_eq!(output.value.overlay_artifact_id.is_some(), true);
    assert_eq!(output.value.refusal_reason, None);

    let calibration_artifact = run
      .artifacts
      .iter()
      .find(|artifact| artifact.role == crate::integrations::minecraft::MINECRAFT_PROJECTION_CALIBRATION_ARTIFACT_ROLE)
      .expect("calibration artifact should exist");
    let calibration_path =
      runtime.recording().run_dir(output.run_id.as_str()).expect("run dir should exist").join(&calibration_artifact.path);
    let calibration: MinecraftProjectionCalibrationArtifact =
      serde_json::from_slice(&fs::read(&calibration_path).expect("calibration bytes")).expect("calibration json");
    assert_eq!(calibration.frame_id, "frame-mc2");
    assert_eq!(calibration.target_block, "0,0,0");
    assert_eq!(calibration.target_semantics, "hit_face_center");
    assert_eq!(calibration.raycast_hit_block_pos.as_deref(), Some("0,0,0"));
    assert_eq!(calibration.raycast_hit_face.as_deref(), Some("North"));
    assert_eq!(calibration.refusal_reason, None);
    assert!(calibration.overlay_ref.as_deref().is_some_and(|value| value.starts_with("artifact://")));
    assert_eq!(calibration.known_limits.len(), 2);

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn temp_runtime_store_entries(prefix: &str) -> Vec<String> {
    let mut entries = fs::read_dir(env::temp_dir())
      .expect("temp dir should read")
      .filter_map(|entry| {
        let entry = entry.ok()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        name.starts_with(prefix).then_some(name)
      })
      .collect::<Vec<_>>();
    entries.sort();
    entries
  }
}
