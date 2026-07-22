//! Product-owned minecraft query-wired inspect fragment (S3b bridge).
//!
//! NOTICE(inspect-composition / S3b): Render-only bridge over product
//! `MinecraftQueryWiredLiveActionSummary`. Stays in the product CLI package with the
//! OperationResult adapter; do not move into `auv-game-minecraft` until
//! OperationResult (+ verification/failure) ownership is owner-approved.
//! TODO(inspect-composition / S3b): unlock full donor graduation with that
//! ownership move — not with inspect-model alone.

use crate::run_read::MinecraftQueryWiredLiveActionSummary;

pub fn append_minecraft_query_wired_section(
  output: &mut String,
  minecraft_query_wired_live_action_summaries: &[MinecraftQueryWiredLiveActionSummary],
) {
  output.push_str("\nMC-19 Query Wired Live Action:\n");
  if minecraft_query_wired_live_action_summaries.is_empty() {
    output.push_str("- none\n");
  } else {
    for summary in minecraft_query_wired_live_action_summaries {
      output.push_str(&format!(
        "- attempted={} action_eligibility={} refusal_reason={} dispatch_command={} dispatch_outcome={} target_app={} target_title={} verification_outcome={} verification_source={} verification_reason={}\n",
        summary.attempted,
        summary.action_eligibility,
        summary.refusal_reason.as_deref().unwrap_or("n/a"),
        summary.dispatch_command.as_deref().unwrap_or("n/a"),
        summary.dispatch_outcome.as_deref().unwrap_or("n/a"),
        summary.target_app.as_deref().unwrap_or("n/a"),
        summary.target_title.as_deref().unwrap_or("n/a"),
        summary.verification_outcome.as_str(),
        summary.verification_source.as_deref().unwrap_or("n/a"),
        summary.verification_reason.as_deref().unwrap_or("n/a"),
      ));
    }
  }
}
