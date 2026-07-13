//! Shared helpers for Minecraft inspect fragment rendering.

use crate::run_read::MinecraftTrainingResultQualityVerdictSummary;

pub(crate) fn holdout_preview_manifest_matches_report(
  manifest: &crate::run_read::MinecraftTrainingResultHoldoutPreviewManifestSummary,
  report: &crate::run_read::MinecraftTrainingResultHoldoutPreviewInspectReportSummary,
) -> bool {
  report.training_result_semantic_manifest_path == manifest.training_result_semantic_manifest_path
    && report.source_training_result_artifact_manifest_path == manifest.source_training_result_artifact_manifest_path
    && report.source_training_result_manifest_path == manifest.source_training_result_manifest_path
    && report.source_training_job_manifest_path == manifest.source_training_job_manifest_path
    && report.source_training_launch_plan_path == manifest.source_training_launch_plan_path
    && report.source_scene_packet_manifest_path == manifest.source_scene_packet_manifest_path
    && report.source_run_ids == manifest.source_run_ids
}

pub(crate) fn holdout_render_quality_manifest_matches_report(
  manifest: &crate::run_read::MinecraftHoldoutRenderQualityManifestSummary,
  report: &crate::run_read::MinecraftHoldoutRenderQualityInspectReportSummary,
) -> bool {
  report.training_result_semantic_manifest_path == manifest.training_result_semantic_manifest_path
    && report.holdout_preview_manifest_path == manifest.holdout_preview_manifest_path
    && report.source_training_result_artifact_manifest_path == manifest.source_training_result_artifact_manifest_path
    && report.source_training_result_manifest_path == manifest.source_training_result_manifest_path
    && report.source_training_job_manifest_path == manifest.source_training_job_manifest_path
    && report.source_scene_packet_manifest_path == manifest.source_scene_packet_manifest_path
    && report.source_run_ids == manifest.source_run_ids
}

pub(crate) fn spatial_query_manifest_matches_report(
  manifest: &crate::run_read::MinecraftTrainingResultSpatialQueryManifestSummary,
  report: &crate::run_read::MinecraftTrainingResultSpatialQueryInspectReportSummary,
) -> bool {
  report.training_result_semantic_manifest_path == manifest.training_result_semantic_manifest_path
    && report.source_training_result_artifact_manifest_path == manifest.source_training_result_artifact_manifest_path
    && report.source_training_result_manifest_path == manifest.source_training_result_manifest_path
    && report.source_training_job_manifest_path == manifest.source_training_job_manifest_path
    && report.source_training_launch_plan_path == manifest.source_training_launch_plan_path
    && report.source_scene_packet_manifest_path == manifest.source_scene_packet_manifest_path
    && report.source_run_ids == manifest.source_run_ids
    && report.query_kind == manifest.query_kind
    && report.target_block == manifest.target_block
    && report.target_face == manifest.target_face
    && report.target_semantics == manifest.target_semantics
}

pub(crate) fn unique_matching_report<T>(reports: &[T], mut matches: impl FnMut(&T) -> bool) -> Option<&T> {
  let mut iter = reports.iter().filter(|report| matches(report));
  let first = iter.next()?;
  if iter.next().is_some() {
    None
  } else {
    Some(first)
  }
}

pub(crate) fn format_quality_verdict_stage_summary(verdict: &MinecraftTrainingResultQualityVerdictSummary) -> String {
  verdict
    .stage_checks
    .iter()
    .map(|check| {
      let reason = check.reasons.first().map(|value| format!(" reason={value}")).unwrap_or_default();
      format!("{}={}{}", check.stage, check.outcome, reason)
    })
    .collect::<Vec<_>>()
    .join(" ")
}

pub(crate) fn format_quality_verdict_line(verdict: &MinecraftTrainingResultQualityVerdictSummary) -> String {
  let mut line = format!(
    "- profile_id={} render_evidence_mode={} evidence_coverage={} quality_verdict={} {} issue={}
",
    verdict.profile_id,
    verdict.render_evidence_mode,
    verdict.evidence_coverage,
    verdict.quality_verdict,
    format_quality_verdict_stage_summary(verdict),
    verdict.issue.as_deref().unwrap_or("n/a"),
  );
  if !verdict.trust_notes.is_empty() {
    line.push_str(&format!(
      "  trust_notes={}
",
      verdict.trust_notes.join(" | ")
    ));
  }
  line
}

pub(crate) fn render_projection_visibility(visibility: &crate::types::ProjectionVisibility) -> &'static str {
  match visibility {
    crate::types::ProjectionVisibility::Visible => "visible",
    crate::types::ProjectionVisibility::BehindCamera => "behind_camera",
    crate::types::ProjectionVisibility::OutOfFrustum => "out_of_frustum",
    crate::types::ProjectionVisibility::OutsideWindow => "outside_window",
  }
}

pub(crate) fn render_minecraft_projected_point(projected_point: Option<&crate::types::MinecraftProjectedPoint>) -> String {
  match projected_point {
    Some(projected_point) => {
      let screen_point =
        projected_point.screen_point.as_ref().map(|point| format!("{},{}", point.x, point.y)).unwrap_or_else(|| "n/a".to_string());
      format!(
        "screen={} visibility={} radius_px={} confidence={} basis={}",
        screen_point,
        render_projection_visibility(&projected_point.visibility),
        projected_point.match_radius_px,
        projected_point.confidence,
        projected_point.basis_frame_id,
      )
    }
    None => "n/a".to_string(),
  }
}

pub(crate) fn render_training_compatibility_status(status: &crate::TrainingCompatibilityStatus) -> &'static str {
  match status {
    crate::TrainingCompatibilityStatus::Ready => "ready",
    crate::TrainingCompatibilityStatus::Partial => "partial",
    crate::TrainingCompatibilityStatus::Blocked => "blocked",
  }
}
