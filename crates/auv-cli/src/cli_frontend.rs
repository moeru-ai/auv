// Shared CLI frontend for root `auv` and donor bins (`auv-minecraft`, `auv-osu`, `auv-godot`).
//
// The root binary tombstones app-specific subcommands; dedicated app binaries
// own their live parse and dispatch paths. This product crate owns their shared
// frontend assembly.

use std::env;
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode};
use std::sync::Arc;

use crate::cli::{CliCommand, InspectClientOptions, help_text, parse_cli, parse_donor_cli, root_donor_tombstone, version_text};

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

pub(crate) async fn dispatch(command: CliCommand) -> Result<i32, String> {
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
      let inputs = crate::integrations::minecraft::projection_workflow::MinecraftProjectionBridgeInputs {
        telemetry_sample: PathBuf::from(telemetry_sample),
        screenshot: screenshot.map(PathBuf::from),
        capture_target_app,
        capture_target_title,
        target_block: parse_block_position(&target_block)?,
        capture_skew_ms,
        screenshot_is_minecraft_window,
      };
      let authority = build_cli_authority(&project_root, &inspect).await?;
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        crate::integrations::minecraft::projection_workflow::run_minecraft_projection_bridge(inputs)
      });
      let output = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: Minecraft projection bridge recording failure for run {run_id}: {failure}");
      }
      let output = output?;
      println!("runId: {run_id}");
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
      let authority = build_cli_authority(&project_root, &inspect).await?;
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        crate::integrations::minecraft::projection_workflow::run_minecraft_calibrate_projection(inputs)
      });
      let output = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: Minecraft projection calibration recording failure for run {run_id}: {failure}");
      }
      let output = output?;
      println!("runId: {run_id}");
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
      let authority = build_cli_authority(&project_root, &inspect).await?;
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        crate::integrations::minecraft::projection_workflow::run_minecraft_live_click(inputs)
      });
      let output = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: Minecraft live click recording failure for run {run_id}: {failure}");
      }
      let output = output?;
      println!("runId: {run_id}");
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
      let authority = build_cli_authority(&project_root, &inspect).await?;
      let store = authority.store.clone().ok_or_else(|| "Minecraft spatial bundle export requires an Inspect run authority".to_string())?;
      let export_run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(export_run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        crate::integrations::minecraft::run_minecraft_spatial_bundle_export(
          store,
          run_id,
          PathBuf::from(output_dir),
          crate::integrations::minecraft::current_git_commit(),
        )
      });
      let output = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: Minecraft spatial bundle export recording failure for run {export_run_id}: {failure}");
      }
      let output = output?;
      println!("runId: {export_run_id}");
      println!("status: completed");
      println!("sourceRunId: {}", output.manifest.source_run.run_id);
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
      let authority = build_cli_authority(&project_root, &inspect).await?;
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        crate::integrations::minecraft::run_minecraft_3dgs_scene_packet_export(
          bundle_manifest_paths.into_iter().map(PathBuf::from).collect(),
          PathBuf::from(output_dir),
        )
      });
      let output = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: Minecraft scene packet export recording failure for run {run_id}: {failure}");
      }
      let output = output?;
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
    CliCommand::MinecraftPrepareTextureSweep {
      sidecar_run_dir,
      output_dir,
      inspect,
    } => {
      let authority = build_cli_authority(&project_root, &inspect).await?;
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        crate::integrations::minecraft::run_minecraft_texture_sweep_preparation(PathBuf::from(sidecar_run_dir), PathBuf::from(output_dir))
      });
      let output = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: Minecraft texture sweep preparation recording failure for run {run_id}: {failure}");
      }
      let output = output?;
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
      let authority = build_cli_authority(&project_root, &inspect).await?;
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        crate::integrations::minecraft::run_minecraft_texture_sweep_sample_build(
          bundle_manifest_paths.into_iter().map(PathBuf::from).collect(),
          PathBuf::from(output_path),
        )
      });
      let output = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: Minecraft texture sweep sample build recording failure for run {run_id}: {failure}");
      }
      let output = output?;
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
      let authority = build_cli_authority(&project_root, &inspect).await?;
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        crate::integrations::minecraft::run_minecraft_texture_sweep_eval(
          PathBuf::from(samples_path),
          PathBuf::from(output_dir),
          require_real_source,
        )
      });
      let output = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: Minecraft texture sweep evaluation recording failure for run {run_id}: {failure}");
      }
      let output = output?;
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
      let authority = build_cli_authority(&project_root, &InspectClientOptions::default()).await?;
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        crate::integrations::osu::run_osu_benchmark(beatmap_path, output_dir)
      });
      let output = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: osu benchmark recording failure for run {run_id}: {failure}");
      }
      let output = output?;
      println!("runId: {run_id}");
      println!("status: completed");
      println!("beatmap: {}", output.map_summary.beatmap_path);
      println!("objects: {}", output.map_summary.total_objects);
      println!("latencyP95Ms: {}", output.benchmark_report.latency.p95_error_ms);
      println!("jitterMs: {}", output.benchmark_report.latency.jitter_ms);
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
      let authority = build_cli_authority(&project_root, &InspectClientOptions::default()).await?;
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        crate::integrations::osu::run_osu_benchmark_with_inputs(
          inputs,
          if capture_verify {
            "osu benchmark typed dispatch with capture verification"
          } else {
            "osu benchmark typed dispatch"
          },
        )
      });
      let output = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: osu benchmark dispatch recording failure for run {run_id}: {failure}");
      }
      let output = output?;
      println!("runId: {run_id}");
      println!("status: completed");
      println!("beatmap: {}", output.map_summary.beatmap_path);
      println!("objects: {}", output.map_summary.total_objects);
      println!("latencyP95Ms: {}", output.benchmark_report.latency.p95_error_ms);
      println!("jitterMs: {}", output.benchmark_report.latency.jitter_ms);
      if let Some(coverage) = &output.benchmark_report.capture_coverage {
        println!("captureCoveredActions: {}", coverage.captured_action_count);
        println!("captureMissingActions: {}", coverage.missing_action_count);
      }
      println!("output: {}", output.output_dir.display());
    }
    CliCommand::OsuExportDataset {
      run_artifact_dir,
      output_dir,
    } => {
      let authority = build_cli_authority(&project_root, &InspectClientOptions::default()).await?;
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        crate::integrations::osu::run_osu_dataset_export(PathBuf::from(run_artifact_dir), PathBuf::from(output_dir))
      });
      let output = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: osu dataset export recording failure for run {run_id}: {failure}");
      }
      let output = output?;
      println!("runId: {run_id}");
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
      let authority = build_cli_authority(&project_root, &InspectClientOptions::default()).await?;
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        crate::integrations::osu::run_osu_detection_eval(
          PathBuf::from(run_artifact_dir),
          PathBuf::from(detections_path),
          output_dir.map(PathBuf::from).unwrap_or_else(|| temp_runtime_store_root().join("osu-eval-detections-output")),
        )
      });
      let output = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: osu detection evaluation recording failure for run {run_id}: {failure}");
      }
      let output = output?;
      println!("runId: {run_id}");
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
      let authority = build_cli_authority(&project_root, &InspectClientOptions::default()).await?;
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        crate::integrations::osu::run_osu_vision_demo(
          PathBuf::from(beatmap_path),
          target_app,
          output_dir.map(PathBuf::from).unwrap_or_else(|| temp_runtime_store_root().join("osu-vision-demo-output")),
          dispatch_limit,
          capture_verify,
        )
      });
      let output = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: osu vision demo recording failure for run {run_id}: {failure}");
      }
      let output = output?;
      println!("runId: {run_id}");
      println!("status: completed");
      println!("beatmap: {}", output.map_summary.beatmap_path);
      println!("objects: {}", output.map_summary.total_objects);
      println!("latencyP95Ms: {}", output.benchmark_report.latency.p95_error_ms);
      println!("jitterMs: {}", output.benchmark_report.latency.jitter_ms);
      println!("dispatchSamples: {}", output.dispatch_samples.len());
      println!("captureSamples: {}", output.capture_samples.len());
      println!("hasProjectionArtifact: {}", output.projection.as_ref().is_some());
      println!("hasVisualTruthManifest: {}", output.visual_truth_manifest.as_ref().is_some());
      if let Some(coverage) = &output.benchmark_report.capture_coverage {
        println!("captureCoveredActions: {}", coverage.captured_action_count);
        println!("captureMissingActions: {}", coverage.missing_action_count);
      }
      println!("output: {}", output.output_dir.display());
    }
    CliCommand::Invoke {
      request,
      inspect,
      mut output,
    } => {
      let authority = build_cli_authority(&project_root, &inspect).await?;
      output.inspect_hint = authority.store.is_some();
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
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        invoked_command.invoke(input)
      });
      let direct_result = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: invoke recording failure for run {run_id}: {failure}");
      }
      let result = auv_cli_invoke::InvokeResult::from_command_result(run_id, &command, direct_result);
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

fn resolve_store_root(project_root: &Path, explicit: Option<&String>) -> PathBuf {
  explicit.map(PathBuf::from).unwrap_or_else(|| auv_runtime::default_project_store_root(project_root.to_path_buf()))
}

fn open_inspect_authority_store(store_root: &Path) -> Result<Arc<dyn auv_tracing::RunStore>, String> {
  auv_tracing::FileRunStore::open(store_root)
    .map(|store| Arc::new(store) as Arc<dyn auv_tracing::RunStore>)
    .map_err(|error| format!("failed to open Inspect run authority {}: {error}", store_root.display()))
}

#[derive(Clone)]
struct CliFrontendAuthority {
  dispatch: auv_tracing::Dispatch,
  store: Option<Arc<dyn auv_tracing::RunStore>>,
}

async fn build_cli_authority(project_root: &Path, inspect: &InspectClientOptions) -> Result<CliFrontendAuthority, String> {
  let server_target = if should_try_server_write(inspect) {
    if let Some(url) = resolve_inspect_server_target(inspect)? {
      Some(url)
    } else if inspect.require_server_write {
      return Err("inspect server write is required but no inspect server is configured".to_string());
    } else if server_write_explicitly_requested(inspect) {
      return Err("inspect server write was requested but no inspect server is configured".to_string());
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
        Err(error) if server_write_explicitly_requested(inspect) || !should_write_local(inspect) => {
          return Err(format!("failed to connect requested inspect authority {url}: {error}"));
        }
        Err(error) => {
          eprintln!("warning: failed to connect inspect authority {url}: {error}; using local tracing authority");
          None
        }
      }
    }
    None => None,
  };

  let store = match store {
    Some(store) => Some(store),
    None if should_write_local(inspect) => {
      Some(open_inspect_authority_store(&resolve_store_root(project_root, inspect.store_root.as_ref()))?)
    }
    None if no_store_requested(inspect) => None,
    None => return Err("invoke requires one configured V1 run authority unless local and server recording are both disabled".to_string()),
  };
  let dispatch = match &store {
    Some(store) => auv_tracing::configure().run_store(store.clone()).build(),
    None => auv_tracing::configure().build(),
  }
  .map_err(|error| format!("failed to configure invoke tracing: {error}"))?;
  Ok(CliFrontendAuthority { dispatch, store })
}

#[derive(serde::Serialize)]
struct InvokeFrontendLifecycle {
  frontend: &'static str,
}

impl auv_tracing::EventPayload for InvokeFrontendLifecycle {
  const NAME: &'static str = "auv.frontend.lifecycle";
  const VERSION: u32 = 1;
}

async fn flush_cli_recording(dispatch: &auv_tracing::Dispatch) -> Option<String> {
  dispatch.flush().await.err().map(|error| error.to_string())
}

fn should_write_local(inspect: &InspectClientOptions) -> bool {
  !matches!(inspect.local_write, crate::cli::InspectWriteSetting::Disabled)
}

fn should_try_server_write(inspect: &InspectClientOptions) -> bool {
  inspect.require_server_write || !matches!(inspect.server_write, crate::cli::InspectWriteSetting::Disabled)
}

fn server_write_explicitly_requested(inspect: &InspectClientOptions) -> bool {
  inspect.require_server_write
    || matches!(inspect.server_write, crate::cli::InspectWriteSetting::Enabled)
    || (inspect.server_url.is_some() && !matches!(inspect.server_write, crate::cli::InspectWriteSetting::Disabled))
}

fn no_store_requested(inspect: &InspectClientOptions) -> bool {
  !inspect.require_server_write
    && matches!(inspect.local_write, crate::cli::InspectWriteSetting::Disabled)
    && matches!(inspect.server_write, crate::cli::InspectWriteSetting::Disabled)
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
    assert_eq!(manifest.source_run.run_id, source_run_id.into());
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
    let authority = CliFrontendAuthority {
      dispatch,
      store: Some(store.clone()),
    };
    let invoked_call = call.clone();
    let run_id = RunId::new();
    let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
    let future = root.in_scope(|| {
      auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
      invoked_call.call()
    });
    let direct_result = root.instrument(future).await;
    let recording_failure = flush_cli_recording(&authority.dispatch).await;

    assert_eq!(direct_result, Ok(7));
    assert_eq!(recording_failure, None);
    assert_eq!(call.call_count(), 2);
    let snapshot = store.load_snapshot(run_id).await.expect("snapshot").expect("recorded run");
    assert_eq!(snapshot.run_id(), run_id);
    assert_eq!(snapshot.events().len(), 3);
  }

  #[tokio::test]
  async fn cli_authority_allows_no_store_when_all_recording_is_disabled() {
    let inspect = InspectClientOptions {
      local_write: crate::cli::InspectWriteSetting::Disabled,
      server_write: crate::cli::InspectWriteSetting::Disabled,
      ..InspectClientOptions::default()
    };

    let authority = build_cli_authority(Path::new(env!("CARGO_MANIFEST_DIR")), &inspect).await.expect("no-store dispatch");
    let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(RunId::new()));

    assert!(!root.is_enabled());
    assert!(!root.can_publish_artifacts());
  }

  #[test]
  fn no_store_requires_both_recording_paths_to_be_disabled() {
    let mut inspect = InspectClientOptions::default();
    assert!(!no_store_requested(&inspect));

    inspect.local_write = crate::cli::InspectWriteSetting::Disabled;
    assert!(!no_store_requested(&inspect));

    inspect.server_write = crate::cli::InspectWriteSetting::Disabled;
    assert!(no_store_requested(&inspect));
  }

  #[tokio::test]
  async fn explicit_inspect_server_failure_does_not_fall_back_to_local_storage() {
    let inspect = InspectClientOptions {
      server_write: crate::cli::InspectWriteSetting::Enabled,
      server_url: Some("http://127.0.0.1:1".to_string()),
      ..InspectClientOptions::default()
    };

    let Err(error) = build_cli_authority(Path::new(env!("CARGO_MANIFEST_DIR")), &inspect).await else {
      panic!("explicit inspect authority failure must not fall back to local storage")
    };

    assert!(error.contains("failed to connect requested inspect authority"), "unexpected error: {error}");
  }

  #[tokio::test]
  async fn cli_commit_unknown_preserves_direct_result_without_retry_or_canonical_advice() {
    let call = CountingCall::default();
    let store = Arc::new(CommitUnknownStore::new());
    let dispatch = auv_tracing::configure().run_store(store.clone()).build().expect("dispatch");
    let authority = CliFrontendAuthority {
      dispatch,
      store: Some(store.clone()),
    };

    let invoked_call = call.clone();
    let run_id = RunId::new();
    let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
    let future = root.in_scope(|| {
      auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
      invoked_call.call()
    });
    let direct_result = root.instrument(future).await;
    let recording_failure = flush_cli_recording(&authority.dispatch).await;

    assert_eq!(call.call_count(), 1);
    assert_eq!(direct_result, Ok(7));
    assert_eq!(store.attempted_run_id(), Some(run_id));
    let failure = recording_failure.expect("recording failure");
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
}
