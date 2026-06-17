// File: src/main.rs
mod cli;
mod xtask;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;

use image::ImageReader;

use auv_cli::app::{analyze_app_probe, probe_app};
use auv_cli::contract::{
  OPERATION_RESULT_API_VERSION, OperationOutput, OperationResult, OperationStatus,
  VerificationResult,
};
use auv_cli::model::{InvokeRequest, RunStatus};
use auv_cli::scroll_scan::{
  ScanRegion, ScanTarget, ScanWindowRegionOptions, StopPolicy, scan_window_region,
};
use auv_cli::{build_default_runtime, build_runtime_with_store_root};
use auv_tracing_driver::run_builder::RunSpec;
use cli::{CliCommand, InspectClientOptions, help_text, parse_cli};

#[tokio::main]
async fn main() {
  if let Err(error) = run().await {
    eprintln!("error: {error}");
    process::exit(1);
  }
}

async fn run() -> Result<(), String> {
  let arguments = env::args().skip(1).collect::<Vec<_>>();
  let command = parse_cli(&arguments)?;
  let project_root =
    env::current_dir().map_err(|error| format!("failed to resolve current directory: {error}"))?;
  if let CliCommand::XtaskGenerateSwiftBridge = &command {
    let outputs = xtask::generate_swift_bridge_for_ide(&project_root)?;
    println!("generated Swift bridge files for IDE indexing");
    for output in outputs {
      println!("output: {output}");
    }
    return Ok(());
  }

  if let CliCommand::McpServe = &command {
    auv_cli::mcp::serve_stdio(project_root.clone()).await?;
    return Ok(());
  }

  if let CliCommand::PermissionCheck { json } = &command {
    return run_permission_check(*json);
  }

  if let CliCommand::InspectServe {
    host,
    port,
    store_root,
    write,
  } = &command
  {
    let store_root = resolve_store_root(&project_root, store_root.as_ref());
    let store = auv_tracing_driver::store::LocalStore::new(store_root.clone())?;
    let recorder = Arc::new(auv_tracing_driver::BroadcastRunRecorder::new(1024));
    let token = resolve_inspect_serve_write_token(write)?;
    let config = auv_cli::inspect_server::InspectServeConfig {
      host: host.clone(),
      port: *port,
      store_root: Some(store_root.clone()),
      write: auv_cli::inspect_server::InspectWriteConfig {
        enabled: write.enabled || token.is_some(),
        token,
        no_token: write.no_token,
      },
    };
    auv_cli::inspect_server::serve(store, recorder, config).await?;
    return Ok(());
  }

  match command {
    CliCommand::Help => {
      print!("{}", help_text());
    }
    CliCommand::PermissionCheck { .. } => {
      unreachable!("permission check is handled before runtime setup")
    }
    CliCommand::CandidateActionRun { request, inspect } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = runtime.run_candidate_action_command(
        auv_cli::candidate_action_command::CandidateActionCommandRequest {
          app_bundle_id: request.app_bundle_id,
          query: request.query,
          role: request.role,
          action: request.action,
          intent: request.intent,
          proposer_model: request.proposer_model,
          proposer_base_url: request.proposer_base_url,
          reveal_shortcut: request.reveal_shortcut,
          reveal_settle_ms: request.reveal_settle_ms,
          stable_frames: request.stable_frames,
          stable_frame_delay_ms: request.stable_frame_delay_ms,
          max_centroid_drift_px: request.max_centroid_drift_px,
          require_stable_text: request.require_stable_text,
          dev_self_minted_consent: request.dev_self_minted_consent,
          human_gesture_consent: request.human_gesture_consent,
          human_gesture_timeout_ms: request.human_gesture_timeout_ms,
          proposal_id: request.proposal_id,
          promotion_id: request.promotion_id,
          decision_id: request.decision_id,
          execution_id: request.execution_id,
          granted_by: request.granted_by,
          promotion_scope_note: request.promotion_scope_note,
          promotion_evidence_note: request.promotion_evidence_note,
          execution_scope_note: request.execution_scope_note,
          execution_evidence_note: request.execution_evidence_note,
        },
      )?;
      println!("runId: {}", output.run_id);
      println!("status: {}", output.value.status.as_str());
      if let Some(proposal_artifact_id) = output.value.proposal_artifact_id.as_deref() {
        println!("proposalArtifact: {proposal_artifact_id}");
      }
      println!("promotionArtifact: {}", output.value.promotion_artifact_id);
      if let Some(decision_artifact_id) = output.value.decision_artifact_id.as_deref() {
        println!("decisionArtifact: {decision_artifact_id}");
      }
      if let Some(execution_artifact_id) = output.value.execution_artifact_id.as_deref() {
        println!("executionArtifact: {execution_artifact_id}");
      }
      if !output.value.promotion_refusals.is_empty() {
        println!(
          "promotionRefusals: {}",
          output.value.promotion_refusals.join(",")
        );
      }
    }
    CliCommand::MinecraftProjectionBridge {
      telemetry_sample,
      screenshot,
      target_block,
      capture_skew_ms,
      screenshot_is_minecraft_window,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let output = run_minecraft_projection_bridge(
        &runtime,
        PathBuf::from(telemetry_sample),
        PathBuf::from(screenshot),
        &target_block,
        capture_skew_ms,
        screenshot_is_minecraft_window,
      )?;
      println!("runId: {}", output.run_id);
      println!(
        "projectionArtifact: {}",
        output.value.projection_artifact_id
      );
      println!(
        "screenshotArtifact: {}",
        output.value.screenshot_artifact_id
      );
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
      println!(
        "projectionArtifact: {}",
        output.value.projection_artifact_id
      );
      println!(
        "screenshotArtifact: {}",
        output.value.screenshot_artifact_id
      );
      println!(
        "operationResultArtifact: {}",
        output.value.operation_result_artifact_id
      );
      println!("inputSummary: {}", output.value.input_summary);
      for artifact in &output.value.artifact_paths {
        println!("artifact: {}", artifact.display());
      }
    }
    CliCommand::XtaskGenerateSwiftBridge => unreachable!("xtask is handled before runtime setup"),
    CliCommand::ListCommandsTombstone => {
      return Err(
        "`list-commands` has been removed; use `auv-cli invoke --help` instead".to_string(),
      );
    }
    CliCommand::InvokeHelp { command_id } => {
      let registry = auv_cli_invoke::default_registry();
      if let Some(command_id) = command_id {
        let command = registry.resolve(&command_id).ok_or_else(|| {
          format!(
            "unknown command {command_id}; use `auv-cli invoke --help` to inspect available entries"
          )
        })?;
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
      let probe = probe_app(
        &project_root,
        &runtime,
        &bundle_id,
        output_dir.map(PathBuf::from),
      )?;
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
      println!(
        "annotations: {}",
        output.analysis.annotation_candidates.len()
      );
    }
    CliCommand::OsuBenchmark {
      beatmap_path,
      output_dir,
    } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let recording = runtime.recording().handle();
      let beatmap_path = PathBuf::from(beatmap_path);
      let output_dir = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| temp_runtime_store_root().join("osu-benchmark-output"));
      let output = auv_cli::osu::run_osu_benchmark(&recording, beatmap_path, output_dir)?;
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
      let output_dir = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| temp_runtime_store_root().join("osu-dispatch-output"));
      let mut inputs =
        auv_game_osu::BenchmarkInputs::typed_dispatch(beatmap_path, output_dir, target_app);
      if let Some(dispatch_limit) = dispatch_limit {
        inputs.dispatch_limit = Some(dispatch_limit);
      }
      inputs.capture_verify = capture_verify;
      let output = auv_cli::osu::run_osu_benchmark_with_inputs(
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
        println!(
          "verificationCapturedActions: {}",
          summary.captured_action_count
        );
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
      let output = auv_cli::osu::run_osu_dataset_export(
        &recording,
        PathBuf::from(run_artifact_dir),
        PathBuf::from(output_dir),
      )?;
      println!("runId: {}", output.run_id);
      println!("status: completed");
      println!(
        "exportedFrames: {}",
        output.value.dataset_manifest.exported_frames.len()
      );
      println!(
        "skippedFrames: {}",
        output.value.dataset_manifest.skipped_frames.len()
      );
      println!("output: {}", output.value.output_dir.display());
    }
    CliCommand::OsuEvalDetections {
      run_artifact_dir,
      detections_path,
      output_dir,
    } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let recording = runtime.recording().handle();
      let output = auv_cli::osu::run_osu_detection_eval(
        &recording,
        PathBuf::from(run_artifact_dir),
        PathBuf::from(detections_path),
        output_dir
          .map(PathBuf::from)
          .unwrap_or_else(|| temp_runtime_store_root().join("osu-eval-detections-output")),
      )?;
      println!("runId: {}", output.run_id);
      println!("status: completed");
      println!(
        "totalFrames: {}",
        output.value.visual_eval_report.total_frames
      );
      println!(
        "labelMatchedFrames: {}",
        output.value.visual_eval_report.label_matched_frames
      );
      println!(
        "spatialMatchedFrames: {}",
        output.value.visual_eval_report.spatial_matched_frames
      );
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
      let output = auv_cli::osu::run_osu_vision_demo(
        &recording,
        PathBuf::from(beatmap_path),
        target_app,
        output_dir
          .map(PathBuf::from)
          .unwrap_or_else(|| temp_runtime_store_root().join("osu-vision-demo-output")),
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
      println!(
        "hasEvidenceArtifact: {}",
        output
          .value
          .output_dir
          .join("evidence_summary.json")
          .exists()
      );
      println!(
        "hasProjectionArtifact: {}",
        output.value.projection.as_ref().is_some()
      );
      println!(
        "hasVisualTruthManifest: {}",
        output.value.visual_truth_manifest.as_ref().is_some()
      );
      if let Some(summary) = &output.value.verification_summary {
        println!(
          "verificationCapturedActions: {}",
          summary.captured_action_count
        );
        println!("verificationMissingFrames: {}", summary.missing_frame_count);
      }
      println!("output: {}", output.value.output_dir.display());
    }
    CliCommand::Invoke { request, inspect } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let result = runtime.invoke(request)?;
      println!("runId: {}", result.run_id);
      println!("status: {}", result.status.as_str());
      println!("output: {}", result.output_summary);
      for artifact in &result.artifact_paths {
        println!("artifact: {}", artifact.display());
      }

      if let Some(failure) = &result.failure_message {
        return Err(format!(
          "{} (inspect with `auv-cli inspect {}`)",
          failure, result.run_id
        ));
      }

      if result.status == RunStatus::Failed {
        return Err(format!("run {} finished in failed state", result.run_id));
      }
    }
    CliCommand::Inspect { run_id } => {
      let runtime = build_default_runtime(project_root.clone())?;
      print!("{}", runtime.inspect(&run_id)?);
    }
    CliCommand::InspectServe { .. } => {
      unreachable!("inspect serve is handled before runtime setup")
    }
    CliCommand::McpServe => {
      unreachable!("mcp serve is handled before runtime setup")
    }
    CliCommand::ScanWindowRegion {
      target,
      region,
      max_pages,
      max_scrolls,
      direction,
      scroll_amount,
      settle_ms,
      min_confidence,
      max_observations,
    } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let region = parse_scan_region_arg(&region)?;
      let run_id = scan_window_region(
        &runtime,
        ScanWindowRegionOptions {
          target: ScanTarget {
            application_id: Some(target),
            window_title: None,
            region,
          },
          stop_policy: StopPolicy::UntilEnd {
            max_pages,
            max_scrolls,
            no_progress_limit: 2,
          },
          direction,
          scroll_amount,
          settle_ms,
          min_confidence,
          max_observations,
        },
      )?;
      println!("runId: {run_id}");
      println!("status: scanned");
    }
  }

  Ok(())
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
struct MinecraftLiveClickOutput {
  screenshot_artifact_id: String,
  projection_artifact_id: String,
  operation_result_artifact_id: String,
  input_summary: String,
  artifact_paths: Vec<PathBuf>,
}

fn build_minecraft_world_diff_verification(
  verdict: &auv_game_minecraft::verify::WorldDiffVerdict,
  evidence: Vec<auv_cli::contract::ArtifactRef>,
) -> auv_cli::contract::VerificationResult {
  use auv_cli::contract::{
    FailureLayer, VERIFICATION_RESULT_API_VERSION, VerificationMethod, VerificationResult,
  };

  let failure_layer = match verdict.failure {
    None => None,
    Some(auv_game_minecraft::verify::WorldDiffFailure::VerificationUnreliable) => {
      Some(FailureLayer::VerificationUnreliable)
    }
    Some(auv_game_minecraft::verify::WorldDiffFailure::StateChangedNoMatch) => {
      Some(FailureLayer::StateChangedNoMatch)
    }
    Some(auv_game_minecraft::verify::WorldDiffFailure::SemanticMismatch) => {
      Some(FailureLayer::SemanticMismatch)
    }
  };

  VerificationResult {
    api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
    method: VerificationMethod::SemanticMatch,
    executed: verdict.executed,
    state_changed: verdict.state_changed,
    semantic_matched: verdict.semantic_matched,
    failure_layer,
    evidence,
    consumed_candidate_ref: None,
    consumed_node_ref: None,
    consumed_recognition_artifact_ref: None,
    consumed_recognition_id: None,
    consumed_recognized_item_id: None,
    observed_label: verdict.observed_block_id.clone(),
  }
}

fn build_minecraft_operation_result(
  run_id: &auv_tracing_driver::trace::RunId,
  verification: VerificationResult,
) -> OperationResult {
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
) -> Result<(PathBuf, auv_cli::contract::ArtifactRef), String> {
  let artifact_json = serde_json::to_string_pretty(operation_result)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| format!("failed to serialize minecraft operation result: {error}"))?;
  let artifact_path = std::env::temp_dir().join(format!(
    "auv-minecraft-operation-result-{}-{}.json",
    context.run_id(),
    auv_cli::model::now_millis()
  ));
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
  runtime: &auv_cli::runtime::Runtime,
  telemetry_sample: PathBuf,
  post_telemetry_sample: Option<PathBuf>,
  screenshot: PathBuf,
  target_block: &str,
  target_app: &str,
  target_title: &str,
  capture_skew_ms: Option<i64>,
  screenshot_is_minecraft_window: bool,
) -> Result<
  auv_tracing_driver::recorded_operation::RecordedOperationOutput<MinecraftLiveClickOutput>,
  String,
> {
  let target_block = parse_block_position(target_block)?;
  let pre_frame = auv_game_minecraft::read_latest_spatial_frame_from_tail(&telemetry_sample)?
    .ok_or_else(|| {
      format!(
        "no valid minecraft frame found in {}",
        telemetry_sample.display()
      )
    })?;
  let post_sample_path = post_telemetry_sample.unwrap_or_else(|| telemetry_sample.clone());
  let screenshot_image = ImageReader::open(&screenshot)
    .map_err(|error| {
      format!(
        "failed to open screenshot {}: {error}",
        screenshot.display()
      )
    })?
    .decode()
    .map_err(|error| {
      format!(
        "failed to decode screenshot {}: {error}",
        screenshot.display()
      )
    })?
    .to_rgb8();

  runtime.run_recorded_operation(
    RunSpec::new(
      auv_tracing_driver::trace::RunType::Execute,
      "auv.minecraft.live_click",
    ),
    "Minecraft live click",
    |context| {
      let (staged_screenshot_path, screenshot_ref) = context.stage_artifact_file_with_ref(
        "minecraft-screenshot",
        &screenshot,
        screenshot
          .file_name()
          .and_then(|name| name.to_str())
          .unwrap_or("minecraft-screenshot.png"),
        Some("minecraft screenshot bound to live telemetry frame".to_string()),
      )?;
      let screenshot_artifact_id = screenshot_ref.artifact_id.as_str().to_string();
      let capture_timestamp_ms = if let Some(skew) = capture_skew_ms {
        if skew >= 0 {
          pre_frame.monotonic_timestamp_ms.saturating_sub(skew as u64)
        } else {
          pre_frame
            .monotonic_timestamp_ms
            .saturating_add((-skew) as u64)
        }
      } else {
        pre_frame.monotonic_timestamp_ms
      };

      let evidence = auv_game_minecraft::evidence::build_projection_evidence(
        pre_frame.clone(),
        auv_game_minecraft::evidence::ScreenshotCapture {
          image: screenshot_image,
          artifact_ref: format!("artifact://{screenshot_artifact_id}"),
          capture_monotonic_timestamp_ms: capture_timestamp_ms,
          is_minecraft_window: screenshot_is_minecraft_window,
        },
        &auv_game_minecraft::MinecraftBlockTarget::new(target_block),
        Some(250),
      )?;

      let projection_artifact = match &evidence {
        auv_game_minecraft::evidence::ProjectionEvidence::Bound { artifact, .. } => {
          artifact.clone()
        }
        auv_game_minecraft::evidence::ProjectionEvidence::Refused { refusal, .. } => evidence
          .artifact()
          .clone()
          .with_mismatch_refusal_reason(refusal.reason),
      };
      let (staged_projection_path, projection_ref) =
        stage_minecraft_projection_artifact(context, &projection_artifact)?;
      let projection_artifact_id = projection_ref.artifact_id.as_str().to_string();

      let projected_point = match &evidence {
        auv_game_minecraft::evidence::ProjectionEvidence::Bound { artifact, .. } => {
          artifact.projected_point.clone().ok_or_else(|| {
            "minecraft projection evidence is bound but missing projected point".to_string()
          })?
        }
        auv_game_minecraft::evidence::ProjectionEvidence::Refused { refusal, .. } => {
          return Err(format!(
            "minecraft live click refused before input dispatch: {:?}",
            refusal.reason
          ));
        }
      };

      let window_point = auv_game_minecraft::input_target::projected_window_point(&projected_point)
        .ok_or_else(|| "projected minecraft point is not window-clickable".to_string())?;

      let mut inputs = std::collections::BTreeMap::new();
      inputs.insert("title".to_string(), target_title.to_string());
      inputs.insert("offset_x".to_string(), format!("{:.3}", window_point.0.x));
      inputs.insert("offset_y".to_string(), format!("{:.3}", window_point.0.y));

      let invoke_result = runtime.invoke_resolved(
        InvokeRequest {
          command_id: "input.clickWindowPoint".to_string(),
          target: auv_cli::model::ExecutionTarget {
            application_id: Some(target_app.to_string()),
            target_label: None,
          },
          inputs,
          dry_run: false,
        },
        auv_cli_invoke::default_registry()
          .resolve("input.clickWindowPoint")
          .ok_or_else(|| "input.clickWindowPoint command is not registered".to_string())?,
      )?;
      let post_frame = auv_game_minecraft::read_latest_spatial_frame_from_tail(&post_sample_path)?
        .ok_or_else(|| {
          format!(
            "no valid minecraft post frame found in {}",
            post_sample_path.display()
          )
        })?;

      let world_diff_request = auv_game_minecraft::verify::WorldDiffRequest::new(
        auv_game_minecraft::MinecraftBlockTarget::new(target_block),
      )
      .allow_same_block_state_change();
      let verification = build_minecraft_world_diff_verification(
        &auv_game_minecraft::verify::evaluate_world_diff(
          &pre_frame,
          &post_frame,
          &world_diff_request,
        ),
        vec![screenshot_ref.clone(), projection_ref.clone()],
      );
      let operation_result = build_minecraft_operation_result(context.run_id(), verification);
      let (staged_operation_result_path, operation_result_ref) =
        stage_operation_result_artifact(context, &operation_result)?;

      Ok::<MinecraftLiveClickOutput, String>(MinecraftLiveClickOutput {
        screenshot_artifact_id,
        projection_artifact_id,
        operation_result_artifact_id: operation_result_ref.artifact_id.as_str().to_string(),
        input_summary: invoke_result.output_summary,
        artifact_paths: vec![
          staged_screenshot_path,
          staged_projection_path,
          staged_operation_result_path,
        ],
      })
    },
  )
}

fn run_minecraft_projection_bridge(
  runtime: &auv_cli::runtime::Runtime,
  telemetry_sample: PathBuf,
  screenshot: PathBuf,
  target_block: &str,
  capture_skew_ms: Option<i64>,
  screenshot_is_minecraft_window: bool,
) -> Result<
  auv_tracing_driver::recorded_operation::RecordedOperationOutput<MinecraftBridgeOutput>,
  String,
> {
  let target_block = parse_block_position(target_block)?;
  let frame = auv_game_minecraft::read_latest_spatial_frame_from_tail(&telemetry_sample)?
    .ok_or_else(|| {
      format!(
        "no valid minecraft frame found in {}",
        telemetry_sample.display()
      )
    })?;

  let screenshot_image = ImageReader::open(&screenshot)
    .map_err(|error| {
      format!(
        "failed to open screenshot {}: {error}",
        screenshot.display()
      )
    })?
    .decode()
    .map_err(|error| {
      format!(
        "failed to decode screenshot {}: {error}",
        screenshot.display()
      )
    })?
    .to_rgb8();

  runtime.run_recorded_operation(
    auv_tracing_driver::run_builder::RunSpec::new(
      auv_tracing_driver::trace::RunType::Execute,
      "auv.minecraft.bridge",
    ),
    "Minecraft projection bridge",
    |context| {
      let (staged_screenshot_path, screenshot_ref) = context.stage_artifact_file_with_ref(
        "minecraft-screenshot",
        &screenshot,
        screenshot
          .file_name()
          .and_then(|name| name.to_str())
          .unwrap_or("minecraft-screenshot.png"),
        Some("minecraft screenshot bound to live telemetry frame".to_string()),
      )?;
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

      let evidence = auv_game_minecraft::evidence::build_projection_evidence(
        frame,
        auv_game_minecraft::evidence::ScreenshotCapture {
          image: screenshot_image,
          artifact_ref: format!("artifact://{screenshot_artifact_id}"),
          capture_monotonic_timestamp_ms: capture_timestamp_ms,
          is_minecraft_window: screenshot_is_minecraft_window,
        },
        &auv_game_minecraft::MinecraftBlockTarget::new(target_block),
        Some(250),
      )?;

      let projection_artifact = match &evidence {
        auv_game_minecraft::evidence::ProjectionEvidence::Bound { artifact, .. } => {
          artifact.clone()
        }
        auv_game_minecraft::evidence::ProjectionEvidence::Refused { refusal, .. } => evidence
          .artifact()
          .clone()
          .with_mismatch_refusal_reason(refusal.reason),
      };
      let (staged_projection_path, projection_ref) =
        stage_minecraft_projection_artifact(context, &projection_artifact)?;
      let projection_artifact_id = projection_ref.artifact_id.as_str().to_string();
      let mut artifact_paths = vec![staged_screenshot_path, staged_projection_path];
      let mut overlay_artifact_id = None;
      let mut refusal_reason = None;

      if let auv_game_minecraft::evidence::ProjectionEvidence::Bound { overlay, .. } = evidence {
        let overlay_path = std::env::temp_dir().join(format!(
          "auv-minecraft-overlay-{}-{}.png",
          context.run_id(),
          auv_cli::model::now_millis()
        ));
        overlay
          .save(&overlay_path)
          .map_err(|error| format!("failed to save overlay image: {error}"))?;
        let (staged_overlay_path, overlay_ref) = context.stage_artifact_file_with_ref(
          "minecraft-overlay",
          &overlay_path,
          "minecraft-overlay.png",
          Some("projected minecraft overlay on real screenshot".to_string()),
        )?;
        let _ = fs::remove_file(&overlay_path);
        overlay_artifact_id = Some(overlay_ref.artifact_id.as_str().to_string());
        artifact_paths.push(staged_overlay_path);
      } else if let auv_game_minecraft::evidence::ProjectionEvidence::Refused { refusal, .. } =
        evidence
      {
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

fn stage_minecraft_projection_artifact(
  context: &mut auv_tracing_driver::recorded_operation::RecordedOperationContext<'_>,
  projection_artifact: &auv_game_minecraft::MinecraftProjectionArtifact,
) -> Result<(PathBuf, auv_cli::contract::ArtifactRef), String> {
  projection_artifact.validate()?;
  let artifact_json = serde_json::to_string_pretty(projection_artifact)
    .map_err(|error| format!("failed to serialize minecraft projection artifact: {error}"))?;
  let artifact_path = std::env::temp_dir().join(format!(
    "auv-minecraft-projection-{}-{}.json",
    context.run_id(),
    auv_cli::model::now_millis()
  ));
  fs::write(&artifact_path, artifact_json.as_bytes())
    .map_err(|error| format!("failed to write minecraft projection artifact: {error}"))?;
  let staged = context.stage_artifact_file_with_ref(
    auv_cli::runtime::MINECRAFT_PROJECTION_ARTIFACT_ROLE,
    &artifact_path,
    "projection-artifact.json",
    Some("durable minecraft projection artifact".to_string()),
  );
  let _ = fs::remove_file(&artifact_path);
  staged.map_err(|error| error.to_string())
}

fn parse_block_position(raw: &str) -> Result<auv_game_minecraft::BlockPosition, String> {
  let parts = raw.split(',').map(str::trim).collect::<Vec<_>>();
  if parts.len() != 3 {
    return Err(format!("invalid --target-block {raw:?}; expected x,y,z"));
  }
  let x = parts[0]
    .parse::<i32>()
    .map_err(|error| format!("invalid target block x: {error}"))?;
  let y = parts[1]
    .parse::<i32>()
    .map_err(|error| format!("invalid target block y: {error}"))?;
  let z = parts[2]
    .parse::<i32>()
    .map_err(|error| format!("invalid target block z: {error}"))?;
  Ok(auv_game_minecraft::BlockPosition::new(x, y, z))
}

fn parse_scan_region_arg(raw: &str) -> Result<ScanRegion, String> {
  let values = raw
    .split(',')
    .map(|value| value.trim().parse::<f64>())
    .collect::<Result<Vec<_>, _>>()
    .map_err(|error| format!("invalid --region ratios: {error}"))?;
  if values.len() != 4 {
    return Err("--region must contain four comma-separated ratios".to_string());
  }
  Ok(ScanRegion {
    left_ratio: values[0],
    top_ratio: values[1],
    right_ratio: values[2],
    bottom_ratio: values[3],
  })
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
    println!(
      "{}",
      serde_json::to_string_pretty(&report)
        .map_err(|error| format!("failed to encode permission report: {error}"))?
    );
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
    executable: env::current_exe()
      .ok()
      .map(|path| path.display().to_string()),
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
    ("granted", "granted") => {
      "AUV has the macOS permissions needed for capture and AX-backed automation.".to_string()
    }
    ("missing", "missing") => {
      "Grant Accessibility and Screen Recording to the terminal or app that launches auv-cli, then rerun this check."
        .to_string()
    }
    ("missing", _) => {
      "Grant Accessibility to the terminal or app that launches auv-cli, then rerun this check."
        .to_string()
    }
    (_, "missing") => {
      "Grant Screen Recording to the terminal or app that launches auv-cli, then rerun this check."
        .to_string()
    }
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
  println!(
    "accessibility: {}",
    permission_status_line(report.accessibility)
  );
  println!(
    "screen recording preflight: {}",
    permission_status_line(report.screen_recording_preflight)
  );
  println!(
    "screen capture kit probe: {}",
    permission_status_line(report.screen_capture_kit)
  );
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
  explicit
    .map(PathBuf::from)
    .unwrap_or_else(|| auv_cli::default_project_store_root(project_root.to_path_buf()))
}

fn resolve_inspect_serve_write_token(
  write: &cli::InspectServeWriteOptions,
) -> Result<Option<String>, String> {
  if write.token.is_some() && write.token_file.is_some() {
    return Err("--write-token cannot be combined with --write-token-file".to_string());
  }

  if let Some(token) = &write.token {
    return normalize_write_token("--write-token", token.clone()).map(Some);
  }

  if let Some(path) = &write.token_file {
    let token = std::fs::read_to_string(path)
      .map_err(|error| format!("failed to read write token file {path}: {error}"))?
      .trim()
      .to_string();
    return normalize_write_token("--write-token-file", token).map(Some);
  }

  if write.enabled && !write.no_token {
    let token = format!(
      "session-{}-{}",
      std::process::id(),
      auv_cli::model::now_millis()
    );
    return normalize_write_token("generated write token", token).map(Some);
  }

  Ok(None)
}

fn normalize_write_token(source: &str, token: String) -> Result<String, String> {
  if token.trim().is_empty() {
    Err(format!("{source} resolved to an empty write token"))
  } else {
    Ok(token)
  }
}

fn build_runtime_for_inspect(
  project_root: &Path,
  inspect: &InspectClientOptions,
) -> Result<auv_cli::runtime::Runtime, String> {
  let server_target = if should_try_server_write(inspect) {
    if let Some((url, token)) = resolve_inspect_server_target(inspect)? {
      Some((url, token))
    } else if inspect.require_server_write {
      return Err(
        "inspect server write is required but no inspect server is configured".to_string(),
      );
    } else if matches!(inspect.server_write, cli::InspectWriteSetting::Enabled) {
      eprintln!("warning: inspect server write requested but no inspect server is configured");
      None
    } else {
      None
    }
  } else {
    None
  };

  let local_write_enabled = should_write_local(inspect);
  let store_root = if local_write_enabled {
    resolve_store_root(project_root, inspect.store_root.as_ref())
  } else {
    temp_runtime_store_root()
  };
  let store = auv_tracing_driver::store::LocalStore::new(store_root.clone())?;
  let mut recorders: Vec<Arc<dyn auv_tracing_driver::RunRecorder>> = Vec::new();

  if let Some((url, token)) = server_target {
    recorders.push(Arc::new(auv_tracing_driver::InspectServerRunRecorder::new(
      url,
      token,
      inspect.require_server_write,
    )));
  }

  let recorder: Arc<dyn auv_tracing_driver::RunRecorder> = match recorders.len() {
    0 => Arc::new(auv_tracing_driver::NoopRunRecorder),
    1 => recorders.remove(0),
    _ => Arc::new(auv_tracing_driver::CompositeRunRecorder::new(recorders)),
  };
  let recording = auv_tracing_driver::RunRecordingBackend::new(store, recorder)
    .with_local_snapshot_write_enabled(local_write_enabled)
    .with_temporary_store_cleanup(!local_write_enabled);
  Ok(
    build_runtime_with_store_root(project_root.to_path_buf(), store_root)?
      .with_recording(recording),
  )
}

fn should_write_local(inspect: &InspectClientOptions) -> bool {
  !matches!(inspect.local_write, cli::InspectWriteSetting::Disabled)
}

fn should_try_server_write(inspect: &InspectClientOptions) -> bool {
  inspect.require_server_write
    || !matches!(inspect.server_write, cli::InspectWriteSetting::Disabled)
}

fn resolve_inspect_server_target(
  inspect: &InspectClientOptions,
) -> Result<Option<(String, Option<String>)>, String> {
  let explicit_token = resolve_client_token(inspect)?;
  if let Some(url) = &inspect.server_url {
    return Ok(Some((url.clone(), explicit_token)));
  }
  let Some(session) = read_discovered_inspect_session(inspect)? else {
    return Ok(None);
  };
  if !session.write_enabled {
    return Ok(None);
  }
  if !is_local_inspect_url(&session.url) {
    if inspect.require_server_write {
      return Err(format!(
        "inspect server write is required but discovered inspect server URL is not local: {}",
        session.url
      ));
    }
    eprintln!(
      "warning: ignoring discovered inspect server with non-local URL: {}",
      session.url
    );
    return Ok(None);
  }
  Ok(Some((session.url, explicit_token.or(session.write_token))))
}

fn read_discovered_inspect_session(
  inspect: &InspectClientOptions,
) -> Result<Option<auv_cli::inspect_server::InspectServerSession>, String> {
  match auv_cli::inspect_server::read_inspect_session() {
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
    Some(host) => host
      .parse::<std::net::IpAddr>()
      .is_ok_and(|address| address.is_loopback()),
    None => false,
  }
}

fn resolve_client_token(inspect: &InspectClientOptions) -> Result<Option<String>, String> {
  if let Some(token) = &inspect.server_token {
    return normalize_write_token("--inspect-server-token", token.clone()).map(Some);
  }
  if let Some(path) = &inspect.server_token_file {
    let token = std::fs::read_to_string(path)
      .map_err(|error| format!("failed to read inspect server token file {path}: {error}"))?
      .trim()
      .to_string();
    return normalize_write_token("--inspect-server-token-file", token).map(Some);
  }
  Ok(None)
}

fn temp_runtime_store_root() -> PathBuf {
  std::env::temp_dir().join(format!(
    "auv-runtime-store-{}-{}",
    std::process::id(),
    auv_cli::model::now_millis()
  ))
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::path::Path;
  use std::sync::Mutex;

  use image::{Rgb, RgbImage};

  use super::*;

  static ENV_LOCK: Mutex<()> = Mutex::new(());

  #[test]
  fn inspect_serve_write_token_rejects_token_and_token_file_conflict() {
    let write = cli::InspectServeWriteOptions {
      enabled: true,
      token: Some("secret".to_string()),
      token_file: Some("token.txt".to_string()),
      no_token: false,
    };

    let error =
      resolve_inspect_serve_write_token(&write).expect_err("conflicting token sources reject");

    assert!(error.contains("--write-token"));
    assert!(error.contains("--write-token-file"));
  }

  #[test]
  fn inspect_serve_write_token_rejects_empty_token_file() {
    let path = std::env::temp_dir().join(format!(
      "auv-empty-write-token-{}.txt",
      auv_cli::model::now_millis()
    ));
    fs::write(&path, " \n\t").expect("token file should write");
    let write = cli::InspectServeWriteOptions {
      enabled: true,
      token: None,
      token_file: Some(path.display().to_string()),
      no_token: false,
    };

    let error =
      resolve_inspect_serve_write_token(&write).expect_err("empty token file should reject");

    assert!(error.contains("empty"));
    let _ = fs::remove_file(path);
  }

  #[test]
  fn inspect_serve_write_token_rejects_empty_explicit_token() {
    let write = cli::InspectServeWriteOptions {
      enabled: true,
      token: Some(String::new()),
      token_file: None,
      no_token: false,
    };

    let error =
      resolve_inspect_serve_write_token(&write).expect_err("empty explicit token should reject");

    assert!(error.contains("empty"));
  }

  #[test]
  fn inspect_serve_write_token_generates_non_empty_session_token() {
    let write = cli::InspectServeWriteOptions {
      enabled: true,
      token: None,
      token_file: None,
      no_token: false,
    };

    let token = resolve_inspect_serve_write_token(&write)
      .expect("generated token should resolve")
      .expect("write-enabled serve should generate a token");

    assert!(token.starts_with("session-"));
    assert!(!token.is_empty());
  }

  #[test]
  fn inspect_server_target_prefers_explicit_url_and_token_file() {
    let path = std::env::temp_dir().join(format!(
      "auv-client-write-token-{}.txt",
      auv_cli::model::now_millis()
    ));
    fs::write(&path, "secret\n").expect("token file should write");
    let inspect = InspectClientOptions {
      server_url: Some("http://127.0.0.1:9876/".to_string()),
      server_token_file: Some(path.display().to_string()),
      ..InspectClientOptions::default()
    };

    let target = resolve_inspect_server_target(&inspect).expect("explicit target should resolve");

    let _ = fs::remove_file(path);
    assert_eq!(
      target,
      Some((
        "http://127.0.0.1:9876/".to_string(),
        Some("secret".to_string())
      ))
    );
  }

  #[test]
  fn required_inspect_server_write_rejects_missing_target() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = std::env::temp_dir().join(format!(
      "auv-missing-inspect-session-{}",
      auv_cli::model::now_millis()
    ));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    unsafe {
      std::env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let inspect = InspectClientOptions {
      server_write: cli::InspectWriteSetting::Enabled,
      require_server_write: true,
      ..InspectClientOptions::default()
    };

    let error = match build_runtime_for_inspect(&root, &inspect) {
      Ok(_) => panic!("required server write without target should fail"),
      Err(error) => error,
    };

    unsafe {
      std::env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert!(error.contains("inspect server write is required"));
  }

  #[test]
  fn required_missing_server_with_local_write_disabled_does_not_leave_temp_store() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = std::env::temp_dir().join(format!(
      "auv-missing-required-server-no-local-{}",
      auv_cli::model::now_millis()
    ));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    unsafe {
      std::env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let prefix = format!("auv-runtime-store-{}-", std::process::id());
    let before = temp_runtime_store_entries(&prefix);
    let inspect = InspectClientOptions {
      local_write: cli::InspectWriteSetting::Disabled,
      server_write: cli::InspectWriteSetting::Enabled,
      require_server_write: true,
      ..InspectClientOptions::default()
    };

    let error = match build_runtime_for_inspect(&root, &inspect) {
      Ok(_) => panic!("required server write without target should fail"),
      Err(error) => error,
    };
    let after = temp_runtime_store_entries(&prefix);

    unsafe {
      std::env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert!(error.contains("inspect server write is required"));
    assert_eq!(after, before);
  }

  #[test]
  fn optional_inspect_server_write_ignores_malformed_discovered_session() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = std::env::temp_dir().join(format!(
      "auv-malformed-inspect-session-{}",
      auv_cli::model::now_millis()
    ));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    fs::write(&session_path, "not json").expect("malformed session should write");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(&session_path, fs::Permissions::from_mode(0o600))
        .expect("session file permissions should change");
    }
    unsafe {
      std::env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let inspect = InspectClientOptions {
      server_write: cli::InspectWriteSetting::Default,
      require_server_write: false,
      ..InspectClientOptions::default()
    };

    let runtime = build_runtime_for_inspect(&root, &inspect);

    unsafe {
      std::env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert!(runtime.is_ok());
  }

  #[test]
  fn required_inspect_server_write_rejects_malformed_discovered_session() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = std::env::temp_dir().join(format!(
      "auv-required-malformed-inspect-session-{}",
      auv_cli::model::now_millis()
    ));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    fs::write(&session_path, "not json").expect("malformed session should write");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(&session_path, fs::Permissions::from_mode(0o600))
        .expect("session file permissions should change");
    }
    unsafe {
      std::env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let inspect = InspectClientOptions {
      server_write: cli::InspectWriteSetting::Default,
      require_server_write: true,
      ..InspectClientOptions::default()
    };

    let error = match build_runtime_for_inspect(&root, &inspect) {
      Ok(_) => panic!("required server write should reject malformed session"),
      Err(error) => error,
    };

    unsafe {
      std::env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert!(error.contains("failed to parse inspect session"));
  }

  #[test]
  fn default_discovery_ignores_non_local_session_url() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = std::env::temp_dir().join(format!(
      "auv-remote-inspect-session-{}",
      auv_cli::model::now_millis()
    ));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    fs::write(
      &session_path,
      serde_json::to_string(&auv_cli::inspect_server::InspectServerSession {
        url: "http://203.0.113.7:8765".to_string(),
        store_root: root.display().to_string(),
        write_enabled: true,
        write_token: Some("secret".to_string()),
        pid: 123,
        started_at_millis: 456,
      })
      .expect("session should encode"),
    )
    .expect("session should write");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(&session_path, fs::Permissions::from_mode(0o600))
        .expect("session file permissions should change");
    }
    unsafe {
      std::env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let inspect = InspectClientOptions {
      server_write: cli::InspectWriteSetting::Default,
      require_server_write: false,
      ..InspectClientOptions::default()
    };

    let target =
      resolve_inspect_server_target(&inspect).expect("optional discovery should ignore remote URL");

    unsafe {
      std::env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert_eq!(target, None);
  }

  fn mc2_temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("auv-{label}-{}", auv_cli::model::now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }

  fn write_mc2_test_telemetry(path: &Path) {
    let frame = auv_game_minecraft::MinecraftSpatialFrame {
      spatial_frame_id: "frame-mc2".to_string(),
      world_tick: 42,
      monotonic_timestamp_ms: 5_000,
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
    };
    let body = serde_json::to_string(&frame).expect("frame should serialize");
    fs::write(path, format!("{body}\n")).expect("telemetry sample should write");
  }

  fn write_mc2_test_screenshot(path: &Path) {
    RgbImage::from_pixel(64, 64, Rgb([0, 0, 0]))
      .save(path)
      .expect("screenshot should write");
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

    let verification = build_minecraft_world_diff_verification(&verdict, Vec::new());

    assert_eq!(
      verification.method,
      auv_cli::contract::VerificationMethod::SemanticMatch
    );
    assert_eq!(verification.executed, true);
    assert_eq!(verification.state_changed, true);
    assert_eq!(verification.semantic_matched, Some(true));
    assert_eq!(verification.failure_layer, None);
    assert_eq!(
      verification.observed_label.as_deref(),
      Some("minecraft:air")
    );
  }

  #[test]
  fn minecraft_world_diff_verification_maps_failure_layers() {
    let cases = [
      (
        auv_game_minecraft::verify::WorldDiffFailure::VerificationUnreliable,
        Some(auv_cli::contract::FailureLayer::VerificationUnreliable),
        None,
      ),
      (
        auv_game_minecraft::verify::WorldDiffFailure::StateChangedNoMatch,
        Some(auv_cli::contract::FailureLayer::StateChangedNoMatch),
        Some(false),
      ),
      (
        auv_game_minecraft::verify::WorldDiffFailure::SemanticMismatch,
        Some(auv_cli::contract::FailureLayer::SemanticMismatch),
        Some(false),
      ),
    ];

    for (failure, expected_layer, semantic_matched) in cases {
      let verdict = auv_game_minecraft::verify::WorldDiffVerdict {
        executed: true,
        state_changed: matches!(
          failure,
          auv_game_minecraft::verify::WorldDiffFailure::StateChangedNoMatch
        ),
        semantic_matched,
        failure: Some(failure),
        observed_block_id: Some("minecraft:stone".to_string()),
        observed_item_delta: Some(0),
      };

      let verification = build_minecraft_world_diff_verification(&verdict, Vec::new());
      assert_eq!(verification.failure_layer, expected_layer);
      assert_eq!(
        verification.observed_label.as_deref(),
        Some("minecraft:stone")
      );
    }
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

    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())
      .expect("runtime should build");
    let output = run_minecraft_live_click(
      &runtime,
      telemetry_path,
      Some(post_telemetry_path),
      screenshot_path,
      "0,0,0",
      "FixtureApp",
      "Fixture Window",
      Some(0),
      true,
    )
    .expect("live click should record");

    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("run should persist");
    assert_eq!(run.artifacts.len(), 3);
    assert_eq!(run.artifacts[0].role, "minecraft-screenshot");
    assert_eq!(
      run.artifacts[1].role,
      auv_cli::runtime::MINECRAFT_PROJECTION_ARTIFACT_ROLE
    );
    assert_eq!(run.artifacts[2].role, "operation-result");

    let verifications = runtime
      .list_verifications(output.run_id.as_str())
      .expect("verifications should list");
    assert_eq!(verifications.len(), 1);
    assert_eq!(
      verifications[0].method,
      auv_cli::contract::VerificationMethod::SemanticMatch
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn minecraft_bridge_run_persists_telemetry_and_projection_artifacts() {
    let project_root = mc2_temp_dir("mc2-bridge-project");
    let store_root = mc2_temp_dir("mc2-bridge-store");
    let telemetry_path = project_root.join("telemetry.jsonl");
    let screenshot_path = project_root.join("frame.png");
    write_mc2_test_telemetry(&telemetry_path);
    write_mc2_test_screenshot(&screenshot_path);

    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())
      .expect("runtime should build");
    let output = run_minecraft_projection_bridge(
      &runtime,
      telemetry_path,
      screenshot_path,
      "0,0,0",
      Some(0),
      true,
    )
    .expect("bridge should succeed");

    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("run should persist");
    assert_eq!(run.artifacts.len(), 3);
    assert_eq!(run.artifacts[0].role, "minecraft-screenshot");
    assert_eq!(
      run.artifacts[1].role,
      auv_cli::runtime::MINECRAFT_PROJECTION_ARTIFACT_ROLE
    );
    assert_eq!(run.artifacts[2].role, "minecraft-overlay");

    let inspect_text = runtime
      .inspect(output.run_id.as_str())
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

    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())
      .expect("runtime should build");
    let output = run_minecraft_projection_bridge(
      &runtime,
      telemetry_path,
      screenshot_path,
      "0,0,0",
      Some(999),
      true,
    )
    .expect("bridge refusal should still record");

    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("run should persist");
    assert_eq!(run.artifacts.len(), 2);
    assert_eq!(run.artifacts[0].role, "minecraft-screenshot");
    assert_eq!(
      run.artifacts[1].role,
      auv_cli::runtime::MINECRAFT_PROJECTION_ARTIFACT_ROLE
    );

    let inspect_text = runtime
      .inspect(output.run_id.as_str())
      .expect("inspect should render run");
    assert!(inspect_text.contains("MC-2 Projection Artifacts:"));
    assert!(inspect_text.contains("capture_skew_ms=999"));
    assert_eq!(
      output.value.refusal_reason.as_deref(),
      Some("CaptureSkewUnreliable")
    );
    assert_eq!(output.value.overlay_artifact_id, None);

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn temp_runtime_store_entries(prefix: &str) -> Vec<String> {
    let mut entries = fs::read_dir(std::env::temp_dir())
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
