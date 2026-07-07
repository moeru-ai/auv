use std::path::Path;

use auv_driver::geometry::WindowPoint;

use crate::training_result_spatial_query::{
  TrainingResultSpatialQueryKind, TrainingResultSpatialQueryManifest, TrainingResultSpatialQueryStatus,
};
use crate::training_result_spatial_query_action::{
  TrainingResultSpatialQueryActionEligibility, TrainingResultSpatialQueryActionReadiness, derive_action_readiness,
};
use crate::types::BlockPosition;

pub const MC19_V1_D4_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT: &str =
  "mc19_v1_d4_query_wired_live_action_non_stub_click_no_gameplay_verification";

#[derive(Clone, Debug, PartialEq)]
pub struct QueryActionWiringLineage {
  pub manifest_path: String,
  pub training_result_semantic_manifest_path: String,
  pub query_kind: TrainingResultSpatialQueryKind,
  pub target_block: BlockPosition,
  pub status: TrainingResultSpatialQueryStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryActionWiringOutcome {
  pub attempted: bool,
  pub action_eligibility: TrainingResultSpatialQueryActionEligibility,
  pub refusal_reason: Option<String>,
  pub window_point: Option<WindowPoint>,
  pub click_summary: Option<String>,
  pub known_limits: Vec<String>,
}

pub trait QueryLiveClickExecutor {
  fn attempt_click(&self, window_point: WindowPoint, lineage: &QueryActionWiringLineage) -> Result<String, String>;
}

pub fn wire_query_manifest_to_action(
  manifest: &TrainingResultSpatialQueryManifest,
  lineage: &QueryActionWiringLineage,
  executor: &impl QueryLiveClickExecutor,
) -> QueryActionWiringOutcome {
  let readiness = derive_action_readiness(manifest);
  let mut known_limits = manifest.known_limits.clone();
  known_limits.push(MC19_V1_D4_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT.to_string());
  wire_readiness_to_action(&readiness, lineage, known_limits, executor)
}

fn wire_readiness_to_action(
  readiness: &TrainingResultSpatialQueryActionReadiness,
  lineage: &QueryActionWiringLineage,
  known_limits: Vec<String>,
  executor: &impl QueryLiveClickExecutor,
) -> QueryActionWiringOutcome {
  match readiness.eligibility {
    TrainingResultSpatialQueryActionEligibility::ClickReady => {
      let Some(window_point) = readiness.window_point else {
        return QueryActionWiringOutcome {
          attempted: false,
          action_eligibility: readiness.eligibility,
          refusal_reason: Some("click_ready eligibility missing window_point; defensive refusal".to_string()),
          window_point: None,
          click_summary: None,
          known_limits,
        };
      };

      match executor.attempt_click(window_point, lineage) {
        Ok(summary) => QueryActionWiringOutcome {
          attempted: true,
          action_eligibility: readiness.eligibility,
          refusal_reason: None,
          window_point: Some(window_point),
          click_summary: Some(summary),
          known_limits,
        },
        Err(message) => QueryActionWiringOutcome {
          attempted: true,
          action_eligibility: readiness.eligibility,
          refusal_reason: Some(message),
          window_point: Some(window_point),
          click_summary: None,
          known_limits,
        },
      }
    }
    TrainingResultSpatialQueryActionEligibility::AnswerNonClickable | TrainingResultSpatialQueryActionEligibility::NotConsumable => {
      QueryActionWiringOutcome {
        attempted: false,
        action_eligibility: readiness.eligibility,
        refusal_reason: readiness.refusal_reason.clone(),
        window_point: readiness.window_point,
        click_summary: None,
        known_limits,
      }
    }
  }
}

pub fn query_action_wiring_lineage_from_manifest(
  manifest: &TrainingResultSpatialQueryManifest,
  manifest_path: &Path,
) -> QueryActionWiringLineage {
  QueryActionWiringLineage {
    manifest_path: manifest_path.display().to_string(),
    training_result_semantic_manifest_path: manifest.training_result_semantic_manifest_path.clone(),
    query_kind: manifest.query_kind,
    target_block: manifest.target_block,
    status: manifest.status,
  }
}

#[cfg(test)]
mod tests {
  use std::cell::Cell;

  use auv_driver::geometry::Point;

  use super::*;
  use crate::training_result_spatial_query::TrainingResultSpatialQueryReason;
  use crate::types::{MinecraftTargetSemantics, ProjectionVisibility};

  struct CountingExecutor {
    calls: Cell<usize>,
    summary: Option<String>,
    error: Option<String>,
  }

  impl CountingExecutor {
    fn success(summary: impl Into<String>) -> Self {
      Self {
        calls: Cell::new(0),
        summary: Some(summary.into()),
        error: None,
      }
    }
  }

  impl QueryLiveClickExecutor for CountingExecutor {
    fn attempt_click(&self, _window_point: WindowPoint, _lineage: &QueryActionWiringLineage) -> Result<String, String> {
      self.calls.set(self.calls.get() + 1);
      if let Some(error) = &self.error {
        return Err(error.clone());
      }
      Ok(self.summary.clone().unwrap_or_else(|| "clicked".to_string()))
    }
  }

  fn base_manifest() -> TrainingResultSpatialQueryManifest {
    TrainingResultSpatialQueryManifest {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_semantic_manifest_path: "/tmp/semantic.json".to_string(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch.json".to_string(),
      source_training_package_manifest_path: "/tmp/package.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/normalized".to_string(),
      query_kind: TrainingResultSpatialQueryKind::BlockProjection,
      target_block: BlockPosition::new(511, 73, 728),
      target_face: None,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
      selected_backend: None,
      status: TrainingResultSpatialQueryStatus::Answered,
      reason: None,
      visibility: Some(ProjectionVisibility::Visible),
      screen_point: Some(Point::new(854.0, 480.0)),
      match_radius_px: Some(8.0),
      confidence: Some(0.9),
      basis_frame_id: Some("frame-1".to_string()),
      comparison_verdict: None,
      known_limits: vec!["upstream limit".to_string()],
    }
  }

  fn lineage_for(manifest: &TrainingResultSpatialQueryManifest) -> QueryActionWiringLineage {
    query_action_wiring_lineage_from_manifest(manifest, Path::new("/tmp/query.json"))
  }

  #[test]
  fn click_ready_invokes_executor_once() {
    let manifest = base_manifest();
    let lineage = lineage_for(&manifest);
    let executor = CountingExecutor::success("live click dispatched");

    let outcome = wire_query_manifest_to_action(&manifest, &lineage, &executor);

    assert_eq!(executor.calls.get(), 1);
    assert!(outcome.attempted);
    assert_eq!(outcome.action_eligibility, TrainingResultSpatialQueryActionEligibility::ClickReady);
    assert_eq!(outcome.window_point, Some(WindowPoint::new(854.0, 480.0)));
    assert_eq!(outcome.click_summary.as_deref(), Some("live click dispatched"));
    assert!(outcome.refusal_reason.is_none());
    assert!(outcome.known_limits.iter().any(|limit| limit == MC19_V1_D4_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT));
  }

  #[test]
  fn answer_non_clickable_refuses_without_executor() {
    let mut manifest = base_manifest();
    manifest.visibility = Some(ProjectionVisibility::OutsideWindow);
    manifest.screen_point = None;
    let lineage = lineage_for(&manifest);
    let executor = CountingExecutor::success("should not run");

    let outcome = wire_query_manifest_to_action(&manifest, &lineage, &executor);

    assert_eq!(executor.calls.get(), 0);
    assert!(!outcome.attempted);
    assert_eq!(outcome.action_eligibility, TrainingResultSpatialQueryActionEligibility::AnswerNonClickable);
    assert_eq!(outcome.refusal_reason.as_deref(), Some("visibility=outside_window"));
    assert!(outcome.click_summary.is_none());
  }

  #[test]
  fn not_consumable_refuses_without_executor() {
    let mut manifest = base_manifest();
    manifest.status = TrainingResultSpatialQueryStatus::Blocked;
    manifest.reason = Some(TrainingResultSpatialQueryReason::SemanticSourceNotReady);
    manifest.visibility = None;
    manifest.screen_point = None;
    let lineage = lineage_for(&manifest);
    let executor = CountingExecutor::success("should not run");

    let outcome = wire_query_manifest_to_action(&manifest, &lineage, &executor);

    assert_eq!(executor.calls.get(), 0);
    assert!(!outcome.attempted);
    assert_eq!(outcome.action_eligibility, TrainingResultSpatialQueryActionEligibility::NotConsumable);
    assert_eq!(outcome.refusal_reason.as_deref(), Some("status=blocked reason=semantic_source_not_ready"));
    assert!(outcome.click_summary.is_none());
  }

  #[test]
  fn click_ready_missing_window_point_defensively_refuses() {
    let readiness = TrainingResultSpatialQueryActionReadiness {
      eligibility: TrainingResultSpatialQueryActionEligibility::ClickReady,
      window_point: None,
      refusal_reason: None,
    };
    let manifest = base_manifest();
    let lineage = lineage_for(&manifest);
    let executor = CountingExecutor::success("should not run");

    let outcome =
      wire_readiness_to_action(&readiness, &lineage, vec![MC19_V1_D4_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT.to_string()], &executor);

    assert_eq!(executor.calls.get(), 0);
    assert!(!outcome.attempted);
    assert_eq!(outcome.action_eligibility, TrainingResultSpatialQueryActionEligibility::ClickReady);
    assert_eq!(outcome.refusal_reason.as_deref(), Some("click_ready eligibility missing window_point; defensive refusal"));
  }
}
