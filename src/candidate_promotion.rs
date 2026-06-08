//! L7 candidate-promotion 闸门(refusal-first, v0)。
//!
//! recognition evidence -> [本闸门] -> contract::Candidate。
//! 默认拒绝:缺任一前置即返回带类型理由的 `Refused`。
//! 本类型不是 action-result schema:它在 recognition -> candidate 边，
//! 位于 ActionResolverDecision / InputActionResult 上游，不复制它们。

use serde::{Deserialize, Serialize};

use crate::contract::{
  Candidate, CandidateEvidence, CandidateLiveness, ControlRequirements, LivenessPreconditions,
  RecognitionResult, RecognitionScope, TargetGrounding, TargetSpec, WindowRefPrecondition,
};

/// 必须由调用方显式提供的过墙前置。缺任何一项 => 拒绝。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PromotionContext {
  pub projection: PromotionProjection,
  pub stability: StabilityInput,
  pub freshness: Option<PromotionFreshness>,
  pub permission: Option<ActionPermission>,
  /// Caller-provided wall-clock used to evaluate freshness and consent expiry.
  /// Keep time outside the pure gate so tests and replay stay deterministic.
  pub checked_at_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionProjection {
  Unavailable {
    reason: String,
  },
  /// AX 已寻址元素,窗口空间恒等可用。
  IdentityWindowAddressable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StabilityInput {
  /// NOTICE(l6-stability-producer-deferred):
  /// L6 producer 尚未落地; 生产环境当前应显式传这个,让闸门默认拒绝。
  /// 等 owner 批准的 L6 stability slice 落地后，再接真实 producer。
  Unproven {
    reason: String,
  },
  Proven {
    observed_frames: u32,
  },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionFreshness {
  pub source_artifact: Option<crate::contract::ArtifactRef>,
  pub source_operation_id: Option<String>,
  pub observed_at_millis: Option<u64>,
  pub max_age_ms: Option<u64>,
  pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPermission {
  pub granted_by: String,
  pub scope_note: String,
  pub consent: ActionConsentRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionConsentRecord {
  pub consent_id: String,
  pub recognition_id: String,
  pub run_id: crate::trace::RunId,
  pub scope: ActionConsentScope,
  pub approved_action: ActionConsentAction,
  pub target_item_id: String,
  pub approved_at_millis: u64,
  pub expires_at_millis: Option<u64>,
  pub evidence_note: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionConsentAction {
  CandidatePromotion,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionConsentScope {
  pub surface: crate::contract::RecognitionSurface,
  pub app_bundle_id: Option<String>,
  pub window_title: Option<String>,
  pub window_number: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PromotionAudit {
  pub freshness_source_artifact: Option<crate::contract::ArtifactRef>,
  pub freshness_source_operation_id: Option<String>,
  pub consent_id: String,
  pub consent_scope: ActionConsentScope,
  pub consent_granted_by: String,
  pub projection_kind: String,
  pub stability_observed_frames: Option<u32>,
}

/// 闸门决定。不是 action-result schema。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidatePromotion {
  Refused {
    reasons: Vec<PromotionRefusal>,
  },
  Promoted {
    candidates: Vec<Candidate>,
    residual_known_limits: Vec<String>,
    audit: PromotionAudit,
  },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionRefusal {
  EmptyRecognition,
  NoUnambiguousTarget,
  NoRuntimeEvidence,
  MissingCaptureArtifact,
  ProjectionUnavailable { reason: String },
  StabilityUnproven { reason: String },
  FreshnessUnknown,
  FreshnessStale { reason: String },
  PermissionMissing,
  PermissionInvalid { reason: String },
}

/// 唯一公共入口。纯函数,无副作用,不落盘,不碰 driver。
pub fn promote_recognition_to_candidates(
  recognition: &RecognitionResult,
  context: &PromotionContext,
) -> CandidatePromotion {
  let mut reasons: Vec<PromotionRefusal> = Vec::new();

  if recognition.all.is_empty() {
    reasons.push(PromotionRefusal::EmptyRecognition);
  }
  if recognition.best.is_none() {
    reasons.push(PromotionRefusal::NoUnambiguousTarget);
  }
  if recognition.evidence.is_empty() {
    reasons.push(PromotionRefusal::NoRuntimeEvidence);
  }
  if recognition.scope.capture_artifact.is_none() {
    reasons.push(PromotionRefusal::MissingCaptureArtifact);
  }
  if let PromotionProjection::Unavailable { reason } = &context.projection {
    reasons.push(PromotionRefusal::ProjectionUnavailable {
      reason: reason.clone(),
    });
  }
  if let StabilityInput::Unproven { reason } = &context.stability {
    reasons.push(PromotionRefusal::StabilityUnproven {
      reason: reason.clone(),
    });
  }
  match validate_freshness(recognition, context) {
    Ok(()) => {}
    Err(PromotionRefusal::FreshnessUnknown) => reasons.push(PromotionRefusal::FreshnessUnknown),
    Err(reason) => reasons.push(reason),
  }
  if let Some(best) = recognition.best.as_ref() {
    match validate_permission(recognition, best, context) {
      Ok(()) => {}
      Err(PromotionRefusal::PermissionMissing) => reasons.push(PromotionRefusal::PermissionMissing),
      Err(reason) => reasons.push(reason),
    }
  } else if context.permission.is_none() {
    reasons.push(PromotionRefusal::PermissionMissing);
  }

  if !reasons.is_empty() {
    return CandidatePromotion::Refused { reasons };
  }

  let best = recognition
    .best
    .as_ref()
    .expect("best is Some when no refusal recorded");

  let mut known_limits = recognition.known_limits.clone();
  known_limits.push(
    "promoted under v0 refusal-first gate: coordinate grounding is identity-passthrough only"
      .to_string(),
  );
  known_limits.push(
    "candidate is action-eligible only; execution still requires explicit human approval"
      .to_string(),
  );

  let candidate = Candidate {
    candidate_local_id: format!("promoted-{}", best.item_id),
    kind: best.kind.clone(),
    label: best.text.clone(),
    target_spec: TargetSpec {
      grounding: TargetGrounding::Coordinate,
      anchor_text: best.text.clone(),
      region_hint: None,
      row_index: None,
    },
    evidence: CandidateEvidence {
      artifact_ref: recognition.evidence[0].clone(),
      observation: serde_json::to_value(best).unwrap_or(serde_json::Value::Null),
    },
    liveness: CandidateLiveness {
      preconditions: LivenessPreconditions {
        window_ref: window_ref_from_scope(&recognition.scope),
        // TODO(l7-anchor-recheck-v0): anchor_recheck deferred in this slice;
        // re-open only with owner-approved L6/L7 follow-up that defines text
        // anchor semantics for promoted recognition evidence.
        anchor_recheck: None,
      },
      ttl_hint_ms: None,
    },
    control: ControlRequirements {
      requires_app_frontmost: true,
      requires_window_focus: true,
    },
    known_limits: known_limits.clone(),
  };

  CandidatePromotion::Promoted {
    candidates: vec![candidate],
    residual_known_limits: known_limits,
    audit: promotion_audit(context),
  }
}

fn validate_freshness(
  recognition: &RecognitionResult,
  context: &PromotionContext,
) -> Result<(), PromotionRefusal> {
  let Some(freshness) = context.freshness.as_ref() else {
    return Err(PromotionRefusal::FreshnessUnknown);
  };
  let Some(source_artifact) = freshness.source_artifact.as_ref() else {
    return Err(PromotionRefusal::FreshnessStale {
      reason: "freshness source_artifact is missing".to_string(),
    });
  };
  let Some(capture_artifact) = recognition.scope.capture_artifact.as_ref() else {
    return Err(PromotionRefusal::FreshnessStale {
      reason: "recognition capture_artifact is missing".to_string(),
    });
  };
  if source_artifact != capture_artifact {
    return Err(PromotionRefusal::FreshnessStale {
      reason: "freshness source_artifact does not match recognition capture_artifact".to_string(),
    });
  }
  if freshness
    .source_operation_id
    .as_deref()
    .map(str::trim)
    .unwrap_or_default()
    .is_empty()
  {
    return Err(PromotionRefusal::FreshnessStale {
      reason: "freshness source_operation_id is missing".to_string(),
    });
  }
  let Some(observed_at_millis) = freshness.observed_at_millis else {
    return Err(PromotionRefusal::FreshnessStale {
      reason: "freshness observed_at_millis is missing".to_string(),
    });
  };
  if observed_at_millis > context.checked_at_millis {
    return Err(PromotionRefusal::FreshnessStale {
      reason: "freshness observed_at_millis is in the future".to_string(),
    });
  }
  let Some(max_age_ms) = freshness.max_age_ms else {
    return Err(PromotionRefusal::FreshnessStale {
      reason: "freshness max_age_ms is missing".to_string(),
    });
  };
  if context.checked_at_millis - observed_at_millis > max_age_ms {
    return Err(PromotionRefusal::FreshnessStale {
      reason: "freshness evidence is stale".to_string(),
    });
  }
  Ok(())
}

fn validate_permission(
  recognition: &RecognitionResult,
  best: &crate::contract::RecognizedItem,
  context: &PromotionContext,
) -> Result<(), PromotionRefusal> {
  let Some(permission) = context.permission.as_ref() else {
    return Err(PromotionRefusal::PermissionMissing);
  };
  if permission.granted_by.trim().is_empty() {
    return Err(PromotionRefusal::PermissionInvalid {
      reason: "permission granted_by is missing".to_string(),
    });
  }
  if permission.scope_note.trim().is_empty() {
    return Err(PromotionRefusal::PermissionInvalid {
      reason: "permission scope_note is missing".to_string(),
    });
  }
  let consent = &permission.consent;
  if consent.consent_id.trim().is_empty() {
    return Err(PromotionRefusal::PermissionInvalid {
      reason: "consent_id is missing".to_string(),
    });
  }
  if consent.evidence_note.trim().is_empty() {
    return Err(PromotionRefusal::PermissionInvalid {
      reason: "consent evidence_note is missing".to_string(),
    });
  }
  if consent.recognition_id != recognition.recognition_id {
    return Err(PromotionRefusal::PermissionInvalid {
      reason: "consent recognition_id does not match promotion recognition_id".to_string(),
    });
  }
  let Some(capture_artifact) = recognition.scope.capture_artifact.as_ref() else {
    return Err(PromotionRefusal::PermissionInvalid {
      reason: "recognition capture_artifact is missing".to_string(),
    });
  };
  if consent.run_id != capture_artifact.run_id {
    return Err(PromotionRefusal::PermissionInvalid {
      reason: "consent run_id does not match capture artifact run_id".to_string(),
    });
  }
  if consent.scope != consent_scope_from_recognition(recognition) {
    return Err(PromotionRefusal::PermissionInvalid {
      reason: "consent scope does not match recognition scope".to_string(),
    });
  }
  if consent.approved_action != ActionConsentAction::CandidatePromotion {
    return Err(PromotionRefusal::PermissionInvalid {
      reason: "consent approved_action is not candidate_promotion".to_string(),
    });
  }
  if consent.target_item_id != best.item_id {
    return Err(PromotionRefusal::PermissionInvalid {
      reason: "consent target_item_id does not match recognized item".to_string(),
    });
  }
  if consent.approved_at_millis > context.checked_at_millis {
    return Err(PromotionRefusal::PermissionInvalid {
      reason: "consent approved_at_millis is in the future".to_string(),
    });
  }
  let Some(expires_at_millis) = consent.expires_at_millis else {
    return Err(PromotionRefusal::PermissionInvalid {
      reason: "consent expires_at_millis is missing".to_string(),
    });
  };
  if expires_at_millis <= context.checked_at_millis {
    return Err(PromotionRefusal::PermissionInvalid {
      reason: "consent is expired".to_string(),
    });
  }
  if consent.approved_at_millis >= expires_at_millis {
    return Err(PromotionRefusal::PermissionInvalid {
      reason: "consent expires before or at approval time".to_string(),
    });
  }
  Ok(())
}

fn consent_scope_from_recognition(recognition: &RecognitionResult) -> ActionConsentScope {
  ActionConsentScope {
    surface: recognition.scope.surface,
    app_bundle_id: recognition.scope.app_bundle_id.clone(),
    window_title: recognition.scope.window_title.clone(),
    window_number: recognition.scope.window_number,
  }
}

fn promotion_audit(context: &PromotionContext) -> PromotionAudit {
  let freshness = context
    .freshness
    .as_ref()
    .expect("freshness is present when promotion is audited");
  let permission = context
    .permission
    .as_ref()
    .expect("permission is present when promotion is audited");
  PromotionAudit {
    freshness_source_artifact: freshness.source_artifact.clone(),
    freshness_source_operation_id: freshness.source_operation_id.clone(),
    consent_id: permission.consent.consent_id.clone(),
    consent_scope: permission.consent.scope.clone(),
    consent_granted_by: permission.granted_by.clone(),
    projection_kind: projection_kind(&context.projection),
    stability_observed_frames: stability_observed_frames(&context.stability),
  }
}

fn projection_kind(projection: &PromotionProjection) -> String {
  match projection {
    PromotionProjection::Unavailable { .. } => "unavailable".to_string(),
    PromotionProjection::IdentityWindowAddressable => "identity_window_addressable".to_string(),
  }
}

fn stability_observed_frames(stability: &StabilityInput) -> Option<u32> {
  match stability {
    StabilityInput::Unproven { .. } => None,
    StabilityInput::Proven { observed_frames } => Some(*observed_frames),
  }
}

fn window_ref_from_scope(scope: &RecognitionScope) -> Option<WindowRefPrecondition> {
  let app_bundle_id = scope.app_bundle_id.clone()?;
  Some(WindowRefPrecondition {
    app_bundle_id,
    window_title_substring: scope.window_title.clone(),
    window_number: scope.window_number,
  })
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::{
    ActionConsentAction, ActionConsentRecord, ActionConsentScope, ActionPermission,
    CandidatePromotion, PromotionContext, PromotionFreshness, PromotionProjection,
    PromotionRefusal, StabilityInput, promote_recognition_to_candidates,
  };
  use crate::contract::{
    ArtifactRef, Candidate, RecognitionBox, RecognitionResult, RecognitionScope, RecognitionSource,
    RecognitionSurface, RecognizedItem,
  };
  use crate::trace::{ArtifactId, EventId, RunId, SpanId};

  fn sample_artifact_ref() -> ArtifactRef {
    ArtifactRef {
      run_id: RunId::new("run_candidate_promotion"),
      artifact_id: ArtifactId::new("artifact_candidate_promotion"),
      span_id: SpanId::new("span_candidate_promotion"),
      captured_event_id: Some(EventId::new("event_candidate_promotion")),
    }
  }

  fn sample_recognition() -> RecognitionResult {
    let artifact_ref = sample_artifact_ref();
    RecognitionResult {
      recognition_id: "recognition_candidate_promotion".to_string(),
      source: RecognitionSource::Custom,
      scope: RecognitionScope {
        surface: RecognitionSurface::Region,
        display_ref: None,
        native_display_id: None,
        app_bundle_id: Some("com.megacrit.cardcrawl".to_string()),
        window_title: Some("Slay the Spire".to_string()),
        window_number: Some(7),
        region_hint: None,
        capture_artifact: Some(artifact_ref.clone()),
        capture_contract_artifact: None,
      },
      best: Some(RecognizedItem {
        item_id: "item_1".to_string(),
        kind: "button".to_string(),
        box_: RecognitionBox {
          x: 1638,
          y: 792,
          width: 228,
          height: 178,
        },
        text: Some("End Turn".to_string()),
        provider_score: Some(0.99),
        detail: json!({
          "backend": "manual-fixture"
        }),
      }),
      filtered: vec![RecognizedItem {
        item_id: "item_1".to_string(),
        kind: "button".to_string(),
        box_: RecognitionBox {
          x: 1638,
          y: 792,
          width: 228,
          height: 178,
        },
        text: Some("End Turn".to_string()),
        provider_score: Some(0.99),
        detail: json!({
          "backend": "manual-fixture"
        }),
      }],
      all: vec![RecognizedItem {
        item_id: "item_1".to_string(),
        kind: "button".to_string(),
        box_: RecognitionBox {
          x: 1638,
          y: 792,
          width: 228,
          height: 178,
        },
        text: Some("End Turn".to_string()),
        provider_score: Some(0.99),
        detail: json!({
          "backend": "manual-fixture"
        }),
      }],
      detail: json!({
        "backend": "manual-fixture"
      }),
      evidence: vec![artifact_ref],
      known_limits: vec!["recognition evidence only".to_string()],
    }
  }

  fn sample_context() -> PromotionContext {
    PromotionContext {
      projection: PromotionProjection::IdentityWindowAddressable,
      stability: StabilityInput::Proven { observed_frames: 3 },
      freshness: Some(PromotionFreshness {
        source_artifact: Some(sample_artifact_ref()),
        source_operation_id: Some("observe.slay.fixture".to_string()),
        observed_at_millis: Some(1_000),
        max_age_ms: Some(500),
        notes: vec!["same-frame fixture".to_string()],
      }),
      permission: Some(sample_permission()),
      checked_at_millis: 1_100,
    }
  }

  fn sample_permission() -> ActionPermission {
    ActionPermission {
      granted_by: "unit-test".to_string(),
      scope_note: "synthetic promotion proof".to_string(),
      consent: ActionConsentRecord {
        consent_id: "consent_candidate_promotion".to_string(),
        recognition_id: "recognition_candidate_promotion".to_string(),
        run_id: sample_artifact_ref().run_id,
        scope: ActionConsentScope {
          surface: RecognitionSurface::Region,
          app_bundle_id: Some("com.megacrit.cardcrawl".to_string()),
          window_title: Some("Slay the Spire".to_string()),
          window_number: Some(7),
        },
        approved_action: ActionConsentAction::CandidatePromotion,
        target_item_id: "item_1".to_string(),
        approved_at_millis: 1_000,
        expires_at_millis: Some(1_500),
        evidence_note: "unit test approval".to_string(),
      },
    }
  }

  fn assert_promoted_candidate(candidate: &Candidate) {
    assert_eq!(candidate.candidate_local_id, "promoted-item_1");
    assert_eq!(candidate.kind, "button");
    assert_eq!(candidate.label.as_deref(), Some("End Turn"));
    assert_eq!(
      serde_json::to_value(&candidate.target_spec.grounding).expect("grounding should serialize"),
      json!("coordinate")
    );
    assert_eq!(
      candidate
        .liveness
        .preconditions
        .window_ref
        .as_ref()
        .map(|window| window.app_bundle_id.as_str()),
      Some("com.megacrit.cardcrawl")
    );
    assert_eq!(
      candidate
        .liveness
        .preconditions
        .window_ref
        .as_ref()
        .and_then(|window| window.window_title_substring.as_deref()),
      Some("Slay the Spire")
    );
    assert_eq!(
      candidate
        .evidence
        .observation
        .get("item_id")
        .and_then(|value| value.as_str()),
      Some("item_1")
    );
    assert!(
      candidate.known_limits.contains(
        &"promoted under v0 refusal-first gate: coordinate grounding is identity-passthrough only"
          .to_string()
      )
    );
  }

  #[test]
  fn refusal_first_defaults_to_stability_unproven_when_other_inputs_exist() {
    let recognition = sample_recognition();
    let context = PromotionContext {
      stability: StabilityInput::Unproven {
        reason: "l6 producer not landed".to_string(),
      },
      ..sample_context()
    };

    let decision = promote_recognition_to_candidates(&recognition, &context);

    assert_eq!(
      decision,
      CandidatePromotion::Refused {
        reasons: vec![PromotionRefusal::StabilityUnproven {
          reason: "l6 producer not landed".to_string()
        }]
      }
    );
  }

  #[test]
  fn refusal_collects_all_missing_prerequisites() {
    let mut recognition = sample_recognition();
    recognition.all.clear();
    recognition.best = None;
    recognition.evidence.clear();
    recognition.scope.capture_artifact = None;

    let decision = promote_recognition_to_candidates(
      &recognition,
      &PromotionContext {
        projection: PromotionProjection::Unavailable {
          reason: "no projection basis".to_string(),
        },
        stability: StabilityInput::Unproven {
          reason: "no stability proof".to_string(),
        },
        freshness: None,
        permission: None,
        checked_at_millis: 1_100,
      },
    );

    assert_eq!(
      decision,
      CandidatePromotion::Refused {
        reasons: vec![
          PromotionRefusal::EmptyRecognition,
          PromotionRefusal::NoUnambiguousTarget,
          PromotionRefusal::NoRuntimeEvidence,
          PromotionRefusal::MissingCaptureArtifact,
          PromotionRefusal::ProjectionUnavailable {
            reason: "no projection basis".to_string()
          },
          PromotionRefusal::StabilityUnproven {
            reason: "no stability proof".to_string()
          },
          PromotionRefusal::FreshnessUnknown,
          PromotionRefusal::PermissionMissing,
        ]
      }
    );
  }

  #[test]
  fn missing_projection_refuses_with_projection_reason() {
    let recognition = sample_recognition();
    let decision = promote_recognition_to_candidates(
      &recognition,
      &PromotionContext {
        projection: PromotionProjection::Unavailable {
          reason: "no projection basis".to_string(),
        },
        ..sample_context()
      },
    );

    assert_eq!(
      decision,
      CandidatePromotion::Refused {
        reasons: vec![PromotionRefusal::ProjectionUnavailable {
          reason: "no projection basis".to_string()
        }]
      }
    );
  }

  #[test]
  fn missing_permission_refuses() {
    let recognition = sample_recognition();
    let decision = promote_recognition_to_candidates(
      &recognition,
      &PromotionContext {
        permission: None,
        ..sample_context()
      },
    );

    assert_eq!(
      decision,
      CandidatePromotion::Refused {
        reasons: vec![PromotionRefusal::PermissionMissing]
      }
    );
  }

  fn assert_refusal_contains(decision: CandidatePromotion, expected: PromotionRefusal) {
    match decision {
      CandidatePromotion::Refused { reasons } => assert!(
        reasons.contains(&expected),
        "expected refusal {expected:?}, got {reasons:?}"
      ),
      CandidatePromotion::Promoted { .. } => panic!("expected refusal, got promoted"),
    }
  }

  #[test]
  fn expired_consent_refuses() {
    let recognition = sample_recognition();
    let mut permission = sample_permission();
    permission.consent.expires_at_millis = Some(1_050);

    assert_refusal_contains(
      promote_recognition_to_candidates(
        &recognition,
        &PromotionContext {
          permission: Some(permission),
          ..sample_context()
        },
      ),
      PromotionRefusal::PermissionInvalid {
        reason: "consent is expired".to_string(),
      },
    );
  }

  #[test]
  fn future_consent_approval_refuses() {
    let recognition = sample_recognition();
    let mut permission = sample_permission();
    permission.consent.approved_at_millis = 1_200;

    assert_refusal_contains(
      promote_recognition_to_candidates(
        &recognition,
        &PromotionContext {
          permission: Some(permission),
          ..sample_context()
        },
      ),
      PromotionRefusal::PermissionInvalid {
        reason: "consent approved_at_millis is in the future".to_string(),
      },
    );
  }

  #[test]
  fn consent_for_different_item_refuses() {
    let recognition = sample_recognition();
    let mut permission = sample_permission();
    permission.consent.target_item_id = "item_2".to_string();

    assert_refusal_contains(
      promote_recognition_to_candidates(
        &recognition,
        &PromotionContext {
          permission: Some(permission),
          ..sample_context()
        },
      ),
      PromotionRefusal::PermissionInvalid {
        reason: "consent target_item_id does not match recognized item".to_string(),
      },
    );
  }

  #[test]
  fn consent_for_different_recognition_refuses() {
    let recognition = sample_recognition();
    let mut permission = sample_permission();
    permission.consent.recognition_id = "recognition_other".to_string();

    assert_refusal_contains(
      promote_recognition_to_candidates(
        &recognition,
        &PromotionContext {
          permission: Some(permission),
          ..sample_context()
        },
      ),
      PromotionRefusal::PermissionInvalid {
        reason: "consent recognition_id does not match promotion recognition_id".to_string(),
      },
    );
  }

  #[test]
  fn consent_for_different_run_refuses() {
    let recognition = sample_recognition();
    let mut permission = sample_permission();
    permission.consent.run_id = RunId::new("run_other");

    assert_refusal_contains(
      promote_recognition_to_candidates(
        &recognition,
        &PromotionContext {
          permission: Some(permission),
          ..sample_context()
        },
      ),
      PromotionRefusal::PermissionInvalid {
        reason: "consent run_id does not match capture artifact run_id".to_string(),
      },
    );
  }

  #[test]
  fn consent_for_different_scope_refuses() {
    let recognition = sample_recognition();
    let mut permission = sample_permission();
    permission.consent.scope.window_title = Some("Other Window".to_string());

    assert_refusal_contains(
      promote_recognition_to_candidates(
        &recognition,
        &PromotionContext {
          permission: Some(permission),
          ..sample_context()
        },
      ),
      PromotionRefusal::PermissionInvalid {
        reason: "consent scope does not match recognition scope".to_string(),
      },
    );
  }

  #[test]
  fn permission_without_scope_note_refuses() {
    let recognition = sample_recognition();
    let mut permission = sample_permission();
    permission.scope_note = "   ".to_string();

    assert_refusal_contains(
      promote_recognition_to_candidates(
        &recognition,
        &PromotionContext {
          permission: Some(permission),
          ..sample_context()
        },
      ),
      PromotionRefusal::PermissionInvalid {
        reason: "permission scope_note is missing".to_string(),
      },
    );
  }

  #[test]
  fn consent_without_evidence_note_refuses() {
    let recognition = sample_recognition();
    let mut permission = sample_permission();
    permission.consent.evidence_note = "   ".to_string();

    assert_refusal_contains(
      promote_recognition_to_candidates(
        &recognition,
        &PromotionContext {
          permission: Some(permission),
          ..sample_context()
        },
      ),
      PromotionRefusal::PermissionInvalid {
        reason: "consent evidence_note is missing".to_string(),
      },
    );
  }

  #[test]
  fn missing_capture_does_not_make_freshness_or_permission_valid() {
    let mut recognition = sample_recognition();
    recognition.scope.capture_artifact = None;

    let decision = promote_recognition_to_candidates(&recognition, &sample_context());

    assert_refusal_contains(decision.clone(), PromotionRefusal::MissingCaptureArtifact);
    assert_refusal_contains(
      decision.clone(),
      PromotionRefusal::FreshnessStale {
        reason: "recognition capture_artifact is missing".to_string(),
      },
    );
    assert_refusal_contains(
      decision,
      PromotionRefusal::PermissionInvalid {
        reason: "recognition capture_artifact is missing".to_string(),
      },
    );
  }

  #[test]
  fn stale_freshness_refuses() {
    let recognition = sample_recognition();
    let mut context = sample_context();
    context
      .freshness
      .as_mut()
      .expect("freshness")
      .observed_at_millis = Some(500);

    assert_refusal_contains(
      promote_recognition_to_candidates(&recognition, &context),
      PromotionRefusal::FreshnessStale {
        reason: "freshness evidence is stale".to_string(),
      },
    );
  }

  #[test]
  fn freshness_for_different_capture_refuses() {
    let recognition = sample_recognition();
    let mut context = sample_context();
    let freshness = context.freshness.as_mut().expect("freshness");
    freshness.source_artifact = Some(ArtifactRef {
      run_id: RunId::new("run_other"),
      artifact_id: ArtifactId::new("artifact_other"),
      span_id: SpanId::new("span_other"),
      captured_event_id: None,
    });

    assert_refusal_contains(
      promote_recognition_to_candidates(&recognition, &context),
      PromotionRefusal::FreshnessStale {
        reason: "freshness source_artifact does not match recognition capture_artifact".to_string(),
      },
    );
  }

  #[test]
  fn freshness_without_operation_id_refuses() {
    let recognition = sample_recognition();
    let mut context = sample_context();
    context
      .freshness
      .as_mut()
      .expect("freshness")
      .source_operation_id = Some("   ".to_string());

    assert_refusal_contains(
      promote_recognition_to_candidates(&recognition, &context),
      PromotionRefusal::FreshnessStale {
        reason: "freshness source_operation_id is missing".to_string(),
      },
    );
  }

  #[test]
  fn proven_context_promotes_single_candidate_without_new_parallel_types() {
    let recognition = sample_recognition();
    let decision = promote_recognition_to_candidates(&recognition, &sample_context());

    match decision {
      CandidatePromotion::Promoted {
        candidates,
        residual_known_limits,
        audit,
      } => {
        assert_eq!(candidates.len(), 1);
        assert_promoted_candidate(&candidates[0]);
        assert!(
          residual_known_limits.contains(
            &"candidate is action-eligible only; execution still requires explicit human approval"
              .to_string()
          )
        );
        assert_eq!(audit.consent_id, "consent_candidate_promotion");
        assert_eq!(audit.freshness_source_artifact, Some(sample_artifact_ref()));
        assert_eq!(
          audit.freshness_source_operation_id.as_deref(),
          Some("observe.slay.fixture")
        );
        assert_eq!(audit.stability_observed_frames, Some(3));
      }
      other => panic!("expected Promoted, got {other:?}"),
    }
  }
}
