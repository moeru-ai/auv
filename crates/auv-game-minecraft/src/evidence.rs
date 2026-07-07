use image::RgbImage;

use crate::artifact::MinecraftProjectionArtifact;
use crate::bind::bind_capture_to_frame;
use crate::overlay::render_projection_overlay;
use crate::projection::MinecraftProjector;
use crate::types::{MinecraftBlockTarget, MinecraftSpatialFrame, RaycastHit};
use crate::verify::{MismatchRefusal, evaluate_mismatch_refusal};

/// A real captured screenshot plus the monotonic timestamp taken at the capture
/// instant. The image is owned so the overlay can be drawn onto it.
#[derive(Clone, Debug)]
pub struct ScreenshotCapture {
  pub image: RgbImage,
  pub artifact_ref: String,
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

  let refusal = evaluate_mismatch_refusal(&frame, &projected, target, is_minecraft_window, max_capture_skew_ms);
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

#[cfg(test)]
mod tests {
  use image::{Rgb, RgbImage};

  use super::*;
  use crate::types::{BlockPosition, MinecraftBlockTarget, PlayerPose, RaycastHit, Vec3, Viewport};
  use crate::verify::MismatchRefusalReason;

  fn identity_matrix() -> [f64; 16] {
    [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
  }

  fn frame_at(ts: u64) -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: "frame-1".to_string(),
      world_tick: 1,
      monotonic_timestamp_ms: ts,
      telemetry_session_id: None,
      viewport: Viewport::new(64, 64),
      view_matrix: identity_matrix(),
      projection_matrix: identity_matrix(),
      player_pose: PlayerPose {
        eye_position: Vec3::new(0.0, 0.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: Some(RaycastHit {
        block_pos: BlockPosition::new(0, 0, 0),
        face: crate::types::BlockFace::North,
        block_id: "minecraft:stone".to_string(),
      }),
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: None,
      resource_pack_ids: Vec::new(),
    }
  }

  fn capture_at(ts: u64, is_minecraft_window: bool) -> ScreenshotCapture {
    ScreenshotCapture {
      image: RgbImage::from_pixel(64, 64, Rgb([0, 0, 0])),
      artifact_ref: "shot.png".to_string(),
      capture_monotonic_timestamp_ms: ts,
      is_minecraft_window,
      screenshot_dimensions: None,
    }
  }

  fn capture_with_size(ts: u64, is_minecraft_window: bool, width: u32, height: u32) -> ScreenshotCapture {
    ScreenshotCapture {
      image: RgbImage::from_pixel(width, height, Rgb([0, 0, 0])),
      artifact_ref: "shot.png".to_string(),
      capture_monotonic_timestamp_ms: ts,
      is_minecraft_window,
      screenshot_dimensions: None,
    }
  }

  // A block centered slightly in front of the camera so it projects to a visible
  // point inside the 64x64 viewport. The center of the block AABB projects to the
  // viewport center under the identity matrices used here.
  fn visible_target() -> MinecraftBlockTarget {
    MinecraftBlockTarget::new(BlockPosition::new(0, 0, 0))
  }

  #[test]
  fn refuses_when_capture_skew_exceeds_tolerance() {
    // skew = 2600 - 2000 = 600ms, tolerance 250ms.
    let evidence =
      build_projection_evidence(frame_at(2_600), capture_at(2_000, true), &visible_target(), Some(250)).expect("evidence builds");

    assert!(evidence.is_refused());
    if let ProjectionEvidence::Refused { refusal, artifact } = evidence {
      assert_eq!(refusal.reason, Some(MismatchRefusalReason::CaptureSkewUnreliable));
      // The artifact still records what was projected, even on refusal.
      assert_eq!(artifact.spatial_frame_id, "frame-1");
    } else {
      panic!("expected refusal");
    }
  }

  #[test]
  fn refuses_when_window_is_not_minecraft() {
    let evidence =
      build_projection_evidence(frame_at(1_000), capture_at(1_000, false), &visible_target(), Some(250)).expect("evidence builds");

    assert!(evidence.is_refused());
    if let ProjectionEvidence::Refused { refusal, .. } = evidence {
      assert_eq!(refusal.reason, Some(MismatchRefusalReason::NotMinecraftWindow));
    } else {
      panic!("expected refusal");
    }
  }

  #[test]
  fn binds_and_overlays_when_in_tolerance_and_visible() {
    // Zero skew, minecraft window, target projects inside the viewport.
    let evidence =
      build_projection_evidence(frame_at(1_000), capture_at(1_000, true), &visible_target(), Some(250)).expect("evidence builds");

    match evidence {
      ProjectionEvidence::Bound { artifact, overlay } => {
        assert_eq!(overlay.width(), 64);
        assert_eq!(overlay.height(), 64);
        // The raycast badge is drawn at (6,6) when a raycast hit is present.
        assert_eq!(overlay.get_pixel(6, 6), &Rgb([0, 255, 255]));
        assert_eq!(artifact.spatial_frame_id, "frame-1");
        assert!(artifact.projected_point.is_some());
      }
      ProjectionEvidence::Refused { refusal, .. } => {
        panic!("expected a bound overlay, got refusal: {:?}", refusal.reason);
      }
    }
  }

  #[test]
  fn scales_projection_to_hidpi_capture_dimensions() {
    let base =
      build_projection_evidence(frame_at(1_000), capture_at(1_000, true), &visible_target(), Some(250)).expect("base evidence builds");
    let hidpi = build_projection_evidence(frame_at(1_000), capture_with_size(1_000, true, 128, 128), &visible_target(), Some(250))
      .expect("hidpi evidence builds");

    let (base_artifact, _base_overlay) = match base {
      ProjectionEvidence::Bound { artifact, overlay } => (artifact, overlay),
      ProjectionEvidence::Refused { refusal, .. } => {
        panic!("expected bound base evidence, got {:?}", refusal.reason)
      }
    };
    let (hidpi_artifact, hidpi_overlay) = match hidpi {
      ProjectionEvidence::Bound { artifact, overlay } => (artifact, overlay),
      ProjectionEvidence::Refused { refusal, .. } => {
        panic!("expected bound hidpi evidence, got {:?}", refusal.reason)
      }
    };

    let base_point = base_artifact.projected_point.as_ref().and_then(|point| point.screen_point).expect("base projected point");
    let hidpi_point = hidpi_artifact.projected_point.as_ref().and_then(|point| point.screen_point).expect("hidpi projected point");

    assert_eq!(hidpi_overlay.width(), 128);
    assert_eq!(hidpi_overlay.height(), 128);
    assert_eq!(hidpi_overlay.get_pixel(6, 6), &Rgb([0, 255, 255]));
    assert!((hidpi_point.x - (base_point.x * 2.0)).abs() < 1e-6);
    assert!((hidpi_point.y - (base_point.y * 2.0)).abs() < 1e-6);
    let base_radius = base_artifact.projected_point.as_ref().expect("base projected point").match_radius_px;
    let hidpi_radius = hidpi_artifact.projected_point.as_ref().expect("hidpi projected point").match_radius_px;
    assert!((hidpi_radius - (base_radius * 2.0)).abs() < 1e-6);
  }

  #[test]
  fn uses_explicit_screenshot_dimensions_over_image_dimensions() {
    let base =
      build_projection_evidence(frame_at(1_000), capture_at(1_000, true), &visible_target(), Some(250)).expect("base evidence builds");

    let mut explicit_capture = capture_at(1_000, true);
    // `screenshot_dimensions` is the projection scaling basis; the overlay
    // canvas remains the owned screenshot image.
    explicit_capture.screenshot_dimensions = Some((128, 128));
    let explicit =
      build_projection_evidence(frame_at(1_000), explicit_capture, &visible_target(), Some(250)).expect("explicit evidence builds");

    let (base_artifact, _base_overlay) = match base {
      ProjectionEvidence::Bound { artifact, overlay } => (artifact, overlay),
      ProjectionEvidence::Refused { refusal, .. } => {
        panic!("expected bound base evidence, got {:?}", refusal.reason)
      }
    };
    let (explicit_artifact, explicit_overlay) = match explicit {
      ProjectionEvidence::Bound { artifact, overlay } => (artifact, overlay),
      ProjectionEvidence::Refused { refusal, .. } => {
        panic!("expected bound explicit evidence, got {:?}", refusal.reason)
      }
    };

    let base_point = base_artifact.projected_point.as_ref().and_then(|point| point.screen_point).expect("base projected point");
    let explicit_point = explicit_artifact.projected_point.as_ref().and_then(|point| point.screen_point).expect("explicit projected point");

    assert_eq!(explicit_overlay.width(), 64);
    assert_eq!(explicit_overlay.height(), 64);
    assert!((explicit_point.x - (base_point.x * 2.0)).abs() < 1e-6);
    assert!((explicit_point.y - (base_point.y * 2.0)).abs() < 1e-6);
    let base_radius = base_artifact.projected_point.as_ref().expect("base projected point").match_radius_px;
    let explicit_radius = explicit_artifact.projected_point.as_ref().expect("explicit projected point").match_radius_px;
    assert!((explicit_radius - (base_radius * 2.0)).abs() < 1e-6);
  }
}
