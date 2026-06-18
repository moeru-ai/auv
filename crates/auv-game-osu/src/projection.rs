use auv_driver::CaptureBinding;
use auv_driver::geometry::{
  CoordinateSpace, ProjectionBasis, ProjectionDerivationFamily, ProjectionSourceSpace, Rect,
};
use auv_driver::window::Window;
use auv_tracing_driver::EvidenceCorrelationKey;
use serde::{Deserialize, Serialize};

use crate::visual_eval::EvalProjection;

const PLAYFIELD_WIDTH: f64 = 512.0;
const PLAYFIELD_HEIGHT: f64 = 384.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionDerivationMethod {
  LayoutRule,
  EmpiricalCalibration,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectionBounds {
  pub x: f64,
  pub y: f64,
  pub width: f64,
  pub height: f64,
}

impl ProjectionBounds {
  fn from_rect(rect: Rect) -> Self {
    Self {
      x: rect.origin.x,
      y: rect.origin.y,
      width: rect.size.width,
      height: rect.size.height,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayfieldProjection {
  pub scale_x: f64,
  pub scale_y: f64,
  pub offset_x: f64,
  pub offset_y: f64,
  pub match_radius_px: f32,
}

impl PlayfieldProjection {
  pub fn for_window(window: &Window, circle_size: f32) -> Result<Self, String> {
    Self::for_capture(
      window.frame.size.width,
      window.frame.size.height,
      circle_size,
    )
  }

  pub fn for_capture(
    capture_width: f64,
    capture_height: f64,
    circle_size: f32,
  ) -> Result<Self, String> {
    if !(capture_width.is_finite() && capture_height.is_finite())
      || capture_width <= 0.0
      || capture_height <= 0.0
    {
      return Err(format!(
        "capture size must have positive finite size, got {}x{}",
        capture_width, capture_height
      ));
    }

    let scale = f64::min(
      capture_width / PLAYFIELD_WIDTH,
      capture_height / PLAYFIELD_HEIGHT,
    );
    if !scale.is_finite() || scale <= 0.0 {
      return Err(format!(
        "failed to derive finite playfield scale from capture {}x{}",
        capture_width, capture_height
      ));
    }

    let playfield_width = PLAYFIELD_WIDTH * scale;
    let playfield_height = PLAYFIELD_HEIGHT * scale;
    let offset_x = (capture_width - playfield_width) / 2.0;
    let offset_y = (capture_height - playfield_height) / 2.0;
    let circle_radius_px = circle_radius_playfield(circle_size) * scale;

    Ok(Self {
      scale_x: scale,
      scale_y: scale,
      offset_x,
      offset_y,
      match_radius_px: circle_radius_px as f32,
    })
  }

  pub fn to_window_point(&self, x: f32, y: f32) -> (f64, f64) {
    (
      f64::from(x) * self.scale_x + self.offset_x,
      f64::from(y) * self.scale_y + self.offset_y,
    )
  }

  pub fn to_eval_projection(&self) -> EvalProjection {
    EvalProjection::PlayfieldToPixels {
      scale_x: self.scale_x as f32,
      scale_y: self.scale_y as f32,
      offset_x: self.offset_x as f32,
      offset_y: self.offset_y as f32,
      match_radius_px: self.match_radius_px,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectionArtifact {
  pub source_window_bounds: ProjectionBounds,
  pub capture_bounds: Option<ProjectionBounds>,
  pub capture_width: Option<u32>,
  pub capture_height: Option<u32>,
  pub capture_scale_factor: Option<f64>,
  pub scale_x: f64,
  pub scale_y: f64,
  pub offset_x: f64,
  pub offset_y: f64,
  pub match_radius_px: f32,
  pub derivation_method: ProjectionDerivationMethod,
  pub verification_reference: Option<String>,
}

impl ProjectionArtifact {
  pub fn from_window_projection(
    window: &Window,
    projection: &PlayfieldProjection,
    verification_reference: Option<String>,
  ) -> Self {
    Self {
      source_window_bounds: ProjectionBounds::from_rect(window.frame),
      capture_bounds: None,
      capture_width: None,
      capture_height: None,
      capture_scale_factor: None,
      scale_x: projection.scale_x,
      scale_y: projection.scale_y,
      offset_x: projection.offset_x,
      offset_y: projection.offset_y,
      match_radius_px: projection.match_radius_px,
      derivation_method: ProjectionDerivationMethod::LayoutRule,
      verification_reference,
    }
  }

  pub fn to_eval_projection(&self) -> Result<EvalProjection, String> {
    let values = [self.scale_x, self.scale_y, self.offset_x, self.offset_y];
    if values.iter().any(|value| !value.is_finite()) || !self.match_radius_px.is_finite() {
      return Err("projection artifact contains non-finite values".to_string());
    }
    if self.scale_x <= 0.0 || self.scale_y <= 0.0 {
      return Err(format!(
        "projection artifact must have positive scales, got scale_x={} scale_y={}",
        self.scale_x, self.scale_y
      ));
    }
    if self.match_radius_px <= 0.0 {
      return Err(format!(
        "projection artifact must have positive match_radius_px, got {}",
        self.match_radius_px
      ));
    }

    Ok(EvalProjection::PlayfieldToPixels {
      scale_x: self.scale_x as f32,
      scale_y: self.scale_y as f32,
      offset_x: self.offset_x as f32,
      offset_y: self.offset_y as f32,
      match_radius_px: self.match_radius_px,
    })
  }

  pub fn to_core_projection_basis(
    &self,
    basis_id: impl Into<String>,
    timestamp_millis: u64,
  ) -> ProjectionBasis {
    let mut basis = ProjectionBasis::new(
      basis_id,
      timestamp_millis,
      ProjectionSourceSpace::Local2d {
        name: "osu_playfield".to_string(),
      },
      CoordinateSpace::Window("osu_playfield_projection".to_string()),
      match self.derivation_method {
        ProjectionDerivationMethod::LayoutRule => ProjectionDerivationFamily::LayoutRule,
        ProjectionDerivationMethod::EmpiricalCalibration => {
          ProjectionDerivationFamily::EmpiricalCalibration
        }
      },
    )
    .with_match_radius_px(f64::from(self.match_radius_px));
    if self.capture_bounds.is_none() {
      basis = basis.with_known_limit("osu projection basis has no bound capture rectangle");
    }
    basis
  }

  pub fn to_core_evidence_correlation_key(
    &self,
    basis_id: impl Into<String>,
  ) -> EvidenceCorrelationKey {
    EvidenceCorrelationKey::new(basis_id)
  }

  pub fn to_core_capture_binding(
    &self,
    source_observation_id: impl Into<String>,
    capture_ref: impl Into<String>,
    capture_skew_ms: i64,
  ) -> CaptureBinding {
    let mut binding = CaptureBinding::new(source_observation_id, capture_ref, capture_skew_ms)
      .with_known_limit(
        "osu capture binding records dataset projection provenance, not input success",
      );
    if self.capture_bounds.is_none() {
      binding = binding.with_known_limit("osu projection artifact has no bound capture rectangle");
    }
    binding
  }

  pub fn with_capture(
    mut self,
    capture_bounds: Rect,
    capture_width: u32,
    capture_height: u32,
    capture_scale_factor: f64,
    projection: &PlayfieldProjection,
  ) -> Self {
    self.capture_bounds = Some(ProjectionBounds::from_rect(capture_bounds));
    self.capture_width = Some(capture_width);
    self.capture_height = Some(capture_height);
    self.capture_scale_factor = Some(capture_scale_factor);
    self.scale_x = projection.scale_x;
    self.scale_y = projection.scale_y;
    self.offset_x = projection.offset_x;
    self.offset_y = projection.offset_y;
    self.match_radius_px = projection.match_radius_px;
    self
  }
}

fn circle_radius_playfield(circle_size: f32) -> f64 {
  54.4 - 4.48 * f64::from(circle_size)
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_driver::geometry::{CoordinateSpace, Rect};
  use auv_driver::window::{Window, WindowRef};

  fn test_window(width: f64, height: f64) -> Window {
    Window {
      reference: WindowRef {
        id: "window-1".to_string(),
      },
      title: Some("osu!".to_string()),
      app_name: Some("osu!".to_string()),
      app_bundle_id: None,
      process_id: None,
      frame: Rect::new(100.0, 200.0, width, height),
      coordinate_space: CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    }
  }

  #[test]
  fn projection_maps_center_for_matching_aspect_ratio() {
    let window = test_window(1024.0, 768.0);
    let projection = PlayfieldProjection::for_window(&window, 4.0).expect("projection");

    let (x, y) = projection.to_window_point(256.0, 192.0);
    assert_eq!(x, 512.0);
    assert_eq!(y, 384.0);
    assert!((projection.match_radius_px - 72.96).abs() < 0.01);
  }

  #[test]
  fn projection_letterboxes_wider_windows() {
    let window = test_window(1280.0, 720.0);
    let projection = PlayfieldProjection::for_window(&window, 4.0).expect("projection");

    let (left, top) = projection.to_window_point(0.0, 0.0);
    let (right, bottom) = projection.to_window_point(512.0, 384.0);

    assert_eq!(left, 160.0);
    assert_eq!(top, 0.0);
    assert_eq!(right, 1120.0);
    assert_eq!(bottom, 720.0);
  }

  #[test]
  fn projection_uses_capture_dimensions_when_they_differ_from_window() {
    let projection = PlayfieldProjection::for_capture(1512.0, 949.0, 5.0).expect("projection");

    assert!((projection.scale_x - 2.4713541666666665).abs() < 1e-9);
    assert!((projection.scale_y - 2.4713541666666665).abs() < 1e-9);
    assert!((projection.offset_x - 123.33333333333337).abs() < 0.001);
    assert!((projection.offset_y - 0.0).abs() < 0.001);
    assert!((projection.match_radius_px - 79.083336).abs() < 0.01);
  }

  #[test]
  fn projection_artifact_exposes_core_projection_basis() {
    let artifact = sample_projection_artifact_with_capture();

    let basis = artifact.to_core_projection_basis("osu-frame-1", 1_000);

    assert_eq!(basis.basis_id, "osu-frame-1");
    assert_eq!(basis.timestamp_millis, 1_000);
    assert_eq!(
      basis.source_space,
      ProjectionSourceSpace::Local2d {
        name: "osu_playfield".to_string()
      }
    );
    assert_eq!(
      basis.derivation_family,
      ProjectionDerivationFamily::LayoutRule
    );
    assert_eq!(
      basis.match_radius_px,
      Some(f64::from(artifact.match_radius_px))
    );
  }

  #[test]
  fn projection_artifact_exposes_core_evidence_correlation_key() {
    let artifact = sample_projection_artifact_with_capture();

    let key = artifact.to_core_evidence_correlation_key("osu-frame-1");

    assert_eq!(key.basis_frame_id, "osu-frame-1");
    assert!(key.action_artifact_id.is_none());
    assert!(key.verification_artifact_id.is_none());
  }

  #[test]
  fn projection_artifact_exposes_core_capture_binding() {
    let artifact = sample_projection_artifact_with_capture();

    let binding = artifact.to_core_capture_binding("osu-frame-1", "artifact://osu-capture-1", -16);

    assert_eq!(binding.source_observation_id, "osu-frame-1");
    assert_eq!(binding.capture_ref, "artifact://osu-capture-1");
    assert_eq!(binding.capture_skew_ms, -16);
    assert!(
      binding
        .known_limits
        .iter()
        .any(|limit| limit.contains("dataset projection provenance"))
    );
  }

  #[test]
  fn projection_artifact_adapts_to_eval_projection() {
    let artifact = ProjectionArtifact {
      source_window_bounds: ProjectionBounds {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 720.0,
      },
      capture_bounds: None,
      capture_width: Some(1512),
      capture_height: Some(949),
      capture_scale_factor: Some(1.18125),
      scale_x: 2.4713541666666665,
      scale_y: 2.4713541666666665,
      offset_x: 123.33333333333337,
      offset_y: 0.0,
      match_radius_px: 79.083336,
      derivation_method: ProjectionDerivationMethod::LayoutRule,
      verification_reference: Some("capture-object-0000-before-16ms.png".to_string()),
    };

    let projection = artifact.to_eval_projection().expect("eval projection");

    assert_eq!(
      projection,
      EvalProjection::PlayfieldToPixels {
        scale_x: 2.4713542,
        scale_y: 2.4713542,
        offset_x: 123.333336,
        offset_y: 0.0,
        match_radius_px: 79.083336,
      }
    );
  }

  #[test]
  fn projection_artifact_rejects_non_finite_values() {
    let artifact = ProjectionArtifact {
      source_window_bounds: ProjectionBounds {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 720.0,
      },
      capture_bounds: None,
      capture_width: None,
      capture_height: None,
      capture_scale_factor: None,
      scale_x: f64::NAN,
      scale_y: 1.0,
      offset_x: 0.0,
      offset_y: 0.0,
      match_radius_px: 20.0,
      derivation_method: ProjectionDerivationMethod::LayoutRule,
      verification_reference: None,
    };

    let error = artifact.to_eval_projection().expect_err("must fail");
    assert!(error.contains("non-finite values"));
  }

  #[test]
  fn projection_rejects_non_positive_window_size() {
    let window = test_window(0.0, 720.0);
    let error = PlayfieldProjection::for_window(&window, 4.0).expect_err("must fail");
    assert!(error.contains("positive finite size"));
  }

  fn sample_projection_artifact_with_capture() -> ProjectionArtifact {
    let projection = PlayfieldProjection::for_capture(1024.0, 768.0, 4.0).expect("projection");
    ProjectionArtifact {
      source_window_bounds: ProjectionBounds {
        x: 0.0,
        y: 0.0,
        width: 1024.0,
        height: 768.0,
      },
      capture_bounds: Some(ProjectionBounds {
        x: 0.0,
        y: 0.0,
        width: 1024.0,
        height: 768.0,
      }),
      capture_width: Some(1024),
      capture_height: Some(768),
      capture_scale_factor: Some(1.0),
      scale_x: projection.scale_x,
      scale_y: projection.scale_y,
      offset_x: projection.offset_x,
      offset_y: projection.offset_y,
      match_radius_px: projection.match_radius_px,
      derivation_method: ProjectionDerivationMethod::LayoutRule,
      verification_reference: None,
    }
  }
}
