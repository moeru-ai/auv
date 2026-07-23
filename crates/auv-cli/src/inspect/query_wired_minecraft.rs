//! Product-owned Minecraft query-wired projection over canonical artifacts.

use auv_driver::geometry::WindowPoint;
use auv_game_minecraft::inspect::read_minecraft_quality_spatial_inspection;
use auv_game_minecraft::{MinecraftArtifactReadError, TrainingResultSpatialQueryActionReadiness, derive_action_readiness};
use auv_tracing::{ArtifactUri, RunSnapshot, RunStore};

#[derive(Clone, Debug, PartialEq)]
pub struct MinecraftQueryWiredLiveActionSummary {
  query_artifact: ArtifactUri,
  readiness: TrainingResultSpatialQueryActionReadiness,
}

pub async fn collect_minecraft_query_wired_live_action_summaries(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<MinecraftQueryWiredLiveActionSummary>, MinecraftArtifactReadError> {
  let inspection = read_minecraft_quality_spatial_inspection(store, snapshot).await?;
  Ok(
    inspection
      .spatial_queries()
      .iter()
      .map(|query| MinecraftQueryWiredLiveActionSummary {
        query_artifact: query.artifact().uri.clone(),
        readiness: derive_action_readiness(&query.artifact().payload),
      })
      .collect(),
  )
}

pub fn append_minecraft_query_wired_section(output: &mut String, summaries: &[MinecraftQueryWiredLiveActionSummary]) {
  output.push_str("\nMC-19 Query Wired Live Action:\n");
  if summaries.is_empty() {
    output.push_str("- none\n");
    return;
  }

  for summary in summaries {
    // TODO(minecraft-query-wired-canonical-action-evidence): Action attempt, dispatch, and verification
    // fields remain omitted until typed canonical producers record them in the
    // same authority; never recover them from generic operation summaries.
    output.push_str(&format!(
      "- query_artifact={} operation_evidence=not_recorded attempted=n/a action_eligibility=n/a refusal_reason=n/a operation_status=n/a operation_message=n/a dispatch_command=n/a dispatch_outcome=n/a target_app=n/a target_title=n/a mc14_action_eligibility={} readiness_class={} window_point={} mc14_refusal_reason={} source_readiness=ready source_query_artifact={} verification_outcome=n/a verification_reason=n/a\n",
      summary.query_artifact,
      summary.readiness.eligibility.as_str(),
      auv_query_readiness::map_action_eligibility_to_readiness_class(summary.readiness.eligibility.as_str())
        .as_deref()
        .unwrap_or("n/a"),
      summary.readiness.window_point.map(window_point_label).unwrap_or_else(|| "n/a".to_string()),
      summary.readiness.refusal_reason.as_deref().unwrap_or("n/a"),
      summary.query_artifact,
    ));
  }
}

fn window_point_label(point: WindowPoint) -> String {
  format!("{},{}", point.0.x, point.0.y)
}
