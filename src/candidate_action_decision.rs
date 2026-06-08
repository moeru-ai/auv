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

  pub fn with_post_action_verification(mut self, verification: VerificationResult) -> Self {
    self.post_action_verifications.push(verification);
    self
  }
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidateActionDeliveryPlan {
  pub selected_method: String,
  pub target_grounding: TargetGrounding,
  pub target_query: String,
  pub window_number: Option<i64>,
  pub window_title: Option<String>,
  pub app_bundle_id: Option<String>,
  pub window_x: f64,
  pub window_y: f64,
  pub click_count: u32,
}

pub trait CandidateActionExecutor {
  fn execute(
    &mut self,
    plan: &CandidateActionDeliveryPlan,
  ) -> AuvResult<auv_driver::InputActionResult>;
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

  let succeeded = input_action_result
    .attempts
    .iter()
    .any(|attempt| attempt.succeeded);
  let candidate_ref = candidate_ref_from_source(
    decision.source_candidate_promotion_artifact.as_ref(),
    &promotion.promotion_id,
    &candidate.candidate_local_id,
  );
  let mut evidence_artifacts = Vec::new();
  evidence_artifacts.push(source_candidate_action_decision_artifact.clone());
  if let Some(source_promotion) = decision.source_candidate_promotion_artifact.clone() {
    evidence_artifacts.push(source_promotion);
  }
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
  let operation_completed = succeeded && semantic_matched != Some(false);
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
    "input_delivery": if succeeded { "attempted" } else { "failed" },
    "selected_path": selected_path,
    "attempt_count": input_action_result.attempts.len(),
    "attempts_succeeded": attempts_succeeded,
    "operation_status": if operation_completed { "completed" } else { "failed" },
    "verification": verification_summary,
    "verification_count": 1 + request.post_action_verifications.len(),
    "post_action_verification_count": request.post_action_verifications.len(),
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
    input_action_result,
    operation_result,
    verification_result: primary_verification,
    side_effect: CandidateActionExecutionSideEffect::SingleInputDelivered,
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
  record_candidate_action_execution_artifact(
    context,
    promotion,
    decision,
    request,
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
  let input_action_result =
    executor
      .execute(&plan)
      .map_err(|error| CandidateActionDecisionError::ExecutionFailed {
        reason: error.to_string(),
      })?;
  build_candidate_action_execution_artifact(
    promotion,
    decision,
    request,
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

  Ok(CandidateActionDeliveryPlan {
    selected_method: decision.action_resolver_decision.selected_method.clone(),
    target_grounding: candidate.target_spec.grounding,
    target_query: decision.action_resolver_decision.target_query.clone(),
    window_number: window_ref.window_number,
    window_title: window_ref.window_title_substring.clone(),
    app_bundle_id: Some(window_ref.app_bundle_id.clone()),
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

#[cfg(target_os = "macos")]
pub struct MacosCandidateActionExecutor;

#[cfg(target_os = "macos")]
impl CandidateActionExecutor for MacosCandidateActionExecutor {
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

fn execution_message(
  delivery_succeeded: bool,
  semantic_matched: Option<bool>,
  has_post_action_verification: bool,
) -> String {
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
    CandidateActionExecutor, build_candidate_action_decision_artifact,
    build_candidate_action_execution_artifact, execute_and_record_single_candidate_action,
    execute_single_candidate_action, record_candidate_action_decision_artifact,
    record_candidate_action_execution_artifact,
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
    let capture_artifact = sample_artifact_ref();
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
        box_: RecognitionBox {
          x,
          y,
          width: 300,
          height: 80,
        },
        text: Some("Text Area".to_string()),
        provider_score: Some(0.99),
        detail: json!({"backend": "ax-fixture"}),
      }),
      filtered: vec![RecognizedItem {
        item_id: "item_text_area".to_string(),
        kind: "text_area".to_string(),
        box_: RecognitionBox {
          x,
          y,
          width: 300,
          height: 80,
        },
        text: Some("Text Area".to_string()),
        provider_score: Some(0.99),
        detail: json!({"backend": "ax-fixture"}),
      }],
      all: vec![RecognizedItem {
        item_id: "item_text_area".to_string(),
        kind: "text_area".to_string(),
        box_: RecognitionBox {
          x,
          y,
          width: 300,
          height: 80,
        },
        text: Some("Text Area".to_string()),
        provider_score: Some(0.99),
        detail: json!({"backend": "ax-fixture"}),
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
    observed_plan: Option<CandidateActionDeliveryPlan>,
  }

  impl CandidateActionExecutor for FakeExecutor {
    fn execute(
      &mut self,
      plan: &CandidateActionDeliveryPlan,
    ) -> AuvResult<auv_driver::InputActionResult> {
      self.observed_plan = Some(plan.clone());
      Ok(self.result.clone())
    }
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
    let request =
      CandidateActionExecutionRequest::new("execution_text_area", "text-area-action-execution")
        .with_source_candidate_action_decision_artifact(source_decision_ref())
        .with_consent(execution_consent());

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
    let request =
      CandidateActionExecutionRequest::new("execution_text_area", "text-area-action-execution")
        .with_source_candidate_action_decision_artifact(source_decision_ref())
        .with_consent(execution_consent())
        .with_post_action_verification(semantic_verification(true));

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
    let request =
      CandidateActionExecutionRequest::new("execution_text_area", "text-area-action-execution")
        .with_source_candidate_action_decision_artifact(source_decision_ref())
        .with_consent(execution_consent())
        .with_post_action_verification(semantic_verification(false));

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
    let request =
      CandidateActionExecutionRequest::new("execution_text_area", "text-area-action-execution")
        .with_source_candidate_action_decision_artifact(source_decision_ref())
        .with_consent(execution_consent());

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
      observed_plan: None,
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
          });
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
      observed_plan: None,
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
      observed_plan: None,
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
    assert_eq!(
      artifact.verification_result.method,
      VerificationMethod::Custom {
        name: "activation_only".to_string()
      }
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
    let result = executor.execute(&CandidateActionDeliveryPlan {
      selected_method: "pointer-click".to_string(),
      target_grounding: TargetGrounding::Coordinate,
      target_query: "env-gated-smoke".to_string(),
      window_number: Some(window_number),
      window_title: std::env::var("AUV_L8B_WINDOW_TITLE").ok(),
      app_bundle_id: std::env::var("AUV_L8B_APP_BUNDLE_ID").ok(),
      window_x,
      window_y,
      click_count: 1,
    });

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
    let window_x = std::env::var("AUV_L8B_WINDOW_X")
      .ok()
      .and_then(|value| value.parse::<i64>().ok())
      .unwrap_or(161);
    let window_y = std::env::var("AUV_L8B_WINDOW_Y")
      .ok()
      .and_then(|value| value.parse::<i64>().ok())
      .unwrap_or(60);

    let project_root = temp_dir("candidate-action-execute-recorded-smoke-project");
    let store_root = temp_dir("candidate-action-execute-recorded-smoke-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())
      .expect("runtime should build");
    let mut recognition_1 = sample_frame("recognition_frame_1", window_x, window_y);
    let mut recognition_2 = sample_frame("recognition_frame_2", window_x, window_y);
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
    assert!(inspect.contains("verification=activation_only"));
    assert!(inspect.contains("semantic_matched=n/a"));
    eprintln!(
      "L8b recorded smoke run_id={} store={}",
      output.run_id.as_str(),
      store_root.display()
    );
  }

  fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("auv-{}-{}", label, crate::model::now_millis()))
  }
}
