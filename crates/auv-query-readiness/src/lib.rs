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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn eligibility_as_str_covers_all_labels() {
    assert_eq!(DerivedActionEligibility::NotConsumable.as_str(), "not_consumable");
    assert_eq!(DerivedActionEligibility::AnswerNonClickable.as_str(), "answer_non_clickable");
    assert_eq!(DerivedActionEligibility::ClickReady.as_str(), "click_ready");
  }

  #[test]
  fn constructors_set_eligibility_and_refusal_reason() {
    let not_consumable = DerivedActionReadiness::not_consumable("status=blocked");
    assert_eq!(not_consumable.eligibility, DerivedActionEligibility::NotConsumable);
    assert_eq!(not_consumable.refusal_reason.as_deref(), Some("status=blocked"));

    let answer_non_clickable = DerivedActionReadiness::answer_non_clickable("visibility=outside_window");
    assert_eq!(answer_non_clickable.eligibility, DerivedActionEligibility::AnswerNonClickable);
    assert_eq!(answer_non_clickable.refusal_reason.as_deref(), Some("visibility=outside_window"));

    let click_ready = DerivedActionReadiness::click_ready();
    assert_eq!(click_ready.eligibility, DerivedActionEligibility::ClickReady);
    assert!(click_ready.refusal_reason.is_none());
  }

  #[test]
  fn format_query_not_consumable_refusal_with_and_without_reason() {
    assert_eq!(format_query_not_consumable_refusal("failed", Some("target_absent")), "status=failed reason=target_absent");
    assert_eq!(format_query_not_consumable_refusal("blocked", None), "status=blocked");
  }
}
