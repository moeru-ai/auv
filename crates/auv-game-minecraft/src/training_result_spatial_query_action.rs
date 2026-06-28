use auv_driver::geometry::WindowPoint;
use auv_query_readiness::{DerivedActionReadiness, format_query_not_consumable_refusal};

use crate::input_target::projected_window_point;
use crate::training_result_spatial_query::{
  TrainingResultSpatialQueryManifest, TrainingResultSpatialQueryStatus,
};
use crate::types::{MinecraftProjectedPoint, ProjectionVisibility};

pub type TrainingResultSpatialQueryActionEligibility =
  auv_query_readiness::DerivedActionEligibility;

#[derive(Clone, Debug, PartialEq)]
pub struct TrainingResultSpatialQueryActionReadiness {
  pub eligibility: TrainingResultSpatialQueryActionEligibility,
  pub window_point: Option<WindowPoint>,
  pub refusal_reason: Option<String>,
}

pub fn derive_action_readiness(
  manifest: &TrainingResultSpatialQueryManifest,
) -> TrainingResultSpatialQueryActionReadiness {
  if manifest.status != TrainingResultSpatialQueryStatus::Answered {
    let derived = DerivedActionReadiness::not_consumable(format_query_not_consumable_refusal(
      manifest.status.as_str(),
      manifest.reason.map(|reason| reason.as_str()),
    ));
    return TrainingResultSpatialQueryActionReadiness {
      eligibility: derived.eligibility,
      window_point: None,
      refusal_reason: derived.refusal_reason,
    };
  }

  let Some(visibility) = manifest.visibility else {
    let derived =
      DerivedActionReadiness::answer_non_clickable("answered query missing visibility witness");
    return TrainingResultSpatialQueryActionReadiness {
      eligibility: derived.eligibility,
      window_point: None,
      refusal_reason: derived.refusal_reason,
    };
  };

  let projected = MinecraftProjectedPoint {
    screen_point: manifest.screen_point,
    visibility,
    match_radius_px: manifest.match_radius_px.unwrap_or(8.0),
    basis_frame_id: manifest
      .basis_frame_id
      .clone()
      .unwrap_or_else(|| "unknown".to_string()),
    confidence: manifest.confidence.unwrap_or(0.0),
  };

  if let Some(window_point) = projected_window_point(&projected) {
    let derived = DerivedActionReadiness::click_ready();
    return TrainingResultSpatialQueryActionReadiness {
      eligibility: derived.eligibility,
      window_point: Some(window_point),
      refusal_reason: derived.refusal_reason,
    };
  }

  let derived = DerivedActionReadiness::answer_non_clickable(answer_non_clickable_refusal_reason(
    visibility, manifest,
  ));
  TrainingResultSpatialQueryActionReadiness {
    eligibility: derived.eligibility,
    window_point: None,
    refusal_reason: derived.refusal_reason,
  }
}

fn answer_non_clickable_refusal_reason(
  visibility: ProjectionVisibility,
  manifest: &TrainingResultSpatialQueryManifest,
) -> String {
  if visibility != ProjectionVisibility::Visible {
    return format!("visibility={}", visibility_label(visibility));
  }
  if manifest.screen_point.is_none() {
    return "visibility=visible missing_screen_point".to_string();
  }
  format!("visibility={}", visibility_label(visibility))
}

fn visibility_label(visibility: ProjectionVisibility) -> &'static str {
  match visibility {
    ProjectionVisibility::Visible => "visible",
    ProjectionVisibility::BehindCamera => "behind_camera",
    ProjectionVisibility::OutOfFrustum => "out_of_frustum",
    ProjectionVisibility::OutsideWindow => "outside_window",
  }
}

#[cfg(test)]
mod tests {
  use auv_driver::geometry::Point;

  use super::*;
  use crate::training_result_spatial_query::{
    TrainingResultSpatialQueryKind, TrainingResultSpatialQueryManifest,
    TrainingResultSpatialQueryReason,
  };
  use crate::types::{BlockPosition, MinecraftTargetSemantics};

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
      known_limits: Vec::new(),
    }
  }

  #[test]
  fn click_ready_when_answered_visible_and_screen_point_present() {
    let readiness = derive_action_readiness(&base_manifest());

    assert_eq!(
      readiness.eligibility,
      TrainingResultSpatialQueryActionEligibility::ClickReady
    );
    assert_eq!(readiness.window_point, Some(WindowPoint::new(854.0, 480.0)));
    assert!(readiness.refusal_reason.is_none());
  }

  #[test]
  fn answer_non_clickable_when_answered_outside_window() {
    let mut manifest = base_manifest();
    manifest.visibility = Some(ProjectionVisibility::OutsideWindow);
    manifest.screen_point = None;

    let readiness = derive_action_readiness(&manifest);

    assert_eq!(
      readiness.eligibility,
      TrainingResultSpatialQueryActionEligibility::AnswerNonClickable
    );
    assert!(readiness.window_point.is_none());
    assert_eq!(
      readiness.refusal_reason.as_deref(),
      Some("visibility=outside_window")
    );
  }

  #[test]
  fn not_consumable_when_query_failed() {
    let mut manifest = base_manifest();
    manifest.status = TrainingResultSpatialQueryStatus::Failed;
    manifest.reason = Some(TrainingResultSpatialQueryReason::TargetBlockAbsentFromScenePacket);
    manifest.visibility = None;
    manifest.screen_point = None;

    let readiness = derive_action_readiness(&manifest);

    assert_eq!(
      readiness.eligibility,
      TrainingResultSpatialQueryActionEligibility::NotConsumable
    );
    assert!(readiness.window_point.is_none());
    assert_eq!(
      readiness.refusal_reason.as_deref(),
      Some("status=failed reason=target_block_absent_from_scene_packet")
    );
  }

  #[test]
  fn not_consumable_when_query_blocked() {
    let mut manifest = base_manifest();
    manifest.status = TrainingResultSpatialQueryStatus::Blocked;
    manifest.reason = Some(TrainingResultSpatialQueryReason::SemanticSourceNotReady);
    manifest.visibility = None;
    manifest.screen_point = None;

    let readiness = derive_action_readiness(&manifest);

    assert_eq!(
      readiness.eligibility,
      TrainingResultSpatialQueryActionEligibility::NotConsumable
    );
    assert_eq!(
      readiness.refusal_reason.as_deref(),
      Some("status=blocked reason=semantic_source_not_ready")
    );
  }

  #[test]
  fn answer_non_clickable_when_answered_behind_camera() {
    let mut manifest = base_manifest();
    manifest.visibility = Some(ProjectionVisibility::BehindCamera);

    let readiness = derive_action_readiness(&manifest);

    assert_eq!(
      readiness.eligibility,
      TrainingResultSpatialQueryActionEligibility::AnswerNonClickable
    );
    assert_eq!(
      readiness.refusal_reason.as_deref(),
      Some("visibility=behind_camera")
    );
  }

  #[test]
  fn answer_non_clickable_when_visible_but_missing_screen_point() {
    let mut manifest = base_manifest();
    manifest.screen_point = None;

    let readiness = derive_action_readiness(&manifest);

    assert_eq!(
      readiness.eligibility,
      TrainingResultSpatialQueryActionEligibility::AnswerNonClickable
    );
    assert_eq!(
      readiness.refusal_reason.as_deref(),
      Some("visibility=visible missing_screen_point")
    );
  }

  #[test]
  fn answer_non_clickable_when_answered_missing_visibility() {
    let mut manifest = base_manifest();
    manifest.visibility = None;

    let readiness = derive_action_readiness(&manifest);

    assert_eq!(
      readiness.eligibility,
      TrainingResultSpatialQueryActionEligibility::AnswerNonClickable
    );
    assert_eq!(
      readiness.refusal_reason.as_deref(),
      Some("answered query missing visibility witness")
    );
  }
}
