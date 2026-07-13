//! Product-owned osu query-wired inspect fragment (S3b bridge).
//!
//! NOTICE(inspect-composition / S3b): Render-only bridge over product
//! `OsuQueryWiredLiveActionSummary`. Stays in the product CLI package with the
//! OperationResult adapter; do not move into `auv-game-osu` until
//! OperationResult (+ verification/failure) ownership is owner-approved.
//! TODO(inspect-composition / S3b): unlock full donor graduation with that
//! ownership move — not with inspect-model alone.

use crate::run_read::OsuQueryWiredLiveActionSummary;

pub fn append_osu_query_wired_section(output: &mut String, summaries: &[OsuQueryWiredLiveActionSummary]) {
  output.push_str("\nOsu Visual Truth Query Wired Live Action:\n");
  if summaries.is_empty() {
    output.push_str("- none\n");
  } else {
    for summary in summaries {
      output.push_str(&format!(
        "- operation_result_artifact={} query_artifact={} attempted={} action_eligibility={} pixel_point={} window_point={} refusal_reason={} operation_status={} operation_message={} dispatch_command={} dispatch_outcome={} target_app={} target_title={} readiness_class={} source_readiness_ref={} verification_outcome={} verification_source={} verification_reason={} issue={}\n",
        summary.operation_result_artifact_id.as_deref().unwrap_or("n/a"),
        summary.query_artifact_id.as_deref().unwrap_or("n/a"),
        summary.attempted,
        summary.action_eligibility,
        summary.pixel_point.as_deref().unwrap_or("n/a"),
        summary.window_point.as_deref().unwrap_or("n/a"),
        summary.refusal_reason.as_deref().unwrap_or("n/a"),
        summary.operation_status.as_deref().unwrap_or("n/a"),
        summary.operation_message.as_deref().unwrap_or("n/a"),
        summary.dispatch_command.as_deref().unwrap_or("n/a"),
        summary.dispatch_outcome.as_deref().unwrap_or("n/a"),
        summary.target_app.as_deref().unwrap_or("n/a"),
        summary.target_title.as_deref().unwrap_or("n/a"),
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
