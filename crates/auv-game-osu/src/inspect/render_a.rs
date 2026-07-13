//! Osu inspect fragment A: Semantic, Spatial Query, Action Readiness.

use crate::run_read::{
  OsuVisualTruthSemanticInspectReportLineage, OsuVisualTruthSemanticManifestLineage, OsuVisualTruthSpatialQueryInspectReportLineage,
  OsuVisualTruthSpatialQueryManifestLineage, derive_osu_visual_truth_spatial_query_action_readiness,
};

pub(crate) fn append_sections_a(
  output: &mut String,
  osu_visual_truth_semantic_manifests: &[OsuVisualTruthSemanticManifestLineage],
  osu_visual_truth_semantic_inspect_reports: &[OsuVisualTruthSemanticInspectReportLineage],
  osu_visual_truth_spatial_query_manifests: &[OsuVisualTruthSpatialQueryManifestLineage],
  osu_visual_truth_spatial_query_inspect_reports: &[OsuVisualTruthSpatialQueryInspectReportLineage],
) {
  output.push_str("\nOsu Visual Truth Semantic:\n");
  if osu_visual_truth_semantic_manifests.is_empty() && osu_visual_truth_semantic_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in osu_visual_truth_semantic_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} semantic_status={} semantic_reason={} frame_count={} beatmap_path={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest.semantic_status,
          manifest.semantic_reason.as_deref().unwrap_or("n/a"),
          manifest.frame_count,
          manifest.beatmap_path,
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      } else {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} semantic_status=n/a issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in osu_visual_truth_semantic_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- inspect_artifact={} semantic_status={} projection_eval_ready={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.semantic_status,
          report.projection_eval_ready,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nOsu Visual Truth Spatial Query:\n");
  if osu_visual_truth_spatial_query_manifests.is_empty() && osu_visual_truth_spatial_query_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in osu_visual_truth_spatial_query_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- query_artifact={} object_index={} capture_phase={} status={} reason={} pixel_visibility={} pixel_point={} query_backend={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest.object_index,
          manifest.capture_phase,
          manifest.status,
          manifest.reason.as_deref().unwrap_or("n/a"),
          manifest.pixel_visibility.as_deref().unwrap_or("n/a"),
          match (manifest.pixel_x, manifest.pixel_y) {
            (Some(x), Some(y)) => format!("{x},{y}"),
            _ => "n/a".to_string(),
          },
          manifest.query_backend,
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in osu_visual_truth_spatial_query_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- inspect_artifact={} status={} semantic_status={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.status,
          report.semantic_status,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nOsu Visual Truth Spatial Query Action Readiness:\n");
  if osu_visual_truth_spatial_query_manifests.is_empty() {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in osu_visual_truth_spatial_query_manifests {
      let readiness = derive_osu_visual_truth_spatial_query_action_readiness(manifest_lineage);
      let manifest = manifest_lineage.manifest.as_ref();
      output.push_str(&format!(
        "- query_artifact={} status={} action_eligibility={} pixel_point={} refusal_reason={} issue={}\n",
        manifest_lineage.artifact.artifact_id,
        manifest.as_ref().map(|value| value.status.as_str()).unwrap_or("n/a"),
        readiness.action_eligibility,
        readiness.pixel_point.as_deref().unwrap_or("n/a"),
        readiness.refusal_reason.as_deref().unwrap_or("n/a"),
        readiness.issue.as_deref().unwrap_or("n/a"),
      ));
    }
  }
}
