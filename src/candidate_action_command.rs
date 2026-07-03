use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::ax_recognition::{AxBestSelectionStrategy, AxRecognitionPolicy};
#[cfg(target_os = "macos")]
use crate::ax_recognition::{AxRecognitionRuntimeContext, map_ax_tree_to_recognition_result};
#[cfg(target_os = "macos")]
use crate::candidate_action_decision::MacosCandidateActionExecutor;
use crate::candidate_action_decision::{
  CandidateActionDecisionRequest, CandidateActionExecutionConsent,
  CandidateActionExecutionConsentAction, CandidateActionExecutionRequest,
  CandidateActionExecutionSideEffect, CandidateActionKind, CandidateActionPostActionProbe,
  execute_and_record_single_candidate_action, record_candidate_action_decision_artifact,
};
#[cfg(target_os = "macos")]
use crate::candidate_promotion::ConsentProvenance;
use crate::candidate_promotion::{
  ActionPermission, CandidatePromotion, ConsentGrade, PromotionRefusal,
};
#[cfg(target_os = "macos")]
use crate::candidate_promotion_recording::record_candidate_promotion_artifact_with_recognition_projection;
use crate::candidate_promotion_recording::{
  CandidatePromotionArtifactRequest, CandidatePromotionConsentInput,
  explicit_consent_for_candidate_promotion, freshness_from_capture_backed_recognition,
};
use crate::model::{AuvResult, now_millis};
use crate::stability::StabilityPolicy;
use auv_driver::Driver;
use auv_tracing_driver::recorded_operation::RecordedOperationContext;
const CANDIDATE_ACTION_PROPOSAL_ARTIFACT_ROLE: &str = "candidate-action-proposal";
const CANDIDATE_ACTION_PROPOSAL_ARTIFACT_VERSION: &str = "candidate_action_proposal_artifact_v0";
const OPENAI_RESPONSES_API_URL: &str = "https://api.openai.com/v1/responses";
const OPENAI_RESPONSES_TIMEOUT_MS: u64 = 30_000;

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateActionCommandRequest {
  pub app_bundle_id: String,
  pub query: Option<String>,
  pub role: Option<String>,
  pub action: Option<CandidateActionKind>,
  pub intent: Option<String>,
  pub proposer_model: Option<String>,
  pub proposer_base_url: Option<String>,
  pub reveal_shortcut: Option<String>,
  pub reveal_settle_ms: u64,
  pub stable_frames: u32,
  pub stable_frame_delay_ms: u64,
  pub max_centroid_drift_px: f64,
  pub require_stable_text: bool,
  pub dev_self_minted_consent: bool,
  pub human_gesture_consent: bool,
  pub human_gesture_timeout_ms: u64,
  pub proposal_id: String,
  pub promotion_id: String,
  pub decision_id: String,
  pub execution_id: String,
  pub granted_by: String,
  pub promotion_scope_note: String,
  pub promotion_evidence_note: String,
  pub execution_scope_note: String,
  pub execution_evidence_note: String,
}

impl CandidateActionCommandRequest {
  pub fn validate(&self) -> AuvResult<()> {
    if self.app_bundle_id.trim().is_empty() {
      return Err("--target-app is required".to_string());
    }
    match self.mode()? {
      CandidateActionCommandMode::Direct {
        query,
        role,
        action,
      } => {
        if query.trim().is_empty() {
          return Err("--query is required".to_string());
        }
        if role.trim().is_empty() {
          return Err("--role is required".to_string());
        }
        if let CandidateActionKind::TypeText { text } = action
          && text.trim().is_empty()
        {
          return Err("--text must not be empty when --action type-text".to_string());
        }
      }
      CandidateActionCommandMode::ModelProposal {
        intent,
        proposer_model,
        ..
      } => {
        if intent.trim().is_empty() {
          return Err("--intent must not be empty".to_string());
        }
        if proposer_model.trim().is_empty() {
          return Err(
            "--proposer-model or AUV_MODEL_PROPOSER_MODEL is required when --intent is set"
              .to_string(),
          );
        }
      }
    }
    if self.stable_frames == 0 {
      return Err("--stable-frames must be greater than 0".to_string());
    }
    if self.dev_self_minted_consent && self.human_gesture_consent {
      return Err(
        "--dev-self-minted-consent cannot be combined with --human-gesture-consent".to_string(),
      );
    }
    if self.human_gesture_timeout_ms == 0 {
      return Err("--human-gesture-timeout-ms must be greater than 0".to_string());
    }
    if self.dev_self_minted_consent {
      if self.granted_by.trim().is_empty() {
        return Err("--granted-by is required when --dev-self-minted-consent is set".to_string());
      }
      if self.promotion_scope_note.trim().is_empty() {
        return Err("--promotion-scope-note must not be empty".to_string());
      }
      if self.promotion_evidence_note.trim().is_empty() {
        return Err("--promotion-evidence-note must not be empty".to_string());
      }
      if self.execution_scope_note.trim().is_empty() {
        return Err("--execution-scope-note must not be empty".to_string());
      }
      if self.execution_evidence_note.trim().is_empty() {
        return Err("--execution-evidence-note must not be empty".to_string());
      }
    }
    if self.proposal_id.trim().is_empty() {
      return Err("--proposal-id must not be empty".to_string());
    }
    Ok(())
  }

  fn mode(&self) -> AuvResult<CandidateActionCommandMode> {
    if let Some(intent) = self.intent.as_deref() {
      if self.query.is_some() || self.role.is_some() || self.action.is_some() {
        return Err(
          "--intent cannot be combined with --query, --role, --action, or --text".to_string(),
        );
      }
      return Ok(CandidateActionCommandMode::ModelProposal {
        intent: intent.to_string(),
        proposer_model: self
          .proposer_model
          .clone()
          .or_else(|| read_env_trimmed("AUV_MODEL_PROPOSER_MODEL"))
          .unwrap_or_default(),
        proposer_base_url: self
          .proposer_base_url
          .clone()
          .or_else(|| read_env_trimmed("AUV_MODEL_PROPOSER_BASE_URL"))
          .unwrap_or_else(|| OPENAI_RESPONSES_API_URL.to_string()),
      });
    }

    let query = self
      .query
      .clone()
      .ok_or_else(|| "--query is required".to_string())?;
    let role = self
      .role
      .clone()
      .ok_or_else(|| "--role is required".to_string())?;
    let action = self
      .action
      .clone()
      .ok_or_else(|| "--action is required when --intent is not set".to_string())?;
    Ok(CandidateActionCommandMode::Direct {
      query,
      role,
      action,
    })
  }
}

#[derive(Clone, Debug, PartialEq)]
enum CandidateActionCommandMode {
  Direct {
    query: String,
    role: String,
    action: CandidateActionKind,
  },
  ModelProposal {
    intent: String,
    proposer_model: String,
    proposer_base_url: String,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateActionCommandStatus {
  PromotionRefused,
  ExecutedSingleAction,
  BlockedNotReady,
}

impl CandidateActionCommandStatus {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::PromotionRefused => "promotion_refused",
      Self::ExecutedSingleAction => "executed_single_action",
      Self::BlockedNotReady => "blocked_not_ready",
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateActionCommandOutput {
  pub status: CandidateActionCommandStatus,
  pub proposal_artifact_id: Option<String>,
  pub promotion_artifact_id: String,
  pub decision_artifact_id: Option<String>,
  pub execution_artifact_id: Option<String>,
  pub promotion_refusals: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct CandidateActionProposalArtifact {
  artifact_version: String,
  proposal_id: String,
  source_recognition_artifact: Option<crate::contract::ArtifactRef>,
  observed_recognition_ids: Vec<String>,
  proposal_input_recognition_id: String,
  intent: String,
  provider: String,
  model: String,
  selected_item_path: String,
  selected_item_id: Option<String>,
  selected_item_kind: Option<String>,
  selected_item_text: Option<String>,
  selected_action: CandidateActionKind,
  proposal_observed_in_latest_frame: bool,
  detail: serde_json::Value,
  known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct PreparedCandidateActionPlan {
  action: CandidateActionKind,
  observations: Vec<crate::contract::RecognitionResult>,
  recognition_artifact_ref: Option<crate::contract::ArtifactRef>,
  proposal_artifact_id: Option<String>,
  approval_target_summary: String,
}

#[derive(Clone, Debug, PartialEq)]
struct ModelSelectionProposal {
  provider: String,
  model: String,
  intent: String,
  selected_item_path: String,
  selected_action: CandidateActionKind,
  reason: String,
  raw_response_text: String,
  raw_response_json: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ModelProposalResponse {
  selected_item_path: String,
  selected_action_kind: String,
  selected_action_text: Option<String>,
  reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct ProposalObservedItem {
  item_id: String,
  path: String,
  kind: String,
  text: Option<String>,
  role: Option<String>,
  focused: Option<bool>,
  bounds: ProposalObservedRect,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct ProposalObservedRect {
  x: i64,
  y: i64,
  width: i64,
  height: i64,
}

trait CandidateActionProposer {
  fn propose(
    &self,
    app_bundle_id: &str,
    observation: &crate::contract::RecognitionResult,
    intent: &str,
  ) -> AuvResult<ModelSelectionProposal>;
}

struct OpenAiResponsesCandidateActionProposer {
  api_key: String,
  model: String,
  endpoint: String,
}

#[cfg(target_os = "macos")]
pub fn execute_candidate_action_command(
  context: &mut RecordedOperationContext<'_>,
  request: &CandidateActionCommandRequest,
) -> AuvResult<CandidateActionCommandOutput> {
  request.validate()?;
  let plan = prepare_candidate_action_plan(context, request)?;
  let human_gesture_approval =
    request_human_gesture_approval(context, request, &plan.approval_target_summary)?;
  if human_gesture_approval.is_some() {
    // NOTICE(candidate-action-human-approval-frontmost-restore):
    // LocalAuthentication can transiently make the approval UI the frontmost
    // surface. Re-activate the original target after approval so the existing
    // execution readiness gate can verify the intended app/window instead of
    // failing on approval UI focus theft. This does not bypass readiness; it
    // restores the precondition that readiness is meant to check.
    activate_app(&request.app_bundle_id)?;
    context.record_event(
      "candidate.action.command.target.reactivated",
      Some(format!(
        "reactivated target app {} after human approval before execution readiness",
        request.app_bundle_id
      )),
    );
  }

  let latest = plan
    .observations
    .last()
    .ok_or_else(|| "candidate action command captured no observations".to_string())?;

  let mut promotion_request = CandidatePromotionArtifactRequest::new(
    request.promotion_id.clone(),
    format!("{}-promotion", request.promotion_id),
  );
  promotion_request.source_recognition_artifact = plan.recognition_artifact_ref.clone();
  promotion_request.stability_policy = StabilityPolicy {
    min_frames: request.stable_frames,
    max_centroid_drift_px: request.max_centroid_drift_px,
    require_stable_text: request.require_stable_text,
  };
  promotion_request.freshness = Some(
    freshness_from_capture_backed_recognition(
      latest,
      "candidate.action.command.capture_ax_tree",
      "freshness derived from same-run AX capture",
    )
    .map_err(|error| error.to_string())?,
  );
  promotion_request.permission =
    promotion_permission_for_request(request, latest, human_gesture_approval.as_ref())?;

  let (promotion_artifact_ref, promotion) =
    record_candidate_promotion_artifact_with_recognition_projection(
      context,
      &plan.observations,
      &promotion_request,
    )?;

  if let CandidatePromotion::Refused { reasons } = &promotion.decision {
    let refusal_labels = promotion_refusal_labels(reasons);
    context.record_event(
      "candidate.action.command.promotion.refused",
      Some(format!(
        "promotion {} refused before decide/execute: {}",
        promotion_artifact_ref.artifact_id,
        refusal_labels.join(", ")
      )),
    );
    return Ok(CandidateActionCommandOutput {
      status: CandidateActionCommandStatus::PromotionRefused,
      proposal_artifact_id: plan.proposal_artifact_id,
      promotion_artifact_id: promotion_artifact_ref.artifact_id.as_str().to_string(),
      decision_artifact_id: None,
      execution_artifact_id: None,
      promotion_refusals: refusal_labels,
    });
  }

  context.record_event(
    "candidate.action.command.promotion.ready",
    Some(format!(
      "promotion {} recorded; building action decision",
      promotion_artifact_ref.artifact_id
    )),
  );

  let decision_request = CandidateActionDecisionRequest::new(
    request.decision_id.clone(),
    format!("{}-decision", request.decision_id),
  )
  .with_action(plan.action.clone())
  .with_source_candidate_promotion_artifact(promotion_artifact_ref.clone());
  let (decision_artifact_ref, decision) =
    record_candidate_action_decision_artifact(context, &promotion, &decision_request)?;

  context.record_event(
    "candidate.action.command.execution.begin",
    Some(format!(
      "decision {} recorded; executing one approved candidate action",
      decision_artifact_ref.artifact_id
    )),
  );

  let execution_request = CandidateActionExecutionRequest::new(
    request.execution_id.clone(),
    format!("{}-execution", request.execution_id),
  )
  .with_source_candidate_action_decision_artifact(decision_artifact_ref.clone())
  .with_action(plan.action.clone())
  .with_post_action_probe(CandidateActionPostActionProbe::focused_ax_node_reobserved());
  let execution_request = match execution_consent_for_request(
    request,
    &promotion,
    &decision,
    &decision_artifact_ref,
    human_gesture_approval.as_ref(),
    &plan.action,
  ) {
    Some(ExecutionConsentForRequest::DevSelfMinted(consent)) => execution_request
      .allow_dev_self_minted_consent()
      .with_consent(consent),
    Some(ExecutionConsentForRequest::HumanGesture(consent)) => {
      execution_request.with_consent(consent)
    }
    None => execution_request,
  };

  let mut executor = MacosCandidateActionExecutor::default();
  let (execution_artifact_ref, execution) = execute_and_record_single_candidate_action(
    context,
    &mut executor,
    &promotion,
    &decision,
    &execution_request,
  )?;

  Ok(CandidateActionCommandOutput {
    status: command_status_for_execution_side_effect(&execution.side_effect),
    proposal_artifact_id: plan.proposal_artifact_id,
    promotion_artifact_id: promotion_artifact_ref.artifact_id.as_str().to_string(),
    decision_artifact_id: Some(decision_artifact_ref.artifact_id.as_str().to_string()),
    execution_artifact_id: Some(execution_artifact_ref.artifact_id.as_str().to_string()),
    promotion_refusals: Vec::new(),
  })
}

fn command_status_for_execution_side_effect(
  side_effect: &CandidateActionExecutionSideEffect,
) -> CandidateActionCommandStatus {
  match side_effect {
    CandidateActionExecutionSideEffect::SingleInputDelivered => {
      CandidateActionCommandStatus::ExecutedSingleAction
    }
    CandidateActionExecutionSideEffect::BlockedNotReady => {
      CandidateActionCommandStatus::BlockedNotReady
    }
  }
}

#[cfg(not(target_os = "macos"))]
pub fn execute_candidate_action_command(
  _context: &mut RecordedOperationContext<'_>,
  _request: &CandidateActionCommandRequest,
) -> AuvResult<CandidateActionCommandOutput> {
  Err("candidate action command is currently implemented only for macOS".to_string())
}

#[cfg(target_os = "macos")]
fn prepare_candidate_action_plan(
  context: &mut RecordedOperationContext<'_>,
  request: &CandidateActionCommandRequest,
) -> AuvResult<PreparedCandidateActionPlan> {
  let mode = request.mode()?;
  let observation_label = match &mode {
    CandidateActionCommandMode::Direct {
      query,
      role,
      action,
    } => format!(
      "capturing {} AX frame(s) for app {} query {:?} role {:?} action {}",
      request.stable_frames,
      request.app_bundle_id,
      query,
      role,
      action.label()
    ),
    CandidateActionCommandMode::ModelProposal { intent, .. } => format!(
      "capturing {} AX frame(s) for app {} for model intent {:?}",
      request.stable_frames, request.app_bundle_id, intent
    ),
  };
  context.record_event(
    "candidate.action.command.observe.begin",
    Some(observation_label),
  );

  let observation = capture_candidate_action_observations(context, request, &mode)?;

  match mode {
    CandidateActionCommandMode::Direct {
      query,
      role,
      action,
    } => {
      let recognition_artifact_ref = observation.recorded_recognition_artifact_ref.clone();
      let observations = observation.filtered(query.as_str(), role.as_str())?;
      Ok(PreparedCandidateActionPlan {
        action,
        observations,
        recognition_artifact_ref,
        proposal_artifact_id: None,
        approval_target_summary: format!("query {:?} role {:?}", query, role),
      })
    }
    CandidateActionCommandMode::ModelProposal {
      intent,
      proposer_model,
      proposer_base_url,
    } => {
      let proposer =
        OpenAiResponsesCandidateActionProposer::from_request(&proposer_model, &proposer_base_url)?;
      let latest = observation
        .wide_observations
        .last()
        .ok_or_else(|| "candidate action command captured no observations".to_string())?;
      let proposal = proposer.propose(&request.app_bundle_id, latest, &intent)?;
      let narrowed =
        narrow_observations_for_model_proposal(&observation.wide_observations, &proposal)?;
      let proposal_artifact_id = Some(record_model_proposal_artifact(
        context,
        request,
        observation.recorded_recognition_artifact_ref.clone(),
        &narrowed,
        &proposal,
      )?);
      let selected_path = proposal.selected_item_path.clone();
      let selected_action = proposal.selected_action.label().to_string();
      Ok(PreparedCandidateActionPlan {
        action: proposal.selected_action,
        observations: narrowed,
        recognition_artifact_ref: observation.recorded_recognition_artifact_ref,
        proposal_artifact_id,
        approval_target_summary: format!(
          "intent {:?} -> path {:?} action {}",
          intent, selected_path, selected_action
        ),
      })
    }
  }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, PartialEq)]
struct CapturedCandidateActionObservations {
  wide_observations: Vec<crate::contract::RecognitionResult>,
  recorded_recognition_artifact_ref: Option<crate::contract::ArtifactRef>,
}

#[cfg(target_os = "macos")]
impl CapturedCandidateActionObservations {
  fn filtered(self, query: &str, role: &str) -> AuvResult<Vec<crate::contract::RecognitionResult>> {
    self
      .wide_observations
      .into_iter()
      .enumerate()
      .map(|(frame_index, recognition)| {
        refilter_recognition_frame(
          recognition,
          AxRecognitionPolicy {
            query: Some(query.to_string()),
            role: Some(role.to_string()),
            require_bounds: true,
            best_selection: AxBestSelectionStrategy::SingleFilteredItem,
          },
          frame_index,
        )
      })
      .collect()
  }
}

#[cfg(target_os = "macos")]
fn capture_candidate_action_observations(
  context: &mut RecordedOperationContext<'_>,
  request: &CandidateActionCommandRequest,
  mode: &CandidateActionCommandMode,
) -> AuvResult<CapturedCandidateActionObservations> {
  let mut wide_observations = Vec::new();
  let mut recognition_artifact_ref = None;

  for frame_index in 0..request.stable_frames {
    activate_app(&request.app_bundle_id)?;
    if let Some(shortcut) = request.reveal_shortcut.as_deref() {
      press_shortcut(shortcut)?;
      if request.reveal_settle_ms > 0 {
        thread::sleep(Duration::from_millis(request.reveal_settle_ms));
      }
    }

    let capture =
      auv_driver_macos::native::ax_tree::capture_ax_tree_snapshot(&request.app_bundle_id, 8, 80)?;
    let report = auv_driver_macos::native::ax_tree::render_ax_tree_report(&capture);
    let ax_report_path = std::env::temp_dir().join(format!(
      "auv-candidate-action-command-ax-{}-{}-{}.txt",
      frame_index,
      now_millis(),
      std::process::id()
    ));
    std::fs::write(&ax_report_path, report).map_err(|error| {
      format!(
        "failed to write temporary AX tree report {}: {error}",
        ax_report_path.display()
      )
    })?;

    let recognition_id = format!("{}-frame-{}", request.promotion_id, frame_index);
    let policy = AxRecognitionPolicy {
      query: None,
      role: None,
      require_bounds: true,
      best_selection: AxBestSelectionStrategy::None,
    };
    let window_number =
      resolve_target_window_number(&request.app_bundle_id, &capture.snapshot.window_title)?;
    let (_, ax_tree_artifact_ref) = context.stage_artifact_file_with_ref(
      "ax-tree",
      &ax_report_path,
      format!("{recognition_id}.txt"),
      Some(format!(
        "Source AX tree artifact for candidate action command frame {frame_index}."
      )),
    )?;
    let mut recognition = map_ax_tree_to_recognition_result(
      &capture.snapshot,
      &AxRecognitionRuntimeContext {
        recognition_id: recognition_id.clone(),
        source_artifact: ax_tree_artifact_ref.clone(),
        window_number,
      },
      &policy,
    )
    .map_err(|error| format!("failed to map AX tree into recognition result: {error}"))?;
    append_known_limit(
      &mut recognition.known_limits,
      "wide AX observation is addressability evidence only; target selection may still be narrowed later",
    );
    if matches!(mode, CandidateActionCommandMode::ModelProposal { .. }) {
      append_known_limit(
        &mut recognition.known_limits,
        "final target may be model-proposed before promotion; proposal remains subject to the same refusal-first gate",
      );
    }
    let recognition_json = serde_json::to_string_pretty(&recognition)
      .map(|mut rendered| {
        rendered.push('\n');
        rendered
      })
      .map_err(|error| format!("failed to encode AX recognition result JSON: {error}"))?;
    let recognition_source_path =
      std::env::temp_dir().join(format!("{}-recognition.json", recognition_id));
    std::fs::write(&recognition_source_path, recognition_json).map_err(|error| {
      format!(
        "failed to write AX recognition temp artifact {}: {error}",
        recognition_source_path.display()
      )
    })?;
    let (_, recorded_recognition_artifact_ref) = context.stage_artifact_file_with_ref(
      "ax-recognition",
      &recognition_source_path,
      format!("{recognition_id}-recognition.json"),
      Some(
        "AX tree-backed RecognitionResult runtime artifact for candidate-action command"
          .to_string(),
      ),
    )?;
    let _ = std::fs::remove_file(&recognition_source_path);
    context.record_event(
      "ax.recognition.artifact_recorded",
      Some(format!(
        "recorded {} from AX tree {}",
        recorded_recognition_artifact_ref.artifact_id, ax_tree_artifact_ref.artifact_id
      )),
    );
    let _ = std::fs::remove_file(&ax_report_path);
    recognition_artifact_ref = Some(recorded_recognition_artifact_ref);
    wide_observations.push(recognition);

    if frame_index + 1 < request.stable_frames && request.stable_frame_delay_ms > 0 {
      thread::sleep(Duration::from_millis(request.stable_frame_delay_ms));
    }
  }

  Ok(CapturedCandidateActionObservations {
    wide_observations,
    recorded_recognition_artifact_ref: recognition_artifact_ref,
  })
}

#[cfg(target_os = "macos")]
fn resolve_target_window_number(app_bundle_id: &str, window_title: &str) -> AuvResult<Option<i64>> {
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver
    .open_local()
    .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
  let mut selector =
    auv_driver::WindowSelector::default().owned_by(auv_driver::App::bundle_id(app_bundle_id));
  selector.main_visible = true;
  if !window_title.trim().is_empty() {
    selector = selector.title_exact(window_title);
  }
  match session.window().resolve(selector) {
    Ok(window) => Ok(window.reference.id.parse::<i64>().ok()),
    Err(_) => Ok(None),
  }
}

#[cfg(target_os = "macos")]
fn refilter_recognition_frame(
  mut recognition: crate::contract::RecognitionResult,
  policy: AxRecognitionPolicy,
  frame_index: usize,
) -> AuvResult<crate::contract::RecognitionResult> {
  let filtered = recognition
    .all
    .iter()
    .filter(|item| recognized_item_matches_policy(item, &policy))
    .cloned()
    .collect::<Vec<_>>();
  let best = match policy.best_selection {
    AxBestSelectionStrategy::None => None,
    AxBestSelectionStrategy::SingleFilteredItem if filtered.len() == 1 => filtered.first().cloned(),
    AxBestSelectionStrategy::SingleFilteredItem => None,
    AxBestSelectionStrategy::HighestScore => filtered
      .iter()
      .max_by(|left, right| {
        left
          .provider_score
          .partial_cmp(&right.provider_score)
          .unwrap_or(std::cmp::Ordering::Equal)
      })
      .cloned(),
  };
  recognition.filtered = filtered;
  recognition.best = best;
  recognition.detail["query"] = serde_json::json!(policy.query);
  recognition.detail["role"] = serde_json::json!(policy.role);
  recognition.detail["best_selection"] = serde_json::json!(policy.best_selection);
  recognition.detail["filtered_node_count"] = serde_json::json!(recognition.filtered.len());
  if recognition.best.is_none() {
    append_known_limit(
      &mut recognition.known_limits,
      format!(
        "frame {frame_index} has no single deterministic AX target after direct filter narrowing"
      ),
    );
  }
  Ok(recognition)
}

fn narrow_observations_for_model_proposal(
  observations: &[crate::contract::RecognitionResult],
  proposal: &ModelSelectionProposal,
) -> AuvResult<Vec<crate::contract::RecognitionResult>> {
  observations
    .iter()
    .enumerate()
    .map(|(frame_index, recognition)| {
      let selected_item = recognition
        .all
        .iter()
        .find(|item| recognized_item_path(item) == Some(proposal.selected_item_path.as_str()))
        .cloned();
      let mut narrowed = recognition.clone();
      narrowed.filtered = selected_item.iter().cloned().collect();
      narrowed.best = selected_item;
      narrowed.detail["selection_provenance"] = serde_json::json!({
        "kind": "model_proposal",
        "provider": proposal.provider,
        "model": proposal.model,
        "selected_item_path": proposal.selected_item_path,
        "selected_action": proposal.selected_action,
      });
      append_known_limit(
        &mut narrowed.known_limits,
        "best target was narrowed from wide AX observation by a model proposal before promotion",
      );
      if narrowed.best.is_none() {
        append_known_limit(
          &mut narrowed.known_limits,
          format!(
            "frame {frame_index} no longer contains the model-proposed AX path; stability may refuse"
          ),
        );
      }
      Ok(narrowed)
    })
    .collect()
}

#[cfg(target_os = "macos")]
fn record_model_proposal_artifact(
  context: &mut RecordedOperationContext<'_>,
  request: &CandidateActionCommandRequest,
  source_recognition_artifact: Option<crate::contract::ArtifactRef>,
  observations: &[crate::contract::RecognitionResult],
  proposal: &ModelSelectionProposal,
) -> AuvResult<String> {
  let latest = observations
    .last()
    .ok_or_else(|| "proposal recording requires at least one narrowed observation".to_string())?;
  let selected = latest.best.as_ref().ok_or_else(|| {
    "proposal-selected target must remain addressable in latest frame".to_string()
  })?;
  let artifact = CandidateActionProposalArtifact {
    artifact_version: CANDIDATE_ACTION_PROPOSAL_ARTIFACT_VERSION.to_string(),
    proposal_id: request.proposal_id.clone(),
    source_recognition_artifact,
    observed_recognition_ids: observations
      .iter()
      .map(|recognition| recognition.recognition_id.clone())
      .collect(),
    proposal_input_recognition_id: latest.recognition_id.clone(),
    intent: proposal.intent.clone(),
    provider: proposal.provider.clone(),
    model: proposal.model.clone(),
    selected_item_path: proposal.selected_item_path.clone(),
    selected_item_id: Some(selected.item_id.clone()),
    selected_item_kind: Some(selected.kind.clone()),
    selected_item_text: selected.text.clone(),
    selected_action: proposal.selected_action.clone(),
    proposal_observed_in_latest_frame: true,
    detail: serde_json::json!({
      "reason": proposal.reason,
      "raw_response_text": proposal.raw_response_text,
      "raw_response_json": proposal.raw_response_json,
      "selected_item_path": proposal.selected_item_path,
      "selected_action": proposal.selected_action,
    }),
    known_limits: vec![
      "model proposal is a fallible producer only; the same promotion/consent/readiness/verify spine still arbitrates execution".to_string(),
      "proposal artifact records why the target was chosen, not why it was allowed".to_string(),
    ],
  };
  let rendered = serde_json::to_string_pretty(&artifact)
    .map(|mut rendered| {
      rendered.push('\n');
      rendered
    })
    .map_err(|error| {
      format!("failed to encode candidate-action proposal artifact JSON: {error}")
    })?;
  let artifact_source_path = std::env::temp_dir().join(format!(
    "auv-candidate-action-proposal-{}-{}-{}.json",
    sanitize_artifact_label(&request.proposal_id),
    now_millis(),
    std::process::id()
  ));
  std::fs::write(&artifact_source_path, rendered).map_err(|error| {
    format!(
      "failed to write candidate-action proposal temp artifact {}: {error}",
      artifact_source_path.display()
    )
  })?;
  let (_, artifact_ref) = context.stage_artifact_file_with_ref(
    CANDIDATE_ACTION_PROPOSAL_ARTIFACT_ROLE,
    &artifact_source_path,
    format!("{}.json", sanitize_artifact_label(&request.proposal_id)),
    Some("Model-produced candidate-action proposal artifact.".to_string()),
  )?;
  let _ = std::fs::remove_file(&artifact_source_path);
  context.record_event(
    "candidate.action.command.proposal.recorded",
    Some(format!(
      "recorded {} for model proposal path {} action {}",
      artifact_ref.artifact_id,
      proposal.selected_item_path,
      proposal.selected_action.label()
    )),
  );
  Ok(artifact_ref.artifact_id.as_str().to_string())
}

impl OpenAiResponsesCandidateActionProposer {
  fn from_request(model: &str, endpoint: &str) -> AuvResult<Self> {
    let api_key = read_env_trimmed("OPENAI_API_KEY")
      .ok_or_else(|| "OPENAI_API_KEY is required for --intent proposer mode".to_string())?;
    Ok(Self {
      api_key,
      model: model.to_string(),
      endpoint: endpoint.to_string(),
    })
  }
}

impl CandidateActionProposer for OpenAiResponsesCandidateActionProposer {
  fn propose(
    &self,
    app_bundle_id: &str,
    observation: &crate::contract::RecognitionResult,
    intent: &str,
  ) -> AuvResult<ModelSelectionProposal> {
    let observed_items = observation
      .all
      .iter()
      .map(observed_item_for_model)
      .collect::<Vec<_>>();
    if observed_items.is_empty() {
      return Err("model proposer requires at least one observed AX item".to_string());
    }
    let request_body = serde_json::json!({
      "model": self.model,
      "input": [
        {
          "role": "system",
          "content": [
            {
              "type": "input_text",
              "text": "You are a proposer only for AUV candidate-action targeting. You must choose exactly one observed AX item path and one action. Never invent a path. Never approve execution. Output strict JSON with keys: selected_item_path, selected_action_kind, selected_action_text, reason. Allowed selected_action_kind values: click, type_text."
            }
          ]
        },
        {
          "role": "user",
          "content": [
            {
              "type": "input_text",
              "text": format!(
                "app_bundle_id={app_bundle_id}\nintent={intent}\nobserved_items={}",
                serde_json::to_string_pretty(&observed_items)
                  .map_err(|error| format!("failed to encode observed items for proposer prompt: {error}"))?
              )
            }
          ]
        }
      ]
    });
    let endpoint = self.endpoint.clone();
    let api_key = self.api_key.clone();
    // NOTICE(candidate-action-proposer-blocking-http):
    // `candidate-action run` currently executes inside the CLI's Tokio runtime,
    // while this proposer path still uses blocking reqwest. Construct and drop
    // the blocking client inside a dedicated thread so proposer mode cannot
    // panic on Tokio runtime shutdown. Revisit when this command path becomes
    // async end-to-end.
    let response_json = thread::spawn(move || -> AuvResult<serde_json::Value> {
      let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(OPENAI_RESPONSES_TIMEOUT_MS))
        .build()
        .map_err(|error| format!("failed to build model proposer HTTP client: {error}"))?;
      let response = client
        .post(&endpoint)
        .bearer_auth(api_key)
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .map_err(|error| format!("candidate-action proposer HTTP request failed: {error}"))?;
      if !response.status().is_success() {
        let status = response.status();
        let body = response
          .text()
          .unwrap_or_else(|_| "<failed to read error body>".to_string());
        return Err(format!(
          "candidate-action proposer HTTP request failed with status {status}: {body}"
        ));
      }
      response.json::<serde_json::Value>().map_err(|error| {
        format!("failed to decode candidate-action proposer JSON response: {error}")
      })
    })
    .join()
    .map_err(|_| "candidate-action proposer HTTP thread panicked".to_string())??;
    let response_text = response_output_text(&response_json).ok_or_else(|| {
      "candidate-action proposer response did not contain output_text".to_string()
    })?;
    let parsed: ModelProposalResponse = serde_json::from_str(&response_text)
      .map_err(|error| format!("failed to parse candidate-action proposer JSON text: {error}"))?;
    let selected_action = parse_model_selected_action(&parsed)?;
    Ok(ModelSelectionProposal {
      provider: "openai.responses".to_string(),
      model: self.model.clone(),
      intent: intent.to_string(),
      selected_item_path: parsed.selected_item_path,
      selected_action,
      reason: parsed.reason,
      raw_response_text: response_text,
      raw_response_json: response_json,
    })
  }
}

#[cfg(target_os = "macos")]
fn self_minted_promotion_permission(
  request: &CandidateActionCommandRequest,
  recognition: &crate::contract::RecognitionResult,
) -> AuvResult<Option<ActionPermission>> {
  if !request.dev_self_minted_consent {
    return Ok(None);
  }

  // NOTICE(candidate-action-command-dev-consent):
  // This command can self-mint consent records only behind an explicit dev flag
  // so local smoke runs can exercise the full path. Product-grade consent must
  // come from an external human approval source before this command is treated
  // as a real in-the-loop action surface.
  Ok(Some(
    explicit_consent_for_candidate_promotion(
      &request.promotion_id,
      recognition,
      CandidatePromotionConsentInput {
        granted_by: request.granted_by.clone(),
        scope_note: request.promotion_scope_note.clone(),
        evidence_note: request.promotion_evidence_note.clone(),
        approved_at_millis: now_millis(),
        provenance: ConsentProvenance::DevSelfMinted,
      },
    )
    .map_err(|error| error.to_string())?,
  ))
}

#[cfg(target_os = "macos")]
fn promotion_permission_for_request(
  request: &CandidateActionCommandRequest,
  recognition: &crate::contract::RecognitionResult,
  human_gesture_approval: Option<&HumanGestureApproval>,
) -> AuvResult<Option<ActionPermission>> {
  if let Some(approval) = human_gesture_approval {
    return Ok(Some(
      explicit_consent_for_candidate_promotion(
        &request.promotion_id,
        recognition,
        CandidatePromotionConsentInput {
          granted_by: approval.granted_by.clone(),
          scope_note: human_gesture_scope_note(
            &request.promotion_scope_note,
            approval,
            "candidate_promotion_only",
          ),
          evidence_note: human_gesture_evidence_note(
            &request.promotion_evidence_note,
            approval,
            request.human_gesture_timeout_ms,
          ),
          approved_at_millis: approval.approved_at_millis,
          provenance: ConsentProvenance::HumanGesture,
        },
      )
      .map_err(|error| error.to_string())?,
    ));
  }
  self_minted_promotion_permission(request, recognition)
}

#[cfg(target_os = "macos")]
fn self_minted_execution_consent(
  request: &CandidateActionCommandRequest,
  promotion: &crate::candidate_promotion_recording::CandidatePromotionArtifact,
  decision: &crate::candidate_action_decision::CandidateActionDecisionArtifact,
  decision_artifact_ref: &crate::contract::ArtifactRef,
  action: &CandidateActionKind,
) -> Option<CandidateActionExecutionConsent> {
  if !request.dev_self_minted_consent {
    return None;
  }

  Some(CandidateActionExecutionConsent {
    consent_id: format!("consent-{}", request.execution_id),
    execution_id: request.execution_id.clone(),
    granted_by: request.granted_by.clone(),
    scope_note: request.execution_scope_note.clone(),
    run_id: decision_artifact_ref.run_id.as_str().to_string(),
    source_promotion_id: promotion.promotion_id.clone(),
    source_decision_id: decision.decision_id.clone(),
    candidate_local_id: decision.candidate_local_id.clone(),
    approved_action: CandidateActionExecutionConsentAction::from_action(action),
    provenance: ConsentProvenance::DevSelfMinted,
    grade: ConsentGrade::DevOnly,
    approved_at_millis: now_millis(),
    evidence_note: request.execution_evidence_note.clone(),
  })
}

#[cfg(target_os = "macos")]
enum ExecutionConsentForRequest {
  DevSelfMinted(CandidateActionExecutionConsent),
  HumanGesture(CandidateActionExecutionConsent),
}

#[cfg(target_os = "macos")]
fn execution_consent_for_request(
  request: &CandidateActionCommandRequest,
  promotion: &crate::candidate_promotion_recording::CandidatePromotionArtifact,
  decision: &crate::candidate_action_decision::CandidateActionDecisionArtifact,
  decision_artifact_ref: &crate::contract::ArtifactRef,
  human_gesture_approval: Option<&HumanGestureApproval>,
  action: &CandidateActionKind,
) -> Option<ExecutionConsentForRequest> {
  if let Some(approval) = human_gesture_approval {
    return Some(ExecutionConsentForRequest::HumanGesture(
      human_gesture_execution_consent(
        request,
        promotion,
        decision,
        decision_artifact_ref,
        approval,
        action,
      ),
    ));
  }
  self_minted_execution_consent(request, promotion, decision, decision_artifact_ref, action)
    .map(ExecutionConsentForRequest::DevSelfMinted)
}

#[cfg(target_os = "macos")]
fn human_gesture_execution_consent(
  request: &CandidateActionCommandRequest,
  promotion: &crate::candidate_promotion_recording::CandidatePromotionArtifact,
  decision: &crate::candidate_action_decision::CandidateActionDecisionArtifact,
  decision_artifact_ref: &crate::contract::ArtifactRef,
  approval: &HumanGestureApproval,
  action: &CandidateActionKind,
) -> CandidateActionExecutionConsent {
  CandidateActionExecutionConsent {
    consent_id: format!("consent-{}", request.execution_id),
    execution_id: request.execution_id.clone(),
    granted_by: approval.granted_by.clone(),
    scope_note: human_gesture_scope_note(
      &request.execution_scope_note,
      approval,
      "execute_single_candidate_action",
    ),
    run_id: decision_artifact_ref.run_id.as_str().to_string(),
    source_promotion_id: promotion.promotion_id.clone(),
    source_decision_id: decision.decision_id.clone(),
    candidate_local_id: decision.candidate_local_id.clone(),
    approved_action: CandidateActionExecutionConsentAction::from_action(action),
    provenance: ConsentProvenance::HumanGesture,
    grade: ConsentGrade::HumanApproved,
    approved_at_millis: approval.approved_at_millis,
    evidence_note: human_gesture_evidence_note(
      &request.execution_evidence_note,
      approval,
      request.human_gesture_timeout_ms,
    ),
  }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct HumanGestureApproval {
  granted_by: String,
  mechanism: String,
  approved_at_millis: u64,
}

#[cfg(target_os = "macos")]
fn request_human_gesture_approval(
  context: &mut RecordedOperationContext<'_>,
  request: &CandidateActionCommandRequest,
  approval_target_summary: &str,
) -> AuvResult<Option<HumanGestureApproval>> {
  if !request.human_gesture_consent {
    return Ok(None);
  }

  context.record_event(
    "candidate.action.command.consent.requested",
    Some(format!(
      "requesting human gesture approval for app {} target {} timeout_ms={}",
      request.app_bundle_id, approval_target_summary, request.human_gesture_timeout_ms
    )),
  );

  let response = auv_driver_macos::native::auth::request_human_approval(
    human_gesture_prompt_reason(request, approval_target_summary),
    request.human_gesture_timeout_ms,
  )?;

  match response.status {
    auv_driver_macos::native::auth::NativeHumanApprovalStatus::Approved => {
      let mechanism = response.mechanism.trim().to_string();
      let granted_by = human_gesture_granted_by(request, &mechanism);
      let approved_at_millis = response.approved_at_unix_ms.unwrap_or_else(now_millis);
      context.record_event(
        "candidate.action.command.consent.approved",
        Some(format!(
          "human gesture approval granted via {} by {} at {}",
          mechanism, granted_by, approved_at_millis
        )),
      );
      Ok(Some(HumanGestureApproval {
        granted_by,
        mechanism,
        approved_at_millis,
      }))
    }
    status => {
      context.record_event(
        "candidate.action.command.consent.not_approved",
        Some(format!(
          "human gesture approval ended with status={} mechanism={} message={} recovery={}",
          status.as_str(),
          response.mechanism,
          response.error_message.as_deref().unwrap_or(""),
          response.recovery_hint.as_deref().unwrap_or(""),
        )),
      );
      Ok(None)
    }
  }
}

#[cfg(target_os = "macos")]
fn human_gesture_granted_by(request: &CandidateActionCommandRequest, mechanism: &str) -> String {
  if request.granted_by.trim().is_empty() {
    format!("human-gesture:{mechanism}")
  } else {
    request.granted_by.clone()
  }
}

#[cfg(target_os = "macos")]
fn human_gesture_prompt_reason(
  request: &CandidateActionCommandRequest,
  approval_target_summary: &str,
) -> String {
  format!(
    "Approve one AUV candidate action for app {} targeting {}. This approval is limited to promotion {} and execution {}.",
    request.app_bundle_id, approval_target_summary, request.promotion_id, request.execution_id
  )
}

#[cfg(target_os = "macos")]
fn human_gesture_scope_note(
  base_note: &str,
  approval: &HumanGestureApproval,
  scope_binding: &str,
) -> String {
  format!(
    "{base_note}; consent_grade=human_approved; provenance=human_gesture; mechanism={}; binding={scope_binding}",
    approval.mechanism
  )
}

#[cfg(target_os = "macos")]
fn human_gesture_evidence_note(
  base_note: &str,
  approval: &HumanGestureApproval,
  timeout_ms: u64,
) -> String {
  format!(
    "{base_note}; human approval minted via {}; approved_at_millis={}; timeout_ms={timeout_ms}",
    approval.mechanism, approval.approved_at_millis
  )
}

fn recognized_item_matches_policy(
  item: &crate::contract::RecognizedItem,
  policy: &AxRecognitionPolicy,
) -> bool {
  if policy.require_bounds && (item.box_.width <= 0 || item.box_.height <= 0) {
    return false;
  }
  if let Some(role) = policy.role.as_deref()
    && recognized_item_role(item) != Some(role)
  {
    return false;
  }
  if let Some(query) = policy.query.as_deref() {
    let query = normalize_for_matching(query);
    if query.is_empty() {
      return true;
    }
    let searchable = normalize_for_matching(&recognized_item_search_text(item));
    if !searchable.contains(&query) {
      return false;
    }
  }
  true
}

fn recognized_item_search_text(item: &crate::contract::RecognizedItem) -> String {
  [
    item.text.as_deref().unwrap_or(""),
    item
      .detail
      .get("title")
      .and_then(serde_json::Value::as_str)
      .unwrap_or(""),
    item
      .detail
      .get("description")
      .and_then(serde_json::Value::as_str)
      .unwrap_or(""),
    item
      .detail
      .get("identifier")
      .and_then(serde_json::Value::as_str)
      .unwrap_or(""),
    item
      .detail
      .get("placeholder")
      .and_then(serde_json::Value::as_str)
      .unwrap_or(""),
    item
      .detail
      .get("value")
      .and_then(serde_json::Value::as_str)
      .unwrap_or(""),
  ]
  .join(" ")
}

fn recognized_item_role(item: &crate::contract::RecognizedItem) -> Option<&str> {
  item.detail.get("role").and_then(serde_json::Value::as_str)
}

fn recognized_item_path(item: &crate::contract::RecognizedItem) -> Option<&str> {
  item.detail.get("path").and_then(serde_json::Value::as_str)
}

fn observed_item_for_model(item: &crate::contract::RecognizedItem) -> ProposalObservedItem {
  ProposalObservedItem {
    item_id: item.item_id.clone(),
    path: recognized_item_path(item).unwrap_or("").to_string(),
    kind: item.kind.clone(),
    text: item.text.clone(),
    role: recognized_item_role(item).map(str::to_string),
    focused: item
      .detail
      .get("focused")
      .and_then(serde_json::Value::as_bool),
    bounds: ProposalObservedRect {
      x: item.box_.x,
      y: item.box_.y,
      width: item.box_.width,
      height: item.box_.height,
    },
  }
}

fn parse_model_selected_action(parsed: &ModelProposalResponse) -> AuvResult<CandidateActionKind> {
  match parsed.selected_action_kind.as_str() {
    "click" => Ok(CandidateActionKind::Click),
    "type_text" | "type-text" => {
      let text = parsed
        .selected_action_text
        .clone()
        .ok_or_else(|| "type_text proposal requires selected_action_text".to_string())?;
      if text.trim().is_empty() {
        return Err("type_text proposal requires non-empty selected_action_text".to_string());
      }
      Ok(CandidateActionKind::TypeText { text })
    }
    other => Err(format!(
      "invalid proposer selected_action_kind {other:?}; expected click or type_text"
    )),
  }
}

fn response_output_text(value: &serde_json::Value) -> Option<String> {
  value
    .get("output_text")
    .and_then(serde_json::Value::as_str)
    .map(str::to_string)
    .or_else(|| {
      value
        .get("output")
        .and_then(serde_json::Value::as_array)
        .and_then(|output| {
          output.iter().find_map(|item| {
            item
              .get("content")
              .and_then(serde_json::Value::as_array)
              .and_then(|content| {
                content.iter().find_map(|entry| {
                  entry
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                })
              })
          })
        })
    })
}

fn append_known_limit(known_limits: &mut Vec<String>, value: impl Into<String>) {
  let value = value.into();
  if !known_limits.iter().any(|existing| existing == &value) {
    known_limits.push(value);
  }
}

fn read_env_trimmed(key: &str) -> Option<String> {
  std::env::var(key)
    .ok()
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

fn normalize_for_matching(value: &str) -> String {
  value
    .chars()
    .filter(|character| !character.is_whitespace())
    .collect::<String>()
    .to_lowercase()
}

#[cfg(target_os = "macos")]
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

fn promotion_refusal_labels(reasons: &[PromotionRefusal]) -> Vec<String> {
  reasons.iter().map(promotion_refusal_label).collect()
}

fn promotion_refusal_label(reason: &PromotionRefusal) -> String {
  match reason {
    PromotionRefusal::EmptyRecognition => "empty_recognition".to_string(),
    PromotionRefusal::NoUnambiguousTarget => "no_unambiguous_target".to_string(),
    PromotionRefusal::NoRuntimeEvidence => "no_runtime_evidence".to_string(),
    PromotionRefusal::MissingCaptureArtifact => "missing_capture_artifact".to_string(),
    PromotionRefusal::ProjectionUnavailable { .. } => "projection_unavailable".to_string(),
    PromotionRefusal::StabilityUnproven { .. } => "stability_unproven".to_string(),
    PromotionRefusal::FreshnessUnknown => "freshness_unknown".to_string(),
    PromotionRefusal::PermissionMissing => "permission_missing".to_string(),
  }
}

#[cfg(target_os = "macos")]
fn activate_app(app_bundle_id: &str) -> AuvResult<()> {
  use auv_driver::Driver;
  use std::process::Command;

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver
    .open_local()
    .map_err(|error| format!("failed to open macOS driver session: {error}"))?;
  let windows = session
    .window()
    .list()
    .map_err(|error| format!("failed to list windows before activation: {error}"))?;
  let target = windows
    .into_iter()
    .find(|window| window.app_bundle_id.as_deref() == Some(app_bundle_id))
    .ok_or_else(|| format!("failed to find a visible window for app {app_bundle_id}"))?;
  let _lease = session
    .window()
    .prepare_for_input(
      &target,
      auv_driver::input::PrepareForInputOptions {
        activation: auv_driver::input::ActivationPolicy::Foreground {
          settle: Duration::from_millis(250),
        },
        preserve_frontmost: false,
        install_focus_guard: false,
        settle: Duration::ZERO,
      },
    )
    .map_err(|error| format!("failed to activate target app {app_bundle_id}: {error}"))?;
  // NOTICE(candidate-action-foreground-activation):
  // The typed prepare path can focus an input-capable window without making
  // macOS report the app as frontmost quickly enough for the immediate L8b
  // readiness gate. Keep the typed prepare call for target/window validation,
  // then use the same platform foreground activation primitive used by the
  // macOS driver command before re-checking readiness. Remove this once typed
  // window input exposes a verified foreground transition result.
  let output = Command::new("/usr/bin/osascript")
    .arg("-e")
    .arg(format!(
      "tell application id {} to activate",
      applescript_string_literal(app_bundle_id)
    ))
    .output()
    .map_err(|error| format!("failed to run osascript activation: {error}"))?;
  if !output.status.success() {
    return Err(format!(
      "osascript activation failed with status {}: {}",
      output.status,
      String::from_utf8_lossy(&output.stderr).trim()
    ));
  }
  wait_for_frontmost_app(app_bundle_id, Duration::from_millis(1_500))?;
  Ok(())
}

#[cfg(target_os = "macos")]
fn wait_for_frontmost_app(app_bundle_id: &str, timeout: Duration) -> AuvResult<()> {
  let deadline = std::time::Instant::now() + timeout;
  let mut last_frontmost = current_frontmost_bundle_id()?;
  loop {
    if last_frontmost.as_deref() == Some(app_bundle_id) {
      return Ok(());
    }
    if std::time::Instant::now() >= deadline {
      return Err(format!(
        "target app {app_bundle_id} did not become frontmost after activation; last_frontmost={}",
        last_frontmost.as_deref().unwrap_or("unknown")
      ));
    }
    thread::sleep(Duration::from_millis(100));
    last_frontmost = current_frontmost_bundle_id()?;
  }
}

#[cfg(target_os = "macos")]
fn current_frontmost_bundle_id() -> AuvResult<Option<String>> {
  use std::process::Command;

  let output = Command::new("/usr/bin/osascript")
    .arg("-e")
    .arg(
      "tell application \"System Events\" to get bundle identifier of first application process whose frontmost is true",
    )
    .output()
    .map_err(|error| format!("failed to query frontmost app bundle id: {error}"))?;
  if !output.status.success() {
    return Ok(None);
  }
  let frontmost = String::from_utf8_lossy(&output.stdout).trim().to_string();
  if frontmost.is_empty() {
    Ok(None)
  } else {
    Ok(Some(frontmost))
  }
}

#[cfg(target_os = "macos")]
fn applescript_string_literal(value: &str) -> String {
  format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
  use std::io::{Read, Write};
  use std::net::TcpListener;
  use std::thread;

  use super::{
    CandidateActionCommandRequest, CandidateActionCommandStatus, CandidateActionProposer,
    ModelProposalResponse, ModelSelectionProposal, OpenAiResponsesCandidateActionProposer,
    command_status_for_execution_side_effect, narrow_observations_for_model_proposal,
    normalize_for_matching, parse_model_selected_action, recognized_item_matches_policy,
    response_output_text,
  };
  use crate::ax_recognition::{AxBestSelectionStrategy, AxRecognitionPolicy};
  use crate::candidate_action_decision::{CandidateActionExecutionSideEffect, CandidateActionKind};
  use crate::contract::{
    RecognitionBox, RecognitionResult, RecognitionScope, RecognitionSource, RecognitionSurface,
    RecognizedItem,
  };
  use serde_json::json;

  fn base_request() -> CandidateActionCommandRequest {
    CandidateActionCommandRequest {
      app_bundle_id: "com.apple.TextEdit".to_string(),
      query: Some("Body".to_string()),
      role: Some("AXTextArea".to_string()),
      action: Some(CandidateActionKind::Click),
      intent: None,
      proposer_model: None,
      proposer_base_url: None,
      reveal_shortcut: None,
      reveal_settle_ms: 250,
      stable_frames: 3,
      stable_frame_delay_ms: 150,
      max_centroid_drift_px: 4.0,
      require_stable_text: true,
      dev_self_minted_consent: false,
      human_gesture_consent: false,
      human_gesture_timeout_ms: 15_000,
      proposal_id: "candidate_proposal".to_string(),
      promotion_id: "candidate_promotion".to_string(),
      decision_id: "candidate_decision".to_string(),
      execution_id: "candidate_execution".to_string(),
      granted_by: String::new(),
      promotion_scope_note: "candidate promotion only".to_string(),
      promotion_evidence_note: "explicit candidate promotion consent".to_string(),
      execution_scope_note: "execute exactly one approved candidate action".to_string(),
      execution_evidence_note: "explicit single-action execution consent".to_string(),
    }
  }

  #[test]
  fn validation_allows_missing_granted_by_without_dev_self_minted_consent() {
    let request = base_request();
    assert_eq!(request.validate(), Ok(()));
  }

  #[test]
  fn validation_requires_granted_by_when_dev_self_minted_consent_is_enabled() {
    let mut request = base_request();
    request.dev_self_minted_consent = true;
    assert_eq!(
      request.validate(),
      Err("--granted-by is required when --dev-self-minted-consent is set".to_string())
    );
  }

  #[test]
  fn validation_rejects_combined_dev_and_human_consent() {
    let mut request = base_request();
    request.dev_self_minted_consent = true;
    request.human_gesture_consent = true;
    request.granted_by = "dev".to_string();
    assert_eq!(
      request.validate(),
      Err("--dev-self-minted-consent cannot be combined with --human-gesture-consent".to_string())
    );
  }

  #[test]
  fn validation_rejects_zero_human_gesture_timeout() {
    let mut request = base_request();
    request.human_gesture_timeout_ms = 0;
    assert_eq!(
      request.validate(),
      Err("--human-gesture-timeout-ms must be greater than 0".to_string())
    );
  }

  #[test]
  fn validation_requires_text_for_type_text_action() {
    let mut request = base_request();
    request.action = Some(CandidateActionKind::TypeText {
      text: String::new(),
    });

    assert_eq!(
      request.validate(),
      Err("--text must not be empty when --action type-text".to_string())
    );
  }

  #[test]
  fn validation_requires_intent_model_in_proposer_mode() {
    let mut request = base_request();
    request.intent = Some("type hello".to_string());
    request.query = None;
    request.role = None;
    request.action = None;
    assert_eq!(
      request.validate(),
      Err(
        "--proposer-model or AUV_MODEL_PROPOSER_MODEL is required when --intent is set".to_string()
      )
    );
  }

  #[test]
  fn validation_rejects_intent_when_direct_target_flags_are_present() {
    let mut request = base_request();
    request.intent = Some("type hello".to_string());
    request.proposer_model = Some("gpt-5.5".to_string());
    assert_eq!(
      request.validate(),
      Err("--intent cannot be combined with --query, --role, --action, or --text".to_string())
    );
  }

  #[test]
  fn command_status_strings_are_stable() {
    assert_eq!(
      CandidateActionCommandStatus::PromotionRefused.as_str(),
      "promotion_refused"
    );
    assert_eq!(
      CandidateActionCommandStatus::ExecutedSingleAction.as_str(),
      "executed_single_action"
    );
    assert_eq!(
      CandidateActionCommandStatus::BlockedNotReady.as_str(),
      "blocked_not_ready"
    );
  }

  #[test]
  fn command_status_tracks_execution_side_effect() {
    assert_eq!(
      command_status_for_execution_side_effect(
        &CandidateActionExecutionSideEffect::SingleInputDelivered
      ),
      CandidateActionCommandStatus::ExecutedSingleAction
    );
    assert_eq!(
      command_status_for_execution_side_effect(
        &CandidateActionExecutionSideEffect::BlockedNotReady
      ),
      CandidateActionCommandStatus::BlockedNotReady
    );
  }

  #[test]
  fn recognized_item_matching_uses_role_and_normalized_text() {
    let item = RecognizedItem {
      item_id: "ax:/window/textarea".to_string(),
      kind: "AXTextArea".to_string(),
      box_: RecognitionBox {
        x: 1,
        y: 2,
        width: 30,
        height: 20,
      },
      text: Some("First Text View".to_string()),
      provider_score: Some(100.0),
      detail: json!({
        "path": "/window/textarea",
        "role": "AXTextArea",
        "focused": true
      }),
    };
    let policy = AxRecognitionPolicy {
      query: Some("firsttextview".to_string()),
      role: Some("AXTextArea".to_string()),
      require_bounds: true,
      best_selection: AxBestSelectionStrategy::SingleFilteredItem,
    };
    assert!(recognized_item_matches_policy(&item, &policy));
    assert_eq!(normalize_for_matching(" First Text View "), "firsttextview");
  }

  #[test]
  fn parse_model_selected_action_supports_click_and_type_text() {
    assert_eq!(
      parse_model_selected_action(&ModelProposalResponse {
        selected_item_path: "/window/button".to_string(),
        selected_action_kind: "click".to_string(),
        selected_action_text: None,
        reason: "button".to_string(),
      })
      .expect("click action should parse"),
      CandidateActionKind::Click
    );
    assert_eq!(
      parse_model_selected_action(&ModelProposalResponse {
        selected_item_path: "/window/textarea".to_string(),
        selected_action_kind: "type_text".to_string(),
        selected_action_text: Some("hello".to_string()),
        reason: "textarea".to_string(),
      })
      .expect("type_text action should parse"),
      CandidateActionKind::TypeText {
        text: "hello".to_string()
      }
    );
  }

  #[test]
  fn response_output_text_reads_top_level_and_nested_shapes() {
    assert_eq!(
      response_output_text(
        &json!({"output_text": "{\"selected_item_path\":\"/p\",\"selected_action_kind\":\"click\",\"reason\":\"ok\"}"})
      ),
      Some(
        "{\"selected_item_path\":\"/p\",\"selected_action_kind\":\"click\",\"reason\":\"ok\"}"
          .to_string()
      )
    );
    assert_eq!(
      response_output_text(&json!({
        "output": [
          {
            "content": [
              {
                "text": "{\"selected_item_path\":\"/p\",\"selected_action_kind\":\"click\",\"reason\":\"ok\"}"
              }
            ]
          }
        ]
      })),
      Some(
        "{\"selected_item_path\":\"/p\",\"selected_action_kind\":\"click\",\"reason\":\"ok\"}"
          .to_string()
      )
    );
  }

  #[test]
  fn narrow_observations_for_model_proposal_selects_same_ax_path_across_frames() {
    let observations = vec![
      sample_wide_recognition("frame-0", "/window/textarea", "Draft 0"),
      sample_wide_recognition("frame-1", "/window/textarea", "Draft 1"),
    ];
    let proposal = ModelSelectionProposal {
      provider: "openai.responses".to_string(),
      model: "gpt-5.5".to_string(),
      intent: "type hello".to_string(),
      selected_item_path: "/window/textarea".to_string(),
      selected_action: CandidateActionKind::TypeText {
        text: "hello".to_string(),
      },
      reason: "main text area".to_string(),
      raw_response_text: "{}".to_string(),
      raw_response_json: json!({}),
    };

    let narrowed = narrow_observations_for_model_proposal(&observations, &proposal)
      .expect("narrowing should work");

    assert_eq!(narrowed.len(), 2);
    assert_eq!(
      narrowed[0]
        .best
        .as_ref()
        .and_then(|item| item.text.as_deref()),
      Some("Draft 0")
    );
    assert_eq!(
      narrowed[1]
        .best
        .as_ref()
        .and_then(|item| item.text.as_deref()),
      Some("Draft 1")
    );
    assert_eq!(narrowed[0].filtered.len(), 1);
    assert_eq!(
      narrowed[0].detail["selection_provenance"]["kind"],
      json!("model_proposal")
    );
  }

  #[test]
  fn openai_responses_proposer_parses_local_fixture_response() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
    let address = listener.local_addr().expect("listener addr");
    let server = thread::spawn(move || {
      let (mut stream, _) = listener.accept().expect("accept request");
      let mut buffer = [0_u8; 8192];
      let _ = stream.read(&mut buffer).expect("read request");
      let body = json!({
        "output_text": "{\"selected_item_path\":\"/window/textarea\",\"selected_action_kind\":\"type_text\",\"selected_action_text\":\"hello from proposer\",\"reason\":\"the textarea matches the user's intent\"}"
      })
      .to_string();
      write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
      )
      .expect("write response");
    });

    let proposer = OpenAiResponsesCandidateActionProposer {
      api_key: "test-key".to_string(),
      model: "gpt-5.5".to_string(),
      endpoint: format!("http://{address}/v1/responses"),
    };
    let proposal = proposer
      .propose(
        "com.apple.TextEdit",
        &sample_wide_recognition("frame-1", "/window/textarea", "Draft 1"),
        "type hello into the main text area",
      )
      .expect("proposal should parse");
    server.join().expect("server thread should finish");

    assert_eq!(proposal.provider, "openai.responses");
    assert_eq!(proposal.model, "gpt-5.5");
    assert_eq!(proposal.selected_item_path, "/window/textarea");
    assert_eq!(
      proposal.selected_action,
      CandidateActionKind::TypeText {
        text: "hello from proposer".to_string()
      }
    );
  }

  #[tokio::test(flavor = "current_thread")]
  async fn openai_responses_proposer_does_not_panic_inside_tokio_runtime() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
    let address = listener.local_addr().expect("listener addr");
    let server = thread::spawn(move || {
      let (mut stream, _) = listener.accept().expect("accept request");
      let mut buffer = [0_u8; 8192];
      let _ = stream.read(&mut buffer).expect("read request");
      let body = json!({
        "output_text": "{\"selected_item_path\":\"/window/textarea\",\"selected_action_kind\":\"click\",\"reason\":\"the textarea is the main editable target\"}"
      })
      .to_string();
      write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
      )
      .expect("write response");
    });

    let proposer = OpenAiResponsesCandidateActionProposer {
      api_key: "test-key".to_string(),
      model: "gpt-5.5".to_string(),
      endpoint: format!("http://{address}/v1/responses"),
    };
    let proposal = proposer
      .propose(
        "com.apple.TextEdit",
        &sample_wide_recognition("frame-1", "/window/textarea", "Draft 1"),
        "focus the main text area",
      )
      .expect("proposal should succeed inside tokio runtime");
    server.join().expect("server thread should finish");

    assert_eq!(proposal.provider, "openai.responses");
    assert_eq!(proposal.selected_item_path, "/window/textarea");
    assert_eq!(proposal.selected_action, CandidateActionKind::Click);
  }

  fn sample_wide_recognition(
    recognition_id: &str,
    matching_path: &str,
    matching_text: &str,
  ) -> RecognitionResult {
    let matching = RecognizedItem {
      item_id: format!("ax:{matching_path}:0"),
      kind: "AXTextArea".to_string(),
      box_: RecognitionBox {
        x: 10,
        y: 20,
        width: 300,
        height: 120,
      },
      text: Some(matching_text.to_string()),
      provider_score: Some(999.0),
      detail: json!({
        "path": matching_path,
        "role": "AXTextArea",
        "focused": true,
      }),
    };
    let decoy = RecognizedItem {
      item_id: "ax:/window/button:1".to_string(),
      kind: "AXButton".to_string(),
      box_: RecognitionBox {
        x: 400,
        y: 20,
        width: 80,
        height: 30,
      },
      text: Some("Save".to_string()),
      provider_score: Some(500.0),
      detail: json!({
        "path": "/window/button",
        "role": "AXButton",
        "focused": false,
      }),
    };
    RecognitionResult {
      recognition_id: recognition_id.to_string(),
      source: RecognitionSource::Custom,
      scope: RecognitionScope {
        surface: RecognitionSurface::Window,
        display_ref: None,
        native_display_id: None,
        app_bundle_id: Some("com.apple.TextEdit".to_string()),
        window_title: Some("Untitled".to_string()),
        window_number: Some(7),
        region_hint: None,
        capture_artifact: None,
        capture_contract_artifact: None,
      },
      best: None,
      filtered: Vec::new(),
      all: vec![matching, decoy],
      detail: json!({
        "provider": "macos.ax_tree",
        "projection_candidate": "identity_window_addressable",
      }),
      evidence: Vec::new(),
      known_limits: Vec::new(),
    }
  }
}

#[cfg(target_os = "macos")]
fn press_shortcut(shortcut: &str) -> AuvResult<()> {
  use auv_driver::Driver;

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver
    .open_local()
    .map_err(|error| format!("failed to open macOS driver session: {error}"))?;
  let _ = session
    .input()
    .press_key(auv_driver::input::KeyPressOptions {
      key: shortcut.to_string(),
      settle: Duration::ZERO,
    })
    .map_err(|error| format!("failed to press reveal shortcut {shortcut}: {error}"))?;
  Ok(())
}
