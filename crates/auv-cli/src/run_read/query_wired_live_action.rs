//! Product-owned verification projection for query-wired presentation.

use auv_runtime::contract::{FailureLayer, OperationOutput, OperationResult, VerificationMethod, VerificationResult};

pub(crate) fn operation_result_verification_claims(operation_result: &OperationResult) -> Vec<&VerificationResult> {
  if !operation_result.verifications.is_empty() {
    return operation_result.verifications.iter().collect();
  }
  if let OperationOutput::Verification { verification } = &operation_result.output {
    return vec![verification];
  }
  Vec::new()
}

fn is_activation_only_verification(verification: &VerificationResult) -> bool {
  matches!(
    &verification.method,
    VerificationMethod::Custom { name } if name == "activation_only"
  )
}

fn verification_failure_layer_label(layer: FailureLayer) -> &'static str {
  match layer {
    FailureLayer::GroundingFailed => "grounding_failed",
    FailureLayer::CandidateExpired => "candidate_expired",
    FailureLayer::ControlFailed => "control_failed",
    FailureLayer::VerificationUnreliable => "verification_unreliable",
    FailureLayer::StateChangedNoMatch => "state_changed_no_match",
    FailureLayer::SemanticMismatch => "semantic_mismatch",
  }
}

fn verification_claim_reason_snippet(verification: &VerificationResult) -> Option<String> {
  if let Some(observed_label) = verification.observed_label.as_deref().filter(|label| !label.is_empty()) {
    return Some(observed_label.to_string());
  }
  verification.failure_layer.map(verification_failure_layer_label).map(str::to_string)
}

fn build_verification_reason_from_claims(claims: &[&VerificationResult]) -> Option<String> {
  let mut parts = claims.iter().filter_map(|claim| verification_claim_reason_snippet(claim)).collect::<Vec<_>>();
  parts.dedup();
  if parts.is_empty() {
    None
  } else {
    Some(parts.join("; "))
  }
}

pub(crate) fn project_verification_outcome_from_claims(claims: &[&VerificationResult]) -> (String, Option<String>) {
  let semantic_claims = claims.iter().copied().filter(|claim| !is_activation_only_verification(claim)).collect::<Vec<_>>();
  let focus: &[&VerificationResult] = if semantic_claims.is_empty() {
    claims
  } else {
    &semantic_claims
  };

  for claim in focus {
    if matches!(claim.failure_layer, Some(FailureLayer::VerificationUnreliable)) {
      return ("unreliable".to_string(), build_verification_reason_from_claims(focus));
    }
  }

  for claim in focus {
    if matches!(claim.failure_layer, Some(FailureLayer::SemanticMismatch | FailureLayer::StateChangedNoMatch))
      || claim.semantic_matched == Some(false)
    {
      return ("failed".to_string(), build_verification_reason_from_claims(focus));
    }
  }

  if focus.iter().all(|claim| is_activation_only_verification(claim)) {
    return (
      "activation_only".to_string(),
      build_verification_reason_from_claims(focus)
        .or_else(|| Some("input delivery recorded; no semantic post-action assertion".to_string())),
    );
  }

  if focus.iter().any(|claim| claim.semantic_matched == Some(true)) {
    return ("passed".to_string(), build_verification_reason_from_claims(focus));
  }

  if focus.iter().any(|claim| claim.state_changed && claim.semantic_matched.is_none()) {
    return ("inconclusive".to_string(), build_verification_reason_from_claims(focus));
  }

  ("absent".to_string(), Some("verification claims present but not mappable to a read-side outcome".to_string()))
}
