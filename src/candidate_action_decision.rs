use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::action_resolver_decision::{ActionResolverDecision, ActionResolverDecisionInput};
use crate::candidate_promotion::CandidatePromotion;
use crate::candidate_promotion_recording::CandidatePromotionArtifact;
use crate::contract::{
  ArtifactRef, Candidate, CandidateRef, FailureLayer, OperationOutput, OperationResult,
  OperationStatus, TargetGrounding, VERIFICATION_RESULT_API_VERSION, VerificationMethod,
  VerificationResult,
};
use crate::model::{AuvResult, now_millis};
use crate::recorded_operation::RecordedOperationContext;
use crate::trace::RunId;

pub const CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE: &str = "candidate-action-decision";
pub const CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE: &str = "candidate-action-execution";
const CANDIDATE_ACTION_DECISION_ARTIFACT_VERSION: &str = "candidate_action_decision_artifact_v0";
const CANDIDATE_ACTION_EXECUTION_ARTIFACT_VERSION: &str = "candidate_action_execution_artifact_v0";
const OPERATION_RESULT_ARTIFACT_ROLE: &str = "operation-result";
static TEMP_JSON_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateActionDecisionRequest {
  pub(crate) decision_id: String,
  pub(crate) source_candidate_promotion_artifact: Option<ArtifactRef>,
  pub(crate) candidate_local_id: Option<String>,
  pub(crate) artifact_role: String,
  pub(crate) artifact_label: String,
  pub(crate) artifact_note: String,
}

impl CandidateActionDecisionRequest {
  pub fn new(decision_id: impl Into<String>, artifact_label: impl Into<String>) -> Self {
    let decision_id = decision_id.into();
    Self {
      decision_id: decision_id.clone(),
      source_candidate_promotion_artifact: None,
      candidate_local_id: None,
      artifact_role: CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE.to_string(),
      artifact_label: artifact_label.into(),
      artifact_note: "Decide-only candidate action resolver decision artifact.".to_string(),
    }
  }

  pub fn with_source_candidate_promotion_artifact(mut self, artifact: ArtifactRef) -> Self {
    self.source_candidate_promotion_artifact = Some(artifact);
    self
  }

  pub fn with_candidate_local_id(mut self, candidate_local_id: impl Into<String>) -> Self {
    self.candidate_local_id = Some(candidate_local_id.into());
    self
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateActionDecisionArtifact {
  pub(crate) artifact_version: String,
  pub(crate) decision_id: String,
  pub(crate) source_candidate_promotion_artifact: Option<ArtifactRef>,
  pub(crate) source_promotion_id: String,
  pub(crate) candidate_local_id: String,
  pub(crate) action_resolver_decision: ActionResolverDecision,
  pub(crate) side_effect: CandidateActionSideEffect,
  pub(crate) detail: serde_json::Value,
  pub(crate) known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateActionSideEffect {
  NoneDecideOnly,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidateActionExecutionRequest {
  pub(crate) execution_id: String,
  pub(crate) source_candidate_action_decision_artifact: Option<ArtifactRef>,
  pub(crate) consent: Option<CandidateActionExecutionConsent>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub(crate) readiness: Option<auv_driver::ReadinessReport>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub(crate) post_action_probe: Option<CandidateActionPostActionProbe>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub(crate) post_action_verifications: Vec<VerificationResult>,
  pub(crate) artifact_role: String,
  pub(crate) artifact_label: String,
  pub(crate) artifact_note: String,
}

impl CandidateActionExecutionRequest {
  pub fn new(execution_id: impl Into<String>, artifact_label: impl Into<String>) -> Self {
    let execution_id = execution_id.into();
    Self {
      execution_id: execution_id.clone(),
      source_candidate_action_decision_artifact: None,
      consent: None,
      readiness: None,
      post_action_probe: None,
      post_action_verifications: Vec::new(),
      artifact_role: CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE.to_string(),
      artifact_label: artifact_label.into(),
      artifact_note: "Single-action candidate execution artifact.".to_string(),
    }
  }

  pub fn with_source_candidate_action_decision_artifact(mut self, artifact: ArtifactRef) -> Self {
    self.source_candidate_action_decision_artifact = Some(artifact);
    self
  }

  pub fn with_consent(mut self, consent: CandidateActionExecutionConsent) -> Self {
    self.consent = Some(consent);
    self
  }

  pub fn with_readiness(mut self, readiness: auv_driver::ReadinessReport) -> Self {
    self.readiness = Some(readiness);
    self
  }

  pub fn with_post_action_probe(mut self, probe: CandidateActionPostActionProbe) -> Self {
    self.post_action_probe = Some(probe);
    self
  }

  pub fn with_post_action_verification(mut self, verification: VerificationResult) -> Self {
    self.post_action_verifications.push(verification);
    self
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidateActionPostActionProbe {
  pub kind: CandidateActionPostActionProbeKind,
  pub require_frontmost: bool,
  pub bounds_tolerance_px: f64,
}

impl CandidateActionPostActionProbe {
  pub fn focused_ax_node_reobserved() -> Self {
    Self {
      kind: CandidateActionPostActionProbeKind::FocusedAxNodeReobserved,
      require_frontmost: true,
      bounds_tolerance_px: 2.0,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateActionPostActionProbeKind {
  FocusedAxNodeReobserved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateActionExecutionConsent {
  pub consent_id: String,
  pub granted_by: String,
  pub scope_note: String,
  pub run_id: String,
  pub source_promotion_id: String,
  pub source_decision_id: String,
  pub candidate_local_id: String,
  pub approved_action: CandidateActionExecutionConsentAction,
  pub approved_at_millis: u64,
  pub evidence_note: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateActionExecutionConsentAction {
  ExecuteSingleCandidateAction,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidateActionExecutionArtifact {
  pub(crate) artifact_version: String,
  pub(crate) execution_id: String,
  pub(crate) source_candidate_action_decision_artifact: ArtifactRef,
  pub(crate) source_candidate_promotion_artifact: Option<ArtifactRef>,
  pub(crate) operation_result_artifact: Option<ArtifactRef>,
  pub(crate) source_promotion_id: String,
  pub(crate) source_decision_id: String,
  pub(crate) candidate_local_id: String,
  pub(crate) action_resolver_decision: ActionResolverDecision,
  pub(crate) consent: CandidateActionExecutionConsent,
  #[serde(default = "default_legacy_readiness_report")]
  pub(crate) readiness: auv_driver::ReadinessReport,
  pub(crate) input_action_result: auv_driver::InputActionResult,
  pub(crate) operation_result: OperationResult,
  pub(crate) verification_result: VerificationResult,
  pub(crate) side_effect: CandidateActionExecutionSideEffect,
  pub(crate) detail: serde_json::Value,
  pub(crate) known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateActionExecutionSideEffect {
  SingleInputDelivered,
  BlockedNotReady,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidateActionDeliveryPlan {
  pub selected_method: String,
  pub target_grounding: TargetGrounding,
  pub target_query: String,
  pub window_number: Option<i64>,
  pub window_title: Option<String>,
  pub app_bundle_id: Option<String>,
  pub expected_window_frame: Option<auv_driver::Rect>,
  pub window_x: f64,
  pub window_y: f64,
  pub click_count: u32,
}

pub trait CandidateActionExecutor {
  fn readiness(
    &mut self,
    plan: &CandidateActionDeliveryPlan,
  ) -> AuvResult<auv_driver::ReadinessReport> {
    let _ = plan;
    Ok(auv_driver::ReadinessReport::ready(vec![
      auv_driver::ReadinessCheck::pass("fixture_readiness", "fixture executor is ready"),
    ]))
  }

  fn execute(
    &mut self,
    plan: &CandidateActionDeliveryPlan,
  ) -> AuvResult<auv_driver::InputActionResult>;

  // TODO(l8b-post-action-evidence-artifact): this hook returns
  // `VerificationResult`s only. A separate post-action observation artifact is
  // deferred until the recorder passes artifact staging into this boundary.
  fn verify_after_execution(
    &mut self,
    plan: &CandidateActionDeliveryPlan,
    probe: Option<&CandidateActionPostActionProbe>,
    candidate: &Candidate,
    candidate_ref: Option<&CandidateRef>,
    input_action_result: &auv_driver::InputActionResult,
    evidence_artifacts: &[ArtifactRef],
  ) -> AuvResult<Vec<VerificationResult>> {
    let _ = (
      plan,
      probe,
      candidate,
      candidate_ref,
      input_action_result,
      evidence_artifacts,
    );
    Ok(Vec::new())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandidateActionDecisionError {
  PromotionDidNotPromote,
  NoPromotedCandidates,
  CandidateNotFound { candidate_local_id: String },
  UnsupportedTargetGrounding { grounding: TargetGrounding },
  MissingWindowReference,
  MissingCandidateBox,
  MissingSourceCandidateActionDecisionArtifact,
  MissingExecutionConsent,
  ExecutionConsentMismatch { reason: String },
  NotReady { reason: String },
  ExecutionFailed { reason: String },
}

impl std::fmt::Display for CandidateActionDecisionError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::PromotionDidNotPromote => write!(
        f,
        "candidate action decide-only requires a promoted CandidatePromotion"
      ),
      Self::NoPromotedCandidates => write!(
        f,
        "candidate action decide-only requires at least one promoted Candidate"
      ),
      Self::CandidateNotFound { candidate_local_id } => {
        write!(
          f,
          "candidate {candidate_local_id} was not promoted by the source artifact"
        )
      }
      Self::UnsupportedTargetGrounding { grounding } => {
        write!(
          f,
          "candidate target grounding {grounding:?} is not supported by L8a decide-only"
        )
      }
      Self::MissingWindowReference => {
        write!(f, "candidate action execution requires a window reference")
      }
      Self::MissingCandidateBox => {
        write!(
          f,
          "candidate action execution requires a candidate evidence box"
        )
      }
      Self::MissingSourceCandidateActionDecisionArtifact => write!(
        f,
        "candidate action execution requires a recorded L8a candidate-action-decision artifact"
      ),
      Self::MissingExecutionConsent => {
        write!(
          f,
          "candidate action execution requires explicit L8b consent"
        )
      }
      Self::ExecutionConsentMismatch { reason } => {
        write!(f, "candidate action execution consent mismatch: {reason}")
      }
      Self::NotReady { reason } => {
        write!(
          f,
          "candidate action execution blocked by readiness: {reason}"
        )
      }
      Self::ExecutionFailed { reason } => {
        write!(
          f,
          "candidate action execution failed before artifact recording: {reason}"
        )
      }
    }
  }
}

impl std::error::Error for CandidateActionDecisionError {}

pub fn build_candidate_action_decision_artifact(
  promotion: &CandidatePromotionArtifact,
  request: &CandidateActionDecisionRequest,
) -> Result<CandidateActionDecisionArtifact, CandidateActionDecisionError> {
  let candidates = match &promotion.decision {
    CandidatePromotion::Promoted { candidates, .. } => candidates,
    CandidatePromotion::Refused { .. } => {
      return Err(CandidateActionDecisionError::PromotionDidNotPromote);
    }
  };
  let candidate = select_candidate(candidates, request)?;
  let action_resolver_decision = decide_candidate_action(candidate)?;
  let known_limits = artifact_known_limits(promotion, candidate);

  Ok(CandidateActionDecisionArtifact {
    artifact_version: CANDIDATE_ACTION_DECISION_ARTIFACT_VERSION.to_string(),
    decision_id: request.decision_id.clone(),
    source_candidate_promotion_artifact: request.source_candidate_promotion_artifact.clone(),
    source_promotion_id: promotion.promotion_id.clone(),
    candidate_local_id: candidate.candidate_local_id.clone(),
    action_resolver_decision,
    side_effect: CandidateActionSideEffect::NoneDecideOnly,
    detail: json!({
      "artifact_version": CANDIDATE_ACTION_DECISION_ARTIFACT_VERSION,
      "producer": "candidate_action_decision",
      "mode": "decide_only",
      "input_delivery": "not_attempted",
      "operation_result": "not_produced",
      "verification_result": "not_produced",
      "source_promotion_decision": "promoted",
      "source_promotion_id": promotion.promotion_id,
      "candidate_local_id": candidate.candidate_local_id,
      "target_grounding": target_grounding_kind(candidate.target_spec.grounding),
    }),
    known_limits,
  })
}

pub fn record_candidate_action_decision_artifact(
  context: &mut RecordedOperationContext<'_>,
  promotion: &CandidatePromotionArtifact,
  request: &CandidateActionDecisionRequest,
) -> AuvResult<(ArtifactRef, CandidateActionDecisionArtifact)> {
  let artifact = build_candidate_action_decision_artifact(promotion, request)
    .map_err(|error| format!("failed to build candidate action decision artifact: {error}"))?;
  let rendered = serde_json::to_string_pretty(&artifact)
    .map(|mut rendered| {
      rendered.push('\n');
      rendered
    })
    .map_err(|error| {
      format!("failed to encode candidate action decision artifact JSON: {error}")
    })?;
  let artifact_source_path = temp_json_path(&request.artifact_label);
  fs::write(&artifact_source_path, rendered).map_err(|error| {
    format!(
      "failed to write candidate action decision temp artifact {}: {error}",
      artifact_source_path.display()
    )
  })?;

  let (_, artifact_ref) = context.stage_artifact_file_with_ref(
    &request.artifact_role,
    &artifact_source_path,
    format!("{}.json", sanitize_artifact_label(&request.artifact_label)),
    Some(request.artifact_note.clone()),
  )?;
  let _ = fs::remove_file(&artifact_source_path);

  context.record_event(
    "candidate.action.decision.artifact_recorded",
    Some(format!(
      "recorded decide-only action decision {} for candidate {}",
      artifact_ref.artifact_id, artifact.candidate_local_id
    )),
  );

  Ok((artifact_ref, artifact))
}

pub fn build_candidate_action_execution_artifact(
  promotion: &CandidatePromotionArtifact,
  decision: &CandidateActionDecisionArtifact,
  request: &CandidateActionExecutionRequest,
  execution_run_id: RunId,
  input_action_result: auv_driver::InputActionResult,
) -> Result<CandidateActionExecutionArtifact, CandidateActionDecisionError> {
  let readiness = request.readiness.clone().unwrap_or_else(|| {
    auv_driver::ReadinessReport::blocked("execution readiness was not supplied")
  });
  let source_candidate_action_decision_artifact = request
    .source_candidate_action_decision_artifact
    .clone()
    .ok_or(CandidateActionDecisionError::MissingSourceCandidateActionDecisionArtifact)?;
  let candidates = match &promotion.decision {
    CandidatePromotion::Promoted { candidates, .. } => candidates,
    CandidatePromotion::Refused { .. } => {
      return Err(CandidateActionDecisionError::PromotionDidNotPromote);
    }
  };
  let candidate = candidates
    .iter()
    .find(|candidate| candidate.candidate_local_id == decision.candidate_local_id)
    .ok_or_else(|| CandidateActionDecisionError::CandidateNotFound {
      candidate_local_id: decision.candidate_local_id.clone(),
    })?;
  let consent = request
    .consent
    .clone()
    .ok_or(CandidateActionDecisionError::MissingExecutionConsent)?;
  validate_execution_consent(
    &consent,
    promotion,
    decision,
    &source_candidate_action_decision_artifact,
  )?;

  let readiness_ready = readiness.is_ready();
  let input_action_result = if readiness_ready {
    input_action_result
  } else {
    blocked_input_action_result(
      readiness
        .selected_blocker
        .as_deref()
        .unwrap_or("candidate action execution readiness failed"),
    )
  };
  let succeeded = input_action_result
    .attempts
    .iter()
    .any(|attempt| attempt.succeeded);
  let candidate_ref = candidate_ref_from_source(
    decision.source_candidate_promotion_artifact.as_ref(),
    &promotion.promotion_id,
    &candidate.candidate_local_id,
  );
  let evidence_artifacts =
    execution_evidence_artifacts(&source_candidate_action_decision_artifact, decision);
  let verification_result = activation_only_verification(
    succeeded,
    candidate,
    candidate_ref.clone(),
    &evidence_artifacts,
  );
  let primary_verification =
    primary_execution_verification(&verification_result, &request.post_action_verifications);
  let mut operation_verifications = vec![verification_result.clone()];
  operation_verifications.extend(request.post_action_verifications.clone());
  let semantic_matched = primary_verification.semantic_matched;
  let verification_summary =
    execution_verification_summary(&verification_result, &request.post_action_verifications);
  let operation_completed = readiness_ready && succeeded && semantic_matched != Some(false);
  let operation_result = OperationResult {
    api_version: crate::contract::OPERATION_RESULT_API_VERSION.to_string(),
    run_id: execution_run_id,
    status: if operation_completed {
      OperationStatus::Completed
    } else {
      OperationStatus::Failed
    },
    operation_id: "candidate.action.execute_single".to_string(),
    evidence_artifacts: evidence_artifacts.clone(),
    output: OperationOutput::Acknowledged {
      message: Some(execution_message(
        readiness_ready,
        succeeded,
        semantic_matched,
        !request.post_action_verifications.is_empty(),
      )),
    },
    verifications: operation_verifications,
    freshness_basis: promotion.promotion_context.freshness.clone(),
    known_limits: execution_known_limits(promotion, candidate),
  };
  let selected_path =
    serde_json::to_value(input_action_result.selected_path).unwrap_or(serde_json::Value::Null);
  let attempts_succeeded = input_action_result
    .attempts
    .iter()
    .filter(|attempt| attempt.succeeded)
    .count();
  let detail = json!({
    "artifact_version": CANDIDATE_ACTION_EXECUTION_ARTIFACT_VERSION,
    "producer": "candidate_action_execution",
    "mode": "execute_single",
    "input_delivery": input_delivery_summary(readiness_ready, succeeded),
    "readiness": readiness_status_label(&readiness.status),
    "readiness_blocker": readiness.selected_blocker,
    "readiness_checks": readiness.checks,
    "selected_path": selected_path,
    "attempt_count": input_action_result.attempts.len(),
    "attempts_succeeded": attempts_succeeded,
    "operation_status": if operation_completed { "completed" } else { "failed" },
    "verification": verification_summary,
    "verification_count": 1 + request.post_action_verifications.len(),
    "post_action_verification_count": request.post_action_verifications.len(),
    "post_action_verifications": post_action_verification_summaries(&request.post_action_verifications),
    "semantic_matched": serde_json::to_value(semantic_matched).unwrap_or(serde_json::Value::Null),
    "source_promotion_id": promotion.promotion_id,
    "source_decision_id": decision.decision_id,
    "candidate_local_id": candidate.candidate_local_id,
  });

  Ok(CandidateActionExecutionArtifact {
    artifact_version: CANDIDATE_ACTION_EXECUTION_ARTIFACT_VERSION.to_string(),
    execution_id: request.execution_id.clone(),
    source_candidate_action_decision_artifact,
    source_candidate_promotion_artifact: decision.source_candidate_promotion_artifact.clone(),
    operation_result_artifact: None,
    source_promotion_id: promotion.promotion_id.clone(),
    source_decision_id: decision.decision_id.clone(),
    candidate_local_id: candidate.candidate_local_id.clone(),
    action_resolver_decision: decision.action_resolver_decision.clone(),
    consent,
    readiness,
    input_action_result,
    operation_result,
    verification_result: primary_verification,
    side_effect: if readiness_ready {
      CandidateActionExecutionSideEffect::SingleInputDelivered
    } else {
      CandidateActionExecutionSideEffect::BlockedNotReady
    },
    detail,
    known_limits: execution_known_limits(promotion, candidate),
  })
}

pub fn record_candidate_action_execution_artifact(
  context: &mut RecordedOperationContext<'_>,
  promotion: &CandidatePromotionArtifact,
  decision: &CandidateActionDecisionArtifact,
  request: &CandidateActionExecutionRequest,
  input_action_result: auv_driver::InputActionResult,
) -> AuvResult<(ArtifactRef, CandidateActionExecutionArtifact)> {
  let mut artifact = build_candidate_action_execution_artifact(
    promotion,
    decision,
    request,
    context.run_id().clone(),
    input_action_result,
  )
  .map_err(|error| format!("failed to build candidate action execution artifact: {error}"))?;

  let operation_result_ref = stage_json_artifact(
    context,
    OPERATION_RESULT_ARTIFACT_ROLE,
    &format!("{}-operation-result", request.artifact_label),
    "Candidate action execution OperationResult.",
    &artifact.operation_result,
  )?;
  artifact.operation_result_artifact = Some(operation_result_ref);

  let artifact_ref = stage_json_artifact(
    context,
    &request.artifact_role,
    &request.artifact_label,
    &request.artifact_note,
    &artifact,
  )?;

  context.record_event(
    "candidate.action.execution.artifact_recorded",
    Some(format!(
      "recorded single-action execution {} for candidate {}",
      artifact_ref.artifact_id, artifact.candidate_local_id
    )),
  );

  Ok((artifact_ref, artifact))
}

pub fn execute_and_record_single_candidate_action<E: CandidateActionExecutor>(
  context: &mut RecordedOperationContext<'_>,
  executor: &mut E,
  promotion: &CandidatePromotionArtifact,
  decision: &CandidateActionDecisionArtifact,
  request: &CandidateActionExecutionRequest,
) -> AuvResult<(ArtifactRef, CandidateActionExecutionArtifact)> {
  let artifact = execute_single_candidate_action(
    executor,
    promotion,
    decision,
    request,
    context.run_id().clone(),
  )
  .map_err(|error| format!("failed to execute single candidate action: {error}"))?;
  let mut request = request.clone().with_readiness(artifact.readiness.clone());
  request.post_action_verifications = artifact
    .operation_result
    .verifications
    .iter()
    .skip(1)
    .cloned()
    .collect();
  record_candidate_action_execution_artifact(
    context,
    promotion,
    decision,
    &request,
    artifact.input_action_result,
  )
}

pub fn execute_single_candidate_action<E: CandidateActionExecutor>(
  executor: &mut E,
  promotion: &CandidatePromotionArtifact,
  decision: &CandidateActionDecisionArtifact,
  request: &CandidateActionExecutionRequest,
  execution_run_id: RunId,
) -> Result<CandidateActionExecutionArtifact, CandidateActionDecisionError> {
  let candidates = match &promotion.decision {
    CandidatePromotion::Promoted { candidates, .. } => candidates,
    CandidatePromotion::Refused { .. } => {
      return Err(CandidateActionDecisionError::PromotionDidNotPromote);
    }
  };
  let candidate = candidates
    .iter()
    .find(|candidate| candidate.candidate_local_id == decision.candidate_local_id)
    .ok_or_else(|| CandidateActionDecisionError::CandidateNotFound {
      candidate_local_id: decision.candidate_local_id.clone(),
    })?;
  let source_candidate_action_decision_artifact = request
    .source_candidate_action_decision_artifact
    .as_ref()
    .ok_or(CandidateActionDecisionError::MissingSourceCandidateActionDecisionArtifact)?;
  let consent = request
    .consent
    .as_ref()
    .ok_or(CandidateActionDecisionError::MissingExecutionConsent)?;
  validate_execution_consent(
    consent,
    promotion,
    decision,
    source_candidate_action_decision_artifact,
  )?;
  let plan = delivery_plan(candidate, decision)?;
  let readiness =
    executor
      .readiness(&plan)
      .map_err(|error| CandidateActionDecisionError::NotReady {
        reason: error.to_string(),
      })?;
  let request = request.clone().with_readiness(readiness.clone());
  if !readiness.is_ready() {
    return build_candidate_action_execution_artifact(
      promotion,
      decision,
      &request,
      execution_run_id,
      blocked_input_action_result(
        readiness
          .selected_blocker
          .as_deref()
          .unwrap_or("candidate action execution readiness failed"),
      ),
    );
  }
  let input_action_result =
    executor
      .execute(&plan)
      .map_err(|error| CandidateActionDecisionError::ExecutionFailed {
        reason: error.to_string(),
      })?;
  let request =
    if input_action_succeeded(&input_action_result) && request.post_action_probe.is_some() {
      let candidate_ref = candidate_ref_from_source(
        decision.source_candidate_promotion_artifact.as_ref(),
        &promotion.promotion_id,
        &candidate.candidate_local_id,
      );
      let evidence_artifacts =
        execution_evidence_artifacts(source_candidate_action_decision_artifact, decision);
      let post_action_verifications = executor
        .verify_after_execution(
          &plan,
          request.post_action_probe.as_ref(),
          candidate,
          candidate_ref.as_ref(),
          &input_action_result,
          &evidence_artifacts,
        )
        .unwrap_or_else(|error| {
          vec![post_action_observation_error_verification(
            candidate,
            candidate_ref.as_ref(),
            &evidence_artifacts,
            format!("post-action verification failed after input delivery: {error}"),
          )]
        });
      let mut request = request;
      request
        .post_action_verifications
        .extend(post_action_verifications);
      request
    } else {
      request
    };
  build_candidate_action_execution_artifact(
    promotion,
    decision,
    &request,
    execution_run_id,
    input_action_result,
  )
}

pub fn delivery_plan(
  candidate: &Candidate,
  decision: &CandidateActionDecisionArtifact,
) -> Result<CandidateActionDeliveryPlan, CandidateActionDecisionError> {
  if decision.candidate_local_id != candidate.candidate_local_id {
    return Err(CandidateActionDecisionError::CandidateNotFound {
      candidate_local_id: decision.candidate_local_id.clone(),
    });
  }
  let window_ref = candidate
    .liveness
    .preconditions
    .window_ref
    .as_ref()
    .ok_or(CandidateActionDecisionError::MissingWindowReference)?;
  let (window_x, window_y) = candidate_box_center(candidate)?;
  let expected_window_frame = candidate_expected_window_frame(candidate);

  Ok(CandidateActionDeliveryPlan {
    selected_method: decision.action_resolver_decision.selected_method.clone(),
    target_grounding: candidate.target_spec.grounding,
    target_query: decision.action_resolver_decision.target_query.clone(),
    window_number: window_ref.window_number,
    window_title: window_ref.window_title_substring.clone(),
    app_bundle_id: Some(window_ref.app_bundle_id.clone()),
    expected_window_frame,
    window_x,
    window_y,
    click_count: 1,
  })
}

fn candidate_box_center(candidate: &Candidate) -> Result<(f64, f64), CandidateActionDecisionError> {
  let Some(best_box) = candidate
    .evidence
    .observation
    .get("box")
    .or_else(|| candidate.evidence.observation.get("box_"))
  else {
    return Err(CandidateActionDecisionError::MissingCandidateBox);
  };
  let Some(x) = best_box.get("x").and_then(|value| value.as_i64()) else {
    return Err(CandidateActionDecisionError::MissingCandidateBox);
  };
  let Some(y) = best_box.get("y").and_then(|value| value.as_i64()) else {
    return Err(CandidateActionDecisionError::MissingCandidateBox);
  };
  let Some(width) = best_box.get("width").and_then(|value| value.as_i64()) else {
    return Err(CandidateActionDecisionError::MissingCandidateBox);
  };
  let Some(height) = best_box.get("height").and_then(|value| value.as_i64()) else {
    return Err(CandidateActionDecisionError::MissingCandidateBox);
  };
  Ok((
    x as f64 + width as f64 / 2.0,
    y as f64 + height as f64 / 2.0,
  ))
}

fn candidate_expected_window_frame(candidate: &Candidate) -> Option<auv_driver::Rect> {
  rect_from_value(
    candidate
      .evidence
      .observation
      .pointer("/detail/window_frame"),
  )
  .or_else(|| {
    rect_from_value(
      candidate
        .evidence
        .observation
        .pointer("/detail/windowFrame"),
    )
  })
  .or_else(|| {
    rect_from_value(
      candidate
        .evidence
        .observation
        .pointer("/detail/source_global_logical_bounds"),
    )
  })
  .or_else(|| {
    rect_from_value(
      candidate
        .evidence
        .observation
        .pointer("/detail/capture_contract/source_global_logical_bounds"),
    )
  })
}

fn rect_from_value(value: Option<&serde_json::Value>) -> Option<auv_driver::Rect> {
  let value = value?;
  Some(auv_driver::Rect::new(
    number_field(value, "x")?,
    number_field(value, "y")?,
    number_field(value, "width")?,
    number_field(value, "height")?,
  ))
}

fn number_field(value: &serde_json::Value, key: &str) -> Option<f64> {
  value.get(key).and_then(|value| {
    value
      .as_f64()
      .or_else(|| value.as_i64().map(|value| value as f64))
  })
}

#[cfg(target_os = "macos")]
pub struct MacosCandidateActionExecutor;

#[cfg(target_os = "macos")]
impl CandidateActionExecutor for MacosCandidateActionExecutor {
  fn readiness(
    &mut self,
    plan: &CandidateActionDeliveryPlan,
  ) -> AuvResult<auv_driver::ReadinessReport> {
    use auv_driver::Driver;

    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    let permissions = session
      .permission()
      .probe()
      .map_err(|error| format!("failed to probe macOS readiness permissions: {error}"))?;
    let windows = session
      .window()
      .list()
      .map_err(|error| format!("failed to list macOS windows for readiness: {error}"))?;
    let frontmost = session
      .window()
      .resolve(auv_driver::WindowSelector {
        app: Some(auv_driver::App::frontmost()),
        title: None,
        main_visible: true,
      })
      .ok();
    let input = auv_driver::ReadinessProbeInput::for_window_target(
      plan.window_number,
      plan.window_title.clone(),
      plan.app_bundle_id.clone(),
      plan.window_x,
      plan.window_y,
    )
    .with_expected_window_frame(plan.expected_window_frame);
    Ok(auv_driver_macos::assess_readiness(
      &permissions,
      &windows,
      frontmost.as_ref(),
      &input,
    ))
  }

  fn execute(
    &mut self,
    plan: &CandidateActionDeliveryPlan,
  ) -> AuvResult<auv_driver::InputActionResult> {
    use auv_driver::Driver;

    if plan.target_grounding != TargetGrounding::Coordinate
      || plan.selected_method.as_str() != "pointer-click"
    {
      return Err(
        "L8b macOS executor currently supports coordinate pointer-click plans only".to_string(),
      );
    }
    let Some(window_number) = plan.window_number else {
      return Err("L8b macOS executor requires window_number".to_string());
    };
    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    let windows = session
      .window()
      .list()
      .map_err(|error| format!("failed to list macOS windows for L8b execution: {error}"))?;
    let window = windows
      .iter()
      .find(|window| {
        window.reference.id == window_number.to_string()
          && plan
            .app_bundle_id
            .as_ref()
            .is_none_or(|expected| window.app_bundle_id.as_deref() == Some(expected.as_str()))
          && plan.window_title.as_ref().is_none_or(|expected| {
            window
              .title
              .as_deref()
              .is_some_and(|title| title.contains(expected))
          })
      })
      .ok_or_else(|| {
        format!(
          "could not resolve macOS window {} for L8b execution; available={}",
          window_number,
          summarize_available_windows(&windows, plan.app_bundle_id.as_deref())
        )
      })?
      .clone();
    session
      .window()
      .click(
        &window,
        auv_driver::WindowPoint::new(plan.window_x, plan.window_y),
        auv_driver::ClickOptions {
          policy: auv_driver::InputPolicy::BackgroundPreferred,
          click: if plan.click_count == 2 {
            auv_driver::Click::Double {
              interval: std::time::Duration::from_millis(100),
            }
          } else {
            auv_driver::Click::Single
          },
          window_strategy: auv_driver::WindowClickStrategy::PidTargeted,
        },
      )
      .map_err(|error| format!("typed macOS window click failed: {error}"))
  }

  fn verify_after_execution(
    &mut self,
    plan: &CandidateActionDeliveryPlan,
    probe: Option<&CandidateActionPostActionProbe>,
    candidate: &Candidate,
    candidate_ref: Option<&CandidateRef>,
    input_action_result: &auv_driver::InputActionResult,
    evidence_artifacts: &[ArtifactRef],
  ) -> AuvResult<Vec<VerificationResult>> {
    use auv_driver::Driver;

    if !input_action_succeeded(input_action_result) {
      return Ok(Vec::new());
    }
    let Some(probe) = probe else {
      return Ok(Vec::new());
    };
    let driver = auv_driver_macos::MacosDriver::new();
    let session = match driver.open_local() {
      Ok(session) => session,
      Err(error) => {
        return Ok(vec![post_action_observation_error_verification(
          candidate,
          candidate_ref,
          evidence_artifacts,
          format!("failed to open typed macOS driver session after execution: {error}"),
        )]);
      }
    };
    let windows = match session.window().list() {
      Ok(windows) => windows,
      Err(error) => {
        return Ok(vec![post_action_observation_error_verification(
          candidate,
          candidate_ref,
          evidence_artifacts,
          format!("failed to list macOS windows after execution: {error}"),
        )]);
      }
    };
    let frontmost = session
      .window()
      .resolve(auv_driver::WindowSelector {
        app: Some(auv_driver::App::frontmost()),
        title: None,
        main_visible: true,
      })
      .ok();
    let target = windows
      .iter()
      .find(|window| window_matches_plan(window, plan));
    let window_alive = post_action_window_alive(
      plan,
      target,
      frontmost.as_ref(),
      probe.require_frontmost,
      probe.bounds_tolerance_px,
    );
    match probe.kind {
      CandidateActionPostActionProbeKind::FocusedAxNodeReobserved => {
        Ok(vec![post_action_focused_ax_node_verification(
          plan,
          candidate,
          candidate_ref,
          evidence_artifacts,
          &window_alive,
          probe.bounds_tolerance_px,
        )])
      }
    }
  }
}

#[cfg(target_os = "macos")]
fn summarize_available_windows(
  windows: &[auv_driver::Window],
  app_bundle_id: Option<&str>,
) -> String {
  let rendered = windows
    .iter()
    .filter(|window| {
      app_bundle_id.is_none_or(|expected| window.app_bundle_id.as_deref() == Some(expected))
    })
    .take(8)
    .map(|window| {
      format!(
        "{}:{}:{}",
        window.reference.id,
        window.app_bundle_id.as_deref().unwrap_or(""),
        window.title.as_deref().unwrap_or("")
      )
    })
    .collect::<Vec<_>>();
  if rendered.is_empty() {
    "none".to_string()
  } else {
    rendered.join(",")
  }
}

fn select_candidate<'a>(
  candidates: &'a [Candidate],
  request: &CandidateActionDecisionRequest,
) -> Result<&'a Candidate, CandidateActionDecisionError> {
  if candidates.is_empty() {
    return Err(CandidateActionDecisionError::NoPromotedCandidates);
  }
  if let Some(candidate_local_id) = request.candidate_local_id.as_deref() {
    return candidates
      .iter()
      .find(|candidate| candidate.candidate_local_id == candidate_local_id)
      .ok_or_else(|| CandidateActionDecisionError::CandidateNotFound {
        candidate_local_id: candidate_local_id.to_string(),
      });
  }
  Ok(&candidates[0])
}

fn decide_candidate_action(
  candidate: &Candidate,
) -> Result<ActionResolverDecision, CandidateActionDecisionError> {
  match candidate.target_spec.grounding {
    TargetGrounding::AxNode => Ok(ActionResolverDecision::new(ActionResolverDecisionInput {
      operation: "candidate.action.decide_only",
      target_query: &target_query(candidate),
      primary_method: "ax-action",
      selected_method: "ax-action",
      fallback_allowed: false,
      fallback_used: false,
      fallback_reason: None,
      policy: "candidate-ax-node",
      cursor_disturbance: "none",
      press_mechanism: "ax-action",
    })),
    TargetGrounding::Coordinate => Ok(ActionResolverDecision::new(ActionResolverDecisionInput {
      operation: "candidate.action.decide_only",
      target_query: &target_query(candidate),
      primary_method: "pointer-click",
      selected_method: "pointer-click",
      fallback_allowed: false,
      fallback_used: false,
      fallback_reason: None,
      policy: "candidate-coordinate-pointer",
      cursor_disturbance: "warp-visible",
      press_mechanism: "pointer-click",
    })),
    TargetGrounding::OcrAnchor | TargetGrounding::VisualRow => {
      Err(CandidateActionDecisionError::UnsupportedTargetGrounding {
        grounding: candidate.target_spec.grounding,
      })
    }
  }
}

fn target_query(candidate: &Candidate) -> String {
  candidate
    .target_spec
    .anchor_text
    .clone()
    .or_else(|| candidate.label.clone())
    .unwrap_or_else(|| candidate.candidate_local_id.clone())
}

fn validate_execution_consent(
  consent: &CandidateActionExecutionConsent,
  promotion: &CandidatePromotionArtifact,
  decision: &CandidateActionDecisionArtifact,
  source_decision_artifact: &ArtifactRef,
) -> Result<(), CandidateActionDecisionError> {
  if consent.consent_id.trim().is_empty() {
    return Err(consent_mismatch("consent_id is empty"));
  }
  if consent.granted_by.trim().is_empty() {
    return Err(consent_mismatch("granted_by is empty"));
  }
  if consent.scope_note.trim().is_empty() {
    return Err(consent_mismatch("scope_note is empty"));
  }
  if consent.source_promotion_id != promotion.promotion_id {
    return Err(consent_mismatch(
      "source_promotion_id does not match promotion",
    ));
  }
  if consent.source_decision_id != decision.decision_id {
    return Err(consent_mismatch(
      "source_decision_id does not match decision",
    ));
  }
  if consent.candidate_local_id != decision.candidate_local_id {
    return Err(consent_mismatch(
      "candidate_local_id does not match decision",
    ));
  }
  if consent.run_id != source_decision_artifact.run_id.as_str() {
    return Err(consent_mismatch(
      "run_id does not match source candidate-action-decision artifact",
    ));
  }
  if consent.approved_action != CandidateActionExecutionConsentAction::ExecuteSingleCandidateAction
  {
    return Err(consent_mismatch("approved_action is not execute_single"));
  }
  Ok(())
}

fn consent_mismatch(reason: impl Into<String>) -> CandidateActionDecisionError {
  CandidateActionDecisionError::ExecutionConsentMismatch {
    reason: reason.into(),
  }
}

fn candidate_ref_from_source(
  source_candidate_promotion_artifact: Option<&ArtifactRef>,
  source_promotion_id: &str,
  candidate_local_id: &str,
) -> Option<CandidateRef> {
  source_candidate_promotion_artifact.map(|artifact| CandidateRef {
    source_run_id: artifact.run_id.clone(),
    source_span_id: artifact.span_id.clone(),
    source_operation_id: source_promotion_id.to_string(),
    source_artifact_id: artifact.artifact_id.clone(),
    candidate_local_id: candidate_local_id.to_string(),
  })
}

fn activation_only_verification(
  succeeded: bool,
  candidate: &Candidate,
  candidate_ref: Option<CandidateRef>,
  evidence_artifacts: &[ArtifactRef],
) -> VerificationResult {
  VerificationResult {
    api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
    method: VerificationMethod::Custom {
      name: "activation_only".to_string(),
    },
    executed: succeeded,
    state_changed: false,
    semantic_matched: None,
    failure_layer: (!succeeded).then_some(FailureLayer::ControlFailed),
    evidence: evidence_artifacts.to_vec(),
    consumed_candidate_ref: candidate_ref,
    consumed_node_ref: None,
    consumed_recognition_artifact_ref: Some(candidate.evidence.artifact_ref.clone()),
    consumed_recognition_id: None,
    consumed_recognized_item_id: candidate
      .evidence
      .observation
      .get("item_id")
      .and_then(|value| value.as_str())
      .map(str::to_string),
    observed_label: candidate.label.clone(),
  }
}

fn default_legacy_readiness_report() -> auv_driver::ReadinessReport {
  auv_driver::ReadinessReport::ready(vec![auv_driver::ReadinessCheck::unknown(
    "legacy_readiness",
    "readiness report missing from legacy candidate-action-execution artifact",
  )])
}

fn blocked_input_action_result(reason: &str) -> auv_driver::InputActionResult {
  auv_driver::InputActionResult {
    selected_path: auv_driver::InputDeliveryPath::Unsupported,
    attempts: vec![auv_driver::InputAttempt::failure(
      auv_driver::InputDeliveryPath::Unsupported,
      reason,
    )],
    fallback_reason: Some(reason.to_string()),
    mouse_disturbance: auv_driver::DisturbanceLevel::None,
    focus_disturbance: auv_driver::DisturbanceLevel::None,
    clipboard_disturbance: auv_driver::DisturbanceLevel::None,
  }
}

fn input_action_succeeded(input_action_result: &auv_driver::InputActionResult) -> bool {
  input_action_result
    .attempts
    .iter()
    .any(|attempt| attempt.succeeded)
}

fn execution_evidence_artifacts(
  source_candidate_action_decision_artifact: &ArtifactRef,
  decision: &CandidateActionDecisionArtifact,
) -> Vec<ArtifactRef> {
  let mut evidence_artifacts = vec![source_candidate_action_decision_artifact.clone()];
  if let Some(source_promotion) = decision.source_candidate_promotion_artifact.clone() {
    evidence_artifacts.push(source_promotion);
  }
  evidence_artifacts
}

fn primary_execution_verification(
  activation_only: &VerificationResult,
  post_action_verifications: &[VerificationResult],
) -> VerificationResult {
  post_action_verifications
    .iter()
    .find(|verification| verification.semantic_matched.is_some())
    .or_else(|| post_action_verifications.first())
    .cloned()
    .unwrap_or_else(|| activation_only.clone())
}

fn execution_verification_summary(
  activation_only: &VerificationResult,
  post_action_verifications: &[VerificationResult],
) -> String {
  let primary = primary_execution_verification(activation_only, post_action_verifications);
  if post_action_verifications.is_empty() {
    "activation_only".to_string()
  } else {
    format!(
      "{}+post_action:{}",
      verification_method_label(&activation_only.method),
      verification_method_label(&primary.method),
    )
  }
}

fn post_action_verification_summaries(
  verifications: &[VerificationResult],
) -> Vec<serde_json::Value> {
  verifications
    .iter()
    .map(|verification| {
      json!({
        "method": verification_method_label(&verification.method),
        "executed": verification.executed,
        "state_changed": verification.state_changed,
        "semantic_matched": serde_json::to_value(verification.semantic_matched).unwrap_or(serde_json::Value::Null),
        "failure_layer": verification.failure_layer.as_ref().map(failure_layer_label),
        "observed_label": verification.observed_label,
        "evidence_count": verification.evidence.len(),
      })
    })
    .collect()
}

fn execution_message(
  readiness_ready: bool,
  delivery_succeeded: bool,
  semantic_matched: Option<bool>,
  has_post_action_verification: bool,
) -> String {
  if !readiness_ready {
    return "single candidate action blocked by readiness; input delivery not attempted"
      .to_string();
  }
  if !delivery_succeeded {
    return "single candidate action delivery failed".to_string();
  }
  match (semantic_matched, has_post_action_verification) {
    (Some(true), _) => {
      "single candidate action delivered and semantic verification matched".to_string()
    }
    (Some(false), _) => {
      "single candidate action delivered, but semantic verification did not match".to_string()
    }
    (None, true) => {
      "single candidate action delivered with post-action verification evidence".to_string()
    }
    (None, false) => {
      "single candidate action activated; semantic verification remains activation_only".to_string()
    }
  }
}

fn input_delivery_summary(readiness_ready: bool, delivery_succeeded: bool) -> &'static str {
  if !readiness_ready {
    "not_attempted"
  } else if delivery_succeeded {
    "attempted"
  } else {
    "failed"
  }
}

fn readiness_status_label(status: &auv_driver::ReadinessStatus) -> &'static str {
  match status {
    auv_driver::ReadinessStatus::Ready => "ready",
    auv_driver::ReadinessStatus::NotReady => "not_ready",
  }
}

fn verification_method_label(method: &VerificationMethod) -> String {
  match method {
    VerificationMethod::TextVisible => "text_visible".to_string(),
    VerificationMethod::AxText => "ax_text".to_string(),
    VerificationMethod::StateChanged => "state_changed".to_string(),
    VerificationMethod::CandidateAlive => "candidate_alive".to_string(),
    VerificationMethod::SemanticMatch => "semantic_match".to_string(),
    VerificationMethod::NoProgressBoundary => "no_progress_boundary".to_string(),
    VerificationMethod::Custom { name } => name.clone(),
  }
}

fn failure_layer_label(layer: &FailureLayer) -> &'static str {
  match layer {
    FailureLayer::GroundingFailed => "grounding_failed",
    FailureLayer::CandidateExpired => "candidate_expired",
    FailureLayer::ControlFailed => "control_failed",
    FailureLayer::VerificationUnreliable => "verification_unreliable",
    FailureLayer::StateChangedNoMatch => "state_changed_no_match",
    FailureLayer::SemanticMismatch => "semantic_mismatch",
  }
}

fn post_action_observation_error_verification(
  candidate: &Candidate,
  candidate_ref: Option<&CandidateRef>,
  evidence_artifacts: &[ArtifactRef],
  reason: impl Into<String>,
) -> VerificationResult {
  post_action_verification(
    candidate,
    candidate_ref,
    evidence_artifacts,
    VerificationMethod::SemanticMatch,
    false,
    Some(FailureLayer::VerificationUnreliable),
    reason.into(),
  )
}

fn post_action_verification(
  candidate: &Candidate,
  candidate_ref: Option<&CandidateRef>,
  evidence_artifacts: &[ArtifactRef],
  method: VerificationMethod,
  semantic_matched: bool,
  failure_layer: Option<FailureLayer>,
  observed_label: String,
) -> VerificationResult {
  VerificationResult {
    api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
    method,
    executed: true,
    state_changed: semantic_matched,
    semantic_matched: Some(semantic_matched),
    failure_layer,
    evidence: evidence_artifacts.to_vec(),
    consumed_candidate_ref: candidate_ref.cloned(),
    consumed_node_ref: None,
    consumed_recognition_artifact_ref: Some(candidate.evidence.artifact_ref.clone()),
    consumed_recognition_id: None,
    consumed_recognized_item_id: candidate
      .evidence
      .observation
      .get("item_id")
      .and_then(|value| value.as_str())
      .map(str::to_string),
    observed_label: Some(observed_label),
  }
}

struct PostActionWindowAlive {
  target_present: bool,
  target_frontmost: bool,
  bounds_stable: bool,
  point_inside: bool,
  target_app_bundle_id: Option<String>,
}

impl PostActionWindowAlive {
  fn is_alive(&self) -> bool {
    self.target_present && self.target_frontmost && self.bounds_stable && self.point_inside
  }

  fn summary(&self) -> String {
    format!(
      "target_present={} target_frontmost={} bounds_stable={} point_inside={}",
      self.target_present, self.target_frontmost, self.bounds_stable, self.point_inside
    )
  }
}

fn post_action_window_alive(
  plan: &CandidateActionDeliveryPlan,
  target: Option<&auv_driver::Window>,
  frontmost: Option<&auv_driver::Window>,
  require_frontmost: bool,
  bounds_tolerance_px: f64,
) -> PostActionWindowAlive {
  let target_present = target.is_some();
  let target_frontmost = if require_frontmost {
    target
      .zip(frontmost)
      .is_some_and(|(target, frontmost)| target.reference.id == frontmost.reference.id)
  } else {
    true
  };
  let bounds_stable = target
    .and_then(|window| {
      plan
        .expected_window_frame
        .map(|expected| (window.frame, expected))
    })
    .map(|(observed, expected)| rect_drift_px(observed, expected) <= bounds_tolerance_px)
    .unwrap_or(true);
  let point_inside = target
    .map(|window| window_contains_point(window, plan.window_x, plan.window_y))
    .unwrap_or(false);
  PostActionWindowAlive {
    target_present,
    target_frontmost,
    bounds_stable,
    point_inside,
    target_app_bundle_id: target.and_then(|window| window.app_bundle_id.clone()),
  }
}

#[cfg(target_os = "macos")]
fn post_action_focused_ax_node_verification(
  plan: &CandidateActionDeliveryPlan,
  candidate: &Candidate,
  candidate_ref: Option<&CandidateRef>,
  evidence_artifacts: &[ArtifactRef],
  window_alive: &PostActionWindowAlive,
  bounds_tolerance_px: f64,
) -> VerificationResult {
  let Some(app_bundle_id) = window_alive
    .target_app_bundle_id
    .as_deref()
    .or(plan.app_bundle_id.as_deref())
  else {
    return post_action_verification(
      candidate,
      candidate_ref,
      evidence_artifacts,
      VerificationMethod::SemanticMatch,
      false,
      Some(FailureLayer::VerificationUnreliable),
      format!(
        "post-action focused AX reobserve failed: missing target app bundle id; {}",
        window_alive.summary()
      ),
    );
  };
  let capture =
    match auv_driver_macos::native::ax_tree::capture_ax_tree_snapshot(app_bundle_id, 8, 80) {
      Ok(capture) => capture,
      Err(error) => {
        return post_action_verification(
          candidate,
          candidate_ref,
          evidence_artifacts,
          VerificationMethod::SemanticMatch,
          false,
          Some(FailureLayer::VerificationUnreliable),
          format!(
            "post-action focused AX reobserve failed: {error}; {}",
            window_alive.summary()
          ),
        );
      }
    };
  let expected = expected_ax_focus_target(candidate);
  let focused = capture.snapshot.nodes.iter().find(|node| node.focused);
  let focused_matches = focused
    .zip(expected.as_ref())
    .is_some_and(|(focused, expected)| {
      ax_node_matches_expected(focused, expected, bounds_tolerance_px)
    });
  let semantic_matched = window_alive.is_alive() && focused_matches;
  let observed_label = format!(
    "post-action focused_ax_node_reobserved={} focused_path={} focused_role={} expected_role={} expected_text={} {}",
    focused_matches,
    focused.map(|node| node.path.as_str()).unwrap_or("none"),
    focused.map(|node| node.role.as_str()).unwrap_or("none"),
    expected
      .as_ref()
      .and_then(|expected| expected.role.as_deref())
      .unwrap_or("none"),
    expected
      .as_ref()
      .and_then(|expected| expected.text.as_deref())
      .unwrap_or("none"),
    window_alive.summary()
  );
  post_action_verification(
    candidate,
    candidate_ref,
    evidence_artifacts,
    VerificationMethod::SemanticMatch,
    semantic_matched,
    (!semantic_matched).then_some(FailureLayer::SemanticMismatch),
    observed_label,
  )
}

#[cfg(not(target_os = "macos"))]
fn post_action_focused_ax_node_verification(
  _plan: &CandidateActionDeliveryPlan,
  candidate: &Candidate,
  candidate_ref: Option<&CandidateRef>,
  evidence_artifacts: &[ArtifactRef],
  window_alive: &PostActionWindowAlive,
  _bounds_tolerance_px: f64,
) -> VerificationResult {
  post_action_verification(
    candidate,
    candidate_ref,
    evidence_artifacts,
    VerificationMethod::SemanticMatch,
    false,
    Some(FailureLayer::VerificationUnreliable),
    format!(
      "post-action focused AX reobserve is unavailable on this target; {}",
      window_alive.summary()
    ),
  )
}

fn window_matches_plan(window: &auv_driver::Window, plan: &CandidateActionDeliveryPlan) -> bool {
  plan
    .window_number
    .is_none_or(|number| window.reference.id == number.to_string())
    && plan
      .app_bundle_id
      .as_ref()
      .is_none_or(|expected| window.app_bundle_id.as_deref() == Some(expected.as_str()))
    && plan.window_title.as_ref().is_none_or(|expected| {
      window
        .title
        .as_deref()
        .is_some_and(|title| title.contains(expected))
    })
}

fn window_contains_point(window: &auv_driver::Window, window_x: f64, window_y: f64) -> bool {
  window_x >= 0.0
    && window_y >= 0.0
    && window_x <= window.frame.size.width
    && window_y <= window.frame.size.height
}

#[derive(Clone, Debug, PartialEq)]
struct ExpectedAxFocusTarget {
  role: Option<String>,
  text: Option<String>,
  bounds: Option<auv_driver::Rect>,
}

fn expected_ax_focus_target(candidate: &Candidate) -> Option<ExpectedAxFocusTarget> {
  let observation = &candidate.evidence.observation;
  let role = observation
    .pointer("/detail/role")
    .and_then(|value| value.as_str())
    .map(str::to_string);
  let text = observation
    .get("text")
    .and_then(|value| value.as_str())
    .or_else(|| {
      observation
        .pointer("/detail/title")
        .and_then(|value| value.as_str())
    })
    .or_else(|| {
      observation
        .pointer("/detail/value")
        .and_then(|value| value.as_str())
    })
    .or_else(|| {
      observation
        .pointer("/detail/description")
        .and_then(|value| value.as_str())
    })
    .or(candidate.label.as_deref())
    .map(str::to_string)
    .filter(|value| !value.trim().is_empty());
  let bounds = rect_from_value(observation.pointer("/detail/source_global_logical_bounds"))
    .or_else(|| rect_from_value(observation.pointer("/detail/bounds")))
    .or_else(|| {
      rect_from_recognition_box(observation.get("box").or_else(|| observation.get("box_")))
    });
  if role.is_none() && text.is_none() && bounds.is_none() {
    None
  } else {
    Some(ExpectedAxFocusTarget { role, text, bounds })
  }
}

#[cfg(target_os = "macos")]
fn ax_node_matches_expected(
  node: &auv_driver_macos::types::ObservedAxNode,
  expected: &ExpectedAxFocusTarget,
  bounds_tolerance_px: f64,
) -> bool {
  let role_matches = expected.role.as_ref().is_none_or(|role| node.role == *role);
  let text_matches = expected
    .text
    .as_ref()
    .is_none_or(|text| ax_node_searchable_text(node).contains(&normalize_ax_text(text)));
  let bounds_matches = expected.bounds.is_none_or(|bounds| {
    rect_drift_px(observed_ax_rect(&node.bounds), bounds) <= bounds_tolerance_px
  });
  role_matches && text_matches && bounds_matches
}

#[cfg(target_os = "macos")]
fn ax_node_searchable_text(node: &auv_driver_macos::types::ObservedAxNode) -> String {
  normalize_ax_text(
    &[
      node.title.as_str(),
      node.description.as_str(),
      node.value.as_str(),
      node.placeholder.as_str(),
      node.identifier.as_str(),
    ]
    .join(" "),
  )
}

fn normalize_ax_text(raw: &str) -> String {
  raw
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
    .to_lowercase()
}

#[cfg(target_os = "macos")]
fn observed_ax_rect(rect: &auv_driver_macos::types::ObservedRect) -> auv_driver::Rect {
  auv_driver::Rect::new(
    rect.x as f64,
    rect.y as f64,
    rect.width as f64,
    rect.height as f64,
  )
}

fn rect_from_recognition_box(value: Option<&serde_json::Value>) -> Option<auv_driver::Rect> {
  let value = value?;
  Some(auv_driver::Rect::new(
    number_field(value, "x")?,
    number_field(value, "y")?,
    number_field(value, "width")?,
    number_field(value, "height")?,
  ))
}

fn rect_drift_px(left: auv_driver::Rect, right: auv_driver::Rect) -> f64 {
  (left.origin.x - right.origin.x)
    .abs()
    .max((left.origin.y - right.origin.y).abs())
    .max((left.size.width - right.size.width).abs())
    .max((left.size.height - right.size.height).abs())
}

fn execution_known_limits(
  promotion: &CandidatePromotionArtifact,
  candidate: &Candidate,
) -> Vec<String> {
  let mut known_limits = promotion.known_limits.clone();
  for limit in &candidate.known_limits {
    push_known_limit(&mut known_limits, limit);
  }
  push_known_limit(
    &mut known_limits,
    "L8b executes one candidate action only after explicit execution consent",
  );
  push_known_limit(
    &mut known_limits,
    "activation_only verification records input delivery, not semantic success",
  );
  push_known_limit(
    &mut known_limits,
    "L8b execution is blocked before input delivery when readiness is not ready",
  );
  known_limits
}

fn artifact_known_limits(
  promotion: &CandidatePromotionArtifact,
  candidate: &Candidate,
) -> Vec<String> {
  let mut known_limits = promotion.known_limits.clone();
  for limit in &candidate.known_limits {
    push_known_limit(&mut known_limits, limit);
  }
  push_known_limit(
    &mut known_limits,
    "L8a records an ActionResolverDecision only; it does not call auv-driver or produce InputActionResult",
  );
  push_known_limit(
    &mut known_limits,
    "L8a does not prove semantic success; verification remains deferred to L8b",
  );
  known_limits
}

fn push_known_limit(known_limits: &mut Vec<String>, value: impl AsRef<str>) {
  let value = value.as_ref();
  if !known_limits.iter().any(|existing| existing == value) {
    known_limits.push(value.to_string());
  }
}

fn target_grounding_kind(grounding: TargetGrounding) -> &'static str {
  match grounding {
    TargetGrounding::OcrAnchor => "ocr_anchor",
    TargetGrounding::VisualRow => "visual_row",
    TargetGrounding::AxNode => "ax_node",
    TargetGrounding::Coordinate => "coordinate",
  }
}

fn temp_json_path(label: &str) -> std::path::PathBuf {
  let sequence = TEMP_JSON_COUNTER.fetch_add(1, Ordering::Relaxed);
  std::env::temp_dir().join(format!(
    "auv-candidate-action-decision-{}-{}-{}-{}.json",
    sanitize_artifact_label(label),
    now_millis(),
    std::process::id(),
    sequence
  ))
}

fn stage_json_artifact<T: Serialize>(
  context: &mut RecordedOperationContext<'_>,
  role: &str,
  label: &str,
  note: &str,
  value: &T,
) -> AuvResult<ArtifactRef> {
  let rendered = serde_json::to_string_pretty(value)
    .map(|mut rendered| {
      rendered.push('\n');
      rendered
    })
    .map_err(|error| format!("failed to encode {role} artifact JSON: {error}"))?;
  let artifact_source_path = temp_json_path(label);
  fs::write(&artifact_source_path, rendered).map_err(|error| {
    format!(
      "failed to write {role} temp artifact {}: {error}",
      artifact_source_path.display()
    )
  })?;

  let (_, artifact_ref) = context.stage_artifact_file_with_ref(
    role,
    &artifact_source_path,
    format!("{}.json", sanitize_artifact_label(label)),
    Some(note.to_string()),
  )?;
  let _ = fs::remove_file(&artifact_source_path);
  Ok(artifact_ref)
}

fn sanitize_artifact_label(raw: &str) -> String {
  let sanitized = raw
    .chars()
    .map(|character| match character {
      'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
      _ => '-',
    })
    .collect::<String>()
    .trim_matches('-')
    .to_string();
  if sanitized.is_empty() {
    "artifact".to_string()
  } else {
    sanitized
  }
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::path::PathBuf;

  use serde_json::json;

  #[cfg(target_os = "macos")]
  use super::MacosCandidateActionExecutor;
  use super::{
    CandidateActionDecisionArtifact, CandidateActionDecisionError, CandidateActionDecisionRequest,
    CandidateActionDeliveryPlan, CandidateActionExecutionConsent,
    CandidateActionExecutionConsentAction, CandidateActionExecutionRequest,
    CandidateActionExecutionSideEffect, CandidateActionExecutor, CandidateActionPostActionProbe,
    build_candidate_action_decision_artifact, build_candidate_action_execution_artifact,
    execute_and_record_single_candidate_action, execute_single_candidate_action,
    record_candidate_action_decision_artifact, record_candidate_action_execution_artifact,
  };
  use crate::AuvResult;
  use crate::build_runtime_with_store_root;
  use crate::candidate_promotion::CandidatePromotion;
  use crate::candidate_promotion_recording::{
    CandidatePromotionArtifactRequest, CandidatePromotionConsentInput,
    build_candidate_promotion_artifact, explicit_consent_for_candidate_promotion,
    freshness_from_capture_backed_recognition,
  };
  use crate::contract::{
    ArtifactRef, FailureLayer, OperationOutput, OperationStatus, RecognitionBox, RecognitionResult,
    RecognitionScope, RecognitionSource, RecognitionSurface, RecognizedItem, TargetGrounding,
    VerificationMethod,
  };
  use crate::run_builder::RunSpec;
  use crate::stability::StabilityPolicy;
  use crate::trace::{ArtifactId, EventId, RunId, RunType, SpanId, TraceStatusCode};

  fn sample_artifact_ref() -> ArtifactRef {
    ArtifactRef {
      run_id: RunId::new("run_candidate_action_decision_source"),
      artifact_id: ArtifactId::new("artifact_capture"),
      span_id: SpanId::new("span_candidate_action_decision_source"),
      captured_event_id: Some(EventId::new("event_capture")),
    }
  }

  fn sample_frame(recognition_id: &str, x: i64, y: i64) -> RecognitionResult {
    sample_frame_with_box_and_window_frame(
      recognition_id,
      RecognitionBox {
        x,
        y,
        width: 300,
        height: 80,
      },
      serde_json::json!({
        "x": 0.0,
        "y": 0.0,
        "width": 500.0,
        "height": 300.0
      }),
    )
  }

  fn sample_frame_with_box_and_window_frame(
    recognition_id: &str,
    box_: RecognitionBox,
    window_frame: serde_json::Value,
  ) -> RecognitionResult {
    let capture_artifact = sample_artifact_ref();
    let target_bounds = serde_json::json!({
      "x": box_.x,
      "y": box_.y,
      "width": box_.width,
      "height": box_.height
    });
    RecognitionResult {
      recognition_id: recognition_id.to_string(),
      source: RecognitionSource::Custom,
      scope: RecognitionScope {
        surface: RecognitionSurface::Window,
        display_ref: Some("display-main".to_string()),
        native_display_id: Some("69733248".to_string()),
        app_bundle_id: Some("com.apple.TextEdit".to_string()),
        window_title: Some("Untitled".to_string()),
        window_number: Some(11),
        region_hint: None,
        capture_artifact: Some(capture_artifact.clone()),
        capture_contract_artifact: None,
      },
      best: Some(RecognizedItem {
        item_id: "item_text_area".to_string(),
        kind: "text_area".to_string(),
        box_: box_.clone(),
        text: Some("Text Area".to_string()),
        provider_score: Some(0.99),
        detail: json!({
          "backend": "ax-fixture",
          "window_frame": window_frame,
          "source_global_logical_bounds": target_bounds,
        }),
      }),
      filtered: vec![RecognizedItem {
        item_id: "item_text_area".to_string(),
        kind: "text_area".to_string(),
        box_: box_.clone(),
        text: Some("Text Area".to_string()),
        provider_score: Some(0.99),
        detail: json!({
          "backend": "ax-fixture",
          "window_frame": window_frame,
          "source_global_logical_bounds": target_bounds,
        }),
      }],
      all: vec![RecognizedItem {
        item_id: "item_text_area".to_string(),
        kind: "text_area".to_string(),
        box_,
        text: Some("Text Area".to_string()),
        provider_score: Some(0.99),
        detail: json!({
          "backend": "ax-fixture",
          "window_frame": window_frame,
          "source_global_logical_bounds": target_bounds,
        }),
      }],
      detail: json!({"backend": "ax-fixture"}),
      evidence: vec![capture_artifact],
      known_limits: vec!["fixture-backed recognition only".to_string()],
    }
  }

  fn promoted_artifact() -> crate::candidate_promotion_recording::CandidatePromotionArtifact {
    let observations = vec![
      sample_frame("recognition_frame_1", 10, 20),
      sample_frame("recognition_frame_2", 11, 20),
    ];
    let latest = observations.last().expect("latest frame exists");
    let mut request =
      CandidatePromotionArtifactRequest::new("promotion_text_area", "promotion-text-area");
    request.source_recognition_artifact = Some(ArtifactRef {
      run_id: RunId::new("run_candidate_action_decision_source"),
      artifact_id: ArtifactId::new("artifact_recognition"),
      span_id: SpanId::new("span_candidate_action_decision_source"),
      captured_event_id: Some(EventId::new("event_recognition")),
    });
    request.stability_policy = StabilityPolicy {
      min_frames: 2,
      max_centroid_drift_px: 4.0,
      require_stable_text: true,
    };
    request.projection = crate::candidate_promotion::PromotionProjection::IdentityWindowAddressable;
    request.freshness = Some(
      freshness_from_capture_backed_recognition(latest, "debug.captureAxTree", "fresh")
        .expect("latest recognition is capture-backed"),
    );
    request.permission = Some(
      explicit_consent_for_candidate_promotion(
        &request.promotion_id,
        latest,
        CandidatePromotionConsentInput {
          granted_by: "human-review".to_string(),
          scope_note: "candidate promotion only, no action execution".to_string(),
          evidence_note: "unit test consent".to_string(),
          approved_at_millis: 1,
        },
      )
      .expect("latest recognition is capture-backed"),
    );
    build_candidate_promotion_artifact(&observations, &request)
      .expect("promotion artifact should build")
  }

  fn source_decision_ref() -> ArtifactRef {
    ArtifactRef {
      run_id: RunId::new("run_candidate_action_decision_source"),
      artifact_id: ArtifactId::new("artifact_decision"),
      span_id: SpanId::new("span_candidate_action_decision_source"),
      captured_event_id: Some(EventId::new("event_decision")),
    }
  }

  fn execution_consent() -> CandidateActionExecutionConsent {
    CandidateActionExecutionConsent {
      consent_id: "consent_execute_text_area".to_string(),
      granted_by: "human-review".to_string(),
      scope_note: "execute exactly one approved candidate action".to_string(),
      run_id: "run_candidate_action_decision_source".to_string(),
      source_promotion_id: "promotion_text_area".to_string(),
      source_decision_id: "decision_text_area".to_string(),
      candidate_local_id: "promoted-item_text_area".to_string(),
      approved_action: CandidateActionExecutionConsentAction::ExecuteSingleCandidateAction,
      approved_at_millis: 2,
      evidence_note: "unit test execution consent".to_string(),
    }
  }

  fn semantic_verification(semantic_matched: bool) -> crate::contract::VerificationResult {
    crate::contract::VerificationResult {
      api_version: crate::contract::VERIFICATION_RESULT_API_VERSION.to_string(),
      method: VerificationMethod::SemanticMatch,
      executed: true,
      state_changed: semantic_matched,
      semantic_matched: Some(semantic_matched),
      failure_layer: (!semantic_matched).then_some(FailureLayer::SemanticMismatch),
      evidence: vec![sample_artifact_ref()],
      consumed_candidate_ref: None,
      consumed_node_ref: None,
      consumed_recognition_artifact_ref: Some(sample_artifact_ref()),
      consumed_recognition_id: Some("recognition_frame_2".to_string()),
      consumed_recognized_item_id: Some("item_text_area".to_string()),
      observed_label: Some("Text Area".to_string()),
    }
  }

  struct FakeExecutor {
    result: auv_driver::InputActionResult,
    readiness: auv_driver::ReadinessReport,
    observed_plan: Option<CandidateActionDeliveryPlan>,
    executed: bool,
  }

  impl CandidateActionExecutor for FakeExecutor {
    fn readiness(
      &mut self,
      plan: &CandidateActionDeliveryPlan,
    ) -> AuvResult<auv_driver::ReadinessReport> {
      self.observed_plan = Some(plan.clone());
      Ok(self.readiness.clone())
    }

    fn execute(
      &mut self,
      plan: &CandidateActionDeliveryPlan,
    ) -> AuvResult<auv_driver::InputActionResult> {
      self.observed_plan = Some(plan.clone());
      self.executed = true;
      Ok(self.result.clone())
    }
  }

  struct FakeVerifyingExecutor {
    result: auv_driver::InputActionResult,
    readiness: auv_driver::ReadinessReport,
    observed_plan: Option<CandidateActionDeliveryPlan>,
    executed: bool,
    verification_calls: usize,
    verification: crate::contract::VerificationResult,
    verification_error: Option<String>,
  }

  impl CandidateActionExecutor for FakeVerifyingExecutor {
    fn readiness(
      &mut self,
      plan: &CandidateActionDeliveryPlan,
    ) -> AuvResult<auv_driver::ReadinessReport> {
      self.observed_plan = Some(plan.clone());
      Ok(self.readiness.clone())
    }

    fn execute(
      &mut self,
      plan: &CandidateActionDeliveryPlan,
    ) -> AuvResult<auv_driver::InputActionResult> {
      self.observed_plan = Some(plan.clone());
      self.executed = true;
      Ok(self.result.clone())
    }

    fn verify_after_execution(
      &mut self,
      plan: &CandidateActionDeliveryPlan,
      probe: Option<&CandidateActionPostActionProbe>,
      candidate: &crate::contract::Candidate,
      candidate_ref: Option<&crate::contract::CandidateRef>,
      input_action_result: &auv_driver::InputActionResult,
      evidence_artifacts: &[crate::contract::ArtifactRef],
    ) -> AuvResult<Vec<crate::contract::VerificationResult>> {
      self.verification_calls += 1;
      assert!(
        self.executed,
        "post-action verification must run after input delivery"
      );
      assert_eq!(
        probe,
        Some(&CandidateActionPostActionProbe::focused_ax_node_reobserved())
      );
      assert_eq!(plan.window_number, Some(11));
      assert_eq!(candidate.candidate_local_id, "promoted-item_text_area");
      assert!(candidate_ref.is_some());
      assert!(
        input_action_result
          .attempts
          .iter()
          .any(|attempt| attempt.succeeded)
      );
      assert_eq!(evidence_artifacts.len(), 2);
      if let Some(error) = self.verification_error.clone() {
        return Err(error);
      }
      Ok(vec![self.verification.clone()])
    }
  }

  fn ready_report() -> auv_driver::ReadinessReport {
    auv_driver::ReadinessReport::ready(vec![
      auv_driver::ReadinessCheck::pass("accessibility", "accessibility permission granted"),
      auv_driver::ReadinessCheck::pass("target_window_present", "target window is present"),
    ])
  }

  fn not_ready_report(reason: &str) -> auv_driver::ReadinessReport {
    auv_driver::ReadinessReport::blocked(reason)
  }

  fn execution_request() -> CandidateActionExecutionRequest {
    CandidateActionExecutionRequest::new("execution_text_area", "text-area-action-execution")
      .with_source_candidate_action_decision_artifact(source_decision_ref())
      .with_consent(execution_consent())
      .with_readiness(ready_report())
  }

  fn decision_artifact() -> CandidateActionDecisionArtifact {
    let promotion = promoted_artifact();
    let request =
      CandidateActionDecisionRequest::new("decision_text_area", "text-area-action-decision")
        .with_source_candidate_promotion_artifact(ArtifactRef {
          run_id: RunId::new("run_candidate_action_decision_source"),
          artifact_id: ArtifactId::new("artifact_promotion"),
          span_id: SpanId::new("span_candidate_action_decision_source"),
          captured_event_id: Some(EventId::new("event_promotion")),
        });
    build_candidate_action_decision_artifact(&promotion, &request)
      .expect("decision artifact should build")
  }

  #[test]
  fn build_decide_only_artifact_from_promoted_candidate_without_input_delivery() {
    let promotion = promoted_artifact();
    let request =
      CandidateActionDecisionRequest::new("decision_text_area", "text-area-action-decision");
    let artifact = build_candidate_action_decision_artifact(&promotion, &request)
      .expect("promoted candidate should produce decide-only artifact");

    assert_eq!(artifact.source_promotion_id, "promotion_text_area");
    assert_eq!(artifact.candidate_local_id, "promoted-item_text_area");
    assert_eq!(
      artifact.action_resolver_decision.operation,
      "candidate.action.decide_only"
    );
    assert_eq!(
      artifact.action_resolver_decision.selected_method,
      "pointer-click"
    );
    assert_eq!(
      artifact.action_resolver_decision.cursor_disturbance,
      "warp-visible"
    );
    assert_eq!(artifact.detail["input_delivery"], json!("not_attempted"));
    assert_eq!(artifact.detail["operation_result"], json!("not_produced"));
    assert_eq!(
      artifact.detail["verification_result"],
      json!("not_produced")
    );
    let rendered = serde_json::to_value(&artifact).expect("artifact should serialize");
    assert!(rendered.get("input_action_result").is_none());
    assert!(rendered.get("operation_result").is_none());
    assert!(rendered.get("verification_result").is_none());
    assert!(
      artifact
        .known_limits
        .iter()
        .any(|limit| limit.contains("does not call auv-driver"))
    );
  }

  #[test]
  fn refused_promotion_does_not_produce_action_decision() {
    let mut promotion = promoted_artifact();
    promotion.decision = CandidatePromotion::Refused {
      reasons: vec![crate::candidate_promotion::PromotionRefusal::PermissionMissing],
    };
    let request =
      CandidateActionDecisionRequest::new("decision_text_area", "text-area-action-decision");

    let error = build_candidate_action_decision_artifact(&promotion, &request)
      .expect_err("refused promotion must not produce action decision");

    assert_eq!(error, CandidateActionDecisionError::PromotionDidNotPromote);
  }

  #[test]
  fn recorded_operation_persists_decide_only_action_decision_artifact() {
    let project_root = temp_dir("candidate-action-decision-record-project");
    let store_root = temp_dir("candidate-action-decision-record-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())
      .expect("runtime should build");
    let promotion = promoted_artifact();

    let output = runtime
      .run_recorded_operation(
        RunSpec::new(RunType::Execute, "auv.candidate.action.decide_only"),
        "Candidate action decide-only artifact recording",
        |context| {
          let source_path = project_root.join("candidate-promotion-source.json");
          fs::write(&source_path, "{\"fixture\":true}\n").expect("promotion source should write");
          let (_, source_promotion_ref) = context
            .stage_artifact_file_with_ref(
              "candidate-promotion",
              &source_path,
              "candidate-promotion-source.json",
              Some("Recorded source promotion artifact.".to_string()),
            )
            .expect("source promotion artifact should stage");

          let mut request =
            CandidateActionDecisionRequest::new("decision_text_area", "text-area-action-decision");
          request.source_candidate_promotion_artifact = Some(source_promotion_ref);
          record_candidate_action_decision_artifact(context, &promotion, &request)
        },
      )
      .expect("recorded decide-only action decision operation should succeed");

    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("recorded run should persist");
    assert_eq!(run.run.status_code, TraceStatusCode::Ok);
    assert_eq!(run.artifacts.len(), 2);
    assert_eq!(run.artifacts[0].role, "candidate-promotion");
    assert_eq!(run.artifacts[1].role, "candidate-action-decision");
    assert!(
      run
        .events
        .iter()
        .any(|event| event.name == "candidate.action.decision.artifact_recorded")
    );
    let (_artifact_ref, artifact) = output.value;
    assert_eq!(
      artifact.action_resolver_decision.operation,
      "candidate.action.decide_only"
    );
    assert_eq!(artifact.detail["input_delivery"], json!("not_attempted"));
    let inspect = runtime
      .inspect(output.run_id.as_str())
      .expect("recorded decide-only run should inspect");
    assert!(inspect.contains("Candidate Action Decision Lineage:"));
    assert!(inspect.contains("resolver=candidate.action.decide_only"));
    assert!(inspect.contains("side_effect=none_decide_only"));
    assert!(inspect.contains("input_delivery=not_attempted"));
    assert!(inspect.contains("operation_result=not_produced"));
    assert!(inspect.contains("verification_result=not_produced"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn build_execution_artifact_requires_fresh_execution_consent() {
    let promotion = promoted_artifact();
    let decision = decision_artifact();
    let request =
      CandidateActionExecutionRequest::new("execution_text_area", "text-area-action-execution")
        .with_source_candidate_action_decision_artifact(source_decision_ref());

    let error = build_candidate_action_execution_artifact(
      &promotion,
      &decision,
      &request,
      RunId::new("run_l8b_execution"),
      auv_driver::InputActionResult::single_success(auv_driver::InputDeliveryPath::AxPress),
    )
    .expect_err("missing L8b consent must refuse execution");

    assert_eq!(error, CandidateActionDecisionError::MissingExecutionConsent);
  }

  #[test]
  fn build_execution_artifact_records_input_result_and_activation_only_verification() {
    let promotion = promoted_artifact();
    let decision = decision_artifact();
    let request = execution_request();

    let artifact = build_candidate_action_execution_artifact(
      &promotion,
      &decision,
      &request,
      RunId::new("run_l8b_execution"),
      auv_driver::InputActionResult::single_success(
        auv_driver::InputDeliveryPath::WindowTargetedMouse,
      ),
    )
    .expect("approved execution should build");

    assert_eq!(artifact.execution_id, "execution_text_area");
    assert_eq!(artifact.source_decision_id, "decision_text_area");
    assert_eq!(artifact.candidate_local_id, "promoted-item_text_area");
    assert_eq!(
      artifact.input_action_result.selected_path,
      auv_driver::InputDeliveryPath::WindowTargetedMouse
    );
    assert_eq!(artifact.operation_result.status, OperationStatus::Completed);
    assert_eq!(
      artifact.operation_result.output,
      OperationOutput::Acknowledged {
        message: Some(
          "single candidate action activated; semantic verification remains activation_only"
            .to_string()
        )
      }
    );
    assert_eq!(artifact.operation_result.verifications.len(), 1);
    assert!(artifact.verification_result.executed);
    assert!(!artifact.verification_result.state_changed);
    assert_eq!(artifact.verification_result.semantic_matched, None);
    assert_eq!(
      artifact.verification_result.method,
      VerificationMethod::Custom {
        name: "activation_only".to_string()
      }
    );
    assert_eq!(artifact.detail["input_delivery"], json!("attempted"));
    assert_eq!(artifact.detail["readiness"], json!("ready"));
    assert_eq!(artifact.detail["verification"], json!("activation_only"));
    assert_eq!(artifact.detail["verification_count"], json!(1));
    assert_eq!(artifact.detail["post_action_verification_count"], json!(0));
    let rendered = serde_json::to_value(&artifact).expect("execution artifact should serialize");
    assert!(rendered.get("input_action_result").is_some());
    assert!(rendered.get("operation_result").is_some());
    assert!(rendered.get("verification_result").is_some());
    assert!(
      artifact
        .known_limits
        .iter()
        .any(|limit| limit.contains("activation_only verification"))
    );
  }

  #[test]
  fn build_execution_artifact_records_post_action_semantic_verification() {
    let promotion = promoted_artifact();
    let decision = decision_artifact();
    let request = execution_request().with_post_action_verification(semantic_verification(true));

    let artifact = build_candidate_action_execution_artifact(
      &promotion,
      &decision,
      &request,
      RunId::new("run_l8b_execution"),
      auv_driver::InputActionResult::single_success(
        auv_driver::InputDeliveryPath::WindowTargetedMouse,
      ),
    )
    .expect("approved execution with post-action verification should build");

    assert_eq!(artifact.operation_result.status, OperationStatus::Completed);
    assert_eq!(artifact.operation_result.verifications.len(), 2);
    assert_eq!(
      artifact.verification_result.method,
      VerificationMethod::SemanticMatch
    );
    assert_eq!(artifact.verification_result.semantic_matched, Some(true));
    assert_eq!(
      artifact.detail["verification"],
      json!("activation_only+post_action:semantic_match")
    );
    assert_eq!(artifact.detail["verification_count"], json!(2));
    assert_eq!(artifact.detail["post_action_verification_count"], json!(1));
    assert_eq!(artifact.detail["semantic_matched"], json!(true));
  }

  #[test]
  fn build_execution_artifact_marks_semantic_mismatch_failed() {
    let promotion = promoted_artifact();
    let decision = decision_artifact();
    let request = execution_request().with_post_action_verification(semantic_verification(false));

    let artifact = build_candidate_action_execution_artifact(
      &promotion,
      &decision,
      &request,
      RunId::new("run_l8b_execution"),
      auv_driver::InputActionResult::single_success(
        auv_driver::InputDeliveryPath::WindowTargetedMouse,
      ),
    )
    .expect("semantic mismatch should still produce audit artifact");

    assert_eq!(artifact.operation_result.status, OperationStatus::Failed);
    assert_eq!(
      artifact.verification_result.method,
      VerificationMethod::SemanticMatch
    );
    assert_eq!(artifact.verification_result.semantic_matched, Some(false));
    assert_eq!(
      artifact.verification_result.failure_layer,
      Some(FailureLayer::SemanticMismatch)
    );
    assert_eq!(artifact.detail["operation_status"], json!("failed"));
    assert_eq!(artifact.detail["semantic_matched"], json!(false));
  }

  #[test]
  fn failed_execution_artifact_records_control_failed_activation_only() {
    let promotion = promoted_artifact();
    let decision = decision_artifact();
    let request = execution_request();

    let artifact = build_candidate_action_execution_artifact(
      &promotion,
      &decision,
      &request,
      RunId::new("run_l8b_execution"),
      auv_driver::InputActionResult {
        selected_path: auv_driver::InputDeliveryPath::Unsupported,
        attempts: vec![auv_driver::InputAttempt::failure(
          auv_driver::InputDeliveryPath::Unsupported,
          "fixture failure",
        )],
        fallback_reason: Some("fixture failure".to_string()),
        mouse_disturbance: auv_driver::DisturbanceLevel::Unknown,
        focus_disturbance: auv_driver::DisturbanceLevel::Unknown,
        clipboard_disturbance: auv_driver::DisturbanceLevel::None,
      },
    )
    .expect("failed delivery should still produce audit artifact");

    assert_eq!(artifact.operation_result.status, OperationStatus::Failed);
    assert!(!artifact.verification_result.executed);
    assert_eq!(
      artifact.verification_result.failure_layer,
      Some(FailureLayer::ControlFailed)
    );
    assert_eq!(artifact.detail["input_delivery"], json!("failed"));
  }

  #[test]
  fn execution_consent_must_match_decision_and_candidate() {
    let promotion = promoted_artifact();
    let decision = decision_artifact();
    let mut consent = execution_consent();
    consent.candidate_local_id = "wrong_candidate".to_string();
    let request =
      CandidateActionExecutionRequest::new("execution_text_area", "text-area-action-execution")
        .with_source_candidate_action_decision_artifact(source_decision_ref())
        .with_consent(consent);

    let error = build_candidate_action_execution_artifact(
      &promotion,
      &decision,
      &request,
      RunId::new("run_l8b_execution"),
      auv_driver::InputActionResult::single_success(auv_driver::InputDeliveryPath::AxPress),
    )
    .expect_err("mismatched consent must refuse execution");

    assert!(matches!(
      error,
      CandidateActionDecisionError::ExecutionConsentMismatch { .. }
    ));
  }

  #[test]
  fn execute_single_candidate_action_blocks_not_ready_before_executor_delivery() {
    let promotion = promoted_artifact();
    let decision = decision_artifact();
    let request =
      CandidateActionExecutionRequest::new("execution_text_area", "text-area-action-execution")
        .with_source_candidate_action_decision_artifact(source_decision_ref())
        .with_consent(execution_consent());
    let mut executor = FakeExecutor {
      result: auv_driver::InputActionResult::single_success(
        auv_driver::InputDeliveryPath::WindowTargetedMouse,
      ),
      readiness: not_ready_report("accessibility permission is missing"),
      observed_plan: None,
      executed: false,
    };

    let artifact = execute_single_candidate_action(
      &mut executor,
      &promotion,
      &decision,
      &request,
      RunId::new("run_l8b_execution"),
    )
    .expect("not-ready path should still produce an audit artifact");

    assert!(executor.observed_plan.is_some());
    assert!(
      !executor.executed,
      "executor must not deliver input when readiness is not ready"
    );
    assert_eq!(
      artifact.side_effect,
      CandidateActionExecutionSideEffect::BlockedNotReady
    );
    assert_eq!(artifact.detail["readiness"], json!("not_ready"));
    assert_eq!(artifact.detail["input_delivery"], json!("not_attempted"));
    assert_eq!(
      artifact.input_action_result.selected_path,
      auv_driver::InputDeliveryPath::Unsupported
    );
    assert_eq!(artifact.operation_result.status, OperationStatus::Failed);
  }

  #[test]
  fn execute_single_candidate_action_refuses_before_executor_without_consent() {
    let promotion = promoted_artifact();
    let decision = decision_artifact();
    let request =
      CandidateActionExecutionRequest::new("execution_text_area", "text-area-action-execution")
        .with_source_candidate_action_decision_artifact(source_decision_ref());
    let mut executor = FakeExecutor {
      result: auv_driver::InputActionResult::single_success(
        auv_driver::InputDeliveryPath::WindowTargetedMouse,
      ),
      readiness: ready_report(),
      observed_plan: None,
      executed: false,
    };

    let error = execute_single_candidate_action(
      &mut executor,
      &promotion,
      &decision,
      &request,
      RunId::new("run_l8b_execution"),
    )
    .expect_err("missing L8b consent must refuse before input delivery");

    assert_eq!(error, CandidateActionDecisionError::MissingExecutionConsent);
    assert!(
      executor.observed_plan.is_none(),
      "executor must not be called before L8b consent is validated"
    );
    assert!(!executor.executed);
  }

  #[test]
  fn recorded_operation_persists_execution_and_operation_result_artifacts() {
    let project_root = temp_dir("candidate-action-execution-record-project");
    let store_root = temp_dir("candidate-action-execution-record-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())
      .expect("runtime should build");
    let promotion = promoted_artifact();
    let decision = decision_artifact();

    let output = runtime
      .run_recorded_operation(
        RunSpec::new(RunType::Execute, "auv.candidate.action.execute_single"),
        "Candidate action execution artifact recording",
        |context| {
          let decision_source_path = project_root.join("candidate-action-decision-source.json");
          fs::write(&decision_source_path, "{\"fixture\":true}\n")
            .expect("decision source should write");
          let (_, source_decision_ref) = context
            .stage_artifact_file_with_ref(
              "candidate-action-decision",
              &decision_source_path,
              "candidate-action-decision-source.json",
              Some("Recorded source action decision artifact.".to_string()),
            )
            .expect("source decision artifact should stage");

          let request = CandidateActionExecutionRequest::new(
            "execution_text_area",
            "text-area-action-execution",
          )
          .with_source_candidate_action_decision_artifact(source_decision_ref.clone())
          .with_consent(CandidateActionExecutionConsent {
            run_id: source_decision_ref.run_id.as_str().to_string(),
            ..execution_consent()
          })
          .with_readiness(ready_report());
          record_candidate_action_execution_artifact(
            context,
            &promotion,
            &decision,
            &request,
            auv_driver::InputActionResult::single_success(auv_driver::InputDeliveryPath::AxPress),
          )
        },
      )
      .expect("recorded execution operation should succeed");

    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("recorded run should persist");
    assert_eq!(run.run.status_code, TraceStatusCode::Ok);
    assert_eq!(run.artifacts.len(), 3);
    assert_eq!(run.artifacts[0].role, "candidate-action-decision");
    assert_eq!(run.artifacts[1].role, "operation-result");
    assert_eq!(run.artifacts[2].role, "candidate-action-execution");
    let (_artifact_ref, artifact) = output.value;
    assert!(artifact.operation_result_artifact.is_some());
    assert_eq!(
      artifact.input_action_result.selected_path,
      auv_driver::InputDeliveryPath::AxPress
    );
    let verifications = runtime
      .list_verifications(output.run_id.as_str())
      .expect("operation-result verification should read");
    assert_eq!(verifications.len(), 1);
    assert_eq!(
      verifications[0].method,
      VerificationMethod::Custom {
        name: "activation_only".to_string()
      }
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn recorded_operation_can_execute_and_record_with_fake_executor() {
    let project_root = temp_dir("candidate-action-execute-record-project");
    let store_root = temp_dir("candidate-action-execute-record-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())
      .expect("runtime should build");
    let promotion = promoted_artifact();
    let decision = decision_artifact();
    let mut executor = FakeExecutor {
      result: auv_driver::InputActionResult::single_success(
        auv_driver::InputDeliveryPath::WindowTargetedMouse,
      ),
      readiness: ready_report(),
      observed_plan: None,
      executed: false,
    };

    let output = runtime
      .run_recorded_operation(
        RunSpec::new(RunType::Execute, "auv.candidate.action.execute_single"),
        "Candidate action execute-and-record smoke fixture",
        |context| {
          let decision_source_path = project_root.join("candidate-action-decision-source.json");
          fs::write(&decision_source_path, "{\"fixture\":true}\n")
            .expect("decision source should write");
          let (_, source_decision_ref) = context
            .stage_artifact_file_with_ref(
              "candidate-action-decision",
              &decision_source_path,
              "candidate-action-decision-source.json",
              Some("Recorded source action decision artifact.".to_string()),
            )
            .expect("source decision artifact should stage");

          let request = CandidateActionExecutionRequest::new(
            "execution_text_area",
            "text-area-action-execution",
          )
          .with_source_candidate_action_decision_artifact(source_decision_ref.clone())
          .with_consent(CandidateActionExecutionConsent {
            run_id: source_decision_ref.run_id.as_str().to_string(),
            ..execution_consent()
          });
          execute_and_record_single_candidate_action(
            context,
            &mut executor,
            &promotion,
            &decision,
            &request,
          )
        },
      )
      .expect("execute-and-record operation should succeed");

    assert!(executor.observed_plan.is_some());
    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("recorded run should persist");
    assert_eq!(run.artifacts.len(), 3);
    assert_eq!(run.artifacts[0].role, "candidate-action-decision");
    assert_eq!(run.artifacts[1].role, "operation-result");
    assert_eq!(run.artifacts[2].role, "candidate-action-execution");
    let inspect = runtime
      .inspect(output.run_id.as_str())
      .expect("execute-and-record run should inspect");
    assert!(inspect.contains("Candidate Action Execution Lineage:"));
    assert!(inspect.contains("input_delivery=attempted"));
    assert!(inspect.contains("selected_path=window_targeted_mouse"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn recorded_operation_keeps_post_action_semantic_verification_from_execute_path() {
    let project_root = temp_dir("candidate-action-execute-post-action-record-project");
    let store_root = temp_dir("candidate-action-execute-post-action-record-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())
      .expect("runtime should build");
    let promotion = promoted_artifact();
    let decision = decision_artifact();
    let mut executor = FakeVerifyingExecutor {
      result: auv_driver::InputActionResult::single_success(
        auv_driver::InputDeliveryPath::WindowTargetedMouse,
      ),
      readiness: ready_report(),
      observed_plan: None,
      executed: false,
      verification_calls: 0,
      verification: semantic_verification(true),
      verification_error: None,
    };

    let output = runtime
      .run_recorded_operation(
        RunSpec::new(RunType::Execute, "auv.candidate.action.execute_single"),
        "Candidate action execute-and-record post-action verification fixture",
        |context| {
          let decision_source_path = project_root.join("candidate-action-decision-source.json");
          fs::write(&decision_source_path, "{\"fixture\":true}\n")
            .expect("decision source should write");
          let (_, source_decision_ref) = context
            .stage_artifact_file_with_ref(
              "candidate-action-decision",
              &decision_source_path,
              "candidate-action-decision-source.json",
              Some("Recorded source action decision artifact.".to_string()),
            )
            .expect("source decision artifact should stage");

          let request = CandidateActionExecutionRequest::new(
            "execution_text_area",
            "text-area-action-execution",
          )
          .with_source_candidate_action_decision_artifact(source_decision_ref.clone())
          .with_consent(CandidateActionExecutionConsent {
            run_id: source_decision_ref.run_id.as_str().to_string(),
            ..execution_consent()
          })
          .with_post_action_probe(CandidateActionPostActionProbe::focused_ax_node_reobserved());
          execute_and_record_single_candidate_action(
            context,
            &mut executor,
            &promotion,
            &decision,
            &request,
          )
        },
      )
      .expect("execute-and-record operation with post-action verification should succeed");

    assert!(executor.observed_plan.is_some());
    assert!(executor.executed);
    assert_eq!(executor.verification_calls, 1);
    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("recorded run should persist");
    assert_eq!(run.artifacts.len(), 3);
    let (_artifact_ref, artifact) = output.value;
    assert_eq!(artifact.operation_result.status, OperationStatus::Completed);
    assert_eq!(artifact.operation_result.verifications.len(), 2);
    assert_eq!(
      artifact.verification_result.method,
      VerificationMethod::SemanticMatch
    );
    assert_eq!(artifact.verification_result.semantic_matched, Some(true));
    assert_eq!(
      artifact.detail["verification"],
      json!("activation_only+post_action:semantic_match")
    );
    assert_eq!(artifact.detail["post_action_verification_count"], json!(1));
    let verifications = runtime
      .list_verifications(output.run_id.as_str())
      .expect("recorded run verifications should read");
    assert_eq!(verifications.len(), 2);
    assert_eq!(
      verifications[0].method,
      VerificationMethod::Custom {
        name: "activation_only".to_string()
      }
    );
    assert_eq!(verifications[1].method, VerificationMethod::SemanticMatch);
    assert_eq!(verifications[1].semantic_matched, Some(true));
    let inspect = runtime
      .inspect(output.run_id.as_str())
      .expect("execute-and-record run should inspect");
    assert!(inspect.contains("verification=activation_only+post_action:semantic_match"));
    assert!(inspect.contains("semantic_matched=true"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn execute_single_candidate_action_uses_executor_once_and_records_activation_only() {
    let promotion = promoted_artifact();
    let decision = decision_artifact();
    let request =
      CandidateActionExecutionRequest::new("execution_text_area", "text-area-action-execution")
        .with_source_candidate_action_decision_artifact(source_decision_ref())
        .with_consent(execution_consent());
    let mut executor = FakeExecutor {
      result: auv_driver::InputActionResult::single_success(
        auv_driver::InputDeliveryPath::WindowTargetedMouse,
      ),
      readiness: ready_report(),
      observed_plan: None,
      executed: false,
    };

    let artifact = execute_single_candidate_action(
      &mut executor,
      &promotion,
      &decision,
      &request,
      RunId::new("run_l8b_execution"),
    )
    .expect("fake executor should produce execution artifact");

    let plan = executor
      .observed_plan
      .expect("executor should observe plan");
    assert_eq!(plan.selected_method, "pointer-click");
    assert_eq!(plan.target_grounding, TargetGrounding::Coordinate);
    assert_eq!(plan.window_number, Some(11));
    assert_eq!(plan.window_x, 161.0);
    assert_eq!(plan.window_y, 60.0);
    assert_eq!(
      artifact.input_action_result.selected_path,
      auv_driver::InputDeliveryPath::WindowTargetedMouse
    );
    assert!(executor.executed);
    assert_eq!(
      artifact.verification_result.method,
      VerificationMethod::Custom {
        name: "activation_only".to_string()
      }
    );
  }

  #[test]
  fn execute_single_candidate_action_runs_explicit_post_action_probe_after_delivery() {
    let promotion = promoted_artifact();
    let decision = decision_artifact();
    let request =
      CandidateActionExecutionRequest::new("execution_text_area", "text-area-action-execution")
        .with_source_candidate_action_decision_artifact(source_decision_ref())
        .with_consent(execution_consent())
        .with_post_action_probe(CandidateActionPostActionProbe::focused_ax_node_reobserved());
    let mut executor = FakeVerifyingExecutor {
      result: auv_driver::InputActionResult::single_success(
        auv_driver::InputDeliveryPath::WindowTargetedMouse,
      ),
      readiness: ready_report(),
      observed_plan: None,
      executed: false,
      verification_calls: 0,
      verification: semantic_verification(true),
      verification_error: None,
    };

    let artifact = execute_single_candidate_action(
      &mut executor,
      &promotion,
      &decision,
      &request,
      RunId::new("run_l8b_execution"),
    )
    .expect("explicit post-action probe should produce execution artifact");

    assert!(executor.executed);
    assert_eq!(executor.verification_calls, 1);
    assert_eq!(artifact.operation_result.status, OperationStatus::Completed);
    assert_eq!(artifact.operation_result.verifications.len(), 2);
    assert_eq!(
      artifact.verification_result.method,
      VerificationMethod::SemanticMatch
    );
    assert_eq!(artifact.verification_result.semantic_matched, Some(true));
    assert_eq!(
      artifact.detail["verification"],
      json!("activation_only+post_action:semantic_match")
    );
    assert_eq!(artifact.detail["semantic_matched"], json!(true));
  }

  #[test]
  fn execute_single_candidate_action_does_not_run_post_action_probe_when_not_requested() {
    let promotion = promoted_artifact();
    let decision = decision_artifact();
    let request =
      CandidateActionExecutionRequest::new("execution_text_area", "text-area-action-execution")
        .with_source_candidate_action_decision_artifact(source_decision_ref())
        .with_consent(execution_consent());
    let mut executor = FakeVerifyingExecutor {
      result: auv_driver::InputActionResult::single_success(
        auv_driver::InputDeliveryPath::WindowTargetedMouse,
      ),
      readiness: ready_report(),
      observed_plan: None,
      executed: false,
      verification_calls: 0,
      verification: semantic_verification(true),
      verification_error: None,
    };

    let artifact = execute_single_candidate_action(
      &mut executor,
      &promotion,
      &decision,
      &request,
      RunId::new("run_l8b_execution"),
    )
    .expect("execution without explicit post-action probe should still succeed");

    assert!(executor.executed);
    assert_eq!(executor.verification_calls, 0);
    assert_eq!(artifact.operation_result.verifications.len(), 1);
    assert_eq!(
      artifact.verification_result.method,
      VerificationMethod::Custom {
        name: "activation_only".to_string()
      }
    );
    assert_eq!(artifact.verification_result.semantic_matched, None);
    assert_eq!(artifact.detail["verification"], json!("activation_only"));
  }

  #[test]
  fn execute_single_candidate_action_records_artifact_when_post_action_probe_fails_after_delivery()
  {
    let promotion = promoted_artifact();
    let decision = decision_artifact();
    let request =
      CandidateActionExecutionRequest::new("execution_text_area", "text-area-action-execution")
        .with_source_candidate_action_decision_artifact(source_decision_ref())
        .with_consent(execution_consent())
        .with_post_action_probe(CandidateActionPostActionProbe::focused_ax_node_reobserved());
    let mut executor = FakeVerifyingExecutor {
      result: auv_driver::InputActionResult::single_success(
        auv_driver::InputDeliveryPath::WindowTargetedMouse,
      ),
      readiness: ready_report(),
      observed_plan: None,
      executed: false,
      verification_calls: 0,
      verification: semantic_verification(true),
      verification_error: Some("post observe failed".to_string()),
    };

    let artifact = execute_single_candidate_action(
      &mut executor,
      &promotion,
      &decision,
      &request,
      RunId::new("run_l8b_execution"),
    )
    .expect("post-action verification failure after delivery must still record execution");

    assert!(executor.executed);
    assert_eq!(executor.verification_calls, 1);
    assert_eq!(artifact.detail["input_delivery"], json!("attempted"));
    assert_eq!(artifact.operation_result.status, OperationStatus::Failed);
    assert_eq!(artifact.verification_result.semantic_matched, Some(false));
    assert_eq!(
      artifact.verification_result.failure_layer,
      Some(FailureLayer::VerificationUnreliable)
    );
    assert!(
      artifact
        .verification_result
        .observed_label
        .as_deref()
        .is_some_and(|label| label.contains("post-action verification failed after input delivery"))
    );
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn gated_macos_executor_smoke_is_env_gated() {
    if std::env::var("AUV_L8B_EXECUTE_SMOKE").ok().as_deref() != Some("1") {
      eprintln!("skip: set AUV_L8B_EXECUTE_SMOKE=1 to run the side-effecting L8b smoke");
      return;
    }

    let window_number = match std::env::var("AUV_L8B_WINDOW_NUMBER")
      .ok()
      .and_then(|value| value.parse::<i64>().ok())
    {
      Some(window_number) => window_number,
      None => {
        eprintln!("skip: AUV_L8B_WINDOW_NUMBER is required for L8b smoke");
        return;
      }
    };
    let window_x = std::env::var("AUV_L8B_WINDOW_X")
      .ok()
      .and_then(|value| value.parse::<f64>().ok())
      .unwrap_or(10.0);
    let window_y = std::env::var("AUV_L8B_WINDOW_Y")
      .ok()
      .and_then(|value| value.parse::<f64>().ok())
      .unwrap_or(10.0);
    let mut executor = MacosCandidateActionExecutor;
    let Some(expected_window_frame) = smoke_expected_window_frame() else {
      eprintln!(
        "skip: AUV_L8B_WINDOW_FRAME_X/Y/WIDTH/HEIGHT are required for readiness-gated L8b smoke"
      );
      return;
    };
    let plan = CandidateActionDeliveryPlan {
      selected_method: "pointer-click".to_string(),
      target_grounding: TargetGrounding::Coordinate,
      target_query: "env-gated-smoke".to_string(),
      window_number: Some(window_number),
      window_title: std::env::var("AUV_L8B_WINDOW_TITLE").ok(),
      app_bundle_id: std::env::var("AUV_L8B_APP_BUNDLE_ID").ok(),
      expected_window_frame: Some(expected_window_frame),
      window_x,
      window_y,
      click_count: 1,
    };
    let readiness = executor
      .readiness(&plan)
      .expect("L8b smoke readiness should probe");
    assert!(
      readiness.is_ready(),
      "L8b smoke readiness failed: {readiness:?}"
    );
    let result = executor.execute(&plan);

    assert!(result.is_ok(), "L8b smoke delivery failed: {result:?}");
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn gated_macos_execute_and_record_smoke_is_env_gated() {
    if std::env::var("AUV_L8B_EXECUTE_SMOKE").ok().as_deref() != Some("1") {
      eprintln!("skip: set AUV_L8B_EXECUTE_SMOKE=1 to run the side-effecting recorded L8b smoke");
      return;
    }

    let Some(window_number) = std::env::var("AUV_L8B_WINDOW_NUMBER")
      .ok()
      .and_then(|value| value.parse::<i64>().ok())
    else {
      eprintln!("skip: AUV_L8B_WINDOW_NUMBER is required for recorded L8b smoke");
      return;
    };
    let window_title = std::env::var("AUV_L8B_WINDOW_TITLE").ok();
    let app_bundle_id =
      std::env::var("AUV_L8B_APP_BUNDLE_ID").unwrap_or_else(|_| "com.apple.TextEdit".to_string());
    let Some(expected_window_frame) = smoke_expected_window_frame() else {
      eprintln!(
        "skip: AUV_L8B_WINDOW_FRAME_X/Y/WIDTH/HEIGHT are required for readiness-gated recorded L8b smoke"
      );
      return;
    };
    let expected_window_frame = serde_json::json!({
      "x": expected_window_frame.origin.x,
      "y": expected_window_frame.origin.y,
      "width": expected_window_frame.size.width,
      "height": expected_window_frame.size.height,
    });
    let Some(target_box) = smoke_focused_target_box(&app_bundle_id) else {
      eprintln!("skip: unable to capture focused AX target bounds for recorded L8b smoke");
      return;
    };

    let project_root = temp_dir("candidate-action-execute-recorded-smoke-project");
    let store_root = temp_dir("candidate-action-execute-recorded-smoke-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())
      .expect("runtime should build");
    let mut recognition_1 = sample_frame_with_box_and_window_frame(
      "recognition_frame_1",
      target_box.clone(),
      expected_window_frame.clone(),
    );
    let mut recognition_2 = sample_frame_with_box_and_window_frame(
      "recognition_frame_2",
      target_box,
      expected_window_frame,
    );
    for recognition in [&mut recognition_1, &mut recognition_2] {
      recognition.scope.app_bundle_id = Some(app_bundle_id.clone());
      recognition.scope.window_title = window_title.clone();
      recognition.scope.window_number = Some(window_number);
    }
    let observations = vec![recognition_1, recognition_2];
    let latest = observations.last().expect("latest frame exists");
    let mut promotion_request = CandidatePromotionArtifactRequest::new(
      "promotion_text_area_smoke",
      "promotion-text-area-smoke",
    );
    promotion_request.source_recognition_artifact = Some(sample_artifact_ref());
    promotion_request.stability_policy = StabilityPolicy {
      min_frames: 2,
      max_centroid_drift_px: 4.0,
      require_stable_text: true,
    };
    promotion_request.projection =
      crate::candidate_promotion::PromotionProjection::IdentityWindowAddressable;
    promotion_request.freshness = Some(
      freshness_from_capture_backed_recognition(latest, "debug.captureAxTree", "fresh")
        .expect("latest recognition is capture-backed"),
    );
    promotion_request.permission = Some(
      explicit_consent_for_candidate_promotion(
        &promotion_request.promotion_id,
        latest,
        CandidatePromotionConsentInput {
          granted_by: "human-review".to_string(),
          scope_note: "candidate promotion only, no action execution".to_string(),
          evidence_note: "env-gated recorded smoke promotion consent".to_string(),
          approved_at_millis: 1,
        },
      )
      .expect("latest recognition is capture-backed"),
    );
    let promotion = build_candidate_promotion_artifact(&observations, &promotion_request)
      .expect("smoke promotion artifact should build");
    let decision_request = CandidateActionDecisionRequest::new(
      "decision_text_area_smoke",
      "text-area-action-decision-smoke",
    )
    .with_source_candidate_promotion_artifact(sample_artifact_ref());
    let decision = build_candidate_action_decision_artifact(&promotion, &decision_request)
      .expect("smoke decision should build");
    let mut executor = MacosCandidateActionExecutor;

    let output = runtime
      .run_recorded_operation(
        RunSpec::new(RunType::Execute, "auv.candidate.action.execute_single"),
        "Candidate action execute-and-record env-gated smoke",
        |context| {
          let decision_source_path =
            project_root.join("candidate-action-decision-smoke-source.json");
          fs::write(&decision_source_path, "{\"smoke\":true}\n")
            .expect("decision source should write");
          let (_, source_decision_ref) = context
            .stage_artifact_file_with_ref(
              "candidate-action-decision",
              &decision_source_path,
              "candidate-action-decision-smoke-source.json",
              Some("Recorded source action decision artifact for L8b smoke.".to_string()),
            )
            .expect("source decision artifact should stage");
          let request = CandidateActionExecutionRequest::new(
            "execution_text_area_smoke",
            "text-area-action-execution-smoke",
          )
          .with_source_candidate_action_decision_artifact(source_decision_ref.clone())
          .with_post_action_probe(CandidateActionPostActionProbe::focused_ax_node_reobserved())
          .with_consent(CandidateActionExecutionConsent {
            consent_id: "consent_execute_text_area_smoke".to_string(),
            granted_by: "human-review".to_string(),
            scope_note: "execute exactly one approved TextEdit smoke action".to_string(),
            run_id: source_decision_ref.run_id.as_str().to_string(),
            source_promotion_id: promotion.promotion_id.clone(),
            source_decision_id: decision.decision_id.clone(),
            candidate_local_id: decision.candidate_local_id.clone(),
            approved_action: CandidateActionExecutionConsentAction::ExecuteSingleCandidateAction,
            approved_at_millis: crate::model::now_millis(),
            evidence_note: "env-gated L8b recorded smoke execution consent".to_string(),
          });
          execute_and_record_single_candidate_action(
            context,
            &mut executor,
            &promotion,
            &decision,
            &request,
          )
        },
      )
      .expect("recorded L8b smoke should execute and persist");

    let inspect = runtime
      .inspect(output.run_id.as_str())
      .expect("recorded L8b smoke should inspect");
    assert!(inspect.contains("Candidate Action Execution Lineage:"));
    assert!(inspect.contains("input_delivery=attempted"));
    assert!(inspect.contains("verification=activation_only+post_action:semantic_match"));
    assert!(inspect.contains("semantic_matched=true"));
    eprintln!(
      "L8b recorded smoke run_id={} store={}",
      output.run_id.as_str(),
      store_root.display()
    );
  }

  fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("auv-{}-{}", label, crate::model::now_millis()))
  }

  #[cfg(target_os = "macos")]
  fn smoke_focused_target_box(app_bundle_id: &str) -> Option<RecognitionBox> {
    let capture =
      auv_driver_macos::native::ax_tree::capture_ax_tree_snapshot(app_bundle_id, 8, 80).ok()?;
    let focused = capture.snapshot.nodes.iter().find(|node| node.focused)?;
    Some(RecognitionBox {
      x: focused.bounds.x,
      y: focused.bounds.y,
      width: focused.bounds.width,
      height: focused.bounds.height,
    })
  }

  #[cfg(target_os = "macos")]
  fn smoke_expected_window_frame() -> Option<auv_driver::Rect> {
    Some(auv_driver::Rect::new(
      std::env::var("AUV_L8B_WINDOW_FRAME_X").ok()?.parse().ok()?,
      std::env::var("AUV_L8B_WINDOW_FRAME_Y").ok()?.parse().ok()?,
      std::env::var("AUV_L8B_WINDOW_FRAME_WIDTH")
        .ok()?
        .parse()
        .ok()?,
      std::env::var("AUV_L8B_WINDOW_FRAME_HEIGHT")
        .ok()?
        .parse()
        .ok()?,
    ))
  }
}
