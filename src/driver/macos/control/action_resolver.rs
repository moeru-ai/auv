use super::super::ProducedArtifact;
use super::super::support::artifacts::{build_text_artifact, sanitize_file_component};
pub(crate) use crate::action_resolver_decision::{ActionResolverDecision, ResolvedActionMethod};
use crate::model::AuvResult;

impl ActionResolverDecision {
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
