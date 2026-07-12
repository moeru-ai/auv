//! Osu donor render sections for inspect output.

use super::*;

pub(super) fn append_sections(
  output: &mut String,
  osu_visual_truth_semantic_manifests: &[OsuVisualTruthSemanticManifestLineage],
  osu_visual_truth_semantic_inspect_reports: &[OsuVisualTruthSemanticInspectReportLineage],
  osu_visual_truth_spatial_query_manifests: &[OsuVisualTruthSpatialQueryManifestLineage],
  osu_visual_truth_spatial_query_inspect_reports: &[OsuVisualTruthSpatialQueryInspectReportLineage],
  osu_query_wired_live_action_summaries: &[OsuQueryWiredLiveActionSummary],
  osu_detection_eval_witness_manifests: &[OsuDetectionEvalWitnessManifestLineage],
  osu_detection_eval_witness_inspect_reports: &[OsuDetectionEvalWitnessInspectReportLineage],
  osu_detection_eval_quality_manifests: &[OsuDetectionEvalQualityManifestLineage],
  osu_detection_eval_quality_inspect_reports: &[OsuDetectionEvalQualityInspectReportLineage],
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

  output.push_str("\nOsu Visual Truth Query Wired Live Action:\n");
  if osu_query_wired_live_action_summaries.is_empty() {
    output.push_str("- none\n");
  } else {
    for summary in osu_query_wired_live_action_summaries {
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

  output.push_str("\nOsu Detection Eval Witness:\n");
  if osu_detection_eval_witness_manifests.is_empty() && osu_detection_eval_witness_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in osu_detection_eval_witness_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- witness_artifact={} status={} reason={} total_frames={} label_matched={} spatial_matched={} spatial_unscored={} spurious={} projection_kind={} frame_witness_count={} detector_model_id={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest.status,
          manifest.reason.as_deref().unwrap_or("n/a"),
          manifest.total_frames,
          manifest.label_matched_frames,
          manifest.spatial_matched_frames,
          manifest.spatial_unscored_frames,
          manifest.spurious_detection_count,
          manifest.projection_kind,
          manifest.frame_witness_count,
          manifest.detector_model_id.as_deref().unwrap_or("n/a"),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in osu_detection_eval_witness_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- witness_inspect_artifact={} status={} frame_witness_count={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.status,
          report.frame_witness_count,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nOsu Detection Eval Quality:\n");
  if osu_detection_eval_quality_manifests.is_empty() && osu_detection_eval_quality_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in osu_detection_eval_quality_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        let derived = derive_osu_detection_eval_quality_verdict_summary(manifest_lineage);
        output.push_str(&format!(
          "- quality_artifact={} witness_status={} status={} verdict={} label_recall={} spatial_recall={} spurious={} derived_verdict={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest.witness_status,
          manifest.status,
          manifest.verdict,
          manifest
            .label_recall
            .map(|value| format!("{value:.3}"))
            .unwrap_or_else(|| "n/a".to_string()),
          manifest
            .spatial_recall
            .map(|value| format!("{value:.3}"))
            .unwrap_or_else(|| "n/a".to_string()),
          manifest
            .spurious_detection_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          derived.verdict,
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in osu_detection_eval_quality_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- quality_inspect_artifact={} verdict={} label_recall_available={} spatial_recall_available={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.verdict,
          report.label_recall_available,
          report.spatial_recall_available,
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }
}
