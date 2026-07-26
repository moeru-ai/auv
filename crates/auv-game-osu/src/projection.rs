use auv_driver::geometry::{CoordinateSpace, ProjectionBasis, ProjectionDerivationFamily, ProjectionSourceSpace, Rect};
use auv_driver::window::Window;
#[cfg(feature = "tracing")]
use auv_tracing::{ArtifactMetadata, Context};
use serde::{Deserialize, Serialize};

use crate::visual_eval::EvalProjection;

const PLAYFIELD_WIDTH: f64 = 512.0;
const PLAYFIELD_HEIGHT: f64 = 384.0;

pub const OSU_PROJECTION_PURPOSE: &str = "auv.osu.projection";

#[cfg(feature = "tracing")]
pub async fn publish_osu_projection(
  context: Option<&Context>,
  projection: &ProjectionArtifact,
) -> Result<Option<ArtifactMetadata>, crate::run_read::OsuArtifactPublishError> {
  crate::run_read::publish_json_artifact(context, OSU_PROJECTION_PURPOSE, projection, ProjectionArtifact::validate).await
}

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
    Self::for_capture(window.frame.size.width, window.frame.size.height, circle_size)
  }

  pub fn for_capture(capture_width: f64, capture_height: f64, circle_size: f32) -> Result<Self, String> {
    if !(capture_width.is_finite() && capture_height.is_finite()) || capture_width <= 0.0 || capture_height <= 0.0 {
      return Err(format!("capture size must have positive finite size, got {}x{}", capture_width, capture_height));
    }

    let scale = f64::min(capture_width / PLAYFIELD_WIDTH, capture_height / PLAYFIELD_HEIGHT);
    if !scale.is_finite() || scale <= 0.0 {
      return Err(format!("failed to derive finite playfield scale from capture {}x{}", capture_width, capture_height));
    }

    let playfield_width = PLAYFIELD_WIDTH * scale;
    let playfield_height = PLAYFIELD_HEIGHT * scale;
    let offset_x = (capture_width - playfield_width) / 2.0;
    let offset_y = (capture_height - playfield_height) / 2.0;
    let circle_radius_px = circle_radius_playfield(circle_size) * scale;
    positive_projection_f32("playfield projection", "scale", scale)?;
    let match_radius_px = positive_projection_f32("playfield projection", "match radius", circle_radius_px)?;

    Ok(Self {
      scale_x: scale,
      scale_y: scale,
      offset_x,
      offset_y,
      match_radius_px,
    })
  }

  pub fn to_window_point(&self, x: f32, y: f32) -> (f64, f64) {
    (f64::from(x) * self.scale_x + self.offset_x, f64::from(y) * self.scale_y + self.offset_y)
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
  pub fn from_window_projection(window: &Window, projection: &PlayfieldProjection, verification_reference: Option<String>) -> Self {
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
    if self.scale_x <= 0.0 || self.scale_y <= 0.0 {
      return Err(format!("projection artifact must have positive scales, got scale_x={} scale_y={}", self.scale_x, self.scale_y));
    }
    if !self.match_radius_px.is_finite() || self.match_radius_px <= 0.0 {
      return Err(format!("projection artifact must have positive match_radius_px, got {}", self.match_radius_px));
    }

    let scale_x = positive_projection_f32("projection artifact", "scale_x", self.scale_x)?;
    let scale_y = positive_projection_f32("projection artifact", "scale_y", self.scale_y)?;
    let offset_x = finite_projection_f32("projection artifact", "offset_x", self.offset_x)?;
    let offset_y = finite_projection_f32("projection artifact", "offset_y", self.offset_y)?;

    Ok(EvalProjection::PlayfieldToPixels {
      scale_x,
      scale_y,
      offset_x,
      offset_y,
      match_radius_px: self.match_radius_px,
    })
  }

  pub fn validate(&self) -> Result<(), String> {
    let bounds = [
      &self.source_window_bounds,
      self.capture_bounds.as_ref().unwrap_or(&self.source_window_bounds),
    ];
    if bounds.iter().flat_map(|bounds| [bounds.x, bounds.y, bounds.width, bounds.height]).any(|value| !value.is_finite()) {
      return Err("projection artifact contains non-finite bounds".to_string());
    }
    if self.source_window_bounds.width <= 0.0 || self.source_window_bounds.height <= 0.0 {
      return Err("projection artifact source window bounds must have positive size".to_string());
    }
    if self.capture_bounds.as_ref().is_some_and(|bounds| bounds.width <= 0.0 || bounds.height <= 0.0) {
      return Err("projection artifact capture bounds must have positive size".to_string());
    }
    if self.capture_width.is_some_and(|value| value == 0) || self.capture_height.is_some_and(|value| value == 0) {
      return Err("projection artifact capture dimensions must be positive".to_string());
    }
    if self.capture_scale_factor.is_some_and(|value| !value.is_finite() || value <= 0.0) {
      return Err("projection artifact capture scale factor must be positive and finite".to_string());
    }
    self.to_eval_projection().map(|_| ())
  }

  pub fn to_core_projection_basis(&self, basis_id: impl Into<String>, timestamp_millis: u64) -> ProjectionBasis {
    ProjectionBasis::new(
      basis_id,
      timestamp_millis,
      ProjectionSourceSpace::Local2d {
        name: "osu_playfield".to_string(),
      },
      CoordinateSpace::Window("osu_playfield_projection".to_string()),
      match self.derivation_method {
        ProjectionDerivationMethod::LayoutRule => ProjectionDerivationFamily::LayoutRule,
        ProjectionDerivationMethod::EmpiricalCalibration => ProjectionDerivationFamily::EmpiricalCalibration,
      },
    )
    .with_match_radius_px(f64::from(self.match_radius_px))
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

fn finite_projection_f32(subject: &str, field: &str, value: f64) -> Result<f32, String> {
  if !value.is_finite() {
    return Err(format!("{subject} {field} must be finite"));
  }
  let converted = value as f32;
  if !converted.is_finite() {
    return Err(format!("{subject} {field} must be representable as a finite f32"));
  }
  Ok(converted)
}

fn positive_projection_f32(subject: &str, field: &str, value: f64) -> Result<f32, String> {
  if value <= 0.0 {
    return Err(format!("{subject} {field} must be positive"));
  }
  let converted = finite_projection_f32(subject, field, value)?;
  if converted <= 0.0 {
    return Err(format!("{subject} {field} must remain positive when represented as f32"));
  }
  Ok(converted)
}

fn circle_radius_playfield(circle_size: f32) -> f64 {
  54.4 - 4.48 * f64::from(circle_size)
}
