//! Osu inspect fragment B: Detection Eval Witness + Quality.

use crate::detection_eval_quality::{DetectionEvalQualityManifest, OSU_DETECTION_EVAL_QUALITY_PURPOSE};
use crate::detection_eval_witness::{DetectionEvalWitnessManifest, OSU_DETECTION_EVAL_WITNESS_PURPOSE};
use crate::run_read::OsuInspectedArtifact;

pub(crate) fn append_sections_b(
  output: &mut String,
  osu_detection_eval_witness_manifests: &[OsuInspectedArtifact<DetectionEvalWitnessManifest>],
  osu_detection_eval_quality_manifests: &[OsuInspectedArtifact<DetectionEvalQualityManifest>],
) {
  output.push_str("\nOsu Detection Eval Witness:\n");
  if osu_detection_eval_witness_manifests.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in osu_detection_eval_witness_manifests {
      let manifest = artifact.payload();
      output.push_str(&format!(
        "- witness_uri={} purpose={} status={} reason={} total_frames={} label_matched={} spatial_matched={} spatial_unscored={} spurious={} projection_kind={} frame_witness_count={} detector_model_id={}\n",
        artifact.uri(),
        OSU_DETECTION_EVAL_WITNESS_PURPOSE,
        manifest.status.as_str(),
        manifest.reason.map(|reason| reason.as_str()).unwrap_or("n/a"),
        manifest.total_frames,
        manifest.label_matched_frames,
        manifest.spatial_matched_frames,
        manifest.spatial_unscored_frames,
        manifest.spurious_detection_count,
        manifest.projection_kind,
        manifest.frame_witnesses.len(),
        manifest.detector_model_id.as_deref().unwrap_or("n/a"),
      ));
    }
  }

  output.push_str("\nOsu Detection Eval Quality:\n");
  if osu_detection_eval_quality_manifests.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in osu_detection_eval_quality_manifests {
      let manifest = artifact.payload();
      output.push_str(&format!(
        "- quality_uri={} purpose={} witness_status={} status={} verdict={} label_recall={} spatial_recall={} spurious={} derived_verdict={}\n",
        artifact.uri(),
        OSU_DETECTION_EVAL_QUALITY_PURPOSE,
        manifest.witness_status.as_str(),
        manifest.status.as_str(),
        manifest.verdict.as_str(),
        manifest
          .metrics
          .as_ref()
          .and_then(|metrics| metrics.label_recall)
          .map(|value| format!("{value:.3}"))
          .unwrap_or_else(|| "n/a".to_string()),
        manifest
          .metrics
          .as_ref()
          .and_then(|metrics| metrics.spatial_recall)
          .map(|value| format!("{value:.3}"))
          .unwrap_or_else(|| "n/a".to_string()),
        manifest
          .metrics
          .as_ref()
          .map(|metrics| metrics.spurious_detection_count.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        manifest.verdict.as_str(),
      ));
    }
  }
}
