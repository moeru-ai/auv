//! Product-owned minecraft query-wired inspect fragment (S3b bridge).
//!
//! NOTICE(inspect-composition / S3b): Render-only bridge over product
//! `MinecraftQueryWiredLiveActionSummary`. Stays in `auv-product` with the
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
        "- operation_result_artifact={} query_artifact={} attempted={} action_eligibility={} window_point={} refusal_reason={} operation_status={} operation_message={} dispatch_command={} dispatch_outcome={} target_app={} target_title={} mc14_action_eligibility={} readiness_class={} source_readiness_ref={} verification_outcome={} verification_source={} verification_reason={} issue={}\n",
        summary.operation_result_artifact_id.as_deref().unwrap_or("n/a"),
        summary.query_artifact_id.as_deref().unwrap_or("n/a"),
        summary.attempted,
        summary.action_eligibility,
        summary.window_point.as_deref().unwrap_or("n/a"),
        summary.refusal_reason.as_deref().unwrap_or("n/a"),
        summary.operation_status.as_deref().unwrap_or("n/a"),
        summary.operation_message.as_deref().unwrap_or("n/a"),
        summary.dispatch_command.as_deref().unwrap_or("n/a"),
        summary.dispatch_outcome.as_deref().unwrap_or("n/a"),
        summary.target_app.as_deref().unwrap_or("n/a"),
        summary.target_title.as_deref().unwrap_or("n/a"),
        summary.mc14_action_eligibility.as_deref().unwrap_or("n/a"),
        summary.readiness_class.as_deref().unwrap_or("n/a"),
        summary.source_readiness_ref.as_deref().unwrap_or("n/a"),
        summary.verification_outcome.as_str(),
        summary.verification_source.as_deref().unwrap_or("n/a"),
        summary.verification_reason.as_deref().unwrap_or("n/a"),
        summary.issue.as_deref().unwrap_or("n/a"),
      ));
    }
  }
}
