//! Balatro inspect text fragments rendered from canonical typed artifacts.

use auv_tracing::ArtifactUri;

use crate::{
  CardDetectionEvalWitnessManifest, CardDetectionQualityManifest, CardDetectionSemanticManifest, CardDetectionSpatialQueryManifest,
};

pub(crate) fn append_sections(
  output: &mut String,
  semantic_manifests: &[(ArtifactUri, CardDetectionSemanticManifest)],
  spatial_query_manifests: &[(ArtifactUri, CardDetectionSpatialQueryManifest)],
  witness_manifests: &[(ArtifactUri, CardDetectionEvalWitnessManifest)],
  quality_manifests: &[(ArtifactUri, CardDetectionQualityManifest)],
) {
  output.push_str("\nBalatro Card Detection Semantic:\n");
  if semantic_manifests.is_empty() {
    output.push_str("- none\n");
  } else {
    for (uri, manifest) in semantic_manifests {
      output.push_str(&format!(
        "- manifest_artifact={uri} semantic_status={} semantic_reason={} ui_detection_count={} entities_detection_count={} frame_source={}\n",
        manifest.semantic_status,
        manifest.semantic_reason.map(|reason| reason.as_str()).unwrap_or("n/a"),
        manifest.ui_detection_count,
        manifest.entities_detection_count,
        manifest.frame_source,
      ));
    }
  }

  output.push_str("\nBalatro Card Detection Spatial Query:\n");
  if spatial_query_manifests.is_empty() {
    output.push_str("- none\n");
  } else {
    for (uri, manifest) in spatial_query_manifests {
      output.push_str(&format!(
        "- query_artifact={uri} target_slot={}:{} status={} reason={} pixel_point={} query_backend={}\n",
        manifest.target_zone,
        manifest.target_index,
        manifest.status.as_str(),
        manifest.reason.map(|reason| reason.as_str()).unwrap_or("n/a"),
        match (manifest.pixel_x, manifest.pixel_y) {
          (Some(x), Some(y)) => format!("{x},{y}"),
          _ => "n/a".to_string(),
        },
        manifest.query_backend.as_str(),
      ));
    }
  }

  output.push_str("\nBalatro Card Detection Eval Witness:\n");
  if witness_manifests.is_empty() {
    output.push_str("- none\n");
  } else {
    for (uri, manifest) in witness_manifests {
      output.push_str(&format!(
        "- witness_artifact={uri} status={} reason={} expected_slot_count={} scored_slot_count={} unscored_slot_count={} below_confidence_slot_count={} quality_backend={} semantic_manifest={} spatial_query_manifest={}\n",
        manifest.status,
        manifest.reason.map(|reason| reason.as_str()).unwrap_or("n/a"),
        manifest.expected_slot_count,
        manifest.scored_slot_count,
        manifest.unscored_slot_count,
        manifest.below_confidence_slot_count,
        manifest.quality_backend.as_str(),
        manifest.card_detection_semantic_manifest_path,
        manifest.card_detection_spatial_query_manifest_path,
      ));
    }
  }

  output.push_str("\nBalatro Card Detection Quality:\n");
  if quality_manifests.is_empty() {
    output.push_str("- none\n");
  } else {
    for (uri, manifest) in quality_manifests {
      let metrics = manifest.metrics.as_ref();
      output.push_str(&format!(
        "- quality_artifact={uri} witness_status={} status={} verdict={} quality_backend={} expected_slot_count={} scored_slot_count={} unscored_slot_count={} slot_coverage_ratio={}\n",
        manifest.witness_status,
        manifest.status,
        manifest.verdict.as_str(),
        manifest
          .quality_backend
          .map(|backend| backend.as_str())
          .unwrap_or("n/a"),
        metrics
          .map(|value| value.expected_slot_count.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        metrics
          .map(|value| value.scored_slot_count.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        metrics
          .map(|value| value.unscored_slot_count.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        metrics
          .and_then(|value| value.slot_coverage_ratio)
          .map(|value| format!("{value:.3}"))
          .unwrap_or_else(|| "n/a".to_string()),
      ));
    }
  }
}
