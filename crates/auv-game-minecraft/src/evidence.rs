use image::RgbImage;

use crate::artifact::MinecraftProjectionArtifact;
use crate::bind::bind_capture_to_frame;
use crate::overlay::render_projection_overlay;
use crate::projection::MinecraftProjector;
use crate::types::{MinecraftBlockTarget, MinecraftSpatialFrame};
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
    self
      .screenshot_dimensions
      .unwrap_or((self.image.width(), self.image.height()))
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
  let (screenshot_width, screenshot_height) = capture.dimensions();
  let bound = bind_capture_to_frame(
    frame,
    capture.artifact_ref,
    capture.capture_monotonic_timestamp_ms,
  );
  let projector = MinecraftProjector::new(bound.frame.clone())?;
  let mut projected = projector.project_block_target(target)?;

  // Scale projection coordinates if screenshot dimensions differ from viewport
  if screenshot_width != bound.frame.viewport.width || screenshot_height != bound.frame.viewport.height {
    let viewport_width = f64::from(bound.frame.viewport.width);
    let viewport_height = f64::from(bound.frame.viewport.height);
    let scale_x = f64::from(screenshot_width) / viewport_width;
    let scale_y = f64::from(screenshot_height) / viewport_height;

    if let Some(ref mut screen_point) = projected.screen_point {
      screen_point.x *= scale_x;
      screen_point.y *= scale_y;
    }
    projected.match_radius_px *= scale_x.max(scale_y);
  }

  let artifact = projector.build_projection_artifact(Some(projected.clone()), None);

  let refusal = evaluate_mismatch_refusal(
    &bound.frame,
    &projected,
    target,
    capture.is_minecraft_window,
    max_capture_skew_ms,
  );
  if refusal.refused {
    let artifact = artifact.with_mismatch_refusal_reason(refusal.reason);
    return Ok(ProjectionEvidence::Refused { artifact, refusal });
  }

  // Not refused implies a visible projected point with a real screen point.
  let raycast_hit = bound.frame.raycast_hit.as_ref();
  let overlay = render_projection_overlay(capture.image, &projected, raycast_hit);
  Ok(ProjectionEvidence::Bound { artifact, overlay })
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

  fn capture_with_size(
    ts: u64,
    is_minecraft_window: bool,
    width: u32,
    height: u32,
  ) -> ScreenshotCapture {
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
    let evidence = build_projection_evidence(
      frame_at(2_600),
      capture_at(2_000, true),
      &visible_target(),
      Some(250),
    )
    .expect("evidence builds");

    assert!(evidence.is_refused());
    if let ProjectionEvidence::Refused { refusal, artifact } = evidence {
      assert_eq!(
        refusal.reason,
        Some(MismatchRefusalReason::CaptureSkewUnreliable)
      );
      // The artifact still records what was projected, even on refusal.
      assert_eq!(artifact.spatial_frame_id, "frame-1");
    } else {
      panic!("expected refusal");
    }
  }

  #[test]
  fn refuses_when_window_is_not_minecraft() {
    let evidence = build_projection_evidence(
      frame_at(1_000),
      capture_at(1_000, false),
      &visible_target(),
      Some(250),
    )
    .expect("evidence builds");

    assert!(evidence.is_refused());
    if let ProjectionEvidence::Refused { refusal, .. } = evidence {
      assert_eq!(
        refusal.reason,
        Some(MismatchRefusalReason::NotMinecraftWindow)
      );
    } else {
      panic!("expected refusal");
    }
  }

  #[test]
  fn binds_and_overlays_when_in_tolerance_and_visible() {
    // Zero skew, minecraft window, target projects inside the viewport.
    let evidence = build_projection_evidence(
      frame_at(1_000),
      capture_at(1_000, true),
      &visible_target(),
      Some(250),
    )
    .expect("evidence builds");

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
        panic!(
          "expected a bound overlay, got refusal: {:?}",
          refusal.reason
        );
      }
    }
  }

  #[test]
  fn scales_projection_to_hidpi_capture_dimensions() {
    let base = build_projection_evidence(
      frame_at(1_000),
      capture_at(1_000, true),
      &visible_target(),
      Some(250),
    )
    .expect("base evidence builds");
    let hidpi = build_projection_evidence(
      frame_at(1_000),
      capture_with_size(1_000, true, 128, 128),
      &visible_target(),
      Some(250),
    )
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

    let base_point = base_artifact
      .projected_point
      .as_ref()
      .and_then(|point| point.screen_point)
      .expect("base projected point");
    let hidpi_point = hidpi_artifact
      .projected_point
      .as_ref()
      .and_then(|point| point.screen_point)
      .expect("hidpi projected point");

    assert_eq!(hidpi_overlay.width(), 128);
    assert_eq!(hidpi_overlay.height(), 128);
    assert_eq!(hidpi_overlay.get_pixel(6, 6), &Rgb([0, 255, 255]));
    assert!((hidpi_point.x - (base_point.x * 2.0)).abs() < 1e-6);
    assert!((hidpi_point.y - (base_point.y * 2.0)).abs() < 1e-6);
    let base_radius = base_artifact
      .projected_point
      .as_ref()
      .expect("base projected point")
      .match_radius_px;
    let hidpi_radius = hidpi_artifact
      .projected_point
      .as_ref()
      .expect("hidpi projected point")
      .match_radius_px;
    assert!((hidpi_radius - (base_radius * 2.0)).abs() < 1e-6);
  }
}
