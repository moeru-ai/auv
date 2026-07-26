use serde::{Deserialize, Serialize};

use auv_driver::geometry::{CoordinateSpace, ProjectionBasis, ProjectionDerivationFamily, ProjectionSourceSpace, Rect};
#[cfg(feature = "tracing")]
use auv_tracing::{ArtifactMetadata, Context};

use crate::types::{MinecraftProjectedPoint, MinecraftSpatialFrame, ProjectionVisibility};
use crate::verify::MismatchRefusalReason;

pub const MINECRAFT_PROJECTION_PURPOSE: &str = "auv.minecraft.projection";

#[cfg(feature = "tracing")]
pub async fn publish_minecraft_projection(
  context: Option<&Context>,
  projection: &MinecraftProjectionArtifact,
) -> Result<Option<ArtifactMetadata>, crate::run_read::MinecraftArtifactPublishError> {
  crate::run_read::publish_json_artifact(context, MINECRAFT_PROJECTION_PURPOSE, projection, MinecraftProjectionArtifact::validate).await
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectionViewportBounds {
  pub x: f64,
  pub y: f64,
  pub width: f64,
  pub height: f64,
}

impl ProjectionViewportBounds {
  pub fn from_rect(rect: Rect) -> Self {
    Self {
      x: rect.origin.x,
      y: rect.origin.y,
      width: rect.size.width,
      height: rect.size.height,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MinecraftProjectionArtifact {
  pub spatial_frame_id: String,
  pub world_tick: u64,
  pub monotonic_timestamp_ms: u64,
  #[serde(default)]
  pub screenshot_artifact_ref: Option<String>,
  #[serde(default)]
  pub mc_capture_skew_ms: Option<i64>,
  pub viewport_bounds: ProjectionViewportBounds,
  pub projected_point: Option<MinecraftProjectedPoint>,
  pub visibility: ProjectionVisibility,
  pub raycast_block_id: Option<String>,
  #[serde(default)]
  pub screen_state: Option<String>,
  #[serde(default)]
  pub resource_pack_ids: Vec<String>,
  #[serde(default)]
  pub mismatch_refusal_reason: Option<MismatchRefusalReason>,
  pub verification_reference: Option<String>,
}

impl MinecraftProjectionArtifact {
  pub fn for_frame(
    frame: &MinecraftSpatialFrame,
    projected_point: Option<MinecraftProjectedPoint>,
    verification_reference: Option<String>,
  ) -> Self {
    Self {
      spatial_frame_id: frame.spatial_frame_id.clone(),
      world_tick: frame.world_tick,
      monotonic_timestamp_ms: frame.monotonic_timestamp_ms,
      screenshot_artifact_ref: frame.screenshot_artifact_ref.clone(),
      mc_capture_skew_ms: frame.mc_capture_skew_ms,
      viewport_bounds: ProjectionViewportBounds::from_rect(frame.viewport.bounds()),
      visibility: projected_point.as_ref().map(|point| point.visibility).unwrap_or(ProjectionVisibility::OutsideWindow),
      projected_point,
      raycast_block_id: frame.raycast_hit.as_ref().map(|hit| hit.block_id.clone()),
      screen_state: frame.screen_state.clone(),
      resource_pack_ids: frame.resource_pack_ids.clone(),
      mismatch_refusal_reason: None,
      verification_reference,
    }
  }

  pub fn with_mismatch_refusal_reason(mut self, reason: Option<MismatchRefusalReason>) -> Self {
    self.mismatch_refusal_reason = reason;
    self
  }

  pub fn to_core_projection_basis(&self) -> ProjectionBasis {
    let basis_id = self.projected_point.as_ref().map(|point| point.basis_frame_id.clone()).unwrap_or_else(|| self.spatial_frame_id.clone());
    let mut basis = ProjectionBasis::new(
      basis_id,
      self.monotonic_timestamp_ms,
      ProjectionSourceSpace::World,
      CoordinateSpace::Window("minecraft_viewport".to_string()),
      ProjectionDerivationFamily::CameraMatrix,
    );
    if let Some(projected_point) = &self.projected_point {
      basis = basis.with_confidence(projected_point.confidence).with_match_radius_px(projected_point.match_radius_px);
    }
    basis
  }

  pub fn validate(&self) -> Result<(), String> {
    let values = [
      self.viewport_bounds.x,
      self.viewport_bounds.y,
      self.viewport_bounds.width,
      self.viewport_bounds.height,
    ];
    if values.iter().any(|value| !value.is_finite()) {
      return Err("projection artifact contains non-finite viewport values".to_string());
    }
    if self.viewport_bounds.width <= 0.0 || self.viewport_bounds.height <= 0.0 {
      return Err(format!(
        "projection artifact must have positive viewport size, got {}x{}",
        self.viewport_bounds.width, self.viewport_bounds.height
      ));
    }
    if let Some(projected_point) = &self.projected_point {
      let point_values = [projected_point.match_radius_px, projected_point.confidence];
      if point_values.iter().any(|value| !value.is_finite()) {
        return Err("projection artifact contains non-finite projected-point values".to_string());
      }
      if let Some(screen_point) = projected_point.screen_point {
        let screen_values = [screen_point.x, screen_point.y];
        if screen_values.iter().any(|value| !value.is_finite()) {
          return Err("projection artifact contains non-finite screen-point values".to_string());
        }
      }
      if projected_point.match_radius_px <= 0.0 {
        return Err(format!("projection artifact must have positive match_radius_px, got {}", projected_point.match_radius_px));
      }
      if !(0.0..=1.0).contains(&projected_point.confidence) {
        return Err(format!("projection artifact confidence must be between 0 and 1, got {}", projected_point.confidence));
      }
    }
    Ok(())
  }
}
