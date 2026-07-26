// Shared CLI frontend for root `auv` and donor bins (`auv-minecraft`, `auv-osu`, `auv-godot`).
//
// The root binary tombstones app-specific subcommands; dedicated app binaries
// own their live parse and dispatch paths. This product crate owns their shared
// frontend assembly.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode};
use std::sync::Arc;

use crate::cli::{CliCommand, TracingOptions, help_text, parse_cli, parse_donor_cli, root_donor_tombstone, version_text};

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
      tracing,
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
      let authority = build_cli_tracing(&project_root, &tracing)?;
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
      tracing,
    } => {
      let inputs = crate::integrations::minecraft::projection_workflow::MinecraftProjectionCalibrationInputs {
        frame_path: PathBuf::from(frame_path),
        screenshot: PathBuf::from(screenshot),
        target_block: parse_block_position(&target_block)?,
        target_semantics: parse_target_semantics(&target_semantics)?,
        screenshot_is_minecraft_window,
      };
      let authority = build_cli_tracing(&project_root, &tracing)?;
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
      tracing,
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
      let authority = build_cli_tracing(&project_root, &tracing)?;
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
    CliCommand::MinecraftExport3dgsScenePacket {
      bundle_manifest_paths,
      output_dir,
      tracing,
    } => {
      let authority = build_cli_tracing(&project_root, &tracing)?;
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
      tracing,
    } => {
      let authority = build_cli_tracing(&project_root, &tracing)?;
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
      tracing,
    } => {
      let authority = build_cli_tracing(&project_root, &tracing)?;
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
      tracing,
    } => {
      let authority = build_cli_tracing(&project_root, &tracing)?;
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
      let authority = build_cli_tracing(&project_root, &TracingOptions::default())?;
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
      let authority = build_cli_tracing(&project_root, &TracingOptions::default())?;
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
      let authority = build_cli_tracing(&project_root, &TracingOptions::default())?;
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
      let authority = build_cli_tracing(&project_root, &TracingOptions::default())?;
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
      let authority = build_cli_tracing(&project_root, &TracingOptions::default())?;
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
      tracing,
      output,
    } => {
      let authority = build_cli_tracing(&project_root, &tracing)?;
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

#[derive(Clone)]
struct CliTracing {
  dispatch: auv_tracing::Dispatch,
}

fn build_cli_tracing(project_root: &Path, options: &TracingOptions) -> Result<CliTracing, String> {
  let store_root = resolve_store_root(project_root, options.store_root.as_ref());
  let store = auv_tracing::FileTracingStore::open(&store_root)
    .map(|store| Arc::new(store) as Arc<dyn auv_tracing::TracingStore>)
    .map_err(|error| format!("failed to open tracing store {}: {error}", store_root.display()))?;
  let dispatch =
    auv_tracing::configure().tracing_store(store).build().map_err(|error| format!("failed to configure invoke tracing: {error}"))?;
  Ok(CliTracing { dispatch })
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

fn temp_runtime_store_root() -> PathBuf {
  env::temp_dir().join(format!("auv-runtime-store-{}-{}", process::id(), auv_runtime::model::now_millis()))
}
