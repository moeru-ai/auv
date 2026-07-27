//! Viewpoint-conditioned spatial reacquisition query.
//!
//! Unlike the existing `training_result_spatial_query` module (which answers "in
//! which recorded frame was this target last seen?"), reacquisition answers:
//!
//! > Given a world-anchored target and an **observer's current frame** (matrices +
//! > viewport), where does that target project on screen right now?
//!
//! This is the minimal query shape for spatial memory: the target was learned from
//! a previous observation, and the question is whether it can be reacquired from a
//! new viewpoint without re-observing it.
//!
//! The current implementation is pure projective geometry — it uses the observer
//! frame's view + projection matrices to reproject the known block position. It
//! does not consult any learned model or 3DGS backend.
//!
//! # Evidence boundary
//!
//! This module makes the reacquisition question *expressible*. It does not yet
//! answer it well enough to serve as a measured baseline. Two gaps are open by
//! design:
//!
//! - **No occlusion test.** See `ReacquisitionStatus::Reacquired`.
//! - **Unmeasured reprojection error.** The unit tests assert self-consistency
//!   against synthetic matrices, not agreement with a second viewpoint's own
//!   truth. Scoring needs a capture holding at least two distinct camera poses;
//!   the telemetry available when this module landed held exactly one pose across
//!   every frame, so no error figure can be quoted.
//!
//! TODO(reacquisition-baseline-calibration): the working hypothesis is that
//! geometry alone suffices wherever an app exposes camera matrices, which would
//! confine learned/3DGS backends to sources that expose none. That is a
//! hypothesis, not a result — no trainer has run against this seam and no
//! reprojection error has been measured. Unlocks when a multi-pose capture
//! exists and an owner names the scoring slice.

use auv_driver::geometry::Point;

use crate::projection::MinecraftProjector;
use crate::types::{
  BlockFace, BlockPosition, MinecraftBlockTarget, MinecraftSpatialFrame, MinecraftTargetSemantics, ProjectionVisibility,
  mc6_projection_target_for_frame,
};

/// Input for a viewpoint-conditioned reacquisition query.
#[derive(Clone, Debug, PartialEq)]
pub struct ReacquisitionQuery {
  /// The observer's current spatial frame (view_matrix, projection_matrix,
  /// viewport). This is the viewpoint from which we attempt to reacquire the
  /// target — it need not have ever observed the target before.
  pub observer_frame: MinecraftSpatialFrame,
  /// World-anchored block position of the target to reacquire.
  pub target_block: BlockPosition,
  /// Optional face constraint (narrows the aim point).
  pub target_face: Option<BlockFace>,
  /// Semantic mode for aim point selection.
  pub target_semantics: MinecraftTargetSemantics,
}

/// Status of a reacquisition attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReacquisitionStatus {
  /// Target's aim point projects inside the observer's frustum and viewport.
  // NOTICE: this is containment, not confirmed visibility. `MinecraftProjector`
  // checks clip.w, NDC range, and viewport bounds only; it runs no depth or
  // occlusion comparison, and `ProjectionVisibility` has no `Occluded` variant.
  // A target behind a wall therefore reports `Reacquired` with a precise screen
  // point, so callers must not read this as "the agent can see it".
  //
  // TODO(reacquisition-occlusion): the Fabric telemetry already carries the
  // stronger signal — `MinecraftSpatialFrame::raycast_hit` is the truth source's
  // own first-hit result — so occlusion can be decided without any learned
  // backend. Deferred because a correct rule must separate "another block
  // occludes the target" from "the ray simply pointed elsewhere", which is a
  // visibility-semantics decision rather than a projection fix. Unlocks when an
  // owner names that slice.
  Reacquired,
  /// Target is geometrically not visible (behind camera, out of frustum, or
  /// outside the viewport bounds).
  NotVisible,
  /// Query could not be evaluated (e.g. invalid matrices).
  Failed,
}

/// Answer to a reacquisition query.
#[derive(Clone, Debug, PartialEq)]
pub struct ReacquisitionAnswer {
  pub status: ReacquisitionStatus,
  /// Projection visibility classification from the observer's viewpoint.
  pub visibility: Option<ProjectionVisibility>,
  /// Screen point in the observer's viewport (present only when `Reacquired`).
  pub screen_point: Option<Point>,
  /// Projected match radius in pixels (present only when `Reacquired`).
  pub match_radius_px: Option<f64>,
  /// Frame id of the observer (echo of `observer_frame.spatial_frame_id`).
  pub observer_frame_id: String,
  /// Confidence in the answer. Currently always 1.0 for the geometry backend
  /// (no uncertainty model). Reserved for learned backends that may report
  /// lower confidence when the target was never directly observed.
  // NOTICE: 1.0 is a structural placeholder meaning "this backend has no
  // uncertainty model", not a calibrated probability. It stays 1.0 for an
  // occluded target and while reprojection error remains unmeasured, so it is
  // not comparable against a learned backend's confidence.
  pub confidence: f64,
  /// If the query failed, the reason.
  pub failure_reason: Option<String>,
}

/// Evaluate a reacquisition query using pure projective geometry.
///
/// This is the candidate baseline backend: it constructs a `MinecraftProjector`
/// from the observer frame and projects the target block. No scene packet, no
/// checkpoint, no learned model — just matrices. Its accuracy is unmeasured and
/// it ignores occlusion; read the module-level evidence boundary before treating
/// its output as truth.
///
/// # Errors
///
/// Returns `Err` only on unrecoverable internal errors (non-finite matrix values
/// that `MinecraftProjector::new` rejects). Geometric non-visibility is a valid
/// `Ok` answer with `status = NotVisible`.
pub fn reacquire_from_geometry(query: &ReacquisitionQuery) -> Result<ReacquisitionAnswer, String> {
  let projector = match MinecraftProjector::new(query.observer_frame.clone()) {
    Ok(projector) => projector,
    Err(error) => {
      return Ok(ReacquisitionAnswer {
        status: ReacquisitionStatus::Failed,
        visibility: None,
        screen_point: None,
        match_radius_px: None,
        observer_frame_id: query.observer_frame.spatial_frame_id.clone(),
        confidence: 0.0,
        failure_reason: Some(error),
      });
    }
  };

  let target = build_target(&query.observer_frame, query.target_block, query.target_face, query.target_semantics);

  let projected = match projector.project_block_target(&target) {
    Ok(projected) => projected,
    Err(error) => {
      return Ok(ReacquisitionAnswer {
        status: ReacquisitionStatus::Failed,
        visibility: None,
        screen_point: None,
        match_radius_px: None,
        observer_frame_id: query.observer_frame.spatial_frame_id.clone(),
        confidence: 0.0,
        failure_reason: Some(error),
      });
    }
  };

  // NOTICE: `Visible` here is frustum + viewport containment, not unoccluded
  // visibility; see `ReacquisitionStatus::Reacquired` for the occlusion gap.
  let status = match projected.visibility {
    ProjectionVisibility::Visible => ReacquisitionStatus::Reacquired,
    _ => ReacquisitionStatus::NotVisible,
  };

  Ok(ReacquisitionAnswer {
    status,
    visibility: Some(projected.visibility),
    screen_point: projected.screen_point,
    match_radius_px: if status == ReacquisitionStatus::Reacquired {
      Some(projected.match_radius_px)
    } else {
      None
    },
    observer_frame_id: query.observer_frame.spatial_frame_id.clone(),
    confidence: 1.0,
    failure_reason: None,
  })
}

fn build_target(
  frame: &MinecraftSpatialFrame,
  block: BlockPosition,
  face: Option<BlockFace>,
  semantics: MinecraftTargetSemantics,
) -> MinecraftBlockTarget {
  let mut target = mc6_projection_target_for_frame(block, frame, semantics);
  if let Some(face) = face {
    target.face = Some(face);
    if semantics == MinecraftTargetSemantics::BlockCenter {
      target.precise_point = None;
    }
  }
  target
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::types::{PlayerPose, Vec3, Viewport};

  fn identity_matrix() -> [f64; 16] {
    [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
  }

  fn observer_frame(view_matrix: [f64; 16], projection_matrix: [f64; 16], viewport: Viewport) -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: "observer-1".to_string(),
      world_tick: 100,
      monotonic_timestamp_ms: 5000,
      telemetry_session_id: None,
      viewport,
      view_matrix,
      projection_matrix,
      player_pose: PlayerPose {
        eye_position: Vec3::new(0.0, 0.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: None,
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: None,
      resource_pack_ids: Vec::new(),
    }
  }

  fn observer_frame_with_eye(view_matrix: [f64; 16], projection_matrix: [f64; 16], viewport: Viewport, eye: Vec3) -> MinecraftSpatialFrame {
    let mut frame = observer_frame(view_matrix, projection_matrix, viewport);
    frame.player_pose.eye_position = eye;
    frame
  }

  #[test]
  fn reacquires_visible_target_from_new_viewpoint() {
    let frame = observer_frame(identity_matrix(), identity_matrix(), Viewport::new(800, 600));
    let query = ReacquisitionQuery {
      observer_frame: frame,
      target_block: BlockPosition::new(0, 0, 0),
      target_face: None,
      target_semantics: MinecraftTargetSemantics::BlockCenter,
    };

    let answer = reacquire_from_geometry(&query).expect("should not error");

    assert_eq!(answer.status, ReacquisitionStatus::Reacquired);
    assert_eq!(answer.visibility, Some(ProjectionVisibility::Visible));
    assert!(answer.screen_point.is_some());
    assert!(answer.match_radius_px.unwrap() > 0.0);
    assert_eq!(answer.observer_frame_id, "observer-1");
    assert_eq!(answer.confidence, 1.0);
    assert!(answer.failure_reason.is_none());
  }

  #[test]
  fn reports_behind_camera_as_not_visible() {
    // Negate w component so clip.w <= 0
    let projection = [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0,
    ];
    let frame = observer_frame(identity_matrix(), projection, Viewport::new(800, 600));
    let query = ReacquisitionQuery {
      observer_frame: frame,
      target_block: BlockPosition::new(0, 0, 0),
      target_face: None,
      target_semantics: MinecraftTargetSemantics::BlockCenter,
    };

    let answer = reacquire_from_geometry(&query).expect("should not error");

    assert_eq!(answer.status, ReacquisitionStatus::NotVisible);
    assert_eq!(answer.visibility, Some(ProjectionVisibility::BehindCamera));
    assert!(answer.screen_point.is_none());
    assert!(answer.match_radius_px.is_none());
  }

  #[test]
  fn reports_out_of_frustum_when_translated_far() {
    // Push the block far away via view_matrix translation
    let view = [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 5.0, 1.0,
    ];
    let frame = observer_frame(view, identity_matrix(), Viewport::new(800, 600));
    let query = ReacquisitionQuery {
      observer_frame: frame,
      target_block: BlockPosition::new(0, 0, 0),
      target_face: None,
      target_semantics: MinecraftTargetSemantics::BlockCenter,
    };

    let answer = reacquire_from_geometry(&query).expect("should not error");

    assert_eq!(answer.status, ReacquisitionStatus::NotVisible);
    assert_eq!(answer.visibility, Some(ProjectionVisibility::OutOfFrustum));
  }

  #[test]
  fn fails_gracefully_on_zero_viewport() {
    let frame = observer_frame(identity_matrix(), identity_matrix(), Viewport::new(0, 600));
    let query = ReacquisitionQuery {
      observer_frame: frame,
      target_block: BlockPosition::new(0, 0, 0),
      target_face: None,
      target_semantics: MinecraftTargetSemantics::BlockCenter,
    };

    let answer = reacquire_from_geometry(&query).expect("should not error");

    assert_eq!(answer.status, ReacquisitionStatus::Failed);
    assert!(answer.failure_reason.is_some());
    assert!(answer.failure_reason.unwrap().contains("positive dimensions"));
  }

  #[test]
  fn reacquires_with_rotation_only_fallback() {
    // Real telemetry: rotation-only view_matrix with non-zero eye position
    let frame = observer_frame_with_eye(
      [
        0.719950, 0.115742, -0.684307, 0.0, -0.0, 0.985996, 0.166769, 0.0, 0.694026, -0.120065, 0.709867, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      [
        0.802706, 0.0, -0.0, -0.0, 0.0, 1.428148, -0.0, -0.0, 0.0, 0.0, -1.000130, -1.0, -0.0, -0.0, -0.100007, -0.0,
      ],
      Viewport::new(1708, 960),
      Vec3::new(511.028439, 73.62, 728.652906),
    );
    let query = ReacquisitionQuery {
      observer_frame: frame,
      target_block: BlockPosition::new(513, 72, 726),
      target_face: None,
      target_semantics: MinecraftTargetSemantics::BlockCenter,
    };

    let answer = reacquire_from_geometry(&query).expect("should not error");

    assert_eq!(answer.status, ReacquisitionStatus::Reacquired);
    let point = answer.screen_point.expect("visible");
    assert!(point.x > 0.0 && point.x < 1708.0);
    assert!(point.y > 0.0 && point.y < 960.0);
  }

  // ROOT CAUSE:
  //
  // If the reacquisition query uses the same lookup-based approach as
  // `select_reference_frame` (find a past frame that saw the target), then
  // reacquisition from a viewpoint that never observed the target is impossible.
  //
  // The fix uses the observer's own matrices to project the world-anchored target,
  // proving that reacquisition is independent of past observation history.
  #[test]
  fn reacquires_target_never_observed_by_this_viewpoint() {
    // Observer has never seen block (5, 70, 5) — it's not in raycast_hit or
    // nearby_blocks. But the block is geometrically visible from this viewpoint.
    let frame = observer_frame(identity_matrix(), identity_matrix(), Viewport::new(800, 600));
    let query = ReacquisitionQuery {
      observer_frame: frame,
      target_block: BlockPosition::new(0, 0, 0),
      target_face: None,
      target_semantics: MinecraftTargetSemantics::BlockCenter,
    };

    // The observer frame has no raycast_hit and no nearby_blocks containing this
    // target. Yet reacquisition must succeed because the query is purely geometric.
    let answer = reacquire_from_geometry(&query).expect("should not error");
    assert_eq!(answer.status, ReacquisitionStatus::Reacquired);
    assert!(answer.screen_point.is_some());
  }
}
