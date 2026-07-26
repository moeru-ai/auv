use image::RgbImage;

use crate::artifact::MinecraftProjectionArtifact;
use crate::bind::bind_capture_to_frame;
use crate::overlay::render_projection_overlay;
use crate::projection::MinecraftProjector;
use crate::types::{MinecraftBlockTarget, MinecraftSpatialFrame, RaycastHit};
use crate::verify::{MismatchRefusal, evaluate_mismatch_refusal_with_capture};

/// A real captured screenshot plus the monotonic timestamp taken at the capture
/// instant. The image is owned so the overlay can be drawn onto it.
#[derive(Clone, Debug)]
pub struct ScreenshotCapture {
  pub image: RgbImage,
  pub artifact_ref: Option<String>,
  pub capture_monotonic_timestamp_ms: u64,
  pub is_minecraft_window: bool,
  /// Optional screenshot dimensions. When present and different from viewport,
  /// projection coordinates are scaled to match screenshot space (e.g. Retina/HiDPI
  /// or display-capture vs Minecraft window framebuffer).
  pub screenshot_dimensions: Option<(u32, u32)>,
}

impl ScreenshotCapture {
  fn dimensions(&self) -> (u32, u32) {
    self.screenshot_dimensions.unwrap_or((self.image.width(), self.image.height()))
  }
}

#[derive(Clone, Debug)]
pub enum ProjectionAssessment {
  Bound {
    artifact: MinecraftProjectionArtifact,
    raycast_hit: Option<RaycastHit>,
  },
  Refused {
    artifact: MinecraftProjectionArtifact,
    refusal: MismatchRefusal,
  },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ProjectionScale {
  x: f64,
  y: f64,
}

impl ProjectionScale {
  fn for_dimensions(screenshot_width: u32, screenshot_height: u32, frame: &MinecraftSpatialFrame) -> Option<Self> {
    if screenshot_width == frame.viewport.width && screenshot_height == frame.viewport.height {
      return None;
    }

    Some(Self {
      x: f64::from(screenshot_width) / f64::from(frame.viewport.width),
      y: f64::from(screenshot_height) / f64::from(frame.viewport.height),
    })
  }

  fn apply_to_point(self, point: &mut auv_driver::geometry::Point) {
    point.x *= self.x;
    point.y *= self.y;
  }

  fn apply_to_radius(self, radius_px: &mut f64) {
    *radius_px *= self.x.max(self.y);
  }
}

/// The outcome of binding one ingested frame to one real capture and projecting
/// a world target onto it.
///
/// Either the bridge produced an overlay-on-frame projection artifact (the
/// MC-2 happy path), or it refused with a structured reason (e.g. the capture
/// skew exceeded tolerance, or the projected point fell outside the window).
/// Both arms carry the projection artifact so the run records what was seen
/// even on refusal.
#[derive(Clone, Debug)]
pub enum ProjectionEvidence {
  /// Target projected to a visible point; `overlay` is the captured frame with
  /// the projection drawn on it.
  Bound {
    artifact: MinecraftProjectionArtifact,
    overlay: RgbImage,
  },
  /// The bridge refused before trusting the projection; no overlay is produced.
  Refused {
    artifact: MinecraftProjectionArtifact,
    refusal: MismatchRefusal,
  },
}

impl ProjectionEvidence {
  pub fn artifact(&self) -> &MinecraftProjectionArtifact {
    match self {
      Self::Bound { artifact, .. } | Self::Refused { artifact, .. } => artifact,
    }
  }

  pub fn is_refused(&self) -> bool {
    matches!(self, Self::Refused { .. })
  }
}

/// Bind one ingested spatial frame to one real screenshot capture, project the
/// world target, and produce overlay-on-frame evidence or a structured refusal.
///
/// This is the crate-local MC-2 bridge orchestration: it composes the already
/// proven pieces (`bind_capture_to_frame` -> `MinecraftProjector` ->
/// `evaluate_mismatch_refusal` -> `render_projection_overlay`) without adding a
/// new contract or result family. The refusal path reuses
/// `evaluate_mismatch_refusal`, so skew-over-threshold, not-minecraft-window,
/// and outside-window all refuse here rather than emitting a misleading overlay.
///
/// `max_capture_skew_ms` is the tolerance handed to the refusal evaluator; pass
/// `None` to skip skew enforcement (e.g. when clock bases are not yet aligned).
pub fn build_projection_evidence(
  frame: MinecraftSpatialFrame,
  capture: ScreenshotCapture,
  target: &MinecraftBlockTarget,
  max_capture_skew_ms: Option<i64>,
) -> Result<ProjectionEvidence, String> {
  let screenshot_dimensions = capture.dimensions();
  let bound = bind_capture_to_frame(frame, capture.artifact_ref, capture.capture_monotonic_timestamp_ms);

  match assess_bound_projection(bound.frame, screenshot_dimensions, capture.is_minecraft_window, target, max_capture_skew_ms)? {
    ProjectionAssessment::Bound {
      artifact,
      raycast_hit,
    } => {
      let projected =
        artifact.projected_point.clone().ok_or_else(|| "projection evidence is bound but missing projected point".to_string())?;
      let overlay = render_projection_overlay(capture.image, &projected, raycast_hit.as_ref());
      Ok(ProjectionEvidence::Bound { artifact, overlay })
    }
    ProjectionAssessment::Refused { artifact, refusal } => Ok(ProjectionEvidence::Refused { artifact, refusal }),
  }
}

pub fn assess_bound_projection(
  frame: MinecraftSpatialFrame,
  screenshot_dimensions: (u32, u32),
  is_minecraft_window: bool,
  target: &MinecraftBlockTarget,
  max_capture_skew_ms: Option<i64>,
) -> Result<ProjectionAssessment, String> {
  let projection_scale = ProjectionScale::for_dimensions(screenshot_dimensions.0, screenshot_dimensions.1, &frame);
  let projector = MinecraftProjector::new(frame.clone())?;
  let mut projected = projector.project_block_target(target)?;

  if let Some(scale) = projection_scale {
    if let Some(ref mut screen_point) = projected.screen_point {
      scale.apply_to_point(screen_point);
    }
    scale.apply_to_radius(&mut projected.match_radius_px);
  }

  let refusal = evaluate_mismatch_refusal_with_capture(&frame, &projected, target, true, is_minecraft_window, max_capture_skew_ms);
  let artifact = projector.build_projection_artifact(Some(projected.clone()), None);
  if refusal.refused {
    return Ok(ProjectionAssessment::Refused {
      artifact: artifact.with_mismatch_refusal_reason(refusal.reason),
      refusal,
    });
  }

  Ok(ProjectionAssessment::Bound {
    artifact,
    raycast_hit: frame.raycast_hit.clone(),
  })
}
