use std::path::PathBuf;

use auv_game_balatro::{
  CardDetectionEvalWitnessInputs, CardDetectionEvalWitnessOutput, CardDetectionQualityInputs, CardDetectionQualityOutput,
  CardDetectionSemanticValidationInputs, CardDetectionSemanticValidationOutput, CardDetectionSpatialQueryInputs,
  CardDetectionSpatialQueryOutput, SlotId, build_card_detection_eval_witness, build_card_detection_quality, query_card_detection_spatial,
  validate_card_detection_semantic,
};
use auv_tracing_driver::RecordingHandle;
use auv_tracing_driver::recorded_operation::RecordedOperationOutput;
use auv_tracing_driver::run_builder::RunSpec;
use auv_tracing_driver::trace::RunType;

use auv_runtime::model::AuvResult;

pub use auv_game_balatro::{
  BALATRO_CARD_DETECTION_EVAL_WITNESS_INSPECT_ROLE, BALATRO_CARD_DETECTION_EVAL_WITNESS_ROLE, BALATRO_CARD_DETECTION_QUALITY_INSPECT_ROLE,
  BALATRO_CARD_DETECTION_QUALITY_ROLE, BALATRO_CARD_DETECTION_SEMANTIC_INSPECT_ROLE, BALATRO_CARD_DETECTION_SEMANTIC_ROLE,
  BALATRO_CARD_DETECTION_SPATIAL_QUERY_INSPECT_ROLE, BALATRO_CARD_DETECTION_SPATIAL_QUERY_ROLE,
};

#[derive(Clone, Debug, PartialEq)]
pub struct BalatroConsumptionProbeChainOutput {
  pub semantic: CardDetectionSemanticValidationOutput,
  pub query: CardDetectionSpatialQueryOutput,
  pub witness: CardDetectionEvalWitnessOutput,
  pub quality: CardDetectionQualityOutput,
}

pub fn run_balatro_card_detection_semantic_validation(
  recording: &RecordingHandle,
  bundle_input: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<CardDetectionSemanticValidationOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.balatro.validate_card_detection_semantic"),
    "balatro validate card detection semantic gate",
    |context| {
      context.record_event(
        "balatro.validate_card_detection_semantic.inputs",
        Some(format!("bundle_input={} output_dir={}", bundle_input.display(), output_dir.display())),
      );
      let result = validate_card_detection_semantic(CardDetectionSemanticValidationInputs {
        bundle_input: bundle_input.clone(),
        output_dir: output_dir.clone(),
      })?;
      context.in_span("balatro.validate_card_detection_semantic.artifacts", |context| {
        for (artifact_name, role) in [
          ("balatro-card-detection-semantic.json", BALATRO_CARD_DETECTION_SEMANTIC_ROLE),
          ("balatro-card-detection-semantic-inspect.json", BALATRO_CARD_DETECTION_SEMANTIC_INSPECT_ROLE),
        ] {
          let artifact_path = result.output_dir.join(artifact_name);
          if artifact_path.exists() {
            context.stage_artifact_file(
              role,
              &artifact_path,
              artifact_name,
              Some(format!("balatro card detection semantic artifact {artifact_name}")),
            )?;
          }
        }
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_balatro_card_detection_spatial_query(
  recording: &RecordingHandle,
  card_detection_semantic_manifest_path: PathBuf,
  target_slot: SlotId,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<CardDetectionSpatialQueryOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.balatro.query_card_detection_spatial"),
    "balatro query card detection spatial target",
    |context| {
      context.record_event(
        "balatro.query_card_detection_spatial.inputs",
        Some(format!(
          "semantic_manifest={} target_slot={target_slot} output_dir={}",
          card_detection_semantic_manifest_path.display(),
          output_dir.display()
        )),
      );
      let result = query_card_detection_spatial(CardDetectionSpatialQueryInputs {
        card_detection_semantic_manifest_path: card_detection_semantic_manifest_path.clone(),
        target_slot,
        output_dir: output_dir.clone(),
      })?;
      context.in_span("balatro.query_card_detection_spatial.artifacts", |context| {
        for (artifact_name, role) in [
          ("balatro-card-detection-spatial-query.json", BALATRO_CARD_DETECTION_SPATIAL_QUERY_ROLE),
          ("balatro-card-detection-spatial-query-inspect.json", BALATRO_CARD_DETECTION_SPATIAL_QUERY_INSPECT_ROLE),
        ] {
          let artifact_path = result.output_dir.join(artifact_name);
          if artifact_path.exists() {
            context.stage_artifact_file(
              role,
              &artifact_path,
              artifact_name,
              Some(format!("balatro card detection spatial query artifact {artifact_name}")),
            )?;
          }
        }
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_balatro_card_detection_eval_witness(
  recording: &RecordingHandle,
  card_detection_semantic_manifest_path: PathBuf,
  card_detection_spatial_query_manifest_path: PathBuf,
  expected_slots_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<CardDetectionEvalWitnessOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.balatro.build_card_detection_eval_witness"),
    "balatro card detection eval witness evidence",
    |context| {
      context.record_event(
        "balatro.build_card_detection_eval_witness.inputs",
        Some(format!(
          "semantic_manifest={} spatial_query_manifest={} expected_slots={} output_dir={}",
          card_detection_semantic_manifest_path.display(),
          card_detection_spatial_query_manifest_path.display(),
          expected_slots_path.display(),
          output_dir.display()
        )),
      );
      let result = build_card_detection_eval_witness(&CardDetectionEvalWitnessInputs {
        card_detection_semantic_manifest_path: card_detection_semantic_manifest_path.clone(),
        card_detection_spatial_query_manifest_path: card_detection_spatial_query_manifest_path.clone(),
        expected_slots_path: expected_slots_path.clone(),
        output_dir: output_dir.clone(),
      })?;
      context.in_span("balatro.build_card_detection_eval_witness.artifacts", |context| {
        for (artifact_name, role) in [
          ("balatro-card-detection-eval-witness.json", BALATRO_CARD_DETECTION_EVAL_WITNESS_ROLE),
          ("balatro-card-detection-eval-witness-inspect.json", BALATRO_CARD_DETECTION_EVAL_WITNESS_INSPECT_ROLE),
        ] {
          let artifact_path = result.output_dir.join(artifact_name);
          if artifact_path.exists() {
            context.stage_artifact_file(
              role,
              &artifact_path,
              artifact_name,
              Some(format!("balatro card detection eval witness artifact {artifact_name}")),
            )?;
          }
        }
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_balatro_card_detection_quality(
  recording: &RecordingHandle,
  witness_manifest_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<CardDetectionQualityOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.balatro.build_card_detection_quality"),
    "balatro card detection quality evidence",
    |context| {
      context.record_event(
        "balatro.build_card_detection_quality.inputs",
        Some(format!("witness_manifest={} output_dir={}", witness_manifest_path.display(), output_dir.display())),
      );
      let result = build_card_detection_quality(&CardDetectionQualityInputs {
        witness_manifest_path: witness_manifest_path.clone(),
        output_dir: output_dir.clone(),
      })?;
      context.in_span("balatro.build_card_detection_quality.artifacts", |context| {
        for (artifact_name, role) in [
          ("balatro-card-detection-quality.json", BALATRO_CARD_DETECTION_QUALITY_ROLE),
          ("balatro-card-detection-quality-inspect.json", BALATRO_CARD_DETECTION_QUALITY_INSPECT_ROLE),
        ] {
          let artifact_path = result.output_dir.join(artifact_name);
          if artifact_path.exists() {
            context.stage_artifact_file(
              role,
              &artifact_path,
              artifact_name,
              Some(format!("balatro card detection quality artifact {artifact_name}")),
            )?;
          }
        }
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_balatro_consumption_probe_chain(
  recording: &RecordingHandle,
  bundle_input: PathBuf,
  expected_slots_path: PathBuf,
  target_slot: SlotId,
  work_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<BalatroConsumptionProbeChainOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.balatro.consumption_probe_chain"),
    "balatro consumption probe chain",
    |context| {
      context.record_event(
        "balatro.consumption_probe_chain.inputs",
        Some(format!(
          "bundle_input={} expected_slots={} target_slot={target_slot} work_dir={}",
          bundle_input.display(),
          expected_slots_path.display(),
          work_dir.display()
        )),
      );
      let semantic = validate_card_detection_semantic(CardDetectionSemanticValidationInputs {
        bundle_input: bundle_input.clone(),
        output_dir: work_dir.join("semantic"),
      })?;
      let query = query_card_detection_spatial(CardDetectionSpatialQueryInputs {
        card_detection_semantic_manifest_path: semantic.manifest_path.clone(),
        target_slot,
        output_dir: work_dir.join("query"),
      })?;
      let witness = build_card_detection_eval_witness(&CardDetectionEvalWitnessInputs {
        card_detection_semantic_manifest_path: semantic.manifest_path.clone(),
        card_detection_spatial_query_manifest_path: query.manifest_path.clone(),
        expected_slots_path: expected_slots_path.clone(),
        output_dir: work_dir.join("witness"),
      })?;
      let quality = build_card_detection_quality(&CardDetectionQualityInputs {
        witness_manifest_path: witness.manifest_path.clone(),
        output_dir: work_dir.join("quality"),
      })?;
      context.in_span("balatro.consumption_probe_chain.artifacts", |context| {
        stage_balatro_probe_artifacts(context, &semantic, &query, &witness, &quality)?;
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(BalatroConsumptionProbeChainOutput {
        semantic,
        query,
        witness,
        quality,
      })
    },
  )
}

fn stage_balatro_probe_artifacts(
  context: &mut auv_tracing_driver::recorded_operation::RecordedOperationContext<'_>,
  semantic: &CardDetectionSemanticValidationOutput,
  query: &CardDetectionSpatialQueryOutput,
  witness: &CardDetectionEvalWitnessOutput,
  quality: &CardDetectionQualityOutput,
) -> Result<(), String> {
  for (path, role, name) in [
    (&semantic.manifest_path, BALATRO_CARD_DETECTION_SEMANTIC_ROLE, "balatro-card-detection-semantic.json"),
    (&semantic.inspect_report_path, BALATRO_CARD_DETECTION_SEMANTIC_INSPECT_ROLE, "balatro-card-detection-semantic-inspect.json"),
    (&query.manifest_path, BALATRO_CARD_DETECTION_SPATIAL_QUERY_ROLE, "balatro-card-detection-spatial-query.json"),
    (&query.inspect_report_path, BALATRO_CARD_DETECTION_SPATIAL_QUERY_INSPECT_ROLE, "balatro-card-detection-spatial-query-inspect.json"),
    (&witness.manifest_path, BALATRO_CARD_DETECTION_EVAL_WITNESS_ROLE, "balatro-card-detection-eval-witness.json"),
    (&witness.inspect_report_path, BALATRO_CARD_DETECTION_EVAL_WITNESS_INSPECT_ROLE, "balatro-card-detection-eval-witness-inspect.json"),
    (&quality.manifest_path, BALATRO_CARD_DETECTION_QUALITY_ROLE, "balatro-card-detection-quality.json"),
    (&quality.inspect_report_path, BALATRO_CARD_DETECTION_QUALITY_INSPECT_ROLE, "balatro-card-detection-quality-inspect.json"),
  ] {
    if path.exists() {
      context.stage_artifact_file(role, path, name, Some(format!("balatro consumption probe artifact {name}")))?;
    }
  }
  Ok(())
}
