use std::fs;
use std::path::PathBuf;

use auv_game_osu::{
  BenchmarkInputs, BenchmarkOutput, DatasetExportInputs, DatasetExportOutput, DetectionEvalInputs,
  DetectionEvalOutput, FrameDetections, RunMode, evaluate_detection_fixture, export_dataset,
  run_benchmark,
};

use crate::{
  contract::{
    NodeRef, OBSERVATION_SNAPSHOT_API_VERSION, ObservationSnapshot, ObservationSource,
    RecognitionBox, RecognitionScope, RecognitionSource, RecognitionSurface, SurfaceNode,
  },
  model::AuvResult,
  session::{FixtureObservationProvider, SessionObservationProvider},
};
use auv_tracing_driver::RecordingHandle;
use auv_tracing_driver::recorded_operation::RecordedOperationOutput;
use auv_tracing_driver::run_builder::RunSpec;
use auv_tracing_driver::trace::{RunType, new_run_id, new_span_id};

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
  FixtureObservationProvider::new(provider_id, snapshots)
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
}
