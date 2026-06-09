use std::thread;
use std::time::Duration;

use crate::ax_recognition::{
  AxBestSelectionStrategy, AxRecognitionPolicy, AxRecognitionRuntimeContext,
  map_ax_tree_to_recognition_result,
};
use crate::candidate_action_decision::{
  CandidateActionDecisionRequest, CandidateActionExecutionConsent,
  CandidateActionExecutionConsentAction, CandidateActionExecutionRequest,
  CandidateActionExecutionSideEffect, CandidateActionKind, CandidateActionPostActionProbe,
  MacosCandidateActionExecutor, execute_and_record_single_candidate_action,
  record_candidate_action_decision_artifact,
};
use crate::candidate_promotion::ConsentProvenance;
use crate::candidate_promotion::{
  ActionPermission, CandidatePromotion, ConsentGrade, PromotionRefusal,
};
use crate::candidate_promotion_recording::{
  CandidatePromotionArtifactRequest, CandidatePromotionConsentInput,
  explicit_consent_for_candidate_promotion, freshness_from_capture_backed_recognition,
  record_candidate_promotion_artifact_with_recognition_projection,
};
use crate::model::{AuvResult, now_millis};
use crate::recorded_operation::RecordedOperationContext;
use crate::stability::StabilityPolicy;
use auv_driver::Driver;

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateActionCommandRequest {
  pub app_bundle_id: String,
  pub query: String,
  pub role: String,
  pub action: CandidateActionKind,
  pub reveal_shortcut: Option<String>,
  pub reveal_settle_ms: u64,
  pub stable_frames: u32,
  pub stable_frame_delay_ms: u64,
  pub max_centroid_drift_px: f64,
  pub require_stable_text: bool,
  pub dev_self_minted_consent: bool,
  pub human_gesture_consent: bool,
  pub human_gesture_timeout_ms: u64,
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
    if self.query.trim().is_empty() {
      return Err("--query is required".to_string());
    }
    if self.role.trim().is_empty() {
      return Err("--role is required".to_string());
    }
    if let CandidateActionKind::TypeText { text } = &self.action
      && text.trim().is_empty()
    {
      return Err("--text must not be empty when --action type-text".to_string());
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
    Ok(())
  }
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
  pub promotion_artifact_id: String,
  pub decision_artifact_id: Option<String>,
  pub execution_artifact_id: Option<String>,
  pub promotion_refusals: Vec<String>,
}

#[cfg(target_os = "macos")]
pub fn execute_candidate_action_command(
  context: &mut RecordedOperationContext<'_>,
  request: &CandidateActionCommandRequest,
) -> AuvResult<CandidateActionCommandOutput> {
  request.validate()?;

  context.record_event(
    "candidate.action.command.observe.begin",
    Some(format!(
      "capturing {} AX frame(s) for app {} query {:?} role {:?}",
      request.stable_frames, request.app_bundle_id, request.query, request.role
    )),
  );

  let mut observations = Vec::new();
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
      query: Some(request.query.clone()),
      role: Some(request.role.clone()),
      require_bounds: true,
      best_selection: AxBestSelectionStrategy::SingleFilteredItem,
    };
    let window_number =
      resolve_target_window_number(&request.app_bundle_id, &capture.snapshot.window_title)?;
    let (_, ax_tree_artifact_ref) = context.stage_artifact_file_with_ref(
      "ax-tree",
      &ax_report_path,
      format!("{recognition_id}.txt"),
      Some(format!(
        "Source AX tree artifact for consent-gated command frame {frame_index}."
      )),
    )?;
    let recognition = map_ax_tree_to_recognition_result(
      &capture.snapshot,
      &AxRecognitionRuntimeContext {
        recognition_id: recognition_id.clone(),
        source_artifact: ax_tree_artifact_ref.clone(),
        window_number,
      },
      &policy,
    )
    .map_err(|error| format!("failed to map AX tree into recognition result: {error}"))?;
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
        "AX tree-backed RecognitionResult runtime artifact for consent-gated command".to_string(),
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
    observations.push(recognition);

    if frame_index + 1 < request.stable_frames && request.stable_frame_delay_ms > 0 {
      thread::sleep(Duration::from_millis(request.stable_frame_delay_ms));
    }
  }

  let latest = observations
    .last()
    .ok_or_else(|| "candidate action command captured no observations".to_string())?;
  let human_gesture_approval = request_human_gesture_approval(context, request)?;

  let mut promotion_request = CandidatePromotionArtifactRequest::new(
    request.promotion_id.clone(),
    format!("{}-promotion", request.promotion_id),
  );
  promotion_request.source_recognition_artifact = recognition_artifact_ref;
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
      &observations,
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
  .with_action(request.action.clone())
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
  .with_action(request.action.clone())
  .with_post_action_probe(CandidateActionPostActionProbe::focused_ax_node_reobserved());
  let execution_request = match execution_consent_for_request(
    request,
    &promotion,
    &decision,
    &decision_artifact_ref,
    human_gesture_approval.as_ref(),
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
    approved_action: CandidateActionExecutionConsentAction::from_action(&request.action),
    provenance: ConsentProvenance::DevSelfMinted,
    grade: crate::candidate_promotion::ConsentGrade::DevOnly,
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
) -> Option<ExecutionConsentForRequest> {
  if let Some(approval) = human_gesture_approval {
    return Some(ExecutionConsentForRequest::HumanGesture(
      human_gesture_execution_consent(
        request,
        promotion,
        decision,
        decision_artifact_ref,
        approval,
      ),
    ));
  }
  self_minted_execution_consent(request, promotion, decision, decision_artifact_ref)
    .map(ExecutionConsentForRequest::DevSelfMinted)
}

#[cfg(target_os = "macos")]
fn human_gesture_execution_consent(
  request: &CandidateActionCommandRequest,
  promotion: &crate::candidate_promotion_recording::CandidatePromotionArtifact,
  decision: &crate::candidate_action_decision::CandidateActionDecisionArtifact,
  decision_artifact_ref: &crate::contract::ArtifactRef,
  approval: &HumanGestureApproval,
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
    approved_action: CandidateActionExecutionConsentAction::from_action(&request.action),
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
) -> AuvResult<Option<HumanGestureApproval>> {
  if !request.human_gesture_consent {
    return Ok(None);
  }

  context.record_event(
    "candidate.action.command.consent.requested",
    Some(format!(
      "requesting human gesture approval for app {} query {:?} role {:?} timeout_ms={}",
      request.app_bundle_id, request.query, request.role, request.human_gesture_timeout_ms
    )),
  );

  let response = auv_driver_macos::native::auth::request_human_approval(
    human_gesture_prompt_reason(request),
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
fn human_gesture_prompt_reason(request: &CandidateActionCommandRequest) -> String {
  format!(
    "Approve one AUV candidate action for app {} targeting query {:?} with role {:?}. This approval is limited to promotion {} and execution {}.",
    request.app_bundle_id, request.query, request.role, request.promotion_id, request.execution_id
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
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::{
    CandidateActionCommandRequest, CandidateActionCommandStatus,
    command_status_for_execution_side_effect,
  };
  use crate::candidate_action_decision::{CandidateActionExecutionSideEffect, CandidateActionKind};

  fn base_request() -> CandidateActionCommandRequest {
    CandidateActionCommandRequest {
      app_bundle_id: "com.apple.TextEdit".to_string(),
      query: "Body".to_string(),
      role: "AXTextArea".to_string(),
      action: CandidateActionKind::Click,
      reveal_shortcut: None,
      reveal_settle_ms: 250,
      stable_frames: 3,
      stable_frame_delay_ms: 150,
      max_centroid_drift_px: 4.0,
      require_stable_text: true,
      dev_self_minted_consent: false,
      human_gesture_consent: false,
      human_gesture_timeout_ms: 15_000,
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
    request.action = CandidateActionKind::TypeText {
      text: String::new(),
    };

    assert_eq!(
      request.validate(),
      Err("--text must not be empty when --action type-text".to_string())
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
