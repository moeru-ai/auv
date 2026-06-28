use std::fs;
use std::path::PathBuf;

use auv_game_osu::{
  BenchmarkInputs, BenchmarkOutput, CapturePhase, DatasetExportInputs, DatasetExportOutput,
  DetectionEvalInputs, DetectionEvalOutput, FrameDetections, ObjectKind, PlayfieldProjection,
  RunMode, VisualTruthManifest, VisualTruthQueryActionWiringOutcome,
  VisualTruthQueryLiveClickExecutor, VisualTruthSemanticValidationInputs,
  VisualTruthSpatialQueryInputs, VisualTruthSpatialQueryOutput, evaluate_detection_fixture,
  export_dataset, query_visual_truth_spatial, run_benchmark, validate_visual_truth_semantic,
  visual_truth_query_action_wiring_lineage_from_manifest,
  wire_visual_truth_spatial_query_manifest_to_action,
};
use crate::osu_query_live_action::{
  InvokeWindowPointClickExecutor, QUERY_WIRED_LIVE_ACTION_OPERATION_ID,
  build_osu_query_wired_live_action_operation_result,
  stage_osu_query_wired_live_action_operation_result,
};

use crate::{
  contract::{
    NodeRef, OBSERVATION_SNAPSHOT_API_VERSION, ObservationSnapshot, ObservationSource,
    RecognitionBox, RecognitionScope, RecognitionSource, RecognitionSurface, SurfaceNode,
  },
  model::AuvResult,
  session::{BufferedObservationProvider, SessionObservationProvider},
};
use auv_tracing_driver::RecordingHandle;
use auv_tracing_driver::recorded_operation::RecordedOperationOutput;
use auv_tracing_driver::run_builder::RunSpec;
use auv_tracing_driver::trace::{RunType, new_run_id, new_span_id};

pub const OSU_VISUAL_TRUTH_SEMANTIC_ROLE: &str = "osu-visual-truth-semantic";
pub const OSU_VISUAL_TRUTH_SEMANTIC_INSPECT_ROLE: &str = "osu-visual-truth-semantic-inspect";
pub const OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE: &str = "osu-visual-truth-spatial-query";
pub const OSU_VISUAL_TRUTH_SPATIAL_QUERY_INSPECT_ROLE: &str =
  "osu-visual-truth-spatial-query-inspect";

pub fn run_osu_benchmark(
  recording: &RecordingHandle,
  beatmap_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<BenchmarkOutput>> {
  run_osu_benchmark_with_inputs(
    recording,
    BenchmarkInputs::new(beatmap_path, output_dir),
    "osu benchmark dry-run",
  )
}

pub fn run_osu_benchmark_with_inputs(
  recording: &RecordingHandle,
  inputs: BenchmarkInputs,
  operation_label: &str,
) -> AuvResult<RecordedOperationOutput<BenchmarkOutput>> {
  let beatmap_path = inputs.beatmap_path.clone();
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.osu.benchmark"),
    operation_label,
    |context| {
      context.record_event(
        "osu.benchmark.inputs",
        Some(format!(
          "beatmap={} run_mode={}",
          beatmap_path.display(),
          match inputs.run_mode {
            RunMode::DryRun => "dry_run",
            RunMode::TypedDispatch => "typed_dispatch",
          }
        )),
      );
      let result = run_benchmark(&inputs)?;
      context.in_span("osu.benchmark.artifacts", |context| {
        for artifact_name in [
          "parsed_map_summary.json",
          "action_schedule.json",
          "dispatch_trace.json",
          "latency_report.json",
          "capture_trace.json",
          "verification_summary.json",
          "visual_truth_manifest.json",
          "projection.json",
          "evidence_summary.json",
        ] {
          let artifact_path = result.output_dir.join(artifact_name);
          if artifact_path.exists() {
            context.stage_artifact_file(
              "osu-benchmark",
              &artifact_path,
              artifact_name,
              Some(format!("osu benchmark artifact {artifact_name}")),
            )?;
          }
        }
        for capture in &result.capture_trace {
          for sample in &capture.captures {
            let artifact_path = result.output_dir.join(&sample.file_name);
            if artifact_path.exists() {
              context.stage_artifact_file(
                "osu-benchmark-capture",
                &artifact_path,
                &sample.file_name,
                Some(format!("osu benchmark capture {}", sample.file_name)),
              )?;
            }
          }
        }
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_osu_dataset_export(
  recording: &RecordingHandle,
  run_artifact_dir: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<DatasetExportOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.osu.export_dataset"),
    "osu export labeled dataset",
    |context| {
      context.record_event(
        "osu.export_dataset.inputs",
        Some(format!(
          "run_artifact_dir={} output_dir={}",
          run_artifact_dir.display(),
          output_dir.display()
        )),
      );
      let result = export_dataset(&DatasetExportInputs {
        run_artifact_dir: run_artifact_dir.clone(),
        output_dir: output_dir.clone(),
      })?;
      context.in_span("osu.export_dataset.artifacts", |context| {
        for artifact_name in ["dataset_manifest.json"] {
          let artifact_path = result.output_dir.join(artifact_name);
          if artifact_path.exists() {
            context.stage_artifact_file(
              "osu-dataset",
              &artifact_path,
              artifact_name,
              Some(format!("osu dataset artifact {artifact_name}")),
            )?;
          }
        }

        stage_dataset_dir(
          context,
          &result.output_dir.join("images"),
          "osu-dataset-image",
        )?;
        stage_dataset_dir(
          context,
          &result.output_dir.join("labels"),
          "osu-dataset-label",
        )?;
        stage_dataset_dir(
          context,
          &result.output_dir.join("overlays"),
          "osu-dataset-overlay",
        )?;

        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_osu_detection_eval(
  recording: &RecordingHandle,
  run_artifact_dir: PathBuf,
  detections_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<DetectionEvalOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.osu.eval_detections"),
    "osu evaluate offline detections",
    |context| {
      context.record_event(
        "osu.eval_detections.inputs",
        Some(format!(
          "run_artifact_dir={} detections_path={} output_dir={}",
          run_artifact_dir.display(),
          detections_path.display(),
          output_dir.display()
        )),
      );
      let result = evaluate_detection_fixture(&DetectionEvalInputs {
        run_artifact_dir: run_artifact_dir.clone(),
        detections_path: detections_path.clone(),
        output_dir: output_dir.clone(),
      })?;
      context.in_span("osu.eval_detections.artifacts", |context| {
        for artifact_name in ["visual_eval_report.json", "detection_eval_manifest.json"] {
          let artifact_path = result.output_dir.join(artifact_name);
          if artifact_path.exists() {
            context.stage_artifact_file(
              "osu-eval-detections",
              &artifact_path,
              artifact_name,
              Some(format!("osu detection eval artifact {artifact_name}")),
            )?;
          }
        }
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_osu_visual_truth_semantic_validation(
  recording: &RecordingHandle,
  run_artifact_dir: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<auv_game_osu::VisualTruthSemanticValidationOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.osu.validate_visual_truth_semantic"),
    "osu validate visual truth semantic gate",
    |context| {
      context.record_event(
        "osu.validate_visual_truth_semantic.inputs",
        Some(format!(
          "run_artifact_dir={} output_dir={}",
          run_artifact_dir.display(),
          output_dir.display()
        )),
      );
      let result = validate_visual_truth_semantic(VisualTruthSemanticValidationInputs {
        run_artifact_dir: run_artifact_dir.clone(),
        output_dir: output_dir.clone(),
      })?;
      context.in_span("osu.validate_visual_truth_semantic.artifacts", |context| {
        for (artifact_name, role) in [
          (
            "osu-visual-truth-semantic.json",
            OSU_VISUAL_TRUTH_SEMANTIC_ROLE,
          ),
          (
            "osu-visual-truth-semantic-inspect.json",
            OSU_VISUAL_TRUTH_SEMANTIC_INSPECT_ROLE,
          ),
        ] {
          let artifact_path = result.output_dir.join(artifact_name);
          if artifact_path.exists() {
            context.stage_artifact_file(
              role,
              &artifact_path,
              artifact_name,
              Some(format!(
                "osu visual truth semantic artifact {artifact_name}"
              )),
            )?;
          }
        }
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_osu_visual_truth_spatial_query(
  recording: &RecordingHandle,
  visual_truth_semantic_manifest_path: PathBuf,
  object_index: usize,
  capture_phase: CapturePhase,
  object_kind: Option<ObjectKind>,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<auv_game_osu::VisualTruthSpatialQueryOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.osu.query_visual_truth_spatial"),
    "osu query visual truth spatial target",
    |context| {
      context.record_event(
        "osu.query_visual_truth_spatial.inputs",
        Some(format!(
          "semantic_manifest={} object_index={object_index} capture_phase={capture_phase:?} output_dir={}",
          visual_truth_semantic_manifest_path.display(),
          output_dir.display()
        )),
      );
      let result = query_visual_truth_spatial(VisualTruthSpatialQueryInputs {
        visual_truth_semantic_manifest_path: visual_truth_semantic_manifest_path.clone(),
        object_index,
        capture_phase: capture_phase.clone(),
        object_kind: object_kind.clone(),
        output_dir: output_dir.clone(),
      })?;
      context.in_span("osu.query_visual_truth_spatial.artifacts", |context| {
        for (artifact_name, role) in [
          ("osu-visual-truth-spatial-query.json", OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE),
          (
            "osu-visual-truth-spatial-query-inspect.json",
            OSU_VISUAL_TRUTH_SPATIAL_QUERY_INSPECT_ROLE,
          ),
        ] {
          let artifact_path = result.output_dir.join(artifact_name);
          if artifact_path.exists() {
            context.stage_artifact_file(
              role,
              &artifact_path,
              artifact_name,
              Some(format!("osu visual truth spatial query artifact {artifact_name}")),
            )?;
          }
        }
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_osu_vision_demo(
  recording: &RecordingHandle,
  beatmap_path: PathBuf,
  target_app: String,
  output_dir: PathBuf,
  dispatch_limit: Option<usize>,
  capture_verify: bool,
) -> AuvResult<RecordedOperationOutput<BenchmarkOutput>> {
  let mut inputs =
    BenchmarkInputs::typed_dispatch(beatmap_path.clone(), output_dir, target_app.clone());
  inputs.dispatch_limit = Some(dispatch_limit.unwrap_or(8).min(8));
  inputs.capture_verify = capture_verify;
  // TODO(osu-p8): broader vision-only control policy is deferred until the owner approves a slice beyond this bounded local demo command.
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.osu.vision_demo"),
    "osu vision-only low-difficulty demo",
    |context| {
      context.record_event(
        "osu.vision_demo.inputs",
        Some(format!(
          "beatmap={} target_app={} dispatch_limit={} capture_verify={}",
          beatmap_path.display(),
          target_app,
          inputs.dispatch_limit.unwrap_or(8),
          inputs.capture_verify
        )),
      );
      let result = run_benchmark(&inputs)?;
      context.in_span("osu.vision_demo.artifacts", |context| {
        for artifact_name in [
          "parsed_map_summary.json",
          "action_schedule.json",
          "dispatch_trace.json",
          "latency_report.json",
          "capture_trace.json",
          "verification_summary.json",
          "visual_truth_manifest.json",
          "projection.json",
          "evidence_summary.json",
        ] {
          let artifact_path = result.output_dir.join(artifact_name);
          if artifact_path.exists() {
            context.stage_artifact_file(
              "osu-vision-demo",
              &artifact_path,
              artifact_name,
              Some(format!("osu vision demo artifact {artifact_name}")),
            )?;
          }
        }
        for capture in &result.capture_trace {
          for sample in &capture.captures {
            let artifact_path = result.output_dir.join(&sample.file_name);
            if artifact_path.exists() {
              context.stage_artifact_file(
                "osu-vision-demo-capture",
                &artifact_path,
                &sample.file_name,
                Some(format!("osu vision demo capture {}", sample.file_name)),
              )?;
            }
          }
        }
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn osu_detection_session_provider(
  provider_id: impl Into<String>,
  frames: Vec<FrameDetections>,
) -> impl SessionObservationProvider {
  let snapshots = frames
    .into_iter()
    .enumerate()
    .map(|(index, frame)| osu_frame_detections_snapshot(index, frame))
    .collect();
  BufferedObservationProvider::new(provider_id, snapshots)
}

fn osu_frame_detections_snapshot(index: usize, frame: FrameDetections) -> ObservationSnapshot {
  let run_id = new_run_id();
  let span_id = new_span_id();
  let nodes = frame
    .detections
    .detections
    .into_iter()
    .enumerate()
    .map(|(detection_index, detection)| {
      let node_id = format!(
        "osu_detection_{}_{}",
        frame.frame.capture_file_name, detection_index
      );
      SurfaceNode {
        node_ref: NodeRef {
          run_id: run_id.clone(),
          span_id: span_id.clone(),
          node_id,
        },
        kind: "osu_detection".to_string(),
        label: Some(detection.label.clone()),
        box_: RecognitionBox {
          x: detection.bbox.x1.round() as i64,
          y: detection.bbox.y1.round() as i64,
          width: detection.bbox.width().round().max(0.0) as i64,
          height: detection.bbox.height().round().max(0.0) as i64,
        },
        source_artifacts: vec![frame.frame.capture_file_name.clone()],
        recognition_id: Some(format!("osu_frame_detection_{index}")),
        recognition_source: Some(RecognitionSource::VisualRow),
        recognition_surface: Some(RecognitionSurface::Window),
        recognized_item_id: Some(format!("detection_{detection_index}")),
        recognized_item_kind: Some("osu_object".to_string()),
        provider_score: Some(f64::from(detection.confidence)),
        detail: serde_json::json!({
          "class_id": detection.class_id,
          "model_id": frame.detections.model_id.0,
          "source_image_size": {
            "width": frame.detections.image_size.width,
            "height": frame.detections.image_size.height,
          },
          "frame": {
            "object_index": frame.frame.object_index,
            "phase": frame.frame.phase,
            "capture_file_name": frame.frame.capture_file_name,
          },
          "coordinate_space": "source_image_pixels"
        }),
      }
    })
    .collect();

  ObservationSnapshot {
    api_version: OBSERVATION_SNAPSHOT_API_VERSION.to_string(),
    snapshot_id: format!("osu_session_observation_{index}"),
    run_id,
    span_id,
    captured_at_millis: auv_tracing_driver::now_millis(),
    source: ObservationSource::Visual,
    scope: RecognitionScope {
      surface: RecognitionSurface::Window,
      display_ref: None,
      native_display_id: None,
      app_bundle_id: None,
      window_title: None,
      window_number: None,
      region_hint: None,
      capture_artifact: None,
      capture_contract_artifact: None,
    },
    capture_contract_ref: None,
    evidence: Vec::new(),
    nodes,
    detail: serde_json::json!({
      "producer": "osu_detection_session_provider",
      "frame_index": index
    }),
    known_limits: vec![
      "osu detections are source-image pixels; this provider is observe-only and does not imply clickable window coordinates".to_string(),
      "session v0 provider has no durable capture artifact link for this fixture projection".to_string(),
    ],
  }
}

fn stage_dataset_dir(
  context: &mut auv_tracing_driver::recorded_operation::RecordedOperationContext<'_>,
  dir: &std::path::Path,
  artifact_kind: &str,
) -> Result<(), String> {
  if !dir.exists() {
    return Ok(());
  }

  let entries = fs::read_dir(dir)
    .map_err(|error| format!("failed to read dataset dir {}: {error}", dir.display()))?;
  for entry in entries {
    let entry = entry.map_err(|error| format!("failed to read dataset entry: {error}"))?;
    let path = entry.path();
    if path.is_file() {
      let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
          format!(
            "dataset artifact path {} has invalid file name",
            path.display()
          )
        })?;
      context.stage_artifact_file(
        artifact_kind,
        &path,
        file_name,
        Some(format!("osu dataset artifact {file_name}")),
      )?;
    }
  }
  Ok(())
}


#[derive(Clone, Debug, PartialEq)]
pub struct QueryWiredLiveActionInputs {
  pub visual_truth_semantic_manifest_path: PathBuf,
  pub object_index: usize,
  pub capture_phase: CapturePhase,
  pub object_kind: Option<ObjectKind>,
  pub output_dir: PathBuf,
  pub target_app: String,
  pub target_title: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryWiredLiveActionOutput {
  pub query: VisualTruthSpatialQueryOutput,
  pub wiring: VisualTruthQueryActionWiringOutcome,
  pub operation_result_artifact_id: String,
}

pub fn run_osu_query_wired_live_action(
  recording: &RecordingHandle,
  inputs: QueryWiredLiveActionInputs,
) -> AuvResult<RecordedOperationOutput<QueryWiredLiveActionOutput>> {
  #[cfg(target_os = "macos")]
  {
    let circle_size = circle_size_for_wired_live_action_inputs(&inputs)?;
    let live_projection =
      build_live_playfield_projection(&inputs.target_app, &inputs.target_title, circle_size)?;
    return recording.run_recorded_operation(
      RunSpec::new(RunType::Execute, QUERY_WIRED_LIVE_ACTION_OPERATION_ID),
      "osu visual truth query wired live action",
      |context| {
        let click_executor = InvokeWindowPointClickExecutor::new(
          context,
          inputs.target_app.as_str(),
          inputs.target_title.as_str(),
        );
        run_osu_query_wired_live_action_core(context, &inputs, &live_projection, &click_executor)
      },
    );
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (recording, inputs);
    Err(
      "osu query wired live action requires macOS for live window projection".to_string(),
    )
  }
}

pub fn run_osu_query_wired_live_action_with_executor<E: VisualTruthQueryLiveClickExecutor>(
  recording: &RecordingHandle,
  inputs: QueryWiredLiveActionInputs,
  live_projection: &PlayfieldProjection,
  executor: &E,
) -> AuvResult<RecordedOperationOutput<QueryWiredLiveActionOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, QUERY_WIRED_LIVE_ACTION_OPERATION_ID),
    "osu visual truth query wired live action",
    |context| run_osu_query_wired_live_action_core(context, &inputs, live_projection, executor),
  )
}

fn run_osu_query_wired_live_action_core<E: VisualTruthQueryLiveClickExecutor>(
  context: &mut auv_tracing_driver::recorded_operation::RecordedOperationContext<'_>,
  inputs: &QueryWiredLiveActionInputs,
  live_projection: &PlayfieldProjection,
  executor: &E,
) -> Result<QueryWiredLiveActionOutput, String> {
  context.record_event(
    "osu.query_wired_live_action.inputs",
    Some(format!(
      "semantic_manifest={} object_index={} capture_phase={:?} object_kind={} target_app={} target_title={} output_dir={}",
      inputs.visual_truth_semantic_manifest_path.display(),
      inputs.object_index,
      inputs.capture_phase,
      inputs
        .object_kind
        .as_ref()
        .map(|kind| format!("{kind:?}"))
        .unwrap_or_else(|| "none".to_string()),
      inputs.target_app,
      inputs.target_title,
      inputs.output_dir.display(),
    )),
  );

  let query = query_visual_truth_spatial(VisualTruthSpatialQueryInputs {
    visual_truth_semantic_manifest_path: inputs.visual_truth_semantic_manifest_path.clone(),
    object_index: inputs.object_index,
    capture_phase: inputs.capture_phase.clone(),
    object_kind: inputs.object_kind.clone(),
    output_dir: inputs.output_dir.clone(),
  })?;

  let (_staged_manifest_path, query_manifest_ref) = context.in_span(
    "osu.query_visual_truth_spatial.artifacts",
    |context| {
      context.stage_artifact_file_with_ref(
        OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE,
        &query.manifest_path,
        "osu-visual-truth-spatial-query.json",
        Some("osu visual truth spatial query manifest".to_string()),
      )
    },
  )?;
  context.in_span("osu.query_visual_truth_spatial.artifacts", |context| {
    context.stage_artifact_file(
      OSU_VISUAL_TRUTH_SPATIAL_QUERY_INSPECT_ROLE,
      &query.inspect_report_path,
      "osu-visual-truth-spatial-query-inspect.json",
      Some("osu visual truth spatial query inspect report".to_string()),
    )?;
    Ok::<_, String>(())
  })?;

  let lineage = visual_truth_query_action_wiring_lineage_from_manifest(
    &query.manifest,
    &query.manifest_path,
  );
  let wiring = wire_visual_truth_spatial_query_manifest_to_action(
    &query.manifest,
    &lineage,
    live_projection,
    executor,
  );

  let operation_result = build_osu_query_wired_live_action_operation_result(
    context.run_id(),
    &wiring,
    Some(query_manifest_ref.clone()),
  );
  let (_staged_operation_result_path, operation_result_ref) =
    stage_osu_query_wired_live_action_operation_result(context, &operation_result)?;

  context.record_event(
    "osu.query_wired_live_action.outcome",
    Some(format!(
      "attempted={} action_eligibility={} refusal_reason={} pixel_point={} window_point={} query_manifest_path={}",
      wiring.attempted,
      wiring.action_eligibility.as_str(),
      wiring.refusal_reason.as_deref().unwrap_or("none"),
      wiring
        .pixel_point
        .map(|(x, y)| format!("{x},{y}"))
        .unwrap_or_else(|| "none".to_string()),
      wiring
        .window_point
        .map(|point| format!("{:.3},{:.3}", point.0.x, point.0.y))
        .unwrap_or_else(|| "none".to_string()),
      query.manifest_path.display(),
    )),
  );

  Ok(QueryWiredLiveActionOutput {
    query,
    wiring,
    operation_result_artifact_id: operation_result_ref.artifact_id.as_str().to_string(),
  })
}

fn circle_size_for_wired_live_action_inputs(inputs: &QueryWiredLiveActionInputs) -> Result<f32, String> {
  use auv_game_osu::VisualTruthSemanticManifest;

  let semantic_json = fs::read_to_string(&inputs.visual_truth_semantic_manifest_path).map_err(|error| {
    format!(
      "failed to read osu visual truth semantic manifest {}: {error}",
      inputs.visual_truth_semantic_manifest_path.display()
    )
  })?;
  let semantic: VisualTruthSemanticManifest = serde_json::from_str(&semantic_json).map_err(|error| {
    format!(
      "failed to parse osu visual truth semantic manifest {}: {error}",
      inputs.visual_truth_semantic_manifest_path.display()
    )
  })?;
  let manifest_json = fs::read_to_string(&semantic.source_visual_truth_manifest_path).map_err(|error| {
    format!(
      "failed to read osu visual truth manifest {}: {error}",
      semantic.source_visual_truth_manifest_path
    )
  })?;
  let manifest: VisualTruthManifest = serde_json::from_str(&manifest_json).map_err(|error| {
    format!(
      "failed to parse osu visual truth manifest {}: {error}",
      semantic.source_visual_truth_manifest_path
    )
  })?;
  Ok(manifest.map_summary.circle_size)
}

#[cfg(target_os = "macos")]
fn build_live_playfield_projection(
  target_app: &str,
  target_title: &str,
  circle_size: f32,
) -> Result<PlayfieldProjection, String> {
  use auv_driver::{App, Driver, WindowSelector};
  use auv_driver_macos::MacosDriver;

  let driver = MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let window = session
    .window()
    .resolve(
      WindowSelector::default()
        .owned_by(App::name(target_app.to_string()))
        .title_contains(target_title),
    )
    .map_err(|error| error.to_string())?;
  PlayfieldProjection::for_window(&window, circle_size)
}


#[cfg(test)]
mod tests {
  use auv_game_osu::{CapturePhase, FrameDetections, FrameKey};
  use auv_inference_common::{BoundingBox, Detection, DetectionSet, ImageSize, ModelId};

  use crate::session::{ObserveRequest, SessionOptions, SessionRuntime};

  use super::osu_detection_session_provider;

  #[test]
  fn osu_detection_provider_projects_into_session_observation() {
    let provider = osu_detection_session_provider(
      "osu.fixture.detector",
      vec![FrameDetections::new(
        FrameKey::from_parts(0, CapturePhase::AfterDispatch, "capture-after.png"),
        DetectionSet {
          model_id: ModelId("osu-yolo-fixture".to_string()),
          image_size: ImageSize {
            width: 640,
            height: 480,
          },
          detections: vec![Detection {
            class_id: 1,
            label: "hit_circle".to_string(),
            confidence: 0.91,
            bbox: BoundingBox {
              x1: 100.2,
              y1: 150.7,
              x2: 132.2,
              y2: 182.7,
            },
          }],
        },
      )],
    );

    let mut session = SessionRuntime::new(SessionOptions::default());
    let provider_id = session.register_provider(provider);
    let observation = session
      .observe(&provider_id, ObserveRequest::default())
      .expect("osu fixture observation should succeed");

    assert_eq!(observation.snapshot.nodes.len(), 1);
    assert!(
      observation
        .snapshot
        .known_limits
        .iter()
        .any(|limit| limit.contains("observe-only"))
    );
    let node = session
      .find_node_by_label("hit_circle")
      .expect("osu detection should be addressable by session lookup");
    assert!(matches!(node.node.provider_score, Some(score) if (score - 0.91).abs() < 1e-6));
    assert_eq!(node.node.box_.width, 32);
    assert_eq!(node.node.box_.height, 32);
  }

  mod osu_query_wired_live_action_tests {
    use std::cell::Cell;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    use auv_driver::geometry::WindowPoint;
    use auv_game_osu::{
      OSU_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT, PlayfieldProjection,
      VisualTruthQueryActionWiringLineage, VisualTruthQueryLiveClickExecutor,
      validate_visual_truth_semantic, VisualTruthSemanticValidationInputs,
    };
    use auv_tracing_driver::recording::{NoopRunRecorder, RunRecordingBackend};
    use auv_tracing_driver::store::LocalStore;

    use auv_game_osu::CapturePhase;
    use crate::osu::{
      run_osu_query_wired_live_action_with_executor, QueryWiredLiveActionInputs,
      OSU_VISUAL_TRUTH_SPATIAL_QUERY_INSPECT_ROLE, OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE,
    };
    use crate::osu_query_live_action::QUERY_WIRED_LIVE_ACTION_OPERATION_ID;

    struct CountingExecutor {
      calls: Cell<usize>,
      summary: String,
    }

    impl CountingExecutor {
      fn success(summary: impl Into<String>) -> Self {
        Self { calls: Cell::new(0), summary: summary.into() }
      }
    }

    impl VisualTruthQueryLiveClickExecutor for CountingExecutor {
      fn attempt_click(
        &self,
        _window_point: WindowPoint,
        _lineage: &VisualTruthQueryActionWiringLineage,
      ) -> Result<String, String> {
        self.calls.set(self.calls.get() + 1);
        Ok(self.summary.clone())
      }
    }

    fn temp_dir(name: &str) -> PathBuf {
      std::env::temp_dir().join(format!("auv-{name}-{}", std::process::id()))
    }

    fn setup_probe_work() -> PathBuf {
      let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates/auv-game-osu/tests/fixtures/osu_visual_truth_probe");
      let work = temp_dir("osu-wired-live-action");
      fs::create_dir_all(&work).expect("work dir");
      for name in ["visual_truth_manifest.json", "projection.json"] {
        fs::copy(fixture_root.join(name), work.join(name)).expect("copy fixture");
      }
      work
    }

    fn live_projection() -> PlayfieldProjection {
      PlayfieldProjection::for_capture(800.0, 600.0, 4.0).expect("projection")
    }

    fn operation_output_message(output: &crate::contract::OperationOutput) -> String {
      match output {
        crate::contract::OperationOutput::Acknowledged { message } => message.clone().unwrap_or_default(),
        _ => String::new(),
      }
    }

    fn read_operation_result_artifact(
      store: &LocalStore,
      run: &auv_tracing_driver::store::CanonicalRun,
    ) -> crate::contract::OperationResult {
      let artifact = run.artifacts.iter().find(|a| a.role == "operation-result").expect("op");
      let artifact_path = store.run_dir(run.run.run_id.as_str()).expect("dir").join(&artifact.path);
      serde_json::from_slice(&fs::read(&artifact_path).expect("read")).expect("parse")
    }

    #[test]
    fn osu_query_wired_live_action_click_ready_records_operation_result() {
      let work = setup_probe_work();
      let temp = work.parent().unwrap().join("osu-wired-click-ready");
      fs::create_dir_all(&temp).expect("temp");
      let semantic_manifest = validate_visual_truth_semantic(VisualTruthSemanticValidationInputs {
        run_artifact_dir: work.clone(),
        output_dir: work.join("semantic-out-click"),
      }).expect("semantic").manifest_path;
      let store = LocalStore::new(temp.join("store")).expect("store");
      let recording = RunRecordingBackend::new(store.clone(), Arc::new(NoopRunRecorder)).handle();
      let executor = CountingExecutor::success("mock live click dispatched");
      let output = run_osu_query_wired_live_action_with_executor(
        &recording,
        QueryWiredLiveActionInputs {
          visual_truth_semantic_manifest_path: semantic_manifest,
          object_index: 0,
          capture_phase: CapturePhase::BeforeDispatch,
          object_kind: None,
          output_dir: temp.join("query-output"),
          target_app: "osu!".into(),
          target_title: "osu".into(),
        },
        &live_projection(),
        &executor,
      ).expect("ok");
      assert!(output.value.wiring.attempted);
      assert_eq!(executor.calls.get(), 1);
      let run = recording.read_run(output.run_id.as_str()).expect("run");
      assert!(run.artifacts.iter().any(|a| a.role == OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE));
      let operation_result = read_operation_result_artifact(&store, &run);
      assert_eq!(operation_result.operation_id, QUERY_WIRED_LIVE_ACTION_OPERATION_ID);
      assert!(operation_output_message(&operation_result.output).contains("mock live click dispatched"));
      assert!(operation_result.known_limits.iter().any(|l| l == OSU_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT));
      let _ = fs::remove_dir_all(&work);
      let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn osu_query_wired_live_action_not_consumable_refuses_without_executor() {
      let work = setup_probe_work();
      let temp = work.parent().unwrap().join("osu-wired-not-consumable");
      fs::create_dir_all(&temp).expect("temp");
      let semantic_manifest = validate_visual_truth_semantic(VisualTruthSemanticValidationInputs {
        run_artifact_dir: work.clone(),
        output_dir: work.join("semantic-out-absent"),
      }).expect("semantic").manifest_path;
      let store = LocalStore::new(temp.join("store")).expect("store");
      let recording = RunRecordingBackend::new(store.clone(), Arc::new(NoopRunRecorder)).handle();
      let executor = CountingExecutor::success("should not run");
      let output = run_osu_query_wired_live_action_with_executor(
        &recording,
        QueryWiredLiveActionInputs {
          visual_truth_semantic_manifest_path: semantic_manifest,
          object_index: 99,
          capture_phase: CapturePhase::BeforeDispatch,
          object_kind: None,
          output_dir: temp.join("query-output"),
          target_app: "osu!".into(),
          target_title: "osu".into(),
        },
        &live_projection(),
        &executor,
      ).expect("ok");
      assert!(!output.value.wiring.attempted);
      assert_eq!(executor.calls.get(), 0);
      let run = recording.read_run(output.run_id.as_str()).expect("run");
      let operation_result = read_operation_result_artifact(&store, &run);
      assert_eq!(output.value.wiring.action_eligibility.as_str(), "not_consumable");
      assert!(output.value.wiring.refusal_reason.as_deref().is_some_and(|r| r.contains("target_absent_from_visual_truth")));
      let _ = fs::remove_dir_all(&work);
      let _ = fs::remove_dir_all(&temp);
    }
  }

}
