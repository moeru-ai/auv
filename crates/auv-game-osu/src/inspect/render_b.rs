//! Osu inspect fragment B: Detection Eval Witness + Quality.

use crate::run_read::{
  OsuDetectionEvalQualityInspectReportLineage, OsuDetectionEvalQualityManifestLineage, OsuDetectionEvalWitnessInspectReportLineage,
  OsuDetectionEvalWitnessManifestLineage, derive_osu_detection_eval_quality_verdict_summary,
};

pub(crate) fn append_sections_b(
  output: &mut String,
  osu_detection_eval_witness_manifests: &[OsuDetectionEvalWitnessManifestLineage],
  osu_detection_eval_witness_inspect_reports: &[OsuDetectionEvalWitnessInspectReportLineage],
  osu_detection_eval_quality_manifests: &[OsuDetectionEvalQualityManifestLineage],
  osu_detection_eval_quality_inspect_reports: &[OsuDetectionEvalQualityInspectReportLineage],
) {
  output.push_str("\nOsu Detection Eval Witness:\n");
  if osu_detection_eval_witness_manifests.is_empty() && osu_detection_eval_witness_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in osu_detection_eval_witness_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- witness_uri={} purpose={} status={} reason={} total_frames={} label_matched={} spatial_matched={} spatial_unscored={} spurious={} projection_kind={} frame_witness_count={} detector_model_id={} issue={}\n",
          manifest_lineage.uri,
          manifest_lineage.purpose,
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
          "- witness_inspect_uri={} purpose={} status={} frame_witness_count={} warnings={} issue={}\n",
          report_lineage.uri,
          report_lineage.purpose,
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
          "- quality_uri={} purpose={} witness_status={} status={} verdict={} label_recall={} spatial_recall={} spurious={} derived_verdict={} issue={}\n",
          manifest_lineage.uri,
          manifest_lineage.purpose,
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
          derived,
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in osu_detection_eval_quality_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- quality_inspect_uri={} purpose={} verdict={} label_recall_available={} spatial_recall_available={} issue={}\n",
          report_lineage.uri,
          report_lineage.purpose,
          report.verdict,
          report.label_recall_available,
          report.spatial_recall_available,
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }
}
