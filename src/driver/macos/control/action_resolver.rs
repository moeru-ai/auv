use serde::Serialize;
use std::collections::BTreeMap;

use super::super::ProducedArtifact;
use super::super::support::artifacts::{build_text_artifact, sanitize_file_component};
use crate::model::AuvResult;

pub(crate) const ACTION_RESOLVER_VERSION: &str = "v0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResolvedActionMethod {
  AxAction,
  PointerClick,
}

impl ResolvedActionMethod {
  pub(crate) fn as_str(self) -> &'static str {
    match self {
      Self::AxAction => "ax-action",
      Self::PointerClick => "pointer-click",
    }
  }

  fn cursor_disturbance(self) -> &'static str {
    match self {
      Self::AxAction => "none",
      Self::PointerClick => "warp-visible",
    }
  }

  fn press_mechanism(self) -> &'static str {
    self.as_str()
  }
}

/// Persisted record of which input method the macOS smart-press
/// pipeline picked for one operation, why it picked that method, and
/// whether the chosen path was a fallback from the primary AX action.
///
/// # Seam role
///
/// Upper / "what method got chosen" half of the v0 action-result pair
/// (per CLAUDE.md). Sibling: [`InputActionResult`] in
/// `crates/auv-driver/src/input.rs` (fully `pub`), which records the
/// actual delivery attempts that resulted from this decision.
///
/// - **Upstream**: typed macOS command handlers (e.g.
///   `debug.smartPress`) call into `crates/auv-driver`; the smart-
///   press recorder wraps that call to produce one of these decisions.
/// - **Downstream**: action-bearing operations attach this decision
///   (and the peer `InputActionResult`) to the resulting
///   `OperationResult` artifact (`src/contract.rs`); the
///   [`ActionResolverDecision::signals`] method flattens it into the
///   operation's signal map.
///
/// `pub(crate)` is intentional: this decision schema is private to the
/// `auv-cli` macOS driver subtree. Cross-crate consumers should observe
/// the decision indirectly through operation signals, not by importing
/// the struct directly. Per CLAUDE.md, this is one of the two action-
/// result schemas in v0; `InputActionResult` is the other. Do not
/// introduce a third action-result schema beside these two.
///
/// [`InputActionResult`]: auv_driver::InputActionResult
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ActionResolverDecision {
  pub(crate) version: &'static str,
  pub(crate) operation: &'static str,
  pub(crate) target_query: String,
  pub(crate) primary_method: &'static str,
  pub(crate) selected_method: &'static str,
  pub(crate) fallback_allowed: bool,
  pub(crate) fallback_used: bool,
  pub(crate) fallback_reason: Option<String>,
  pub(crate) policy: &'static str,
  pub(crate) cursor_disturbance: &'static str,
  pub(crate) press_mechanism: &'static str,
}

impl ActionResolverDecision {
  pub(crate) fn smart_press(
    query: &str,
    selected_method: ResolvedActionMethod,
    fallback_allowed: bool,
    primary_error: Option<&str>,
  ) -> Self {
    let fallback_used = selected_method != ResolvedActionMethod::AxAction;
    Self {
      version: ACTION_RESOLVER_VERSION,
      operation: "debug.smartPress",
      target_query: query.to_string(),
      primary_method: ResolvedActionMethod::AxAction.as_str(),
      selected_method: selected_method.as_str(),
      fallback_allowed,
      fallback_used,
      fallback_reason: fallback_used.then(|| {
        primary_error
          .map(render_resolver_value)
          .unwrap_or_else(|| "primary-method-failed".to_string())
      }),
      policy: if fallback_allowed {
        "ax-first-then-pointer-if-allowed"
      } else {
        "ax-only"
      },
      cursor_disturbance: selected_method.cursor_disturbance(),
      press_mechanism: selected_method.press_mechanism(),
    }
  }

  pub(crate) fn signals(&self) -> BTreeMap<String, String> {
    BTreeMap::from([
      (
        "actionResolver.version".to_string(),
        self.version.to_string(),
      ),
      (
        "actionResolver.operation".to_string(),
        self.operation.to_string(),
      ),
      (
        "actionResolver.target.query".to_string(),
        self.target_query.clone(),
      ),
      (
        "actionResolver.primaryMethod".to_string(),
        self.primary_method.to_string(),
      ),
      (
        "actionResolver.selectedMethod".to_string(),
        self.selected_method.to_string(),
      ),
      (
        "actionResolver.fallbackAllowed".to_string(),
        self.fallback_allowed.to_string(),
      ),
      (
        "actionResolver.fallbackUsed".to_string(),
        self.fallback_used.to_string(),
      ),
      (
        "actionResolver.fallbackReason".to_string(),
        self
          .fallback_reason
          .clone()
          .unwrap_or_else(|| "none".to_string()),
      ),
      ("actionResolver.policy".to_string(), self.policy.to_string()),
      (
        "actionResolver.cursorDisturbance".to_string(),
        self.cursor_disturbance.to_string(),
      ),
      (
        "actionResolver.pressMechanism".to_string(),
        self.press_mechanism.to_string(),
      ),
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
      format!(
        "actionResolverCursorDisturbance={}",
        self.cursor_disturbance
      ),
      format!("actionResolverPressMechanism={}", self.press_mechanism),
    ];
    if let Some(reason) = self.fallback_reason.as_deref() {
      notes.push(format!("actionResolverFallbackReason={reason}"));
    }
    notes
  }

  pub(crate) fn artifact(&self) -> AuvResult<ProducedArtifact> {
    let json = serde_json::to_string_pretty(self)
      .map_err(|error| format!("failed to serialize ActionResolver decision: {error}"))?;
    build_text_artifact(
      "action.resolver.decision",
      "json",
      &format!(
        "action-resolver-{}-{}",
        self.operation.replace('.', "-"),
        sanitize_file_component(&self.target_query)
      ),
      json + "\n",
      "Recorded ActionResolver v0 selected method, fallback policy, and disturbance metadata.",
    )
  }
}

fn render_resolver_value(raw: &str) -> String {
  raw
    .replace('\\', "\\\\")
    .replace('\n', "\\n")
    .replace('\r', "\\r")
    .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn smart_press_ax_decision_records_no_fallback() {
    let decision =
      ActionResolverDecision::smart_press("Run", ResolvedActionMethod::AxAction, true, None);

    assert_eq!(decision.version, ACTION_RESOLVER_VERSION);
    assert_eq!(decision.primary_method, "ax-action");
    assert_eq!(decision.selected_method, "ax-action");
    assert!(decision.fallback_allowed);
    assert!(!decision.fallback_used);
    assert_eq!(decision.fallback_reason, None);
    assert_eq!(decision.cursor_disturbance, "none");
    assert_eq!(decision.press_mechanism, "ax-action");

    let signals = decision.signals();
    assert_eq!(
      signals.get("actionResolver.selectedMethod"),
      Some(&"ax-action".to_string())
    );
    assert_eq!(
      signals.get("actionResolver.fallbackReason"),
      Some(&"none".to_string())
    );
  }

  #[test]
  fn smart_press_pointer_decision_records_reason_and_disturbance() {
    let decision = ActionResolverDecision::smart_press(
      "播放",
      ResolvedActionMethod::PointerClick,
      true,
      Some("AX target\nhad no action"),
    );

    assert_eq!(decision.selected_method, "pointer-click");
    assert!(decision.fallback_used);
    assert_eq!(
      decision.fallback_reason.as_deref(),
      Some("AX target\\nhad no action")
    );
    assert_eq!(decision.cursor_disturbance, "warp-visible");
    assert_eq!(decision.press_mechanism, "pointer-click");

    let notes = decision.notes();
    assert!(notes.contains(&"actionResolverSelectedMethod=pointer-click".to_string()));
    assert!(notes.contains(&"actionResolverFallbackUsed=true".to_string()));
    assert!(notes.contains(&"actionResolverFallbackReason=AX target\\nhad no action".to_string()));
  }
}
