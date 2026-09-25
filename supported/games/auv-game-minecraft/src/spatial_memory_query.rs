//! Deterministic geometric query engine over structured spatial memory.
//!
//! Query responses are computed purely from 3D projective geometry and angular
//! differences; no VLMs or learned 3DGS backends are consulted on the query path.

use serde::{Deserialize, Serialize};

use crate::projection::MinecraftProjector;
use crate::spatial_memory_observation::SpatialClaimStatus;
use crate::spatial_memory_store::SpatialMemoryStore;
use crate::types::{BlockPosition, MinecraftBlockTarget, MinecraftSpatialFrame, PlayerPose, ProjectionVisibility, Viewport};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LandmarkTarget {
  LandmarkId(String),
  BlockPos(BlockPosition),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryKind {
  Visibility,
  ScreenProjection,
  Direction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerStatus {
  Answered,
  Unknown,
  Refusal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisibilityClass {
  Visible,
  Occluded,
  OutOfFrustum,
  Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpatialMemoryQuery {
  pub observer_viewpoint: PlayerPose,
  pub target: LandmarkTarget,
  pub query_kind: QueryKind,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub observer_frame: Option<MinecraftSpatialFrame>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub viewport: Option<Viewport>,
}

impl SpatialMemoryQuery {
  pub fn new(observer_viewpoint: PlayerPose, target: LandmarkTarget, query_kind: QueryKind) -> Self {
    Self {
      observer_viewpoint,
      target,
      query_kind,
      observer_frame: None,
      viewport: None,
    }
  }

  pub fn with_frame(observer_frame: MinecraftSpatialFrame, target: LandmarkTarget, query_kind: QueryKind) -> Self {
    Self {
      observer_viewpoint: observer_frame.player_pose,
      target,
      query_kind,
      observer_frame: Some(observer_frame),
      viewport: None,
    }
  }

  pub fn with_viewport(mut self, viewport: Viewport) -> Self {
    self.viewport = Some(viewport);
    self
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpatialMemoryAnswer {
  pub status: AnswerStatus,
  pub visibility: VisibilityClass,
  pub screen_xy: Option<(f64, f64)>,
  pub yaw_pitch_delta: Option<(f64, f64)>,
  pub confidence: f64,
  pub evidence_observation_ids: Vec<String>,
}

/// Query spatial memory for target visibility, screen projection, or relative direction.
///
/// Execution rules:
/// 1. Finds target in store; returns `Unknown` if missing without guessing.
/// 2. Projects target position onto observer viewport using `MinecraftProjector`.
/// 3. Occlusion is never guessed; without explicit depth evidence it remains `Unknown`.
/// 4. Confidence is determined by claim status and observation evidence count.
pub fn query_spatial_memory(store: &SpatialMemoryStore, q: &SpatialMemoryQuery) -> SpatialMemoryAnswer {
  let landmark = match &q.target {
    LandmarkTarget::LandmarkId(id) => store.get(id),
    LandmarkTarget::BlockPos(pos) => store.landmarks().values().find(|lm| {
      let dx = f64::from(lm.position.x - pos.x);
      let dy = f64::from(lm.position.y - pos.y);
      let dz = f64::from(lm.position.z - pos.z);
      (dx * dx + dy * dy + dz * dz).sqrt() < 0.6
    }),
  };

  let Some(landmark) = landmark else {
    return SpatialMemoryAnswer {
      status: AnswerStatus::Unknown,
      visibility: VisibilityClass::Unknown,
      screen_xy: None,
      yaw_pitch_delta: None,
      confidence: 0.0,
      evidence_observation_ids: Vec::new(),
    };
  };

  let distinct_observations: std::collections::BTreeSet<_> =
    landmark.observations.iter().map(|obs| obs.observation_ref.observation_id.clone()).collect();
  let obs_count = distinct_observations.len();
  let evidence_observation_ids: Vec<String> = distinct_observations.into_iter().collect();

  let confidence = match landmark.status {
    SpatialClaimStatus::Confirmed => {
      let extra = (obs_count.saturating_sub(1) as f64) * 0.02;
      (0.90 + extra).min(0.99)
    }
    SpatialClaimStatus::Candidate => {
      let extra = (obs_count.saturating_sub(1) as f64) * 0.02;
      (0.70 + extra).min(0.85)
    }
    SpatialClaimStatus::Hypothesis => {
      let extra = (obs_count.saturating_sub(1) as f64) * 0.02;
      (0.40 + extra).min(0.50)
    }
  };

  // Direction calculation: yaw and pitch deflection angles from observer to target
  let target_center = landmark.position.center();
  let dx = target_center.x - q.observer_viewpoint.eye_position.x;
  let dy = target_center.y - q.observer_viewpoint.eye_position.y;
  let dz = target_center.z - q.observer_viewpoint.eye_position.z;
  let horiz = (dx * dx + dz * dz).sqrt();
  let target_yaw = (-dx).atan2(dz).to_degrees();
  let target_pitch = (-dy).atan2(horiz).to_degrees();
  let yaw_delta = normalize_angle_deg(target_yaw - q.observer_viewpoint.yaw);
  let pitch_delta = normalize_angle_deg(target_pitch - q.observer_viewpoint.pitch);
  let yaw_pitch_delta = Some((yaw_delta, pitch_delta));

  // Frame resolution: use supplied frame or synthesize from pose and viewport
  let frame = if let Some(mut frame) = q.observer_frame.clone() {
    frame.player_pose = q.observer_viewpoint;
    frame
  } else {
    build_frame_from_pose(q.observer_viewpoint, q.viewport.unwrap_or(Viewport::new(854, 480)))
  };

  let (visibility, screen_xy) = match MinecraftProjector::new(frame) {
    Ok(projector) => {
      let target_block = MinecraftBlockTarget::new(landmark.position);
      match projector.project_block_target(&target_block) {
        Ok(projected) => match projected.visibility {
          ProjectionVisibility::Visible => {
            let xy = projected.screen_point.map(|p| (p.x, p.y));
            (VisibilityClass::Visible, xy)
          }
          ProjectionVisibility::OutOfFrustum | ProjectionVisibility::BehindCamera | ProjectionVisibility::OutsideWindow => {
            (VisibilityClass::OutOfFrustum, None)
          }
        },
        Err(_) => (VisibilityClass::Unknown, None),
      }
    }
    Err(_) => (VisibilityClass::Unknown, None),
  };

  let status = match q.query_kind {
    QueryKind::Visibility => {
      if visibility == VisibilityClass::Unknown {
        AnswerStatus::Unknown
      } else {
        AnswerStatus::Answered
      }
    }
    QueryKind::ScreenProjection => {
      if screen_xy.is_some() {
        AnswerStatus::Answered
      } else {
        AnswerStatus::Unknown
      }
    }
    QueryKind::Direction => {
      if yaw_pitch_delta.is_some() {
        AnswerStatus::Answered
      } else {
        AnswerStatus::Unknown
      }
    }
  };

  SpatialMemoryAnswer {
    status,
    visibility,
    screen_xy,
    yaw_pitch_delta,
    confidence,
    evidence_observation_ids,
  }
}

fn normalize_angle_deg(delta: f64) -> f64 {
  let mut normalized = delta % 360.0;
  if normalized > 180.0 {
    normalized -= 360.0;
  } else if normalized <= -180.0 {
    normalized += 360.0;
  }
  normalized
}

/// Synthesize a valid MinecraftSpatialFrame from PlayerPose and Viewport using
/// standard Minecraft 70-degree vertical FOV and OpenGL coordinate conventions.
fn build_frame_from_pose(pose: PlayerPose, viewport: Viewport) -> MinecraftSpatialFrame {
  let psi = pose.yaw.to_radians();
  let theta = pose.pitch.to_radians();

  let cos_yaw = psi.cos();
  let sin_yaw = psi.sin();
  let cos_pitch = theta.cos();
  let sin_pitch = theta.sin();

  let mut view_matrix = [0.0; 16];
  view_matrix[0] = -cos_yaw;
  view_matrix[1] = -sin_yaw * sin_pitch;
  view_matrix[2] = sin_yaw * cos_pitch;

  view_matrix[5] = cos_pitch;
  view_matrix[6] = sin_pitch;

  view_matrix[8] = -sin_yaw;
  view_matrix[9] = cos_yaw * sin_pitch;
  view_matrix[10] = -cos_yaw * cos_pitch;

  view_matrix[15] = 1.0;

  // Standard Minecraft 70-degree vertical FOV
  let aspect = f64::from(viewport.width) / f64::from(viewport.height.max(1));
  let f = 1.0 / (35.0_f64.to_radians()).tan();

  let mut projection_matrix = [0.0; 16];
  projection_matrix[0] = f / aspect;
  projection_matrix[5] = f;
  projection_matrix[10] = -1.00013;
  projection_matrix[11] = -1.0;
  projection_matrix[14] = -0.100007;

  MinecraftSpatialFrame {
    spatial_frame_id: "synthesized_query_frame".to_string(),
    world_tick: 0,
    monotonic_timestamp_ms: 0,
    telemetry_session_id: None,
    viewport,
    view_matrix,
    projection_matrix,
    player_pose: pose,
    raycast_hit: None,
    nearby_blocks: Vec::new(),
    nearby_entities: Vec::new(),
    inventory_summary: Vec::new(),
    screenshot_artifact_ref: None,
    mc_capture_skew_ms: None,
    screen_state: Some("in_game".to_string()),
    resource_pack_ids: Vec::new(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::m3_query_scoring::M3_PROJECTION_TOLERANCE_PX;
  use crate::reacquisition::{ReacquisitionQuery, reacquire_from_geometry};
  use crate::spatial_memory_store::{ObservationRef, SpatialMemoryStore};
  use crate::types::{BlockFace, MinecraftTargetSemantics, RaycastHit, Vec3};

  #[test]
  fn unknown_target_returns_unknown_status() {
    let store = SpatialMemoryStore::open("empty.json").unwrap();
    let query = SpatialMemoryQuery::new(
      PlayerPose {
        eye_position: Vec3::new(0.0, 64.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      LandmarkTarget::LandmarkId("lm-nonexistent".to_string()),
      QueryKind::Visibility,
    );

    let answer = query_spatial_memory(&store, &query);
    assert_eq!(answer.status, AnswerStatus::Unknown);
    assert_eq!(answer.visibility, VisibilityClass::Unknown);
    assert!(answer.screen_xy.is_none());
    assert_eq!(answer.confidence, 0.0);
    assert!(answer.evidence_observation_ids.is_empty());
  }

  #[test]
  fn query_direction_computes_correct_yaw_pitch_delta() {
    let mut store = SpatialMemoryStore::open("dir_mem.json").unwrap();
    let hit = RaycastHit {
      block_pos: BlockPosition::new(10, 64, 10),
      face: BlockFace::North,
      block_id: "minecraft:stone".to_string(),
    };
    let obs = ObservationRef {
      observation_id: "obs-1".to_string(),
      captured_at_millis: 100,
    };
    store.upsert_from_raycast(&hit, &obs);

    let query = SpatialMemoryQuery::new(
      PlayerPose {
        eye_position: Vec3::new(0.5, 64.5, 0.5),
        yaw: 0.0,
        pitch: 0.0,
      },
      LandmarkTarget::BlockPos(BlockPosition::new(10, 64, 10)),
      QueryKind::Direction,
    );

    let answer = query_spatial_memory(&store, &query);
    assert_eq!(answer.status, AnswerStatus::Answered);
    let (yaw_delta, pitch_delta) = answer.yaw_pitch_delta.expect("yaw pitch delta present");
    // Target is at +X, +Z from observer (dx=10, dz=10) -> yaw should be approx -45 degrees
    assert!((yaw_delta - (-45.0)).abs() < 1.0, "expected yaw ~ -45, got {}", yaw_delta);
    assert!(pitch_delta.abs() < 1.0, "expected pitch ~ 0, got {}", pitch_delta);
    assert_eq!(answer.confidence, 0.90);
  }

  #[test]
  fn cross_validation_with_m3_scoring_tolerance() {
    let mut store = SpatialMemoryStore::open("cv_mem.json").unwrap();
    let block = BlockPosition::new(-22, 81, 43);
    let hit = RaycastHit {
      block_pos: block,
      face: BlockFace::West,
      block_id: "minecraft:grass_block".to_string(),
    };
    let obs1 = ObservationRef {
      observation_id: "v01-obs".to_string(),
      captured_at_millis: 1000,
    };
    let obs2 = ObservationRef {
      observation_id: "v02-obs".to_string(),
      captured_at_millis: 2000,
    };
    store.upsert_from_raycast(&hit, &obs1);
    store.upsert_from_raycast(&hit, &obs2);

    // Frame from v01 in live session
    let v01_frame = MinecraftSpatialFrame {
      spatial_frame_id: "frame-144173-9194252844000".to_string(),
      world_tick: 144173,
      monotonic_timestamp_ms: 9194254,
      telemetry_session_id: Some("session".to_string()),
      viewport: Viewport::new(854, 480),
      view_matrix: [
        -0.98643, 0.034136, -0.160596, 0.0, 0.0, 0.978148, 0.207912, 0.0, 0.164184, 0.20509, -0.964874, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      projection_matrix: [
        0.802706, 0.0, -0.0, -0.0, 0.0, 1.428148, -0.0, -0.0, 0.0, 0.0, -1.00013, -1.0, 0.0, -0.0, -0.100007, -0.0,
      ],
      player_pose: PlayerPose {
        eye_position: Vec3::new(-22.662026, 82.62, 39.552317),
        yaw: -9.449891,
        pitch: 12.000004,
      },
      raycast_hit: Some(hit),
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: Some("in_game".to_string()),
      resource_pack_ids: Vec::new(),
    };

    // Query spatial memory using anchor viewpoint & frame
    let query = SpatialMemoryQuery::with_frame(v01_frame.clone(), LandmarkTarget::BlockPos(block), QueryKind::ScreenProjection);
    let answer = query_spatial_memory(&store, &query);

    assert_eq!(answer.status, AnswerStatus::Answered);
    assert_eq!(answer.visibility, VisibilityClass::Visible);
    let (sx, sy) = answer.screen_xy.expect("projected screen coords");

    // Reference from M3 geometric scoring (reacquisition)
    let m3_query = ReacquisitionQuery {
      observer_frame: v01_frame,
      target_block: block,
      target_face: None,
      target_semantics: MinecraftTargetSemantics::BlockCenter,
    };
    let m3_answer = reacquire_from_geometry(&m3_query).expect("reacquire from geometry");
    let m3_pt = m3_answer.screen_point.expect("m3 screen point");

    let pixel_error = ((sx - m3_pt.x).powi(2) + (sy - m3_pt.y).powi(2)).sqrt();
    assert!(pixel_error <= M3_PROJECTION_TOLERANCE_PX, "pixel error {} px exceeds tolerance {} px", pixel_error, M3_PROJECTION_TOLERANCE_PX);

    // Multi-observation confidence: 2 observations -> 0.90 + 0.02 = 0.92
    assert_eq!(answer.confidence, 0.92);
    assert_eq!(answer.evidence_observation_ids, vec!["v01-obs", "v02-obs"]);
  }
}
