// Shared CLI frontend for root `auv` and donor bins (`auv-minecraft`, `auv-osu`, `auv-godot`).
//
// The root binary tombstones app-specific subcommands; dedicated app binaries
// own their live parse and dispatch paths. This product crate owns their shared
// frontend assembly.

use std::env;
#[cfg(test)]
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode};
use std::sync::Arc;

use crate::cli::{CliCommand, InspectClientOptions, help_text, parse_cli, parse_donor_cli, root_donor_tombstone, version_text};
use crate::integrations::minecraft::verification::query_wired_verification_readable;
use crate::integrations::minecraft::{
  QueryWiredLiveActionInputs, QueryWiredLiveActionTelemetryWitness, run_minecraft_query_wired_live_action,
};
use auv_runtime::app::{analyze_app_probe, probe_app};

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
    ..
  } = &command
  {
    let store_root = resolve_store_root(&project_root, store_root.as_ref());
    let store = open_inspect_authority_store(&store_root)?;
    let config = auv_inspect_server::InspectServeConfig {
      host: host.clone(),
      port: *port,
    };
    auv_inspect_server::serve(store, config, Arc::new(crate::projection::ProductInspectReadProjection::default())).await?;
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
      let inputs = crate::integrations::minecraft::projection_workflow::MinecraftProjectionBridgeInputs {
        telemetry_sample: PathBuf::from(telemetry_sample),
        screenshot: screenshot.map(PathBuf::from),
        capture_target_app,
        capture_target_title,
        target_block: parse_block_position(&target_block)?,
        capture_skew_ms,
        screenshot_is_minecraft_window,
      };
      let (run_id, output) = execute_product_cli_call(&project_root, &inspect, "Minecraft projection bridge", move || {
        crate::integrations::minecraft::projection_workflow::run_minecraft_projection_bridge(inputs)
      })
      .await?;
      println!("runId: {run_id}");
      print_minecraft_projection_publications(&output.publications);
      print_minecraft_projection_refusal(&output.evidence);
    }
    CliCommand::MinecraftCalibrateProjection {
      frame_path,
      screenshot,
      target_block,
      target_semantics,
      screenshot_is_minecraft_window,
      inspect,
    } => {
      let inputs = crate::integrations::minecraft::projection_workflow::MinecraftProjectionCalibrationInputs {
        frame_path: PathBuf::from(frame_path),
        screenshot: PathBuf::from(screenshot),
        target_block: parse_block_position(&target_block)?,
        target_semantics: parse_target_semantics(&target_semantics)?,
        screenshot_is_minecraft_window,
      };
      let (run_id, output) = execute_product_cli_call(&project_root, &inspect, "Minecraft projection calibration", move || {
        crate::integrations::minecraft::projection_workflow::run_minecraft_calibrate_projection(inputs)
      })
      .await?;
      println!("runId: {run_id}");
      print_minecraft_projection_publications(&output.publications);
      print_minecraft_projection_refusal(&output.evidence);
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
      let inputs = crate::integrations::minecraft::projection_workflow::MinecraftLiveClickInputs {
        telemetry_sample: PathBuf::from(telemetry_sample),
        post_telemetry_sample: post_telemetry_sample.map(PathBuf::from),
        screenshot: PathBuf::from(screenshot),
        target_block: parse_block_position(&target_block)?,
        target_app,
        target_title,
        capture_skew_ms,
        screenshot_is_minecraft_window,
      };
      let (run_id, output) = execute_product_cli_call(&project_root, &inspect, "Minecraft live click", move || {
        crate::integrations::minecraft::projection_workflow::run_minecraft_live_click(inputs)
      })
      .await?;
      println!("runId: {run_id}");
      print_minecraft_projection_publications(&output.publications);
      println!("inputSummary: {}", output.input_summary);
      println!("inputPath: {:?}", output.input_action.selected_path);
      println!("inputSucceeded: {}", output.input_action.attempts.last().is_some_and(|attempt| attempt.succeeded));
      println!("verificationExecuted: {}", output.verification.executed);
      println!("verificationSemanticMatched: {:?}", output.verification.semantic_matched);
    }
    CliCommand::MinecraftExportSpatialBundle {
      run_id,
      output_dir,
      inspect,
    } => {
      let (export_run_id, output) =
        execute_product_cli_call_with_store(&project_root, &inspect, "Minecraft spatial bundle export", move |store| {
          crate::integrations::minecraft::run_minecraft_spatial_bundle_export(
            store,
            run_id,
            PathBuf::from(output_dir),
            crate::integrations::minecraft::current_git_commit(),
          )
        })
        .await?;
      println!("runId: {export_run_id}");
      println!("status: completed");
      println!("sourceRunId: {}", output.manifest.source_run.source_run_id);
      println!("spatialFrames: {}", output.manifest.counts.spatial_frames);
      println!("screenshots: {}", output.manifest.counts.screenshots);
      println!("verification: {}", output.manifest.counts.verification);
      println!("overlays: {}", output.manifest.counts.overlays);
      println!("output: {}", output.output_dir.display());
    }
    CliCommand::MinecraftExport3dgsScenePacket {
      bundle_manifest_paths,
      output_dir,
      inspect,
    } => {
      let output = execute_product_cli_call(&project_root, &inspect, "Minecraft scene packet export", move || {
        crate::integrations::minecraft::run_minecraft_3dgs_scene_packet_export(
          bundle_manifest_paths.into_iter().map(PathBuf::from).collect(),
          PathBuf::from(output_dir),
        )
      })
      .await?
      .1;
      println!("status: completed");
      println!("scenePacketSchema: {}", output.manifest.schema_version);
      println!("sourceRuns: {}", output.manifest.source_run_ids.join(","));
      println!("frames: {}", output.manifest.counts.frames);
      println!("screenshots: {}", output.manifest.counts.screenshots);
      println!("missingScreenshots: {}", output.manifest.counts.missing_screenshots);
      println!("manifest: {}", output.manifest_path.display());
      println!("cameras: {}", output.cameras_path.display());
      println!("output: {}", output.output_dir.display());
    }
    CliCommand::MinecraftExport3dgsTrainingPackage {
      scene_packet_manifest_path,
      output_dir,
      inspect,
    } => {
      let output = execute_product_cli_call(&project_root, &inspect, "Minecraft training package export", move || {
        crate::integrations::minecraft::run_minecraft_3dgs_training_package_export(
          PathBuf::from(scene_packet_manifest_path),
          PathBuf::from(output_dir),
        )
      })
      .await?
      .1;
      println!("status: completed");
      println!("trainingPackageSchema: {}", output.manifest.schema_version);
      println!("sourceRuns: {}", output.manifest.source_run_ids.join(","));
      println!("frames: {}", output.manifest.counts.frames);
      println!("images: {}", output.manifest.counts.images);
      println!(
        "compatibilityStatus: {}",
        match output.inspect_report.compatibility_views[0].status {
          auv_game_minecraft::TrainingCompatibilityStatus::Ready => "ready",
          auv_game_minecraft::TrainingCompatibilityStatus::Partial => "partial",
          auv_game_minecraft::TrainingCompatibilityStatus::Blocked => "blocked",
        }
      );
      println!("compatibilityExportedFrames: {}", output.manifest.counts.compatibility_exported_frames);
      println!("manifest: {}", output.manifest_path.display());
      println!("inspectReport: {}", output.inspect_report_path.display());
      if let Some(transforms_path) = output.compatibility_transforms_path.as_ref() {
        println!("nerfstudioTransforms: {}", transforms_path.display());
      } else {
        println!("nerfstudioTransforms: none");
      }
      println!("output: {}", output.output_dir.display());
    }
    CliCommand::MinecraftPrepare3dgsTraining {
      training_package_manifest_path,
      output_dir,
      inspect,
    } => {
      let output = execute_product_cli_call(&project_root, &inspect, "Minecraft training launch preparation", move || {
        crate::integrations::minecraft::run_minecraft_3dgs_training_launch_preparation(
          PathBuf::from(training_package_manifest_path),
          PathBuf::from(output_dir),
        )
      })
      .await?
      .1;
      println!("status: completed");
      println!("trainerBackend: {}", output.manifest.trainer_backend);
      println!(
        "trainerReadiness: {}",
        match output.inspect_report.trainer_readiness {
          auv_game_minecraft::TrainingLaunchReadiness::Ready => "ready",
          auv_game_minecraft::TrainingLaunchReadiness::Blocked => "blocked",
        }
      );
      println!(
        "readinessBlocker: {}",
        match output.inspect_report.readiness_blocker {
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
      println!("launchCommand: {}", output.manifest.launch_command);
      println!("launchPlan: {}", output.manifest_path.display());
      println!("inspectReport: {}", output.inspect_report_path.display());
      println!("runbook: {}", output.runbook_path.display());
      println!("output: {}", output.output_dir.display());
    }
    CliCommand::MinecraftLaunch3dgsTrainingJob {
      training_launch_plan_path,
      output_dir,
      training_job_endpoint,
      training_job_token,
      training_job_submit_command,
      inspect,
    } => {
      let output = execute_product_cli_call(&project_root, &inspect, "Minecraft training job launch", move || {
        crate::integrations::minecraft::run_minecraft_3dgs_training_job_launch_with_environment(
          PathBuf::from(training_launch_plan_path),
          PathBuf::from(output_dir),
          training_job_endpoint,
          training_job_token,
          training_job_submit_command,
        )
      })
      .await?
      .1;
      println!("remoteJobStatus: {}", output.inspect_report.status.as_str());
      println!("trainerBackend: {}", output.manifest.trainer_backend);
      println!("providerBackend: {}", output.manifest.provider_backend);
      println!("jobBackend: {}", output.manifest.job_backend);
      println!(
        "submissionState: {}",
        match output.inspect_report.status {
          auv_game_minecraft::TrainingLaunchJobStatus::Blocked => "blocked_before_submission",
          auv_game_minecraft::TrainingLaunchJobStatus::Failed => "submission_failed",
          auv_game_minecraft::TrainingLaunchJobStatus::Queued
          | auv_game_minecraft::TrainingLaunchJobStatus::Submitted
          | auv_game_minecraft::TrainingLaunchJobStatus::Succeeded => {
            "submission_submitted_or_queued"
          }
        }
      );
      println!("acceptedByProvider: {}", output.inspect_report.accepted_by_provider);
      println!(
        "submissionRecordedAtMillis: {}",
        output.inspect_report.submission_recorded_at_millis.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string())
      );
      println!(
        "readinessBlocker: {}",
        match output.inspect_report.readiness_blocker {
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
      println!("launchCommand: {}", output.manifest.launch_command);
      println!("configuredJobSubmissionCommand: {}", output.manifest.job_submission_command);
      println!("launchPlan: {}", output.manifest.source_training_launch_plan_path);
      println!("manifest: {}", output.manifest_path.display());
      println!("inspectReport: {}", output.inspect_report_path.display());
      println!("runbook: {}", output.runbook_path.display());
      println!("output: {}", output.output_dir.display());
    }
    CliCommand::MinecraftCollect3dgsTrainingJobResult {
      training_job_manifest_path,
      output_dir,
      training_job_endpoint,
      training_job_token,
      training_job_status_command,
      inspect,
    } => {
      let output = execute_product_cli_call(&project_root, &inspect, "Minecraft training result collection", move || {
        crate::integrations::minecraft::run_minecraft_3dgs_training_result_collection_with_environment(
          PathBuf::from(training_job_manifest_path),
          PathBuf::from(output_dir),
          training_job_endpoint,
          training_job_token,
          training_job_status_command,
        )
      })
      .await?
      .1;
      println!("status: {}", output.inspect_report.status.as_str());
      println!("statusMessage: {}", output.inspect_report.status_message.as_deref().unwrap_or("none"));
      println!("remoteResultStatus: {}", output.inspect_report.status.as_str());
      println!("trainerBackend: {}", output.manifest.trainer_backend);
      println!("jobBackend: {}", output.manifest.job_backend);
      println!(
        "statusReason: {}",
        match output.inspect_report.status_reason {
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
        match output.inspect_report.status_reason {
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
          None => match output.inspect_report.status {
            auv_game_minecraft::TrainingResultStatus::Succeeded
            | auv_game_minecraft::TrainingResultStatus::Submitted
            | auv_game_minecraft::TrainingResultStatus::Queued => {
              if !output.inspect_report.result_dir_exists || !output.inspect_report.key_result_artifacts_present {
                "provider_status_recorded_local_results_not_yet_observed"
              } else {
                "provider_status_matches_local_result_observation"
              }
            }
            _ => "provider_status_recorded",
          },
        }
      );
      println!("jobId: {}", output.manifest.job_id);
      println!("jobUrl: {}", output.manifest.job_url.as_deref().unwrap_or("none"));
      println!("resultDir: {}", output.manifest.result_dir);
      println!("resultDirExists: {}", output.inspect_report.result_dir_exists);
      println!("keyResultArtifactsPresent: {}", output.inspect_report.key_result_artifacts_present);
      println!("manifest: {}", output.manifest_path.display());
      println!("inspectReport: {}", output.inspect_report_path.display());
      println!("runbook: {}", output.runbook_path.display());
      println!("output: {}", output.output_dir.display());
    }
    CliCommand::MinecraftFetch3dgsTrainingResultArtifacts {
      training_result_manifest_path,
      output_dir,
      training_job_endpoint,
      training_job_token,
      artifact_fetch_command,
      inspect,
    } => {
      let output = execute_product_cli_call(&project_root, &inspect, "Minecraft training result artifact fetch", move || {
        crate::integrations::minecraft::run_minecraft_3dgs_training_result_artifact_fetch(
          PathBuf::from(training_result_manifest_path),
          PathBuf::from(output_dir),
          training_job_endpoint,
          training_job_token,
          artifact_fetch_command,
        )
      })
      .await?
      .1;
      println!("fetchStatus: {}", output.inspect_report.fetch_status.as_str());
      println!("trainerBackend: {}", output.manifest.trainer_backend);
      println!("jobBackend: {}", output.manifest.job_backend);
      println!("sourceResultStatus: {}", output.manifest.source_result_status.as_str());
      println!("fetchReason: {}", output.inspect_report.fetch_reason.map(|reason| reason.as_str()).unwrap_or("none"));
      println!("sourceResultDir: {}", output.manifest.source_result_dir);
      println!("normalizedResultDir: {}", output.manifest.normalized_result_dir);
      println!("normalizedArtifactCount: {}", output.inspect_report.normalized_artifact_count);
      println!("requiredArtifactsPresent: {}", output.inspect_report.required_artifacts_present);
      println!("manifest: {}", output.manifest_path.display());
      println!("inspectReport: {}", output.inspect_report_path.display());
      println!("output: {}", output.output_dir.display());
    }

    CliCommand::MinecraftInspect3dgsTrainingResultHoldout {
      training_result_semantic_manifest_path,
      holdout_frame_index,
      holdout_render_command,
      output_dir,
      inspect,
    } => {
      let output = execute_product_cli_call(&project_root, &inspect, "Minecraft training holdout inspection", move || {
        crate::integrations::minecraft::run_minecraft_3dgs_training_result_holdout_preview(
          PathBuf::from(training_result_semantic_manifest_path),
          holdout_frame_index,
          holdout_render_command,
          PathBuf::from(output_dir),
        )
      })
      .await?
      .1;
      println!("status: {}", output.manifest.status.as_str());
      println!("reason: {}", output.manifest.reason.map(|reason| reason.as_str()).unwrap_or("none"));
      println!("holdoutFrameIndex: {}", output.manifest.holdout_frame_index);
      println!(
        "spatialFrameId: {}",
        output.manifest.holdout_frame.as_ref().map(|witness| witness.spatial_frame_id.as_str()).unwrap_or("none")
      );
      println!("basisCheckpointPath: {}", output.manifest.basis_checkpoint_path.as_deref().unwrap_or("none"));
      println!("holdoutScreenshotPath: {}", output.manifest.holdout_screenshot_path.as_deref().unwrap_or("none"));
      println!("holdoutPreviewManifest: {}", output.manifest_path.display());
      println!("inspectReport: {}", output.inspect_report_path.display());
    }

    CliCommand::MinecraftMeasure3dgsHoldoutRenderQuality {
      training_result_semantic_manifest_path,
      holdout_preview_manifest_path,
      render_command,
      output_dir,
      inspect,
    } => {
      let output = execute_product_cli_call(&project_root, &inspect, "Minecraft holdout render quality", move || {
        crate::integrations::minecraft::run_minecraft_measure_3dgs_holdout_render_quality(
          PathBuf::from(training_result_semantic_manifest_path),
          PathBuf::from(holdout_preview_manifest_path),
          render_command,
          PathBuf::from(output_dir),
        )
      })
      .await?
      .1;
      println!("status: {}", output.manifest.status.as_str());
      println!("verdict: {}", output.manifest.verdict.as_str());
      println!("imageSizeMatch: {}", output.manifest.image_size_match);
      let metrics = output.manifest.metrics.as_ref();
      println!(
        "l1Mean: {}",
        metrics.and_then(|metrics| metrics.l1_mean).map(|value| value.to_string()).unwrap_or_else(|| "none".to_string())
      );
      println!("mse: {}", metrics.and_then(|metrics| metrics.mse).map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()));
      println!("psnr: {}", metrics.and_then(|metrics| metrics.psnr).map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()));
      println!("manifest: {}", output.manifest_path.display());
      println!("inspectReport: {}", output.inspect_report_path.display());
      println!("output: {}", output.output_dir.display());
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
      let target_block = parse_block_position(&target_block)?;
      let target_face = target_face.as_deref().map(parse_block_face).transpose()?;
      let target_semantics = parse_target_semantics(&target_semantics)?;
      let output = execute_product_cli_call(&project_root, &inspect, "Minecraft training spatial query", move || {
        crate::integrations::minecraft::run_minecraft_3dgs_training_result_spatial_query(
          PathBuf::from(training_result_semantic_manifest_path),
          target_block,
          target_face,
          target_semantics,
          query_command,
          use_checkpoint_native_provider,
          use_closed_scene_toy_provider,
          closed_scene_fixture_path.map(PathBuf::from),
          PathBuf::from(output_dir),
        )
      })
      .await?
      .1;
      println!("status: {}", output.manifest.status.as_str());
      if matches!(
        output.manifest.status,
        auv_game_minecraft::TrainingResultSpatialQueryStatus::Blocked | auv_game_minecraft::TrainingResultSpatialQueryStatus::Failed
      ) {
        println!("reason: {}", output.manifest.reason.map(|reason| reason.as_str()).unwrap_or("none"));
      }
      println!("selectedBackend: {}", output.manifest.selected_backend.map(|backend| backend.as_str()).unwrap_or("none"));
      println!(
        "visibility: {}",
        output.manifest.visibility.map(|visibility| format!("{visibility:?}")).unwrap_or_else(|| "none".to_string())
      );
      if let Some(screen_point) = output.manifest.screen_point {
        println!("screenPoint: {},{}", screen_point.x, screen_point.y);
      } else {
        println!("screenPoint: none");
      }
      println!("basisFrameId: {}", output.manifest.basis_frame_id.as_deref().unwrap_or("none"));
      println!("comparisonVerdict: {}", output.manifest.comparison_verdict.map(|verdict| verdict.as_str()).unwrap_or("none"));
      println!("queryManifest: {}", output.manifest_path.display());
      println!("inspectReport: {}", output.inspect_report_path.display());
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
      let authority = build_invoke_dispatch(&project_root, &inspect).await?;
      let execution = execute_invoke_frontend(&authority, move || run_minecraft_query_wired_live_action(inputs)).await?;
      if let Some(failure) = execution.recording_failure.as_deref() {
        eprintln!("warning: query-wired action instrumentation failed: {failure}");
      }
      let run_id = execution.run_id;
      let output = execution.direct_result?;
      println!("queryStatus: {}", output.query.manifest.status.as_str());
      println!("wiringAttempted: {}", output.wiring.attempted);
      println!("actionEligibility: {}", output.wiring.action_eligibility.as_str());
      println!("inputActionCount: {}", output.input_actions.len());
      println!("verificationCount: {}", output.verifications.len());
      if query_wired_verification_readable(&output.wiring) && should_write_local(&inspect) {
        println!("{}", format_query_wired_inspect_hint(run_id, &inspect));
      }
    }
    CliCommand::MinecraftValidate3dgsTrainingResult {
      training_result_artifact_manifest_path,
      output_dir,
      inspect,
    } => {
      let output = execute_product_cli_call(&project_root, &inspect, "Minecraft training result validation", move || {
        crate::integrations::minecraft::run_minecraft_3dgs_training_result_semantic_validation(
          PathBuf::from(training_result_artifact_manifest_path),
          PathBuf::from(output_dir),
        )
      })
      .await?
      .1;
      println!("status: {}", output.inspect_report.semantic_status.as_str());
      println!("reason: {}", output.inspect_report.semantic_reason.map(|reason| reason.as_str()).unwrap_or("none"));
      println!("trainerBackend: {}", output.manifest.trainer_backend);
      println!("checkpointCount: {}", output.inspect_report.checkpoint_count);
      println!("configTrainer: {}", output.inspect_report.config_trainer.as_deref().unwrap_or("none"));
      println!("semanticManifest: {}", output.manifest_path.display());
      println!("inspectReport: {}", output.inspect_report_path.display());
    }
    CliCommand::MinecraftPrepareTextureSweep {
      sidecar_run_dir,
      output_dir,
      inspect,
    } => {
      let output = execute_product_cli_call(&project_root, &inspect, "Minecraft texture sweep preparation", move || {
        crate::integrations::minecraft::run_minecraft_texture_sweep_preparation(PathBuf::from(sidecar_run_dir), PathBuf::from(output_dir))
      })
      .await?
      .1;
      println!("status: prepared");
      println!("packFormat: {}", output.manifest.pack_format);
      println!("profiles: {}", output.manifest.profiles.len());
      for profile in &output.manifest.profiles {
        println!(
          "profile: {} pack={} expectedTelemetryId={} optionsResourcePacks={}",
          profile.texture_profile, profile.pack_dir, profile.expected_telemetry_resource_pack_id, profile.options_resource_packs_value
        );
      }
      println!("manifest: {}", output.manifest_path.display());
      println!("runbook: {}", output.runbook_path.display());
    }
    CliCommand::MinecraftBuildTextureSweepSamples {
      bundle_manifest_paths,
      output_path,
      inspect,
    } => {
      let output = execute_product_cli_call(&project_root, &inspect, "Minecraft texture sweep sample build", move || {
        crate::integrations::minecraft::run_minecraft_texture_sweep_sample_build(
          bundle_manifest_paths.into_iter().map(PathBuf::from).collect(),
          PathBuf::from(output_path),
        )
      })
      .await?
      .1;
      println!("status: completed");
      println!("samples: {}", output.sample_set.samples.len());
      if let Some(source) = &output.sample_set.source {
        println!("sampleSourceGenerator: {}", source.generator);
        println!("sampleSourceRuns: {}", source.source_run_ids.join(","));
        println!("bundleManifests: {}", source.bundle_manifest_paths.join(","));
      }
      println!("output: {}", output.output_path.display());
    }
    CliCommand::MinecraftEvalTextureSweep {
      samples_path,
      output_dir,
      require_real_source,
      inspect,
    } => {
      let output = execute_product_cli_call(&project_root, &inspect, "Minecraft texture sweep evaluation", move || {
        crate::integrations::minecraft::run_minecraft_texture_sweep_eval(
          PathBuf::from(samples_path),
          PathBuf::from(output_dir),
          require_real_source,
        )
      })
      .await?
      .1;
      println!("status: completed");
      println!("requireRealSource: {require_real_source}");
      println!("passed: {}", output.passed);
      println!("resourcePacks: {}", output.actual_resource_pack_count);
      println!("noiseRefusalExercised: {}", output.noise_refusal_exercised);
      if let Some(source) = &output.source {
        println!("sampleSourceGenerator: {}", source.generator);
        if !source.source_run_ids.is_empty() {
          println!("sampleSourceRuns: {}", source.source_run_ids.join(","));
        }
      }
      for row in &output.rows {
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
      let probe = probe_app(&project_root, &bundle_id, output_dir.map(PathBuf::from))?;
      println!("app: {}", probe.app.bundle_id);
      println!("status: captured");
      println!("probe: {}", probe.output_dir.join("probe.json").display());
      println!("steps: {}", probe.steps.len());
    }
    CliCommand::AppAnalyze { query } => {
      let output = analyze_app_probe(&PathBuf::from(query))?;
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
      let beatmap_path = PathBuf::from(beatmap_path);
      let output_dir = output_dir.map(PathBuf::from).unwrap_or_else(|| temp_runtime_store_root().join("osu-benchmark-output"));
      let output = crate::integrations::osu::run_osu_benchmark(beatmap_path, output_dir).await?;
      println!("status: completed");
      println!("beatmap: {}", output.map_summary.beatmap_path);
      println!("objects: {}", output.map_summary.total_objects);
      println!("latencyP95Ms: {}", output.latency_report.p95_error_ms);
      println!("jitterMs: {}", output.latency_report.jitter_ms);
      println!("output: {}", output.output_dir.display());
    }
    CliCommand::OsuBenchmarkDispatch {
      beatmap_path,
      target_app,
      output_dir,
      dispatch_limit,
      capture_verify,
    } => {
      let beatmap_path = PathBuf::from(beatmap_path);
      let output_dir = output_dir.map(PathBuf::from).unwrap_or_else(|| temp_runtime_store_root().join("osu-dispatch-output"));
      let mut inputs = auv_game_osu::BenchmarkInputs::typed_dispatch(beatmap_path, output_dir, target_app);
      if let Some(dispatch_limit) = dispatch_limit {
        inputs.dispatch_limit = Some(dispatch_limit);
      }
      inputs.capture_verify = capture_verify;
      let output = crate::integrations::osu::run_osu_benchmark_with_inputs(
        inputs,
        if capture_verify {
          "osu benchmark typed dispatch with capture verification"
        } else {
          "osu benchmark typed dispatch"
        },
      )
      .await?;
      println!("status: completed");
      println!("beatmap: {}", output.map_summary.beatmap_path);
      println!("objects: {}", output.map_summary.total_objects);
      println!("latencyP95Ms: {}", output.latency_report.p95_error_ms);
      println!("jitterMs: {}", output.latency_report.jitter_ms);
      if let Some(summary) = &output.verification_summary {
        println!("verificationCapturedActions: {}", summary.captured_action_count);
        println!("verificationMissingFrames: {}", summary.missing_frame_count);
      }
      println!("output: {}", output.output_dir.display());
    }
    CliCommand::OsuExportDataset {
      run_artifact_dir,
      output_dir,
    } => {
      let output = crate::integrations::osu::run_osu_dataset_export(PathBuf::from(run_artifact_dir), PathBuf::from(output_dir)).await?;
      println!("status: completed");
      println!("exportedFrames: {}", output.dataset_manifest.exported_frames.len());
      println!("skippedFrames: {}", output.dataset_manifest.skipped_frames.len());
      println!("output: {}", output.output_dir.display());
    }
    CliCommand::OsuEvalDetections {
      run_artifact_dir,
      detections_path,
      output_dir,
    } => {
      let output = crate::integrations::osu::run_osu_detection_eval(
        PathBuf::from(run_artifact_dir),
        PathBuf::from(detections_path),
        output_dir.map(PathBuf::from).unwrap_or_else(|| temp_runtime_store_root().join("osu-eval-detections-output")),
      )
      .await?;
      println!("status: completed");
      println!("totalFrames: {}", output.visual_eval_report.total_frames);
      println!("labelMatchedFrames: {}", output.visual_eval_report.label_matched_frames);
      println!("spatialMatchedFrames: {}", output.visual_eval_report.spatial_matched_frames);
      println!("output: {}", output.output_dir.display());
    }
    CliCommand::OsuVisionDemo {
      beatmap_path,
      target_app,
      output_dir,
      dispatch_limit,
      capture_verify,
    } => {
      let output = crate::integrations::osu::run_osu_vision_demo(
        PathBuf::from(beatmap_path),
        target_app,
        output_dir.map(PathBuf::from).unwrap_or_else(|| temp_runtime_store_root().join("osu-vision-demo-output")),
        dispatch_limit,
        capture_verify,
      )
      .await?;
      println!("status: completed");
      println!("beatmap: {}", output.map_summary.beatmap_path);
      println!("objects: {}", output.map_summary.total_objects);
      println!("latencyP95Ms: {}", output.latency_report.p95_error_ms);
      println!("jitterMs: {}", output.latency_report.jitter_ms);
      println!("dispatchSamples: {}", output.dispatch_trace.len());
      println!("captureArtifacts: {}", output.capture_trace.len());
      println!(
        "evidenceNotes: {}",
        if output.evidence_summary.evidence_notes.is_empty() {
          "none".to_string()
        } else {
          output.evidence_summary.evidence_notes.join(" | ")
        }
      );
      println!("hasEvidenceArtifact: {}", output.output_dir.join("evidence_summary.json").exists());
      println!("hasProjectionArtifact: {}", output.projection.as_ref().is_some());
      println!("hasVisualTruthManifest: {}", output.visual_truth_manifest.as_ref().is_some());
      if let Some(summary) = &output.verification_summary {
        println!("verificationCapturedActions: {}", summary.captured_action_count);
        println!("verificationMissingFrames: {}", summary.missing_frame_count);
      }
      println!("output: {}", output.output_dir.display());
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
      if let Some(error) = execution.recording_failure {
        eprintln!("warning: invoke recording failure for run {}: {error}", execution.run_id);
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
      let document = crate::inspect::build_product_inspect_document(store.as_ref(), &snapshot)
        .await
        .map_err(|error| format!("failed to inspect Minecraft artifacts for run {run_id}: {error}"))?;
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

fn print_minecraft_projection_publications(
  publications: &crate::integrations::minecraft::projection_workflow::MinecraftProjectionPublications,
) {
  for (label, artifact) in [
    ("screenshotArtifact", publications.screenshot.as_ref()),
    ("spatialFrameArtifact", publications.spatial_frame.as_ref()),
    ("projectionArtifact", publications.projection.as_ref()),
    ("overlayArtifact", publications.overlay.as_ref()),
    ("calibrationArtifact", publications.calibration.as_ref()),
  ] {
    if let Some(artifact) = artifact {
      println!("{label}: {}", artifact.uri());
    }
  }
}

fn print_minecraft_projection_refusal(evidence: &auv_game_minecraft::evidence::ProjectionEvidence) {
  match evidence {
    auv_game_minecraft::evidence::ProjectionEvidence::Bound { .. } => println!("refusalReason: none"),
    auv_game_minecraft::evidence::ProjectionEvidence::Refused { refusal, .. } => {
      println!("refusalReason: {:?}", refusal.reason);
    }
  }
}

fn parse_target_semantics(raw: &str) -> Result<auv_game_minecraft::MinecraftTargetSemantics, String> {
  match raw {
    "hit_face_center" => Ok(auv_game_minecraft::MinecraftTargetSemantics::HitFaceCenter),
    "block_center" => Ok(auv_game_minecraft::MinecraftTargetSemantics::BlockCenter),
    other => Err(format!("invalid --target-semantics {other:?}; expected hit_face_center or block_center")),
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

#[derive(Clone)]
struct InvokeFrontendAuthority {
  dispatch: auv_tracing::Dispatch,
  store: Arc<dyn auv_tracing::RunStore>,
}

async fn build_invoke_dispatch(project_root: &Path, inspect: &InspectClientOptions) -> Result<InvokeFrontendAuthority, String> {
  let server_target = if should_try_server_write(inspect) {
    if let Some(url) = resolve_inspect_server_target(inspect)? {
      Some(url)
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
    Some(url) => {
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
  Ok(InvokeFrontendAuthority { dispatch, store })
}

#[derive(Debug)]
struct InvokeFrontendExecution<T> {
  run_id: auv_tracing::RunId,
  direct_result: Result<T, String>,
  recording_failure: Option<String>,
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
  let run_id = auv_tracing::RunId::new();
  let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
  let future = root.in_scope(|| {
    auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
    call()
  });
  let direct_result = root.instrument(future).await;
  let mut recording_failure = authority.dispatch.flush().await.err().map(|error| error.to_string());
  let canonical_artifacts = match authority.store.load_snapshot(run_id).await {
    Ok(Some(snapshot)) => snapshot.artifacts().values().map(|artifact| artifact.metadata().clone()).collect(),
    Ok(None) => {
      recording_failure.get_or_insert_with(|| "recorded run snapshot is missing after execution".to_string());
      Vec::new()
    }
    Err(error) => {
      recording_failure.get_or_insert_with(|| format!("failed to load recorded run snapshot: {error}"));
      Vec::new()
    }
  };
  Ok(InvokeFrontendExecution {
    run_id,
    direct_result,
    recording_failure,
    canonical_artifacts,
  })
}

async fn execute_product_cli_call<T, F, Fut>(
  project_root: &Path,
  inspect: &InspectClientOptions,
  label: &'static str,
  call: F,
) -> Result<(auv_tracing::RunId, T), String>
where
  T: Send + 'static,
  F: FnOnce() -> Fut + Send + 'static,
  Fut: Future<Output = Result<T, String>> + Send + 'static,
{
  execute_product_cli_call_with_store(project_root, inspect, label, move |_| call()).await
}

async fn execute_product_cli_call_with_store<T, F, Fut>(
  project_root: &Path,
  inspect: &InspectClientOptions,
  label: &'static str,
  call: F,
) -> Result<(auv_tracing::RunId, T), String>
where
  T: Send + 'static,
  F: FnOnce(Arc<dyn auv_tracing::RunStore>) -> Fut + Send + 'static,
  Fut: Future<Output = Result<T, String>> + Send + 'static,
{
  let authority = build_invoke_dispatch(project_root, inspect).await?;
  let store = authority.store.clone();
  let execution = execute_invoke_frontend(&authority, move || call(store)).await?;
  if let Some(failure) = execution.recording_failure.as_deref() {
    eprintln!("warning: {label} instrumentation failed: {failure}");
  }
  Ok((execution.run_id, execution.direct_result?))
}

fn should_write_local(inspect: &InspectClientOptions) -> bool {
  !matches!(inspect.local_write, crate::cli::InspectWriteSetting::Disabled)
}

fn should_try_server_write(inspect: &InspectClientOptions) -> bool {
  inspect.require_server_write || !matches!(inspect.server_write, crate::cli::InspectWriteSetting::Disabled)
}

fn resolve_inspect_server_target(inspect: &InspectClientOptions) -> Result<Option<String>, String> {
  if let Some(url) = &inspect.server_url {
    return Ok(Some(url.clone()));
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
  Ok(Some(session.url))
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
  use image::{Rgb, RgbImage};
  use tower::ServiceExt;

  use super::*;

  #[test]
  fn library_exit_status_returns_typed_codes_without_terminating_the_process() {
    assert_eq!(exit_status(Ok(0)), std::process::ExitCode::SUCCESS);
    assert_eq!(exit_status(Ok(7)), std::process::ExitCode::from(7));
    assert_eq!(exit_status(Err("failed".to_string())), std::process::ExitCode::FAILURE);
  }

  fn minecraft_dispatch_fixture(label: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = env::temp_dir().join(format!("auv-task22-{label}-{}", auv_tracing::RunId::new()));
    fs::create_dir_all(&root).expect("Minecraft dispatch fixture directory should write");
    let telemetry_path = root.join("telemetry.jsonl");
    let frame_path = root.join("frame.json");
    let screenshot_path = root.join("frame.png");
    let frame = auv_game_minecraft::MinecraftSpatialFrame {
      spatial_frame_id: "frame-task22".to_string(),
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
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: None,
      resource_pack_ids: Vec::new(),
    };
    let frame_json = serde_json::to_string(&frame).expect("Minecraft fixture frame should encode");
    fs::write(&telemetry_path, format!("{frame_json}\n")).expect("Minecraft fixture telemetry should write");
    fs::write(&frame_path, frame_json).expect("Minecraft fixture frame should write");
    RgbImage::from_pixel(64, 64, Rgb([0, 0, 0])).save(&screenshot_path).expect("Minecraft fixture screenshot should write");
    (root.clone(), telemetry_path, frame_path, screenshot_path)
  }

  fn minecraft_dispatch_inspect(root: &Path) -> InspectClientOptions {
    InspectClientOptions {
      store_root: Some(root.join("store").display().to_string()),
      server_write: crate::cli::InspectWriteSetting::Disabled,
      ..InspectClientOptions::default()
    }
  }

  async fn minecraft_dispatch_artifact_purposes(root: &Path) -> Vec<String> {
    let run_ids = fs::read_dir(root.join("store").join("runs"))
      .expect("Minecraft dispatch run directory should read")
      .map(|entry| {
        entry
          .expect("Minecraft dispatch run entry should read")
          .file_name()
          .to_string_lossy()
          .parse::<RunId>()
          .expect("Minecraft dispatch run entry should be a run id")
      })
      .collect::<Vec<_>>();
    assert_eq!(run_ids.len(), 1, "Minecraft fixture should record exactly one frontend run");
    let store = auv_tracing::FileRunStore::open(root.join("store")).expect("Minecraft dispatch store should open");
    let snapshot = store
      .load_snapshot(run_ids[0])
      .await
      .expect("Minecraft dispatch snapshot should read")
      .expect("Minecraft dispatch snapshot should exist");
    snapshot.artifacts().values().map(|artifact| artifact.metadata().purpose().as_str().to_string()).collect()
  }

  #[tokio::test]
  async fn minecraft_bridge_dispatch_reaches_projection_workflow() {
    let (root, telemetry_path, _, screenshot_path) = minecraft_dispatch_fixture("bridge");

    let result = dispatch(CliCommand::MinecraftProjectionBridge {
      telemetry_sample: telemetry_path.display().to_string(),
      screenshot: Some(screenshot_path.display().to_string()),
      capture_target_app: None,
      capture_target_title: None,
      target_block: "0,0,0".to_string(),
      capture_skew_ms: Some(0),
      screenshot_is_minecraft_window: true,
      inspect: minecraft_dispatch_inspect(&root),
    })
    .await;

    let purposes = minecraft_dispatch_artifact_purposes(&root).await;
    fs::remove_dir_all(&root).expect("remove Minecraft bridge fixture");
    assert_eq!(result, Ok(0));
    assert!(purposes.iter().any(|purpose| purpose == auv_game_minecraft::artifact::MINECRAFT_PROJECTION_PURPOSE));
    assert!(purposes.iter().any(|purpose| { purpose == crate::integrations::minecraft::projection_workflow::MINECRAFT_OVERLAY_PURPOSE }));
  }

  #[tokio::test]
  async fn minecraft_calibration_dispatch_reaches_projection_workflow() {
    let (root, _, frame_path, screenshot_path) = minecraft_dispatch_fixture("calibration");

    let result = dispatch(CliCommand::MinecraftCalibrateProjection {
      frame_path: frame_path.display().to_string(),
      screenshot: screenshot_path.display().to_string(),
      target_block: "0,0,0".to_string(),
      target_semantics: "hit_face_center".to_string(),
      screenshot_is_minecraft_window: true,
      inspect: minecraft_dispatch_inspect(&root),
    })
    .await;

    let purposes = minecraft_dispatch_artifact_purposes(&root).await;
    fs::remove_dir_all(&root).expect("remove Minecraft calibration fixture");
    assert_eq!(result, Ok(0));
    assert!(purposes.iter().any(|purpose| purpose == auv_game_minecraft::artifact::MINECRAFT_PROJECTION_PURPOSE));
    assert!(
      purposes
        .iter()
        .any(|purpose| { purpose == crate::integrations::minecraft::projection_workflow::MINECRAFT_PROJECTION_CALIBRATION_PURPOSE })
    );
  }

  #[tokio::test]
  async fn minecraft_live_click_dispatch_reaches_projection_refusal_without_live_input() {
    let (root, telemetry_path, _, screenshot_path) = minecraft_dispatch_fixture("live-click");

    let error = dispatch(CliCommand::MinecraftLiveClick {
      telemetry_sample: telemetry_path.display().to_string(),
      screenshot: screenshot_path.display().to_string(),
      target_block: "0,0,0".to_string(),
      target_app: "invalid.fixture.minecraft".to_string(),
      target_title: "Fixture Minecraft".to_string(),
      post_telemetry_sample: None,
      capture_skew_ms: Some(0),
      screenshot_is_minecraft_window: false,
      inspect: minecraft_dispatch_inspect(&root),
    })
    .await
    .expect_err("non-Minecraft screenshot should reach domain refusal before input");

    fs::remove_dir_all(&root).expect("remove Minecraft live-click fixture");
    assert!(error.contains("NotMinecraftWindow"), "unexpected live-click error: {error}");
  }

  #[tokio::test]
  async fn minecraft_spatial_bundle_dispatch_exports_canonical_projection_artifact() {
    let (root, _, frame_path, _) = minecraft_dispatch_fixture("spatial-bundle");
    let store_root = root.join("store");
    let store = Arc::new(auv_tracing::FileRunStore::open(&store_root).expect("Minecraft fixture store should open"));
    let source_dispatch = auv_tracing::configure().run_store(store.clone()).build().expect("Minecraft fixture dispatch should build");
    let source_run_id = RunId::new();
    let source_root = auv_tracing::dispatcher::with_default(&source_dispatch, || auv_tracing::Context::root(source_run_id));
    let frame: auv_game_minecraft::MinecraftSpatialFrame =
      serde_json::from_slice(&fs::read(frame_path).expect("Minecraft fixture frame should read"))
        .expect("Minecraft fixture frame should parse");
    let projection = auv_game_minecraft::MinecraftProjectionArtifact::for_frame(&frame, None, None);
    auv_game_minecraft::artifact::publish_minecraft_projection(Some(&source_root), &projection)
      .await
      .expect("Minecraft projection should publish")
      .expect("Minecraft projection publication should be enabled");
    source_dispatch.flush().await.expect("Minecraft fixture source run should flush");

    let output_dir = root.join("bundle");
    let result = dispatch(CliCommand::MinecraftExportSpatialBundle {
      run_id: source_run_id.to_string(),
      output_dir: output_dir.display().to_string(),
      inspect: minecraft_dispatch_inspect(&root),
    })
    .await;

    assert_eq!(result, Ok(0));
    let manifest = crate::integrations::minecraft::read_spatial_bundle_manifest(output_dir.join("run.json"))
      .expect("Minecraft bundle manifest should parse");
    assert_eq!(manifest.source_run.source_run_id, source_run_id.to_string());
    assert_eq!(manifest.counts.spatial_frames, 1);
    fs::remove_dir_all(&root).expect("remove Minecraft spatial-bundle fixture");
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
    let authority = InvokeFrontendAuthority {
      dispatch,
      store: store.clone(),
    };
    let invoked_call = call.clone();
    let execution = execute_invoke_frontend(&authority, move || invoked_call.call()).await.expect("persisted execution");

    assert_eq!(execution.direct_result, Ok(7));
    assert_eq!(execution.recording_failure, None);
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
    let authority = InvokeFrontendAuthority {
      dispatch,
      store: store.clone(),
    };

    let invoked_call = call.clone();
    let execution =
      execute_invoke_frontend(&authority, move || invoked_call.call()).await.expect("recording failure must preserve the direct result");

    assert_eq!(call.call_count(), 1);
    assert_eq!(execution.direct_result, Ok(7));
    assert_eq!(store.attempted_run_id(), Some(execution.run_id));
    assert!(execution.canonical_artifacts.is_empty());
    let failure = execution.recording_failure.expect("recording failure");
    assert!(failure.contains("instrumentation dispatch failure"), "unexpected failure: {failure}");
    assert_no_canonical_advice(&failure);
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
  fn inspect_server_target_uses_explicit_url() {
    let inspect = InspectClientOptions {
      server_url: Some("http://127.0.0.1:9876/".to_string()),
      ..InspectClientOptions::default()
    };

    let target = resolve_inspect_server_target(&inspect).expect("explicit target should resolve");

    assert_eq!(target, Some("http://127.0.0.1:9876/".to_string()));
  }

  #[tokio::test]
  async fn inspect_serve_adapter_uses_file_authority_and_v1_router() {
    let root = env::temp_dir().join(format!("auv-file-authority-adapter-{}", auv_runtime::model::now_millis()));
    let _ = fs::remove_dir_all(&root);
    let store = open_inspect_authority_store(&root).expect("file authority should open");
    let authority_id = store.authority_id();
    let app = auv_inspect_server::router_with_extension(store, Arc::new(crate::projection::ProductInspectReadProjection::default()));

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
}
