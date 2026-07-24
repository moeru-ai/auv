use std::fs;
use std::path::PathBuf;

pub mod help;
pub mod query_live_action;

use auv_game_osu::{
  BenchmarkInputs, BenchmarkOutput, CapturePhase, DatasetExportInputs, DatasetExportOutput, DetectionEvalInputs, DetectionEvalOutput,
  DetectionEvalQualityOutput, DetectionEvalWitnessInputs, DetectionEvalWitnessOutput, ObjectKind, PlayfieldProjection, VisualTruthManifest,
  VisualTruthQueryActionWiringOutcome, VisualTruthQueryLiveClickExecutor, VisualTruthSemanticValidationInputs,
  VisualTruthSpatialQueryInputs, VisualTruthSpatialQueryOutput, build_detection_eval_quality, build_detection_eval_witness,
  evaluate_detection_fixture, export_dataset, query_visual_truth_spatial, run_benchmark, validate_visual_truth_semantic,
  visual_truth_query_action_wiring_lineage_from_manifest, wire_visual_truth_spatial_query_manifest_to_action,
};
use auv_runtime::model::AuvResult;
use auv_tracing::Context;

#[cfg(target_os = "macos")]
use self::query_live_action::DirectWindowPointClickExecutor;

pub async fn run_osu_benchmark(beatmap_path: PathBuf, output_dir: PathBuf) -> AuvResult<BenchmarkOutput> {
  run_osu_benchmark_with_inputs(BenchmarkInputs::new(beatmap_path, output_dir), "osu benchmark dry-run").await
}

pub async fn run_osu_benchmark_with_inputs(inputs: BenchmarkInputs, _operation_label: &str) -> AuvResult<BenchmarkOutput> {
  let result = run_benchmark(&inputs)?;
  publish_benchmark_projection(&result).await;
  Ok(result)
}

pub async fn run_osu_dataset_export(run_artifact_dir: PathBuf, output_dir: PathBuf) -> AuvResult<DatasetExportOutput> {
  // Benchmark/dataset directories are product outputs, not canonical run
  // artifact families in Task 21.
  export_dataset(&DatasetExportInputs {
    run_artifact_dir,
    output_dir,
  })
}

pub async fn run_osu_detection_eval(
  run_artifact_dir: PathBuf,
  detections_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<DetectionEvalOutput> {
  let result = evaluate_detection_fixture(&DetectionEvalInputs {
    run_artifact_dir,
    detections_path,
    output_dir,
  })?;
  let evidence = build_detection_eval_witness_quality(&result.output_dir)?;
  publish_detection_eval_evidence(&evidence).await;
  Ok(result)
}

#[derive(Clone, Debug, PartialEq)]
pub struct DetectionEvalWitnessQualityOutput {
  pub witness: DetectionEvalWitnessOutput,
  pub quality: DetectionEvalQualityOutput,
}

pub async fn run_osu_detection_eval_witness_quality(detection_eval_output_dir: PathBuf) -> AuvResult<DetectionEvalWitnessQualityOutput> {
  let result = build_detection_eval_witness_quality(&detection_eval_output_dir)?;
  publish_detection_eval_evidence(&result).await;
  Ok(result)
}

fn build_detection_eval_witness_quality(detection_eval_output_dir: &std::path::Path) -> AuvResult<DetectionEvalWitnessQualityOutput> {
  let witness = build_detection_eval_witness(&DetectionEvalWitnessInputs {
    detection_eval_output_dir: detection_eval_output_dir.to_path_buf(),
    output_dir: detection_eval_output_dir.join("witness"),
  })?;
  let quality = build_detection_eval_quality(&auv_game_osu::DetectionEvalQualityInputs {
    witness_manifest_path: witness.manifest_path.clone(),
    output_dir: detection_eval_output_dir.join("quality"),
  })?;
  Ok(DetectionEvalWitnessQualityOutput { witness, quality })
}

async fn publish_detection_eval_evidence(result: &DetectionEvalWitnessQualityOutput) {
  let context = Context::current();
  let _ = auv_game_osu::detection_eval_witness::publish_osu_detection_eval_witness(Some(&context), &result.witness.manifest).await;
  let _ = auv_game_osu::detection_eval_quality::publish_osu_detection_eval_quality(Some(&context), &result.quality.manifest).await;
}

pub async fn run_osu_visual_truth_semantic_validation(
  run_artifact_dir: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<auv_game_osu::VisualTruthSemanticValidationOutput> {
  let result = validate_visual_truth_semantic(VisualTruthSemanticValidationInputs {
    run_artifact_dir,
    output_dir,
  })?;
  let context = Context::current();
  let _ = auv_game_osu::visual_truth_semantic::publish_osu_visual_truth_semantic(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_osu_visual_truth_spatial_query(
  visual_truth_semantic_manifest_path: PathBuf,
  object_index: usize,
  capture_phase: CapturePhase,
  object_kind: Option<ObjectKind>,
  output_dir: PathBuf,
) -> AuvResult<VisualTruthSpatialQueryOutput> {
  let result = query_visual_truth_spatial(VisualTruthSpatialQueryInputs {
    visual_truth_semantic_manifest_path,
    object_index,
    capture_phase,
    object_kind,
    output_dir,
  })?;
  let context = Context::current();
  let _ = auv_game_osu::visual_truth_spatial_query::publish_osu_visual_truth_spatial_query(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_osu_vision_demo(
  beatmap_path: PathBuf,
  target_app: String,
  output_dir: PathBuf,
  dispatch_limit: Option<usize>,
  capture_verify: bool,
) -> AuvResult<BenchmarkOutput> {
  let mut inputs = BenchmarkInputs::typed_dispatch(beatmap_path, output_dir, target_app);
  inputs.dispatch_limit = Some(dispatch_limit.unwrap_or(8).min(8));
  inputs.capture_verify = capture_verify;
  // TODO(osu-vision-policy): broader vision-only control remains outside this
  // bounded local demo until the owner approves a product policy slice.
  let result = run_benchmark(&inputs)?;
  publish_benchmark_projection(&result).await;
  Ok(result)
}

async fn publish_benchmark_projection(result: &BenchmarkOutput) {
  if let Some(projection) = &result.projection {
    let context = Context::current();
    let _ = auv_game_osu::projection::publish_osu_projection(Some(&context), projection).await;
  }
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
}

pub async fn run_osu_query_wired_live_action(inputs: QueryWiredLiveActionInputs) -> AuvResult<QueryWiredLiveActionOutput> {
  #[cfg(target_os = "macos")]
  {
    let circle_size = circle_size_for_wired_live_action_inputs(&inputs)?;
    let live_projection = build_live_playfield_projection(&inputs.target_app, &inputs.target_title, circle_size)?;
    let executor = DirectWindowPointClickExecutor::new(inputs.target_app.clone(), inputs.target_title.clone());
    return run_osu_query_wired_live_action_with_executor(inputs, &live_projection, &executor).await;
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = inputs;
    Err("osu query wired live action requires macOS for live window projection".to_string())
  }
}

pub async fn run_osu_query_wired_live_action_with_executor<E: VisualTruthQueryLiveClickExecutor>(
  inputs: QueryWiredLiveActionInputs,
  live_projection: &PlayfieldProjection,
  executor: &E,
) -> AuvResult<QueryWiredLiveActionOutput> {
  let output = run_osu_query_wired_live_action_core(&inputs, live_projection, executor).await?;
  if let Some(action) = &output.wiring.input_action {
    let context = Context::current();
    let _ = auv_runtime::run_read::publish_input_action_result(Some(&context), action).await;
  }
  Ok(output)
}

async fn run_osu_query_wired_live_action_core<E: VisualTruthQueryLiveClickExecutor>(
  inputs: &QueryWiredLiveActionInputs,
  live_projection: &PlayfieldProjection,
  executor: &E,
) -> AuvResult<QueryWiredLiveActionOutput> {
  let query = run_osu_visual_truth_spatial_query(
    inputs.visual_truth_semantic_manifest_path.clone(),
    inputs.object_index,
    inputs.capture_phase.clone(),
    inputs.object_kind.clone(),
    inputs.output_dir.clone(),
  )
  .await?;
  let lineage = visual_truth_query_action_wiring_lineage_from_manifest(&query.manifest, &query.manifest_path);
  let wiring = wire_visual_truth_spatial_query_manifest_to_action(&query.manifest, &lineage, live_projection, executor);
  Ok(QueryWiredLiveActionOutput { query, wiring })
}

fn circle_size_for_wired_live_action_inputs(inputs: &QueryWiredLiveActionInputs) -> Result<f32, String> {
  let semantic_json = fs::read_to_string(&inputs.visual_truth_semantic_manifest_path).map_err(|error| {
    format!("failed to read osu visual truth semantic manifest {}: {error}", inputs.visual_truth_semantic_manifest_path.display())
  })?;
  let semantic: auv_game_osu::VisualTruthSemanticManifest = serde_json::from_str(&semantic_json).map_err(|error| {
    format!("failed to parse osu visual truth semantic manifest {}: {error}", inputs.visual_truth_semantic_manifest_path.display())
  })?;
  let manifest_json = fs::read_to_string(&semantic.source_visual_truth_manifest_path)
    .map_err(|error| format!("failed to read osu visual truth manifest {}: {error}", semantic.source_visual_truth_manifest_path))?;
  let manifest: VisualTruthManifest = serde_json::from_str(&manifest_json)
    .map_err(|error| format!("failed to parse osu visual truth manifest {}: {error}", semantic.source_visual_truth_manifest_path))?;
  Ok(manifest.map_summary.circle_size)
}

#[cfg(target_os = "macos")]
fn build_live_playfield_projection(target_app: &str, target_title: &str, circle_size: f32) -> Result<PlayfieldProjection, String> {
  let session = auv_driver::open_local().map_err(|error| error.to_string())?;
  let window = session
    .window()
    .resolve(auv_driver::WindowSelector::default().owned_by(auv_driver::App::name(target_app.to_string())).title_contains(target_title))
    .map_err(|error| error.to_string())?;
  PlayfieldProjection::for_window(&window, circle_size)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn direct_benchmark_rejects_missing_beatmap() {
    let root = std::env::temp_dir().join(format!("auv-osu-direct-{}", auv_runtime::model::now_millis()));
    let result = run_osu_benchmark(root.join("missing.osu"), root.join("out")).await;
    assert!(result.is_err());
  }
}
