//! Osu inspect fragment A: Semantic, Spatial Query, Action Readiness.

use crate::run_read::OsuInspectedArtifact;
use crate::visual_truth_semantic::{OSU_VISUAL_TRUTH_SEMANTIC_PURPOSE, VisualTruthSemanticManifest};
use crate::visual_truth_spatial_query::{OSU_VISUAL_TRUTH_SPATIAL_QUERY_PURPOSE, VisualTruthSpatialQueryManifest};
use crate::{CapturePhase, derive_visual_truth_spatial_query_action_readiness};

pub(crate) fn append_sections_a(
  output: &mut String,
  osu_visual_truth_semantic_manifests: &[OsuInspectedArtifact<VisualTruthSemanticManifest>],
  osu_visual_truth_spatial_query_manifests: &[OsuInspectedArtifact<VisualTruthSpatialQueryManifest>],
) {
  output.push_str("\nOsu Visual Truth Semantic:\n");
  if osu_visual_truth_semantic_manifests.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in osu_visual_truth_semantic_manifests {
      let manifest = artifact.payload();
      output.push_str(&format!(
        "- manifest_uri={} purpose={} semantic_status={} semantic_reason={} frame_count={} beatmap_path={}\n",
        artifact.uri(),
        OSU_VISUAL_TRUTH_SEMANTIC_PURPOSE,
        manifest.semantic_status.as_str(),
        manifest.semantic_reason.map(|reason| reason.as_str()).unwrap_or("n/a"),
        manifest.frame_count,
        manifest.beatmap_path,
      ));
    }
  }

  output.push_str("\nOsu Visual Truth Spatial Query:\n");
  if osu_visual_truth_spatial_query_manifests.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in osu_visual_truth_spatial_query_manifests {
      let manifest = artifact.payload();
      output.push_str(&format!(
        "- query_uri={} purpose={} object_index={} capture_phase={} status={} reason={} pixel_visibility={} pixel_point={} query_backend={}\n",
        artifact.uri(),
        OSU_VISUAL_TRUTH_SPATIAL_QUERY_PURPOSE,
        manifest.object_index,
        capture_phase_label(&manifest.capture_phase),
        manifest.status.as_str(),
        manifest.reason.map(|reason| reason.as_str()).unwrap_or("n/a"),
        manifest.pixel_visibility.map(|visibility| visibility.as_str()).unwrap_or("n/a"),
        pixel_point_label(manifest.pixel_x.zip(manifest.pixel_y)),
        manifest.query_backend.as_str(),
      ));
    }
  }

  output.push_str("\nOsu Visual Truth Spatial Query Action Readiness:\n");
  if osu_visual_truth_spatial_query_manifests.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in osu_visual_truth_spatial_query_manifests {
      let manifest = artifact.payload();
      let readiness = derive_visual_truth_spatial_query_action_readiness(manifest);
      output.push_str(&format!(
        "- query_uri={} purpose={} status={} action_eligibility={} pixel_point={} refusal_reason={}\n",
        artifact.uri(),
        OSU_VISUAL_TRUTH_SPATIAL_QUERY_PURPOSE,
        manifest.status.as_str(),
        readiness.eligibility.as_str(),
        pixel_point_label(readiness.pixel_point),
        readiness.refusal_reason.as_deref().unwrap_or("n/a"),
      ));
    }
  }
}

fn capture_phase_label(phase: &CapturePhase) -> &'static str {
  match phase {
    CapturePhase::BeforeDispatch => "before_dispatch",
    CapturePhase::AfterDispatch => "after_dispatch",
  }
}

fn pixel_point_label(point: Option<(f32, f32)>) -> String {
  point.map(|(x, y)| format!("{x},{y}")).unwrap_or_else(|| "n/a".to_string())
}
