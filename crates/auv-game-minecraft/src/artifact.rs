use serde::{Deserialize, Serialize};

use auv_driver::geometry::{
  CoordinateSpace, ProjectionBasis, ProjectionDerivationFamily, ProjectionSourceSpace, Rect,
};
use auv_tracing_driver::EvidenceCorrelationKey;

use crate::types::{MinecraftProjectedPoint, MinecraftSpatialFrame, ProjectionVisibility};
use crate::verify::MismatchRefusalReason;

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
      visibility: projected_point
        .as_ref()
        .map(|point| point.visibility)
        .unwrap_or(ProjectionVisibility::OutsideWindow),
      projected_point,
      raycast_block_id: frame.raycast_hit.as_ref().map(|hit| hit.block_id.clone()),
      screen_state: frame.screen_state.clone(),
      mismatch_refusal_reason: None,
      verification_reference,
    }
  }

  pub fn with_mismatch_refusal_reason(mut self, reason: Option<MismatchRefusalReason>) -> Self {
    self.mismatch_refusal_reason = reason;
    self
  }

  pub fn to_core_projection_basis(&self) -> ProjectionBasis {
    let basis_id = self
      .projected_point
      .as_ref()
      .map(|point| point.basis_frame_id.clone())
      .unwrap_or_else(|| self.spatial_frame_id.clone());
    let mut basis = ProjectionBasis::new(
      basis_id,
      self.monotonic_timestamp_ms,
      ProjectionSourceSpace::World,
      CoordinateSpace::Window("minecraft_viewport".to_string()),
      ProjectionDerivationFamily::CameraMatrix,
    );
    if let Some(projected_point) = &self.projected_point {
      basis = basis
        .with_confidence(projected_point.confidence)
        .with_match_radius_px(projected_point.match_radius_px);
    } else {
      basis = basis.with_known_limit("minecraft projection artifact has no projected point");
    }
    if self.screenshot_artifact_ref.is_none() {
      basis = basis.with_known_limit("minecraft projection basis has no bound screenshot artifact");
    }
    basis
  }

  pub fn to_core_evidence_correlation_key(&self) -> EvidenceCorrelationKey {
    let basis_frame_id = self
      .projected_point
      .as_ref()
      .map(|point| point.basis_frame_id.clone())
      .unwrap_or_else(|| self.spatial_frame_id.clone());
    EvidenceCorrelationKey::new(basis_frame_id)
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
        return Err(format!(
          "projection artifact must have positive match_radius_px, got {}",
          projected_point.match_radius_px
        ));
      }
      if !(0.0..=1.0).contains(&projected_point.confidence) {
        return Err(format!(
          "projection artifact confidence must be between 0 and 1, got {}",
          projected_point.confidence
        ));
      }
    }
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::types::{
    BlockFace, BlockPosition, MinecraftProjectedPoint, MinecraftSpatialFrame, NearbyBlock,
    NearbyEntity, PlayerPose, ProjectionVisibility, RaycastHit, Vec3, Viewport,
  };

  fn test_frame() -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: "frame-1".to_string(),
      world_tick: 42,
      monotonic_timestamp_ms: 1_000,
      viewport: Viewport::new(800, 600),
      view_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      projection_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      player_pose: PlayerPose {
        eye_position: Vec3::new(0.0, 0.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: Some(RaycastHit {
        block_pos: BlockPosition::new(1, 2, 3),
        face: BlockFace::North,
        block_id: "minecraft:stone".to_string(),
      }),
      nearby_blocks: vec![NearbyBlock {
        block_pos: BlockPosition::new(1, 2, 3),
        block_id: "minecraft:stone".to_string(),
      }],
      nearby_entities: vec![NearbyEntity {
        entity_id: "pig-1".to_string(),
        entity_kind: "minecraft:pig".to_string(),
      }],
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: None,
    }
  }

  #[test]
  fn projection_artifact_round_trips_and_validates() {
    let artifact = MinecraftProjectionArtifact::for_frame(
      &test_frame(),
      Some(MinecraftProjectedPoint {
        screen_point: Some(auv_driver::geometry::Point::new(320.0, 240.0)),
        visibility: ProjectionVisibility::Visible,
        match_radius_px: 12.0,
        basis_frame_id: "frame-1".to_string(),
        confidence: 1.0,
      }),
      Some("overlay.png".to_string()),
    );

    artifact.validate().expect("artifact should validate");
    let json = serde_json::to_string(&artifact).expect("serialize artifact");
    let decoded: MinecraftProjectionArtifact =
      serde_json::from_str(&json).expect("deserialize artifact");
    assert_eq!(decoded, artifact);
  }

  #[test]
  fn projection_artifact_rejects_non_finite_values() {
    let mut artifact = MinecraftProjectionArtifact::for_frame(&test_frame(), None, None);
    artifact.viewport_bounds.width = f64::NAN;

    let error = artifact.validate().expect_err("must fail");
    assert!(error.contains("non-finite viewport values"));
  }

  #[test]
  fn projection_artifact_carries_capture_binding_evidence() {
    let mut frame = test_frame();
    frame.screenshot_artifact_ref = Some("artifact://screenshot-1".to_string());
    frame.mc_capture_skew_ms = Some(180);
    frame.screen_state = Some("menu".to_string());

    let artifact = MinecraftProjectionArtifact::for_frame(&frame, None, None);

    assert_eq!(
      artifact.screenshot_artifact_ref.as_deref(),
      Some("artifact://screenshot-1")
    );
    assert_eq!(artifact.mc_capture_skew_ms, Some(180));
    assert_eq!(artifact.screen_state.as_deref(), Some("menu"));
  }

  #[test]
  fn projection_artifact_exposes_core_projection_basis() {
    let artifact = MinecraftProjectionArtifact::for_frame(
      &test_frame(),
      Some(MinecraftProjectedPoint {
        screen_point: Some(auv_driver::geometry::Point::new(320.0, 240.0)),
        visibility: ProjectionVisibility::Visible,
        match_radius_px: 12.0,
        basis_frame_id: "frame-1".to_string(),
        confidence: 0.8,
      }),
      None,
    );

    let basis = artifact.to_core_projection_basis();

    assert_eq!(basis.basis_id, "frame-1");
    assert_eq!(basis.timestamp_millis, 1_000);
    assert_eq!(basis.source_space, ProjectionSourceSpace::World);
    assert_eq!(
      basis.projected_coordinate_space,
      CoordinateSpace::Window("minecraft_viewport".to_string())
    );
    assert_eq!(
      basis.derivation_family,
      ProjectionDerivationFamily::CameraMatrix
    );
    assert_eq!(basis.confidence, 0.8);
    assert_eq!(basis.match_radius_px, Some(12.0));
  }

  #[test]
  fn projection_artifact_exposes_core_evidence_correlation_key() {
    let artifact = MinecraftProjectionArtifact::for_frame(
      &test_frame(),
      Some(MinecraftProjectedPoint {
        screen_point: Some(auv_driver::geometry::Point::new(320.0, 240.0)),
        visibility: ProjectionVisibility::Visible,
        match_radius_px: 12.0,
        basis_frame_id: "frame-1".to_string(),
        confidence: 0.8,
      }),
      None,
    );

    let key = artifact.to_core_evidence_correlation_key();

    assert_eq!(key.basis_frame_id, "frame-1");
    assert!(key.action_artifact_id.is_none());
    assert!(key.verification_artifact_id.is_none());
  }

  #[test]
  fn projection_artifact_carries_mismatch_refusal_reason() {
    let artifact = MinecraftProjectionArtifact::for_frame(&test_frame(), None, None)
      .with_mismatch_refusal_reason(Some(MismatchRefusalReason::MenuLoadingScreen));

    assert_eq!(
      artifact.mismatch_refusal_reason,
      Some(MismatchRefusalReason::MenuLoadingScreen)
    );
  }
}
