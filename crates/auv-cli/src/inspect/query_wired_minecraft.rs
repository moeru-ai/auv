//! Product-owned Minecraft query-wired projection over canonical artifacts.

use auv_driver::geometry::WindowPoint;
use auv_game_minecraft::inspect::read_minecraft_quality_spatial_inspection;
use auv_game_minecraft::{
  MinecraftArtifactReadError, QueryActionWiringOutcome, TrainingResultSpatialQueryActionReadiness, TrainingResultSpatialQueryManifest,
  derive_action_readiness,
};
use auv_runtime::contract::{OperationOutput, OperationResult, OperationStatus, VerificationResult};
use auv_tracing::{ArtifactUri, RunSnapshot, RunStore};

#[derive(Clone, Debug, PartialEq)]
enum MinecraftLiveActionEvidence {
  NotRecorded,
  Recorded(QueryActionWiringOutcome),
}

#[derive(Clone, Debug, PartialEq)]
enum MinecraftOperationEvidence {
  NotRecorded,
  Recorded {
    status: OperationStatus,
    message: Option<String>,
    verifications: Vec<VerificationResult>,
  },
}

#[derive(Clone, Debug, PartialEq)]
pub struct MinecraftQueryWiredLiveActionSummary {
  query_artifact: ArtifactUri,
  readiness: TrainingResultSpatialQueryActionReadiness,
  action: MinecraftLiveActionEvidence,
  operation: MinecraftOperationEvidence,
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
      .map(|query| {
        // TODO(run-contract-task-22): Canonical operation/wiring facts are
        // intentionally absent until their direct producers migrate; do not
        // reconstruct them from retired operation-result roles or trace text.
        project_minecraft_query_wired_live_action_summary(query.artifact().uri.clone(), &query.artifact().payload, None, None)
      })
      .collect(),
  )
}

fn project_minecraft_query_wired_live_action_summary(
  query_artifact: ArtifactUri,
  manifest: &TrainingResultSpatialQueryManifest,
  wiring: Option<&QueryActionWiringOutcome>,
  operation: Option<&OperationResult>,
) -> MinecraftQueryWiredLiveActionSummary {
  let action = wiring.cloned().map(MinecraftLiveActionEvidence::Recorded).unwrap_or(MinecraftLiveActionEvidence::NotRecorded);
  let operation = operation
    .map(|operation| MinecraftOperationEvidence::Recorded {
      status: operation.status,
      message: match &operation.output {
        OperationOutput::Acknowledged { message } => message.clone(),
        _ => None,
      },
      verifications: crate::run_read::operation_result_verification_claims(operation).into_iter().cloned().collect(),
    })
    .unwrap_or(MinecraftOperationEvidence::NotRecorded);
  MinecraftQueryWiredLiveActionSummary {
    query_artifact,
    readiness: derive_action_readiness(manifest),
    action,
    operation,
  }
}

pub fn append_minecraft_query_wired_section(
  output: &mut String,
  minecraft_query_wired_live_action_summaries: &[MinecraftQueryWiredLiveActionSummary],
) {
  output.push_str("\nMC-19 Query Wired Live Action:\n");
  if minecraft_query_wired_live_action_summaries.is_empty() {
    output.push_str("- none\n");
  } else {
    for summary in minecraft_query_wired_live_action_summaries {
      let wiring = match &summary.action {
        MinecraftLiveActionEvidence::NotRecorded => None,
        MinecraftLiveActionEvidence::Recorded(wiring) => Some(wiring),
      };
      let (operation_status, operation_message, verifications) = match &summary.operation {
        MinecraftOperationEvidence::NotRecorded => (None, None, &[][..]),
        MinecraftOperationEvidence::Recorded {
          status,
          message,
          verifications,
        } => (Some(*status), message.as_deref(), verifications.as_slice()),
      };
      let (verification_outcome, verification_reason) = verification_projection(verifications);
      output.push_str(&format!(
        "- query_artifact={} operation_evidence={} attempted={} action_eligibility={} refusal_reason={} operation_status={} operation_message={} dispatch_command={} dispatch_outcome={} target_app={} target_title={} mc14_action_eligibility={} readiness_class={} window_point={} mc14_refusal_reason={} source_readiness=ready source_query_artifact={} verification_outcome={} verification_reason={}\n",
        summary.query_artifact,
        operation_evidence_label(&summary.operation),
        optional_bool(wiring.map(|wiring| wiring.attempted)),
        wiring.map(|wiring| wiring.action_eligibility.as_str()).unwrap_or("n/a"),
        wiring.and_then(|wiring| wiring.refusal_reason.as_deref()).unwrap_or("n/a"),
        operation_status.map(operation_status_label).unwrap_or("n/a"),
        operation_message.unwrap_or("n/a"),
        "n/a",
        wiring.and_then(|wiring| wiring.click_summary.as_deref()).unwrap_or("n/a"),
        "n/a",
        "n/a",
        summary.readiness.eligibility.as_str(),
        auv_query_readiness::map_action_eligibility_to_readiness_class(summary.readiness.eligibility.as_str())
          .as_deref()
          .unwrap_or("n/a"),
        summary.readiness.window_point.map(window_point_label).unwrap_or_else(|| "n/a".to_string()),
        summary.readiness.refusal_reason.as_deref().unwrap_or("n/a"),
        summary.query_artifact,
        verification_outcome,
        verification_reason.as_deref().unwrap_or("n/a"),
      ));
    }
  }
}

fn operation_evidence_label(operation: &MinecraftOperationEvidence) -> &'static str {
  match operation {
    MinecraftOperationEvidence::NotRecorded => "not_recorded",
    MinecraftOperationEvidence::Recorded { .. } => "recorded",
  }
}

fn optional_bool(value: Option<bool>) -> &'static str {
  match value {
    Some(true) => "true",
    Some(false) => "false",
    None => "n/a",
  }
}

fn window_point_label(point: WindowPoint) -> String {
  format!("{},{}", point.0.x, point.0.y)
}

fn operation_status_label(status: OperationStatus) -> &'static str {
  match status {
    OperationStatus::Completed => "completed",
    OperationStatus::Failed => "failed",
  }
}

fn verification_projection(verifications: &[VerificationResult]) -> (&'static str, Option<String>) {
  if verifications.is_empty() {
    return ("n/a", None);
  }
  let claims = verifications.iter().collect::<Vec<_>>();
  let (outcome, reason) = crate::run_read::project_verification_outcome_from_claims(&claims);
  let outcome = match outcome.as_str() {
    "unreliable" => "unreliable",
    "failed" => "failed",
    "activation_only" => "activation_only",
    "passed" => "passed",
    "inconclusive" => "inconclusive",
    _ => "absent",
  };
  (outcome, reason)
}

#[cfg(test)]
mod tests {
  use auv_driver::geometry::WindowPoint;
  use auv_game_minecraft::{QueryActionWiringOutcome, TrainingResultSpatialQueryActionEligibility, TrainingResultSpatialQueryManifest};
  use auv_runtime::contract::{OPERATION_RESULT_API_VERSION, OperationOutput, OperationResult, OperationStatus};
  use auv_tracing::{ArtifactId, ArtifactUri, RunId};
  use serde_json::json;

  use super::*;

  #[test]
  fn direct_domain_projection_preserves_operation_and_wiring_facts() {
    let manifest: TrainingResultSpatialQueryManifest = serde_json::from_value(json!({
      "schema_version": 1,
      "generated_at_millis": 20,
      "training_result_semantic_manifest_path": "semantic.json",
      "source_training_result_artifact_manifest_path": "result-artifacts.json",
      "source_training_result_manifest_path": "result.json",
      "source_training_job_manifest_path": "job.json",
      "source_training_launch_plan_path": "launch.json",
      "source_training_package_manifest_path": "package.json",
      "source_scene_packet_manifest_path": "scene.json",
      "source_bundle_manifest_paths": ["bundle.json"],
      "source_run_ids": ["run-source"],
      "trainer_backend": "nerfstudio",
      "job_backend": "fixture",
      "normalized_result_dir": "normalized",
      "query_kind": "block_projection",
      "target_block": {"x": 511, "y": 73, "z": 728},
      "target_semantics": "hit_face_center",
      "selected_backend": "projection_reference",
      "status": "answered",
      "visibility": "visible",
      "screen_point": {"x": 12.0, "y": 34.0},
      "match_radius_px": 8.0,
      "confidence": 1.0,
      "basis_frame_id": "frame-20",
      "comparison_verdict": "match",
      "known_limits": []
    }))
    .expect("typed spatial query");
    let wiring = QueryActionWiringOutcome {
      attempted: true,
      action_eligibility: TrainingResultSpatialQueryActionEligibility::ClickReady,
      refusal_reason: None,
      window_point: Some(WindowPoint::new(12.0, 34.0)),
      click_summary: Some("live click dispatched".to_string()),
      known_limits: Vec::new(),
    };
    let operation = OperationResult {
      api_version: OPERATION_RESULT_API_VERSION.to_string(),
      run_id: auv_tracing_driver::trace::RunId::new("run_direct_projection"),
      status: OperationStatus::Completed,
      operation_id: "auv.minecraft.query_wired_live_action".to_string(),
      evidence_artifacts: Vec::new(),
      output: OperationOutput::Acknowledged {
        message: Some("operation completed".to_string()),
      },
      verifications: Vec::new(),
      freshness_basis: None,
      known_limits: Vec::new(),
    };
    let summary = project_minecraft_query_wired_live_action_summary(
      ArtifactUri::from_ids(RunId::new(), ArtifactId::new()),
      &manifest,
      Some(&wiring),
      Some(&operation),
    );
    let mut text = String::new();
    append_minecraft_query_wired_section(&mut text, &[summary]);

    assert!(text.contains("operation_evidence=recorded attempted=true action_eligibility=click_ready"));
    assert!(text.contains("operation_status=completed operation_message=operation completed"));
    assert!(text.contains("dispatch_outcome=live click dispatched"));
  }
}
