use std::path::PathBuf;

use auv_game_osu::{BenchmarkInputs, BenchmarkOutput, run_benchmark};

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
  runtime.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.osu.benchmark"),
    "osu benchmark dry-run",
    |context| {
      context.record_event(
        "osu.benchmark.inputs",
        Some(format!("beatmap={}", beatmap_path.display())),
      );
      let result = run_benchmark(&BenchmarkInputs::new(
        beatmap_path.clone(),
        output_dir.clone(),
      ))?;
      context.in_span("osu.benchmark.artifacts", |context| {
        for artifact_name in [
          "parsed_map_summary.json",
          "action_schedule.json",
          "dispatch_trace.json",
          "latency_report.json",
        ] {
          let artifact_path = result.output_dir.join(artifact_name);
          context.stage_artifact_file(
            "osu-benchmark",
            &artifact_path,
            artifact_name,
            Some(format!("osu benchmark artifact {artifact_name}")),
          )?;
        }
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}
