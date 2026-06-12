use std::path::PathBuf;

use auv_game_osu::{BenchmarkInputs, BenchmarkOutput, RunMode, run_benchmark};

use crate::model::AuvResult;
use crate::recorded_operation::RecordedOperationOutput;
use crate::run_builder::RunSpec;
use crate::runtime::Runtime;
use crate::trace::RunType;

pub fn run_osu_benchmark(
  runtime: &Runtime,
  beatmap_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<BenchmarkOutput>> {
  run_osu_benchmark_with_inputs(
    runtime,
    BenchmarkInputs::new(beatmap_path, output_dir),
    "osu benchmark dry-run",
  )
}

pub fn run_osu_benchmark_with_inputs(
  runtime: &Runtime,
  inputs: BenchmarkInputs,
  operation_label: &str,
) -> AuvResult<RecordedOperationOutput<BenchmarkOutput>> {
  let beatmap_path = inputs.beatmap_path.clone();
  runtime.run_recorded_operation(
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
