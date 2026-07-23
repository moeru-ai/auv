use auv_game_minecraft::verify::{QueryWiredPostActionWitness, WorldDiffFailure, WorldDiffVerdict};
use auv_game_minecraft::{
  BlockPosition, MinecraftSpatialFrame, QueryActionWiringOutcome, TailFrameWaitConfig, verify_query_wired_live_action_semantic,
};

use super::QueryWiredLiveActionTelemetryWitness;
use auv_runtime::contract::{ArtifactRef, FailureLayer, VERIFICATION_RESULT_API_VERSION, VerificationMethod, VerificationResult};

const MC20_POST_FRAME_WAIT: TailFrameWaitConfig = TailFrameWaitConfig::new(750, 25);

pub fn map_world_diff_verdict_to_verification_result(verdict: &WorldDiffVerdict, evidence: Vec<ArtifactRef>) -> VerificationResult {
  let failure_layer = match verdict.failure {
    None => None,
    Some(WorldDiffFailure::VerificationUnreliable) => Some(FailureLayer::VerificationUnreliable),
    Some(WorldDiffFailure::StateChangedNoMatch) => Some(FailureLayer::StateChangedNoMatch),
    Some(WorldDiffFailure::SemanticMismatch) => Some(FailureLayer::SemanticMismatch),
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

pub fn build_query_wired_witness_absent_verification() -> VerificationResult {
  map_world_diff_verdict_to_verification_result(
    &WorldDiffVerdict {
      executed: true,
      state_changed: false,
      semantic_matched: None,
      failure: Some(WorldDiffFailure::VerificationUnreliable),
      observed_block_id: None,
      observed_item_delta: None,
    },
    Vec::new(),
  )
}

pub fn build_query_wired_witness_capture_failed_verification(reason: impl Into<String>) -> VerificationResult {
  let mut verification = build_query_wired_witness_absent_verification();
  verification.observed_label = Some(reason.into());
  verification
}

pub struct QueryWiredPostActionVerificationInput<'a> {
  pub telemetry_witness: Option<&'a QueryWiredLiveActionTelemetryWitness>,
  pub input_target_block: BlockPosition,
  pub manifest_target_block: BlockPosition,
  pub pre_frame: Option<MinecraftSpatialFrame>,
  pub verification_expected_item_id: Option<String>,
}

pub fn query_wired_verification_readable(wiring: &QueryActionWiringOutcome) -> bool {
  wiring.attempted && wiring.click_summary.is_some()
}

pub fn build_query_wired_post_action_verifications(
  wiring: &QueryActionWiringOutcome,
  input: QueryWiredPostActionVerificationInput<'_>,
) -> (Vec<VerificationResult>, bool) {
  if !query_wired_verification_readable(wiring) {
    return (Vec::new(), false);
  }
  if input.input_target_block != input.manifest_target_block {
    return (
      vec![build_query_wired_witness_capture_failed_verification(
        format!(
          "target_block input ({},{},{}) does not match query manifest ({},{},{})",
          input.input_target_block.x,
          input.input_target_block.y,
          input.input_target_block.z,
          input.manifest_target_block.x,
          input.manifest_target_block.y,
          input.manifest_target_block.z,
        ),
      )],
      false,
    );
  }
  let Some(witness) = input.telemetry_witness else {
    return (vec![build_query_wired_witness_absent_verification()], true);
  };
  let Some(pre) = input.pre_frame else {
    return (
      vec![build_query_wired_witness_capture_failed_verification(
        "pre frame missing after telemetry witness was configured",
      )],
      false,
    );
  };
  let post_sample_path = witness.post_telemetry_sample.as_ref().unwrap_or(&witness.pre_telemetry_sample);
  let post =
    match auv_game_minecraft::read_latest_spatial_frame_newer_than(post_sample_path, pre.monotonic_timestamp_ms, MC20_POST_FRAME_WAIT) {
      Ok(Some(frame)) => frame,
      Ok(None) => {
        return (
          vec![build_query_wired_witness_capture_failed_verification(
            format!("no valid minecraft post frame found in {}", post_sample_path.display()),
          )],
          false,
        );
      }
      Err(error) => return (vec![build_query_wired_witness_capture_failed_verification(error)], false),
    };
  let verdict = verify_query_wired_live_action_semantic(&QueryWiredPostActionWitness {
    target_block: input.manifest_target_block,
    pre_frame: pre,
    post_frame: post,
    expected_item_id: input.verification_expected_item_id,
  });
  // TODO(minecraft-spatial-frame-purpose-v1): pre/post spatial frame artifacts
  // are intentionally omitted because Task 20 defines no canonical purpose
  // for them. Add typed evidence URIs only after the owner approves one.
  (
    vec![map_world_diff_verdict_to_verification_result(
      &verdict,
      Vec::new(),
    )],
    false,
  )
}
