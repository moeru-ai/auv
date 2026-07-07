use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const ACTION_RESOLVER_VERSION: &str = "v0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResolvedActionMethod {
  AxAction,
  PointerClick,
  WindowTargetedTypeText,
}

impl ResolvedActionMethod {
  pub(crate) fn as_str(self) -> &'static str {
    match self {
      Self::AxAction => "ax-action",
      Self::PointerClick => "pointer-click",
      Self::WindowTargetedTypeText => "window-targeted-type-text",
    }
  }

  fn cursor_disturbance(self) -> &'static str {
    match self {
      Self::AxAction => "none",
      Self::PointerClick => "warp-visible",
      Self::WindowTargetedTypeText => "none",
    }
  }

  fn press_mechanism(self) -> &'static str {
    self.as_str()
  }
}

/// Persisted record of which input method an action resolver selected, why it
/// selected that method, and whether the chosen path was a fallback.
///
/// # Seam role
///
/// Upper / "what method got chosen" half of the v0 action-result pair.
/// Sibling: [`InputActionResult`] in `crates/auv-driver/src/input.rs`, which
/// records actual delivery attempts.
///
/// - `input.smartPress` records this alongside real input delivery.
/// - L8a candidate action planning records this as a **decide-only** artifact
///   with no `InputActionResult`, no driver call, and no side effect.
///
/// This remains `pub(crate)`: consumers should observe the decision through
/// recorded artifacts / lineage instead of importing a public action schema.
/// Per AGENTS.md, this and `InputActionResult` are the two action-result
/// schemas in v0. Do not introduce a third action-result schema beside them.
///
/// [`InputActionResult`]: auv_driver::InputActionResult
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActionResolverDecision {
  pub(crate) version: String,
  pub(crate) operation: String,
  pub(crate) target_query: String,
  pub(crate) primary_method: String,
  pub(crate) selected_method: String,
  pub(crate) fallback_allowed: bool,
  pub(crate) fallback_used: bool,
  pub(crate) fallback_reason: Option<String>,
  pub(crate) policy: String,
  pub(crate) cursor_disturbance: String,
  pub(crate) press_mechanism: String,
}

impl ActionResolverDecision {
  pub(crate) fn smart_press(
    query: &str,
    selected_method: ResolvedActionMethod,
    fallback_allowed: bool,
    primary_error: Option<&str>,
  ) -> Self {
    let fallback_used = selected_method != ResolvedActionMethod::AxAction;
    Self::new(ActionResolverDecisionInput {
      operation: "input.smartPress",
      target_query: query,
      primary_method: ResolvedActionMethod::AxAction.as_str(),
      selected_method: selected_method.as_str(),
      fallback_allowed,
      fallback_used,
      fallback_reason: fallback_used
        .then(|| primary_error.map(render_resolver_value).unwrap_or_else(|| "primary-method-failed".to_string())),
      policy: if fallback_allowed {
        "ax-first-then-pointer-if-allowed"
      } else {
        "ax-only"
      },
      cursor_disturbance: selected_method.cursor_disturbance(),
      press_mechanism: selected_method.press_mechanism(),
    })
  }

  pub(crate) fn new(input: ActionResolverDecisionInput<'_>) -> Self {
    Self {
      version: ACTION_RESOLVER_VERSION.to_string(),
      operation: input.operation.to_string(),
      target_query: input.target_query.to_string(),
      primary_method: input.primary_method.to_string(),
      selected_method: input.selected_method.to_string(),
      fallback_allowed: input.fallback_allowed,
      fallback_used: input.fallback_used,
      fallback_reason: input.fallback_reason,
      policy: input.policy.to_string(),
      cursor_disturbance: input.cursor_disturbance.to_string(),
      press_mechanism: input.press_mechanism.to_string(),
    }
  }

  pub(crate) fn signals(&self) -> BTreeMap<String, String> {
    BTreeMap::from([
      ("actionResolver.version".to_string(), self.version.to_string()),
      ("actionResolver.operation".to_string(), self.operation.to_string()),
      ("actionResolver.target.query".to_string(), self.target_query.clone()),
      ("actionResolver.primaryMethod".to_string(), self.primary_method.to_string()),
      ("actionResolver.selectedMethod".to_string(), self.selected_method.to_string()),
      ("actionResolver.fallbackAllowed".to_string(), self.fallback_allowed.to_string()),
      ("actionResolver.fallbackUsed".to_string(), self.fallback_used.to_string()),
      ("actionResolver.fallbackReason".to_string(), self.fallback_reason.clone().unwrap_or_else(|| "none".to_string())),
      ("actionResolver.policy".to_string(), self.policy.to_string()),
      ("actionResolver.cursorDisturbance".to_string(), self.cursor_disturbance.to_string()),
      ("actionResolver.pressMechanism".to_string(), self.press_mechanism.to_string()),
    ])
  }

  pub(crate) fn notes(&self) -> Vec<String> {
    let mut notes = vec![
      format!("actionResolverVersion={}", self.version),
      format!("actionResolverOperation={}", self.operation),
      format!("actionResolverTargetQuery={}", self.target_query),
      format!("actionResolverPrimaryMethod={}", self.primary_method),
      format!("actionResolverSelectedMethod={}", self.selected_method),
      format!("actionResolverFallbackAllowed={}", self.fallback_allowed),
      format!("actionResolverFallbackUsed={}", self.fallback_used),
      format!("actionResolverPolicy={}", self.policy),
      format!("actionResolverCursorDisturbance={}", self.cursor_disturbance),
      format!("actionResolverPressMechanism={}", self.press_mechanism),
    ];
    if let Some(reason) = self.fallback_reason.as_deref() {
      notes.push(format!("actionResolverFallbackReason={reason}"));
    }
    notes
  }
}

pub(crate) struct ActionResolverDecisionInput<'a> {
  pub(crate) operation: &'a str,
  pub(crate) target_query: &'a str,
  pub(crate) primary_method: &'a str,
  pub(crate) selected_method: &'a str,
  pub(crate) fallback_allowed: bool,
  pub(crate) fallback_used: bool,
  pub(crate) fallback_reason: Option<String>,
  pub(crate) policy: &'a str,
  pub(crate) cursor_disturbance: &'a str,
  pub(crate) press_mechanism: &'a str,
}

pub(crate) fn render_resolver_value(raw: &str) -> String {
  raw.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn smart_press_ax_decision_records_no_fallback() {
    let decision = ActionResolverDecision::smart_press("Run", ResolvedActionMethod::AxAction, true, None);

    assert_eq!(decision.version, ACTION_RESOLVER_VERSION);
    assert_eq!(decision.operation, "input.smartPress");
    assert_eq!(decision.primary_method, "ax-action");
    assert_eq!(decision.selected_method, "ax-action");
    assert!(decision.fallback_allowed);
    assert!(!decision.fallback_used);
    assert_eq!(decision.fallback_reason, None);
    assert_eq!(decision.cursor_disturbance, "none");
    assert_eq!(decision.press_mechanism, "ax-action");

    let signals = decision.signals();
    assert_eq!(signals.get("actionResolver.operation"), Some(&"input.smartPress".to_string()));
    assert_eq!(signals.get("actionResolver.selectedMethod"), Some(&"ax-action".to_string()));
    assert_eq!(signals.get("actionResolver.fallbackReason"), Some(&"none".to_string()));
  }

  #[test]
  fn smart_press_pointer_decision_records_reason_and_disturbance() {
    let decision = ActionResolverDecision::smart_press("播放", ResolvedActionMethod::PointerClick, true, Some("AX target\nhad no action"));

    assert_eq!(decision.selected_method, "pointer-click");
    assert!(decision.fallback_used);
    assert_eq!(decision.fallback_reason.as_deref(), Some("AX target\\nhad no action"));
    assert_eq!(decision.cursor_disturbance, "warp-visible");
    assert_eq!(decision.press_mechanism, "pointer-click");

    let notes = decision.notes();
    assert!(notes.contains(&"actionResolverSelectedMethod=pointer-click".to_string()));
    assert!(notes.contains(&"actionResolverFallbackUsed=true".to_string()));
    assert!(notes.contains(&"actionResolverFallbackReason=AX target\\nhad no action".to_string()));
  }

  #[test]
  fn type_text_method_reports_non_pointer_disturbance() {
    assert_eq!(ResolvedActionMethod::WindowTargetedTypeText.as_str(), "window-targeted-type-text");
    assert_eq!(ResolvedActionMethod::WindowTargetedTypeText.cursor_disturbance(), "none");
    assert_eq!(ResolvedActionMethod::WindowTargetedTypeText.press_mechanism(), "window-targeted-type-text");
  }
}
