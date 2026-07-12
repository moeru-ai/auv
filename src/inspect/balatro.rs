//! Balatro donor render sections for inspect output.

use super::*;

pub(super) fn append_sections(
  output: &mut String,
  balatro_card_detection_semantic_manifests: &[BalatroCardDetectionSemanticManifestLineage],
  balatro_card_detection_semantic_inspect_reports: &[BalatroCardDetectionSemanticInspectReportLineage],
  balatro_card_detection_spatial_query_manifests: &[BalatroCardDetectionSpatialQueryManifestLineage],
  balatro_card_detection_spatial_query_inspect_reports: &[BalatroCardDetectionSpatialQueryInspectReportLineage],
  balatro_card_detection_eval_witness_manifests: &[BalatroCardDetectionEvalWitnessManifestLineage],
  balatro_card_detection_eval_witness_inspect_reports: &[BalatroCardDetectionEvalWitnessInspectReportLineage],
  balatro_card_detection_quality_manifests: &[BalatroCardDetectionQualityManifestLineage],
  balatro_card_detection_quality_inspect_reports: &[BalatroCardDetectionQualityInspectReportLineage],
) {
  output.push_str("\nBalatro Card Detection Semantic:\n");
  if balatro_card_detection_semantic_manifests.is_empty() && balatro_card_detection_semantic_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in balatro_card_detection_semantic_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- manifest_artifact={} semantic_status={} semantic_reason={} ui_detection_count={} entities_detection_count={} frame_source={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest.semantic_status,
          manifest.semantic_reason.as_deref().unwrap_or("n/a"),
          manifest.ui_detection_count,
          manifest.entities_detection_count,
          manifest.frame_source,
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in balatro_card_detection_semantic_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- inspect_artifact={} semantic_status={} detection_bundle_readable={} detection_sets_non_empty={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.semantic_status,
          report.detection_bundle_readable,
          report.detection_sets_non_empty,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nBalatro Card Detection Spatial Query:\n");
  if balatro_card_detection_spatial_query_manifests.is_empty() && balatro_card_detection_spatial_query_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in balatro_card_detection_spatial_query_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- query_artifact={} target_slot={}:{} status={} reason={} pixel_point={} query_backend={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest.target_zone,
          manifest.target_index,
          manifest.status,
          manifest.reason.as_deref().unwrap_or("n/a"),
          match (manifest.pixel_x, manifest.pixel_y) {
            (Some(x), Some(y)) => format!("{x},{y}"),
            _ => "n/a".to_string(),
          },
          manifest.query_backend,
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in balatro_card_detection_spatial_query_inspect_reports {
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

  output.push_str("\nBalatro Card Detection Eval Witness:\n");
  if balatro_card_detection_eval_witness_manifests.is_empty() && balatro_card_detection_eval_witness_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in balatro_card_detection_eval_witness_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- witness_artifact={} status={} reason={} expected_slot_count={} scored_slot_count={} unscored_slot_count={} below_confidence_slot_count={} quality_backend={} semantic_manifest={} spatial_query_manifest={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest.status,
          manifest.reason.as_deref().unwrap_or("n/a"),
          manifest.expected_slot_count,
          manifest.scored_slot_count,
          manifest.unscored_slot_count,
          manifest.below_confidence_slot_count,
          manifest.quality_backend,
          manifest.card_detection_semantic_manifest_path,
          manifest.card_detection_spatial_query_manifest_path,
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in balatro_card_detection_eval_witness_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- witness_inspect_artifact={} status={} slot_score_count={} semantic_readable={} spatial_query_readable={} expected_slots_readable={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.status,
          report.slot_score_count,
          report.semantic_manifest_readable,
          report.spatial_query_manifest_readable,
          report.expected_slots_readable,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nBalatro Card Detection Quality:\n");
  if balatro_card_detection_quality_manifests.is_empty() && balatro_card_detection_quality_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in balatro_card_detection_quality_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- quality_artifact={} witness_status={} status={} verdict={} quality_backend={} expected_slot_count={} scored_slot_count={} unscored_slot_count={} slot_coverage_ratio={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest.witness_status,
          manifest.status,
          manifest.verdict,
          manifest.quality_backend.as_deref().unwrap_or("n/a"),
          manifest
            .expected_slot_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          manifest
            .scored_slot_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          manifest
            .unscored_slot_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          manifest
            .slot_coverage_ratio
            .map(|value| format!("{value:.3}"))
            .unwrap_or_else(|| "n/a".to_string()),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in balatro_card_detection_quality_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- quality_inspect_artifact={} verdict={} quality_backend={} slot_coverage_ratio_available={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.verdict,
          report.quality_backend.as_deref().unwrap_or("n/a"),
          report.slot_coverage_ratio_available,
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }
}
