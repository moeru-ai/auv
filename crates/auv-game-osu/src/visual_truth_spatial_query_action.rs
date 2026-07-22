use crate::visual_truth_spatial_query::{
  VisualTruthPixelVisibility, VisualTruthSpatialQueryManifest, VisualTruthSpatialQueryStatus, validate_answered_spatial_query,
};
use auv_query_readiness::{DerivedActionReadiness, format_query_not_consumable_refusal};

pub type VisualTruthSpatialQueryActionEligibility = auv_query_readiness::DerivedActionEligibility;
#[derive(Clone, Debug, PartialEq)]
pub struct VisualTruthSpatialQueryActionReadiness {
  pub eligibility: VisualTruthSpatialQueryActionEligibility,
  pub pixel_point: Option<(f32, f32)>,
  pub refusal_reason: Option<String>,
}

pub fn derive_visual_truth_spatial_query_action_readiness(
  manifest: &VisualTruthSpatialQueryManifest,
) -> VisualTruthSpatialQueryActionReadiness {
  if manifest.status != VisualTruthSpatialQueryStatus::Answered {
    let derived = DerivedActionReadiness::not_consumable(format_query_not_consumable_refusal(
      manifest.status.as_str(),
      manifest.reason.map(|reason| reason.as_str()),
    ));
    return VisualTruthSpatialQueryActionReadiness {
      eligibility: derived.eligibility,
      pixel_point: None,
      refusal_reason: derived.refusal_reason,
    };
  }

  let (pixel_x, pixel_y, visibility) = match validate_answered_spatial_query(manifest) {
    Ok(answer) => answer,
    Err(reason) => {
      let derived = DerivedActionReadiness::answer_non_clickable(reason);
      return VisualTruthSpatialQueryActionReadiness {
        eligibility: derived.eligibility,
        pixel_point: None,
        refusal_reason: derived.refusal_reason,
      };
    }
  };
  let pixel_point = Some((pixel_x, pixel_y));

  if visibility == VisualTruthPixelVisibility::InsideCapture {
    let derived = DerivedActionReadiness::click_ready();
    return VisualTruthSpatialQueryActionReadiness {
      eligibility: derived.eligibility,
      pixel_point,
      refusal_reason: derived.refusal_reason,
    };
  }

  let derived = DerivedActionReadiness::answer_non_clickable(format!("pixel_visibility={}", visibility.as_str()));
  VisualTruthSpatialQueryActionReadiness {
    eligibility: derived.eligibility,
    pixel_point,
    refusal_reason: derived.refusal_reason,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::benchmark::CapturePhase;
  use crate::visual_truth_spatial_query::{VisualTruthSpatialQueryBackend, VisualTruthSpatialQueryReason};

  fn base_manifest() -> VisualTruthSpatialQueryManifest {
    VisualTruthSpatialQueryManifest {
      schema_version: 1,
      generated_at_millis: 1,
      visual_truth_semantic_manifest_path: "/tmp/semantic.json".to_string(),
      source_run_artifact_dir: "/tmp/run".to_string(),
      source_visual_truth_manifest_path: "/tmp/visual_truth_manifest.json".to_string(),
      source_projection_path: "/tmp/projection.json".to_string(),
      object_index: 0,
      capture_phase: CapturePhase::BeforeDispatch,
      object_kind: None,
      query_backend: VisualTruthSpatialQueryBackend::PlayfieldProjectionReference,
      status: VisualTruthSpatialQueryStatus::Answered,
      reason: None,
      pixel_visibility: Some(VisualTruthPixelVisibility::InsideCapture),
      pixel_x: Some(400.0),
      pixel_y: Some(300.0),
      match_radius_px: Some(20.0),
      capture_width: Some(800),
      capture_height: Some(600),
      known_limits: Vec::new(),
    }
  }

  #[test]
  fn click_ready_when_answered_inside_capture_with_pixel_point() {
    let readiness = derive_visual_truth_spatial_query_action_readiness(&base_manifest());
    assert_eq!(readiness.eligibility, VisualTruthSpatialQueryActionEligibility::ClickReady);
    assert_eq!(readiness.pixel_point, Some((400.0, 300.0)));
  }

  #[test]
  fn answer_non_clickable_when_outside_capture() {
    let mut manifest = base_manifest();
    manifest.pixel_visibility = Some(VisualTruthPixelVisibility::OutsideCapture);
    let readiness = derive_visual_truth_spatial_query_action_readiness(&manifest);
    assert_eq!(readiness.eligibility, VisualTruthSpatialQueryActionEligibility::AnswerNonClickable);
  }

  #[test]
  fn answer_non_clickable_when_inside_visibility_has_negative_coordinates() {
    let mut manifest = base_manifest();
    manifest.pixel_x = Some(-1.0);

    let readiness = derive_visual_truth_spatial_query_action_readiness(&manifest);

    assert_eq!(readiness.eligibility, VisualTruthSpatialQueryActionEligibility::AnswerNonClickable);
    assert!(readiness.pixel_point.is_none());
  }

  #[test]
  fn answer_non_clickable_when_inside_visibility_exceeds_capture_bounds() {
    let mut manifest = base_manifest();
    manifest.pixel_x = Some(801.0);

    let readiness = derive_visual_truth_spatial_query_action_readiness(&manifest);

    assert_eq!(readiness.eligibility, VisualTruthSpatialQueryActionEligibility::AnswerNonClickable);
    assert!(readiness.pixel_point.is_none());
  }

  #[test]
  fn answer_non_clickable_when_inside_visibility_is_on_exclusive_capture_edge() {
    let mut manifest = base_manifest();
    manifest.pixel_x = Some(800.0);

    let readiness = derive_visual_truth_spatial_query_action_readiness(&manifest);

    assert_eq!(readiness.eligibility, VisualTruthSpatialQueryActionEligibility::AnswerNonClickable);
    assert!(readiness.pixel_point.is_none());
  }

  #[test]
  fn answer_non_clickable_when_capture_dimensions_are_missing() {
    let mut manifest = base_manifest();
    manifest.capture_width = None;

    let readiness = derive_visual_truth_spatial_query_action_readiness(&manifest);

    assert_eq!(readiness.eligibility, VisualTruthSpatialQueryActionEligibility::AnswerNonClickable);
    assert!(readiness.pixel_point.is_none());
  }

  #[test]
  fn answer_non_clickable_when_match_radius_is_invalid() {
    let mut manifest = base_manifest();
    manifest.match_radius_px = Some(0.0);

    let readiness = derive_visual_truth_spatial_query_action_readiness(&manifest);

    assert_eq!(readiness.eligibility, VisualTruthSpatialQueryActionEligibility::AnswerNonClickable);
    assert!(readiness.pixel_point.is_none());
  }

  #[test]
  fn not_consumable_when_query_failed() {
    let mut manifest = base_manifest();
    manifest.status = VisualTruthSpatialQueryStatus::Failed;
    manifest.reason = Some(VisualTruthSpatialQueryReason::TargetAbsentFromVisualTruth);
    manifest.pixel_visibility = None;
    manifest.pixel_x = None;
    manifest.pixel_y = None;
    let readiness = derive_visual_truth_spatial_query_action_readiness(&manifest);
    assert_eq!(readiness.eligibility, VisualTruthSpatialQueryActionEligibility::NotConsumable);
  }
}
