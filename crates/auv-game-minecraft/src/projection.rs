use auv_driver::geometry::Point;

use crate::types::{BlockPosition, MinecraftBlockTarget, MinecraftProjectedPoint, MinecraftSpatialFrame, ProjectionVisibility, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
struct ScreenProjection {
  x: f64,
  y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ProjectedScreenPoint {
  Visible(ScreenProjection),
  Hidden(ProjectionVisibility),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MinecraftProjector {
  frame: MinecraftSpatialFrame,
}

impl MinecraftProjector {
  pub fn new(frame: MinecraftSpatialFrame) -> Result<Self, String> {
    validate_matrix(&frame.view_matrix, "view_matrix")?;
    validate_matrix(&frame.projection_matrix, "projection_matrix")?;
    if frame.viewport.width == 0 || frame.viewport.height == 0 {
      return Err(format!("viewport must have positive dimensions, got {}x{}", frame.viewport.width, frame.viewport.height));
    }
    Ok(Self { frame })
  }

  pub fn frame(&self) -> &MinecraftSpatialFrame {
    &self.frame
  }

  pub fn project_block_target(&self, target: &MinecraftBlockTarget) -> Result<MinecraftProjectedPoint, String> {
    if is_zero_matrix(&self.frame.view_matrix) || is_zero_matrix(&self.frame.projection_matrix) {
      return Err("projection basis is invalid: view_matrix/projection_matrix are all zero".to_string());
    }

    let clip = self.project_vec4(target.aim_point());
    let screen_projection = self.projected_screen_point_from_clip(clip, 1.0)?;
    if let ProjectedScreenPoint::Hidden(visibility) = screen_projection {
      return Ok(self.non_visible_point(visibility));
    }
    let ProjectedScreenPoint::Visible(screen_projection) = screen_projection else {
      unreachable!("hidden block targets must return early");
    };

    Ok(MinecraftProjectedPoint {
      screen_point: Some(Point::new(screen_projection.x, screen_projection.y)),
      visibility: ProjectionVisibility::Visible,
      match_radius_px: self.project_block_match_radius(target.block_pos)?,
      basis_frame_id: self.frame.spatial_frame_id.clone(),
      confidence: 1.0,
    })
  }

  pub fn project_block_match_radius(&self, block_pos: BlockPosition) -> Result<f64, String> {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut visible_corner_count = 0usize;

    for corner in block_pos.aabb_corners() {
      match self.project_screen_point(corner, 1.5)? {
        Some(screen_projection) => {
          min_x = min_x.min(screen_projection.x);
          max_x = max_x.max(screen_projection.x);
          min_y = min_y.min(screen_projection.y);
          max_y = max_y.max(screen_projection.y);
          visible_corner_count += 1;
        }
        None => continue,
      }
    }

    if visible_corner_count == 0 {
      return Err("block AABB has no projectable corners in front of the camera".to_string());
    }

    let extent_x = max_x - min_x;
    let extent_y = max_y - min_y;
    let radius = 0.5 * extent_x.max(extent_y);
    if !radius.is_finite() || radius <= 0.0 {
      return Err(format!("projected block radius must be positive finite, got {}", radius));
    }
    Ok(radius)
  }

  pub fn build_projection_artifact(
    &self,
    projected_point: Option<MinecraftProjectedPoint>,
    verification_reference: Option<String>,
  ) -> crate::artifact::MinecraftProjectionArtifact {
    crate::artifact::MinecraftProjectionArtifact::for_frame(&self.frame, projected_point, verification_reference)
  }

  fn non_visible_point(&self, visibility: ProjectionVisibility) -> MinecraftProjectedPoint {
    MinecraftProjectedPoint {
      screen_point: None,
      visibility,
      match_radius_px: 1.0,
      basis_frame_id: self.frame.spatial_frame_id.clone(),
      confidence: 1.0,
    }
  }

  fn project_screen_point(&self, world: Vec3, ndc_limit: f64) -> Result<Option<ScreenProjection>, String> {
    let clip = self.project_vec4(world);
    self.clip_to_screen_projection(clip, ndc_limit)
  }

  fn projected_screen_point_from_clip(&self, clip: [f64; 4], ndc_limit: f64) -> Result<ProjectedScreenPoint, String> {
    if let Some(screen_projection) = self.clip_to_screen_projection(clip, ndc_limit)? {
      return Ok(ProjectedScreenPoint::Visible(screen_projection));
    }

    Ok(ProjectedScreenPoint::Hidden(self.hidden_visibility_from_clip(clip, ndc_limit)?))
  }

  fn hidden_visibility_from_clip(&self, clip: [f64; 4], ndc_limit: f64) -> Result<ProjectionVisibility, String> {
    if !clip.iter().all(|value| value.is_finite()) {
      return Err("projection produced non-finite clip coordinates".to_string());
    }
    if clip[3] <= 0.0 {
      return Ok(ProjectionVisibility::BehindCamera);
    }

    let ndc_x = clip[0] / clip[3];
    let ndc_y = clip[1] / clip[3];
    let ndc_z = clip[2] / clip[3];
    if [ndc_x, ndc_y, ndc_z].iter().any(|value| !value.is_finite()) {
      return Err("projection produced non-finite normalized device coordinates".to_string());
    }

    Ok(
      if !(-ndc_limit..=ndc_limit).contains(&ndc_x)
        || !(-ndc_limit..=ndc_limit).contains(&ndc_y)
        || !(-ndc_limit..=ndc_limit).contains(&ndc_z)
      {
        ProjectionVisibility::OutOfFrustum
      } else {
        ProjectionVisibility::OutsideWindow
      },
    )
  }

  fn clip_to_screen_projection(&self, clip: [f64; 4], ndc_limit: f64) -> Result<Option<ScreenProjection>, String> {
    if !clip.iter().all(|value| value.is_finite()) {
      return Err("projection produced non-finite clip coordinates".to_string());
    }
    if clip[3] <= 0.0 {
      return Ok(None);
    }

    let ndc_x = clip[0] / clip[3];
    let ndc_y = clip[1] / clip[3];
    let ndc_z = clip[2] / clip[3];
    if [ndc_x, ndc_y, ndc_z].iter().any(|value| !value.is_finite()) {
      return Err("projection produced non-finite normalized device coordinates".to_string());
    }
    if !(-ndc_limit..=ndc_limit).contains(&ndc_x) || !(-ndc_limit..=ndc_limit).contains(&ndc_y) || !(-ndc_limit..=ndc_limit).contains(&ndc_z)
    {
      return Ok(None);
    }

    let width = f64::from(self.frame.viewport.width);
    let height = f64::from(self.frame.viewport.height);
    let x = (ndc_x * 0.5 + 0.5) * width;
    let y = (1.0 - (ndc_y * 0.5 + 0.5)) * height;
    if !(0.0..=width).contains(&x) || !(0.0..=height).contains(&y) {
      return Ok(None);
    }

    Ok(Some(ScreenProjection { x, y }))
  }

  fn project_vec4(&self, world: Vec3) -> [f64; 4] {
    let world_vec = if self.uses_rotation_only_view_matrix() {
      // NOTICE(mc2-telemetry-v0-compat): older MC-1 live samples recorded only
      // camera rotation in `view_matrix` and left the translation column at
      // zero. Until those samples are regenerated after the sidecar writes the
      // full render-time `positionMatrix`, subtract the eye position here so
      // MC-2 can still project against telemetry v0 fixtures instead of
      // misclassifying visible targets as behind-camera.
      let eye = self.frame.player_pose.eye_position;
      [world.x - eye.x, world.y - eye.y, world.z - eye.z, 1.0]
    } else {
      [world.x, world.y, world.z, 1.0]
    };
    let view = multiply_mat4_vec4(&self.frame.view_matrix, world_vec);
    multiply_mat4_vec4(&self.frame.projection_matrix, view)
  }

  fn uses_rotation_only_view_matrix(&self) -> bool {
    const EPSILON: f64 = 1e-6;

    self.frame.view_matrix[12].abs() <= EPSILON
      && self.frame.view_matrix[13].abs() <= EPSILON
      && self.frame.view_matrix[14].abs() <= EPSILON
      && (self.frame.player_pose.eye_position.x.abs() > EPSILON
        || self.frame.player_pose.eye_position.y.abs() > EPSILON
        || self.frame.player_pose.eye_position.z.abs() > EPSILON)
  }
}

fn validate_matrix(values: &[f64; 16], field_name: &str) -> Result<(), String> {
  if values.iter().any(|value| !value.is_finite()) {
    return Err(format!("{} contains non-finite values", field_name));
  }
  Ok(())
}

fn is_zero_matrix(values: &[f64; 16]) -> bool {
  values.iter().all(|value| value.abs() <= 1e-12)
}

fn multiply_mat4_vec4(matrix: &[f64; 16], vector: [f64; 4]) -> [f64; 4] {
  [
    matrix[0] * vector[0] + matrix[4] * vector[1] + matrix[8] * vector[2] + matrix[12] * vector[3],
    matrix[1] * vector[0] + matrix[5] * vector[1] + matrix[9] * vector[2] + matrix[13] * vector[3],
    matrix[2] * vector[0] + matrix[6] * vector[1] + matrix[10] * vector[2] + matrix[14] * vector[3],
    matrix[3] * vector[0] + matrix[7] * vector[1] + matrix[11] * vector[2] + matrix[15] * vector[3],
  ]
}
