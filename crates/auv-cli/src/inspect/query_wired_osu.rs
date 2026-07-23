//! Product-owned osu! query-wired presentation over canonical artifacts.

use auv_game_osu::run_read::{OsuArtifactReadError, extract_osu_visual_truth_spatial_query_manifests};
use auv_game_osu::{VisualTruthSpatialQueryActionReadiness, derive_visual_truth_spatial_query_action_readiness};
use auv_tracing::{ArtifactUri, RunSnapshot, RunStore};

#[derive(Clone, Debug, PartialEq)]
pub struct OsuQueryWiredLiveActionSummary {
  query_artifact: ArtifactUri,
  readiness: VisualTruthSpatialQueryActionReadiness,
}

pub async fn collect_osu_query_wired_live_action_summaries(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<OsuQueryWiredLiveActionSummary>, OsuArtifactReadError> {
  let queries = extract_osu_visual_truth_spatial_query_manifests(store, snapshot).await?;
  Ok(
    queries
      .into_iter()
      .map(|query| OsuQueryWiredLiveActionSummary {
        query_artifact: query.uri().clone(),
        readiness: derive_visual_truth_spatial_query_action_readiness(query.payload()),
      })
      .collect(),
  )
}

pub fn append_osu_query_wired_section(output: &mut String, summaries: &[OsuQueryWiredLiveActionSummary]) {
  output.push_str("\nOsu Visual Truth Query Wired Live Action:\n");
  if summaries.is_empty() {
    output.push_str("- none\n");
    return;
  }

  for summary in summaries {
    // TODO(run-contract-task-22): Operation attempt, dispatch, and verification
    // fields remain omitted until typed canonical producers record them in this
    // same RunSnapshot authority; do not recover them from legacy trace text.
    output.push_str(&format!(
      "- query_artifact={} operation_evidence=not_recorded action_eligibility={} readiness_class={} pixel_point={} refusal_reason={} source_readiness=ready source_query_artifact={} verification_evidence=not_recorded\n",
      summary.query_artifact,
      summary.readiness.eligibility.as_str(),
      auv_query_readiness::map_action_eligibility_to_readiness_class(summary.readiness.eligibility.as_str())
        .as_deref()
        .unwrap_or("n/a"),
      pixel_point_label(summary.readiness.pixel_point),
      summary.readiness.refusal_reason.as_deref().unwrap_or("n/a"),
      summary.query_artifact,
    ));
  }
}

fn pixel_point_label(point: Option<(f32, f32)>) -> String {
  point.map(|(x, y)| format!("{x},{y}")).unwrap_or_else(|| "n/a".to_string())
}
