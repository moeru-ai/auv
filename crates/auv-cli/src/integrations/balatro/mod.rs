use std::path::PathBuf;

use auv_game_balatro::{
  CardDetectionEvalWitnessInputs, CardDetectionEvalWitnessOutput, CardDetectionQualityInputs, CardDetectionQualityOutput,
  CardDetectionSemanticValidationInputs, CardDetectionSemanticValidationOutput, CardDetectionSpatialQueryInputs,
  CardDetectionSpatialQueryOutput, SlotId, build_card_detection_eval_witness, build_card_detection_quality, query_card_detection_spatial,
  validate_card_detection_semantic,
};
use auv_runtime::model::AuvResult;
use auv_tracing::Context;

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

pub async fn run_balatro_card_detection_semantic_validation(
  bundle_input: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<CardDetectionSemanticValidationOutput> {
  let result = validate_card_detection_semantic(CardDetectionSemanticValidationInputs {
    bundle_input,
    output_dir,
  })?;
  let context = Context::current();
  let _ = auv_game_balatro::card_detection_semantic::publish_card_detection_semantic(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_balatro_card_detection_spatial_query(
  card_detection_semantic_manifest_path: PathBuf,
  target_slot: SlotId,
  output_dir: PathBuf,
) -> AuvResult<CardDetectionSpatialQueryOutput> {
  let result = query_card_detection_spatial(CardDetectionSpatialQueryInputs {
    card_detection_semantic_manifest_path,
    target_slot,
    output_dir,
  })?;
  let context = Context::current();
  let _ = auv_game_balatro::card_detection_spatial_query::publish_card_detection_spatial_query(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_balatro_card_detection_eval_witness(
  card_detection_semantic_manifest_path: PathBuf,
  card_detection_spatial_query_manifest_path: PathBuf,
  expected_slots_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<CardDetectionEvalWitnessOutput> {
  let result = build_card_detection_eval_witness(&CardDetectionEvalWitnessInputs {
    card_detection_semantic_manifest_path,
    card_detection_spatial_query_manifest_path,
    expected_slots_path,
    output_dir,
  })?;
  let context = Context::current();
  let _ = auv_game_balatro::card_detection_eval_witness::publish_card_detection_witness(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_balatro_card_detection_quality(
  witness_manifest_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<CardDetectionQualityOutput> {
  let result = build_card_detection_quality(&CardDetectionQualityInputs {
    witness_manifest_path,
    output_dir,
  })?;
  let context = Context::current();
  let _ = auv_game_balatro::card_detection_quality::publish_card_detection_quality(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_balatro_consumption_probe_chain(
  bundle_input: PathBuf,
  expected_slots_path: PathBuf,
  target_slot: SlotId,
  work_dir: PathBuf,
) -> AuvResult<BalatroConsumptionProbeChainOutput> {
  let semantic = run_balatro_card_detection_semantic_validation(bundle_input, work_dir.join("semantic")).await?;
  let query = run_balatro_card_detection_spatial_query(semantic.manifest_path.clone(), target_slot, work_dir.join("query")).await?;
  let witness = run_balatro_card_detection_eval_witness(
    semantic.manifest_path.clone(),
    query.manifest_path.clone(),
    expected_slots_path,
    work_dir.join("witness"),
  )
  .await?;
  let quality = run_balatro_card_detection_quality(witness.manifest_path.clone(), work_dir.join("quality")).await?;
  Ok(BalatroConsumptionProbeChainOutput {
    semantic,
    query,
    witness,
    quality,
  })
}
