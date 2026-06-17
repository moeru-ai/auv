use std::fs;
use std::path::PathBuf;

use auv_game_osu::{
  BenchmarkInputs, BenchmarkOutput, DatasetExportInputs, DatasetExportOutput, DetectionEvalInputs,
  DetectionEvalOutput, RunMode, evaluate_detection_fixture, export_dataset, run_benchmark,
};

use crate::model::AuvResult;
use auv_tracing_driver::RecordingHandle;
use auv_tracing_driver::recorded_operation::RecordedOperationOutput;
use auv_tracing_driver::run_builder::RunSpec;
use auv_tracing_driver::trace::RunType;

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
