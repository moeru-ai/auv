//! L7 candidate-promotion 闸门(refusal-first, v0)。
//!
//! recognition evidence -> [本闸门] -> contract::Candidate。
//! 默认拒绝:缺任一前置即返回带类型理由的 `Refused`。
//! 本类型不是 action-result schema:它在 recognition -> candidate 边，
//! 位于 ActionResolverDecision / InputActionResult 上游，不复制它们。

use serde::{Deserialize, Serialize};

use crate::contract::{
  Candidate, CandidateEvidence, CandidateLiveness, ControlRequirements, FreshnessBasis,
  LivenessPreconditions, RecognitionResult, RecognitionScope, TargetGrounding, TargetSpec,
  WindowRefPrecondition,
};

/// 必须由调用方显式提供的过墙前置。缺任何一项 => 拒绝。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PromotionContext {
  pub projection: PromotionProjection,
  pub stability: StabilityInput,
  pub freshness: Option<FreshnessBasis>,
  pub permission: Option<ActionPermission>,
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
pub struct ActionPermission {
  pub granted_by: String,
  pub scope_note: String,
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
  PermissionMissing,
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
  if context.freshness.is_none() {
    reasons.push(PromotionRefusal::FreshnessUnknown);
  }
  if context.permission.is_none() {
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
    ActionPermission, CandidatePromotion, PromotionContext, PromotionProjection, PromotionRefusal,
    StabilityInput, promote_recognition_to_candidates,
  };
  use crate::contract::{
    ArtifactRef, Candidate, FreshnessBasis, RecognitionBox, RecognitionResult, RecognitionScope,
    RecognitionSource, RecognitionSurface, RecognizedItem,
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
      freshness: Some(FreshnessBasis {
        source_artifact: Some(sample_artifact_ref()),
        source_operation_id: Some("observe.slay.fixture".to_string()),
        notes: vec!["same-frame fixture".to_string()],
      }),
      permission: Some(ActionPermission {
        granted_by: "unit-test".to_string(),
        scope_note: "synthetic promotion proof".to_string(),
      }),
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

  #[test]
  fn proven_context_promotes_single_candidate_without_new_parallel_types() {
    let recognition = sample_recognition();
    let decision = promote_recognition_to_candidates(&recognition, &sample_context());

    match decision {
      CandidatePromotion::Promoted {
        candidates,
        residual_known_limits,
      } => {
        assert_eq!(candidates.len(), 1);
        assert_promoted_candidate(&candidates[0]);
        assert!(
          residual_known_limits.contains(
            &"candidate is action-eligible only; execution still requires explicit human approval"
              .to_string()
          )
        );
      }
      other => panic!("expected Promoted, got {other:?}"),
    }
  }
}
