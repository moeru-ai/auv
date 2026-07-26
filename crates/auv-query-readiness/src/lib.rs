//! NOTICE(query-readiness-helper): this crate owns only the shared derived-action
//! eligibility triad and optional refusal-reason shape used by spatial-query
//! consumption probes. It is **not** driver window-probe readiness; see
//! `crates/auv-driver/src/readiness.rs` for that unrelated surface.
//!
//! Manifest-to-input mapping, point geometry, and vertical-specific derive
//! branching stay donor-local per
//! `docs/ai/references/2026-06-27-auv-core-a-query-readiness-graduation-review.md`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DerivedActionEligibility {
  NotConsumable,
  AnswerNonClickable,
  ClickReady,
}

impl DerivedActionEligibility {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::NotConsumable => "not_consumable",
      Self::AnswerNonClickable => "answer_non_clickable",
      Self::ClickReady => "click_ready",
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DerivedActionReadiness {
  pub eligibility: DerivedActionEligibility,
  pub refusal_reason: Option<String>,
}

impl DerivedActionReadiness {
  pub fn not_consumable(reason: impl Into<String>) -> Self {
    Self {
      eligibility: DerivedActionEligibility::NotConsumable,
      refusal_reason: Some(reason.into()),
    }
  }

  pub fn answer_non_clickable(reason: impl Into<String>) -> Self {
    Self {
      eligibility: DerivedActionEligibility::AnswerNonClickable,
      refusal_reason: Some(reason.into()),
    }
  }

  pub fn click_ready() -> Self {
    Self {
      eligibility: DerivedActionEligibility::ClickReady,
      refusal_reason: None,
    }
  }
}

pub fn format_query_not_consumable_refusal(status_label: &str, reason_label: Option<&str>) -> String {
  match reason_label {
    Some(reason) => format!("status={status_label} reason={reason}"),
    None => format!("status={status_label}"),
  }
}

/// Maps `DerivedActionEligibility::as_str()` labels onto inspect
/// `readiness_class` strings shared by ordinary game readers and product
/// query-wired projections.
pub fn map_action_eligibility_to_readiness_class(eligibility: &str) -> Option<String> {
  match eligibility {
    "click_ready" => Some("ready".to_string()),
    "answer_non_clickable" => Some("non_actionable".to_string()),
    "not_consumable" => Some("not_consumable".to_string()),
    _ => None,
  }
}
