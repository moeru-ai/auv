//! Product-owned query-wired live-action readers.
//!
//! This adapter stays in the product CLI package because it reads `OperationResult`,
//! `VerificationResult`, and `FailureLayer` from `auv-runtime::contract`. Neutral
//! readiness and source-reference projection live in `query_wired_projection`;
//! app event names and summaries remain local here.
//!
//! TODO: Reconsider moving this adapter only after operation, verification, and
//! failure contracts have an approved neutral owner outside the product CLI package.

use super::query_wired_projection::{
  classify_manifest_source_readiness_lookup, format_source_readiness_ref, map_action_eligibility_to_readiness_class,
  resolve_query_wired_live_action_source_readiness_ref,
};
use super::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuQueryWiredLiveActionSummary {
  pub operation_result_artifact_id: Option<String>,
  pub query_artifact_id: Option<String>,
  pub attempted: bool,
  pub action_eligibility: String,
  pub pixel_point: Option<String>,
  pub window_point: Option<String>,
  pub refusal_reason: Option<String>,
  pub operation_status: Option<String>,
  pub operation_message: Option<String>,
  pub target_app: Option<String>,
  pub target_title: Option<String>,
  pub dispatch_command: Option<String>,
  pub dispatch_outcome: Option<String>,
  pub readiness_class: Option<String>,
  pub source_readiness_ref: Option<String>,
  pub verification_outcome: String,
  pub verification_source: Option<String>,
  pub verification_reason: Option<String>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftQueryWiredLiveActionSummary {
  pub attempted: bool,
  pub action_eligibility: String,
  pub refusal_reason: Option<String>,
  pub target_app: Option<String>,
  pub target_title: Option<String>,
  pub dispatch_command: Option<String>,
  pub dispatch_outcome: Option<String>,
  pub verification_outcome: String,
  pub verification_source: Option<String>,
  pub verification_reason: Option<String>,
}

fn parse_event_message_field(message: &str, key: &str) -> Option<String> {
  if key == "refusal_reason" {
    return parse_event_message_field_until(message, key, &["query_manifest_path"]);
  }
  let prefix = format!("{key}=");
  for token in message.split_whitespace() {
    if let Some(value) = token.strip_prefix(&prefix) {
      return Some(value.to_string());
    }
  }
  None
}

fn parse_event_message_field_until(message: &str, key: &str, stop_keys: &[&str]) -> Option<String> {
  let prefix = format!("{key}=");
  let start = message.find(&prefix)?;
  let rest = &message[start + prefix.len()..];
  let mut end = rest.len();
  for stop in stop_keys {
    if let Some(idx) = rest.find(&format!(" {stop}=")) {
      end = end.min(idx);
    }
  }
  let value = rest[..end].trim();
  if value.is_empty() {
    None
  } else {
    Some(value.to_string())
  }
}

fn operation_status_label(status: OperationStatus) -> &'static str {
  match status {
    OperationStatus::Completed => "completed",
    OperationStatus::Failed => "failed",
  }
}

fn operation_acknowledged_message(output: &OperationOutput) -> Option<String> {
  match output {
    OperationOutput::Acknowledged { message } => message.clone(),
    _ => None,
  }
}

fn query_artifact_id_from_operation_result(operation_result: &OperationResult) -> Option<String> {
  operation_result.evidence_artifacts.first().map(|artifact| artifact.artifact_id.as_str().to_string()).or_else(|| {
    operation_result
      .freshness_basis
      .as_ref()
      .and_then(|basis| basis.source_artifact.as_ref())
      .map(|artifact| artifact.artifact_id.as_str().to_string())
  })
}

fn find_query_wired_live_action_operation_result(store: &LocalStore, run: &CanonicalRun) -> Option<(ArtifactRefView, OperationResult)> {
  for artifact in &run.artifacts {
    if artifact.role != "operation-result" || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let parsed = read_artifact_json::<OperationResult>(store, run.run.run_id.as_str(), artifact, "operation-result").ok()?;
    if parsed.operation_id == QUERY_WIRED_LIVE_ACTION_OPERATION_ID {
      return Some((artifact_record_view(run.run.run_id.clone(), artifact), parsed));
    }
  }
  None
}

fn derive_dispatch_evidence_from_events(run: &CanonicalRun) -> (Option<String>, Option<String>) {
  let mut dispatch_command = None;
  let mut dispatch_outcome = None;
  for event in &run.events {
    if event.name == "command.resolved" && event.message.as_deref() == Some("resolved input.clickWindowPoint") {
      dispatch_command = Some("input.clickWindowPoint".to_string());
      dispatch_outcome = Some("resolved".to_string());
    }
    if event.name == "command.failed" && dispatch_command.is_some() {
      if let Some(message) = event.message.as_deref() {
        dispatch_outcome = Some(format!("failed: {message}"));
      }
    }
  }
  (dispatch_command, dispatch_outcome)
}

// NOTICE(core-c3-d2): reader-side Layer 3 summary only — verification_outcome projection.
// Still product-local: depends on OperationResult / VerificationResult / FailureLayer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueryWiredLiveActionVerificationProjection {
  pub(crate) verification_outcome: String,
  pub(crate) verification_source: Option<String>,
  pub(crate) verification_reason: Option<String>,
}

fn format_operation_result_verification_source(artifact_id: &str, run_id: &str) -> String {
  format_source_readiness_ref(&[
    ("kind", "operation_result"),
    ("artifact_id", artifact_id),
    ("run_id", run_id),
  ])
}

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

pub(crate) fn resolve_query_wired_live_action_verification_projection(
  attempted: bool,
  operation_result_artifact_id: Option<&str>,
  operation_result: Option<&OperationResult>,
  run_id: &str,
  refusal_reason: Option<&str>,
) -> QueryWiredLiveActionVerificationProjection {
  if !attempted {
    return QueryWiredLiveActionVerificationProjection {
      verification_outcome: "not_attempted".to_string(),
      verification_source: Some(format_source_readiness_ref(&[("kind", "layer1_no_dispatch")])),
      verification_reason: refusal_reason
        .filter(|reason| !reason.is_empty() && *reason != "none")
        .map(str::to_string)
        .or_else(|| Some("post-action verification N/A; action not dispatched".to_string())),
    };
  }

  let Some(operation_result) = operation_result else {
    return QueryWiredLiveActionVerificationProjection {
      verification_outcome: "absent".to_string(),
      verification_source: None,
      verification_reason: Some("attempted=true but operation-result artifact missing on read path".to_string()),
    };
  };

  let verification_source = operation_result_artifact_id.map(|artifact_id| format_operation_result_verification_source(artifact_id, run_id));
  let claims = operation_result_verification_claims(operation_result);
  if claims.is_empty() {
    return QueryWiredLiveActionVerificationProjection {
      verification_outcome: "absent".to_string(),
      verification_source,
      verification_reason: operation_result
        .known_limits
        .first()
        .cloned()
        .or_else(|| Some("no VerificationResult on operation-result; Layer 3 evidence absent".to_string())),
    };
  }

  let (verification_outcome, verification_reason) = project_verification_outcome_from_claims(&claims);
  QueryWiredLiveActionVerificationProjection {
    verification_outcome,
    verification_source,
    verification_reason,
  }
}

pub fn derive_minecraft_query_wired_live_action_summary(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Option<MinecraftQueryWiredLiveActionSummary> {
  let outcome_event = run.events.iter().find(|event| event.name == "minecraft.query_wired_live_action.outcome");
  let operation_result_pair = find_query_wired_live_action_operation_result(store, run);
  if outcome_event.is_none() && operation_result_pair.is_none() {
    return None;
  }

  let (attempted, action_eligibility, refusal_reason) = if let Some(event) = outcome_event {
    let message = event.message.as_deref().unwrap_or("");
    let attempted = parse_event_message_field(message, "attempted").is_some_and(|value| value == "true");
    let action_eligibility = parse_event_message_field(message, "action_eligibility").unwrap_or_else(|| "n/a".to_string());
    let refusal_reason = parse_event_message_field(message, "refusal_reason").filter(|value| value != "none");
    (attempted, action_eligibility, refusal_reason)
  } else {
    (false, "n/a".to_string(), None)
  };

  let inputs_event = run.events.iter().find(|event| event.name == "minecraft.query_wired_live_action.inputs");
  let target_app =
    inputs_event.and_then(|event| event.message.as_deref()).and_then(|message| parse_event_message_field(message, "target_app"));
  let target_title =
    inputs_event.and_then(|event| event.message.as_deref()).and_then(|message| parse_event_message_field(message, "target_title"));

  let operation_result_artifact_id = operation_result_pair.as_ref().map(|(artifact_ref, _)| artifact_ref.artifact_id.as_str().to_string());

  let (dispatch_command, dispatch_outcome) = derive_dispatch_evidence_from_events(run);

  let run_id = run.run.run_id.as_str();
  let operation_result_ref = operation_result_pair.as_ref().map(|(_, result)| result);
  let verification_projection = resolve_query_wired_live_action_verification_projection(
    attempted,
    operation_result_artifact_id.as_deref(),
    operation_result_ref,
    run_id,
    refusal_reason.as_deref(),
  );

  Some(MinecraftQueryWiredLiveActionSummary {
    attempted,
    action_eligibility,
    refusal_reason,
    target_app,
    target_title,
    dispatch_command,
    dispatch_outcome,
    verification_outcome: verification_projection.verification_outcome,
    verification_source: verification_projection.verification_source,
    verification_reason: verification_projection.verification_reason,
  })
}

fn find_osu_query_wired_live_action_operation_result(store: &LocalStore, run: &CanonicalRun) -> Option<(ArtifactRefView, OperationResult)> {
  for artifact in &run.artifacts {
    if artifact.role != "operation-result" || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let parsed = read_artifact_json::<OperationResult>(store, run.run.run_id.as_str(), artifact, "operation-result").ok()?;
    if parsed.operation_id == OSU_QUERY_WIRED_LIVE_ACTION_OPERATION_ID {
      return Some((artifact_record_view(run.run.run_id.clone(), artifact), parsed));
    }
  }
  None
}

pub fn derive_osu_query_wired_live_action_summary(store: &LocalStore, run: &CanonicalRun) -> Option<OsuQueryWiredLiveActionSummary> {
  let outcome_event = run.events.iter().find(|event| event.name == "osu.query_wired_live_action.outcome");
  let operation_result_pair = find_osu_query_wired_live_action_operation_result(store, run);
  if outcome_event.is_none() && operation_result_pair.is_none() {
    return None;
  }

  let mut issue = None;
  let (attempted, action_eligibility, refusal_reason, pixel_point, window_point) = if let Some(event) = outcome_event {
    let message = event.message.as_deref().unwrap_or("");
    let attempted = parse_event_message_field(message, "attempted").is_some_and(|value| value == "true");
    let action_eligibility = parse_event_message_field(message, "action_eligibility").unwrap_or_else(|| "n/a".to_string());
    let refusal_reason = parse_event_message_field(message, "refusal_reason").filter(|value| value != "none");
    let pixel_point = parse_event_message_field(message, "pixel_point").filter(|value| value != "none");
    let window_point = parse_event_message_field(message, "window_point").filter(|value| value != "none");
    (attempted, action_eligibility, refusal_reason, pixel_point, window_point)
  } else {
    (false, "n/a".to_string(), None, None, None)
  };

  let inputs_event = run.events.iter().find(|event| event.name == "osu.query_wired_live_action.inputs");
  let target_app =
    inputs_event.and_then(|event| event.message.as_deref()).and_then(|message| parse_event_message_field(message, "target_app"));
  let target_title =
    inputs_event.and_then(|event| event.message.as_deref()).and_then(|message| parse_event_message_field(message, "target_title"));

  let (operation_result_artifact_id, query_artifact_id, operation_status, operation_message) =
    if let Some((ref artifact_ref, ref operation_result)) = operation_result_pair {
      (
        Some(artifact_ref.artifact_id.as_str().to_string()),
        query_artifact_id_from_operation_result(operation_result),
        Some(operation_status_label(operation_result.status).to_string()),
        operation_acknowledged_message(&operation_result.output),
      )
    } else {
      (None, None, None, None)
    };

  let (dispatch_command, dispatch_outcome) = derive_dispatch_evidence_from_events(run);

  let run_id = run.run.run_id.as_str();
  let manifest_extract = query_artifact_id.as_deref().map(|_| extract_osu_visual_truth_spatial_query_manifests(store, run));

  let mut readiness_class = map_action_eligibility_to_readiness_class(&action_eligibility);
  if let Some(query_id) = query_artifact_id.as_deref() {
    if let Some(Ok(ref manifests)) = manifest_extract {
      if let Some(lineage) = manifests.iter().find(|manifest| manifest.artifact.artifact_id.as_str() == query_id) {
        let readiness = derive_osu_visual_truth_spatial_query_action_readiness(lineage);
        readiness_class = map_action_eligibility_to_readiness_class(&readiness.action_eligibility);
        if readiness.issue.is_some() {
          issue = readiness.issue;
        }
      }
    }
  }

  let manifest_lookup = query_artifact_id.as_deref().and_then(|query_id| {
    manifest_extract.as_ref().and_then(|extract_result| {
      classify_manifest_source_readiness_lookup(
        query_id,
        extract_result,
        |lineage: &OsuVisualTruthSpatialQueryManifestLineage| lineage.artifact.artifact_id.as_str(),
        |lineage| lineage.manifest.is_some(),
      )
    })
  });
  let source_readiness_ref = resolve_query_wired_live_action_source_readiness_ref(
    run_id,
    query_artifact_id.as_deref(),
    operation_result_artifact_id.as_deref(),
    "osu.query_wired_live_action.outcome",
    outcome_event.is_some(),
    manifest_lookup,
  );
  let operation_result_ref = operation_result_pair.as_ref().map(|(_, result)| result);
  let verification_projection = resolve_query_wired_live_action_verification_projection(
    attempted,
    operation_result_artifact_id.as_deref(),
    operation_result_ref,
    run_id,
    refusal_reason.as_deref(),
  );

  Some(OsuQueryWiredLiveActionSummary {
    operation_result_artifact_id,
    query_artifact_id,
    attempted,
    action_eligibility,
    pixel_point,
    window_point,
    refusal_reason,
    operation_status,
    operation_message,
    target_app,
    target_title,
    dispatch_command,
    dispatch_outcome,
    readiness_class,
    source_readiness_ref,
    verification_outcome: verification_projection.verification_outcome,
    verification_source: verification_projection.verification_source,
    verification_reason: verification_projection.verification_reason,
    issue,
  })
}

pub(crate) fn list_osu_query_wired_live_action_summaries(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<OsuQueryWiredLiveActionSummary>> {
  let run = store.read_run(run_id)?;
  Ok(derive_osu_query_wired_live_action_summary(store, &run).into_iter().collect())
}

pub(crate) fn list_minecraft_query_wired_live_action_summaries(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftQueryWiredLiveActionSummary>> {
  let run = store.read_run(run_id)?;
  Ok(derive_minecraft_query_wired_live_action_summary(store, &run).into_iter().collect())
}
