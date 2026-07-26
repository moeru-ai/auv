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
