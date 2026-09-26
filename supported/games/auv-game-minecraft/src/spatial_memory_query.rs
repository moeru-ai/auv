//! Deterministic geometric query engine over structured spatial memory.
//!
//! Query responses are computed purely from 3D projective geometry and angular
//! differences; no VLMs or learned 3DGS backends are consulted on the query path.

use serde::{Deserialize, Serialize};

use crate::occlusion::{MetricDepthMap, OcclusionVerdict, check_occlusion};
use crate::projection::MinecraftProjector;
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
pub enum FovSource {
  FrameTelemetry,
  QueryParam,
  Default,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisibilityClass {
  /// 在视锥内，且当前帧深度图确认前方无遮挡（或未提供深度图时视锥几何可见）。
  Visible,
  /// 在视锥内，但当前帧深度图显示前方有更近的表面遮挡。
  Occluded,
  /// 目标在视锥外（确定）。
  OutOfFrustum,
  /// 无法判断（投影失败、目标未知、未标定或投影越界——无法判断遮挡）。
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
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub vertical_fov_deg: Option<f64>,
}

impl SpatialMemoryQuery {
  pub fn new(observer_viewpoint: PlayerPose, target: LandmarkTarget, query_kind: QueryKind) -> Self {
    Self {
      observer_viewpoint,
      target,
      query_kind,
      observer_frame: None,
      viewport: None,
      vertical_fov_deg: None,
    }
  }

  pub fn with_frame(observer_frame: MinecraftSpatialFrame, target: LandmarkTarget, query_kind: QueryKind) -> Self {
    Self {
      observer_viewpoint: observer_frame.player_pose,
      target,
      query_kind,
      observer_frame: Some(observer_frame),
      viewport: None,
      vertical_fov_deg: None,
    }
  }

  pub fn with_viewport(mut self, viewport: Viewport) -> Self {
    self.viewport = Some(viewport);
    self
  }

  pub fn with_vertical_fov(mut self, fov_deg: f64) -> Self {
    self.vertical_fov_deg = Some(fov_deg);
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
  pub fov_source: FovSource,
  pub effective_fov_deg: f64,
  #[serde(default)]
  pub limitations: Vec<String>,
}

/// Query spatial memory for target visibility, screen projection, or relative direction.
///
/// Execution rules:
/// 1. Finds target in store; returns `Unknown` if missing without guessing.
/// 2. Projects target position onto observer viewport using `MinecraftProjector`.
/// 3. Occlusion is never guessed; without explicit depth evidence it remains `Unknown`.
/// 4. Confidence is determined by claim status and observation evidence count.
pub fn query_spatial_memory(store: &SpatialMemoryStore, q: &SpatialMemoryQuery, depth_map: Option<&MetricDepthMap>) -> SpatialMemoryAnswer {
  let landmark = match &q.target {
    LandmarkTarget::LandmarkId(id) => store.get(id),
    LandmarkTarget::BlockPos(pos) => store.landmarks().values().find(|lm| {
      let dx = f64::from(lm.position.x - pos.x);
      let dy = f64::from(lm.position.y - pos.y);
      let dz = f64::from(lm.position.z - pos.z);
      (dx * dx + dy * dy + dz * dz).sqrt() < store.config().dedup_radius_m
    }),
  };

  // Resolve FOV and its provenance
  let (effective_fov_deg, fov_source) = if let Some(frame) = &q.observer_frame {
    let m5 = frame.projection_matrix[5];
    if m5 > 0.01 {
      let half_fov_rad = (1.0 / m5).atan();
      (half_fov_rad.to_degrees() * 2.0, FovSource::FrameTelemetry)
    } else if let Some(fov) = q.vertical_fov_deg {
      (fov, FovSource::QueryParam)
    } else {
      (70.0, FovSource::Default)
    }
  } else if let Some(fov) = q.vertical_fov_deg {
    (fov, FovSource::QueryParam)
  } else {
    (70.0, FovSource::Default)
  };

  let Some(landmark) = landmark else {
    return SpatialMemoryAnswer {
      status: AnswerStatus::Unknown,
      visibility: VisibilityClass::Unknown,
      screen_xy: None,
      yaw_pitch_delta: None,
      confidence: 0.0,
      evidence_observation_ids: Vec::new(),
      fov_source,
      effective_fov_deg,
      limitations: vec![
        "target not found in spatial memory store".to_string(),
        "occlusion not assessed: frustum containment only, physical occlusion unknown".to_string(),
      ],
    };
  };

  let distinct_observations: std::collections::BTreeSet<_> =
    landmark.observations.iter().map(|obs| obs.observation_ref.observation_id.clone()).collect();
  let evidence_observation_ids: Vec<String> = distinct_observations.into_iter().collect();

  let confidence = landmark.confidence;

  // Aiming point calculation:
  // NOTICE: RaycastHit provides (block_pos, face, block_id), but lacks a continuous float hit_position.
  // We use surface face_center if a surface face was recorded, otherwise falling back to voxel center (+0.5).
  // Known limitation: without sub-block hit coordinates, parallax remains at close range (<3m).
  let target_aim_point = landmark.surface_face.map(|face| landmark.position.face_center(face)).unwrap_or_else(|| landmark.position.center());

  let dx = target_aim_point.x - q.observer_viewpoint.eye_position.x;
  let dy = target_aim_point.y - q.observer_viewpoint.eye_position.y;
  let dz = target_aim_point.z - q.observer_viewpoint.eye_position.z;
  let horiz = (dx * dx + dz * dz).sqrt();
  let target_yaw = (-dx).atan2(dz).to_degrees();
  let target_pitch = (-dy).atan2(horiz).to_degrees();
  let yaw_delta = normalize_angle_deg(target_yaw - q.observer_viewpoint.yaw);
  let pitch_delta = normalize_angle_deg(target_pitch - q.observer_viewpoint.pitch);
  let yaw_pitch_delta = Some((yaw_delta, pitch_delta));

  // Frame resolution: use supplied frame or synthesize from pose, viewport, and effective_fov_deg
  let frame = if let Some(mut frame) = q.observer_frame.clone() {
    frame.player_pose = q.observer_viewpoint;
    frame
  } else {
    build_frame_from_pose(q.observer_viewpoint, q.viewport.unwrap_or(Viewport::new(854, 480)), effective_fov_deg)
  };

  let (mut visibility, screen_xy) = match MinecraftProjector::new(frame) {
    Ok(projector) => {
      let mut target_block = MinecraftBlockTarget::new(landmark.position);
      target_block.face = landmark.surface_face;
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

  let mut limitations = Vec::new();

  // If in frustum, check physical occlusion if depth map is provided:
  if visibility == VisibilityClass::Visible {
    if let Some(dm) = depth_map {
      if let Some(xy) = screen_xy {
        let landmark_dist = (dx * dx + dy * dy + dz * dz).sqrt();
        let verdict = check_occlusion((xy.0 as f32, xy.1 as f32), landmark_dist, Some(dm), 1.0);
        match verdict {
          OcclusionVerdict::Visible => {
            visibility = VisibilityClass::Visible;
            limitations.push("target confirmed visible: no foreground occlusion detected in depth map".to_string());
          }
          OcclusionVerdict::Occluded => {
            visibility = VisibilityClass::Occluded;
            limitations.push("target occluded: depth map detects foreground geometry in front of target".to_string());
          }
          OcclusionVerdict::Unknown => {
            visibility = VisibilityClass::Unknown;
            limitations.push("occlusion unknown: depth map samples unavailable or out of bounds".to_string());
          }
        }
      } else {
        limitations.push("occlusion not assessed: projected screen point unavailable".to_string());
      }
    } else {
      limitations.push("occlusion not assessed: frustum containment only, physical occlusion unknown".to_string());
    }
  } else {
    limitations.push("target outside frustum or projection failed".to_string());
  }

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

  match fov_source {
    FovSource::Default => {
      limitations.push("vertical fov defaulted to 70.0 deg (frame fov unknown)".to_string());
    }
    FovSource::QueryParam => {
      limitations.push(format!("vertical fov specified by query parameter ({effective_fov_deg:.1} deg)"));
    }
    FovSource::FrameTelemetry => {
      limitations.push(format!("vertical fov extracted from frame telemetry projection matrix ({effective_fov_deg:.1} deg)"));
    }
  }

  if landmark.surface_face.is_none() {
    limitations.push("aim point uses voxel center (+0.5); surface face unknown, subject to parallax at close range".to_string());
  } else {
    limitations.push("aim point uses surface face center from raycast hit".to_string());
  }

  SpatialMemoryAnswer {
    status,
    visibility,
    screen_xy,
    yaw_pitch_delta,
    confidence,
    evidence_observation_ids,
    fov_source,
    effective_fov_deg,
    limitations,
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
/// the specified vertical FOV and OpenGL coordinate conventions.
fn build_frame_from_pose(pose: PlayerPose, viewport: Viewport, fov_y_deg: f64) -> MinecraftSpatialFrame {
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

  let aspect = f64::from(viewport.width) / f64::from(viewport.height.max(1));
  let half_fov_rad = (fov_y_deg / 2.0).to_radians();
  let f = 1.0 / half_fov_rad.tan();

  let mut projection_matrix = [0.0; 16];
  projection_matrix[0] = f / aspect;
  projection_matrix[5] = f;
  projection_matrix[10] = -1.00013;
  projection_matrix[11] = -1.0;
  projection_matrix[14] = -0.100007;

  MinecraftSpatialFrame {
    spatial_frame_id: "synthesized-frame".to_string(),
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
  use crate::spatial_memory_store::ObservationRef;
  use crate::types::{BlockFace, MinecraftTargetSemantics, RaycastHit, Vec3};

  #[test]
  fn unknown_target_returns_unknown_status() {
    let store = SpatialMemoryStore::open("empty.json").unwrap();
    let viewpoint = PlayerPose {
      eye_position: Vec3::new(0.0, 64.0, 0.0),
      yaw: 0.0,
      pitch: 0.0,
    };
    let query = SpatialMemoryQuery::new(viewpoint, LandmarkTarget::LandmarkId("nonexistent".to_string()), QueryKind::Visibility);
    let answer = query_spatial_memory(&store, &query, None);

    assert_eq!(answer.status, AnswerStatus::Unknown);
    assert_eq!(answer.visibility, VisibilityClass::Unknown);
    assert_eq!(answer.confidence, 0.0);
    assert!(answer.screen_xy.is_none());
    assert!(answer.limitations.iter().any(|lim| lim.contains("target not found")));
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
      captured_at_millis: 1000,
    };
    store.upsert_from_raycast(&hit, &obs);

    let viewpoint = PlayerPose {
      eye_position: Vec3::new(0.0, 64.0, 0.0),
      yaw: 0.0,
      pitch: 0.0,
    };
    let query = SpatialMemoryQuery::new(viewpoint, LandmarkTarget::BlockPos(BlockPosition::new(10, 64, 10)), QueryKind::Direction);

    let answer = query_spatial_memory(&store, &query, None);
    assert_eq!(answer.status, AnswerStatus::Answered);
    let (yaw_delta, pitch_delta) = answer.yaw_pitch_delta.expect("yaw pitch delta present");
    // Target is at +X, +Z from observer -> yaw should be approx -45 degrees
    assert!((yaw_delta - (-45.0)).abs() < 2.0, "expected yaw ~ -45, got {}", yaw_delta);
    assert!(pitch_delta.abs() < 2.0, "expected pitch ~ 0, got {}", pitch_delta);
    assert_eq!(answer.confidence, 0.90);
  }

  #[test]
  fn fov_parameterization_changes_projection() {
    let mut store = SpatialMemoryStore::open("fov_mem.json").unwrap();
    let block = BlockPosition::new(0, 60, 10);
    let hit = RaycastHit {
      block_pos: block,
      face: BlockFace::North,
      block_id: "minecraft:stone".to_string(),
    };
    let obs = ObservationRef {
      observation_id: "obs-fov".to_string(),
      captured_at_millis: 1000,
    };
    store.upsert_from_raycast(&hit, &obs);

    let viewpoint = PlayerPose {
      eye_position: Vec3::new(0.0, 65.0, 0.0),
      yaw: 0.0,
      pitch: 0.0,
    };

    let q70 = SpatialMemoryQuery::new(viewpoint, LandmarkTarget::BlockPos(block), QueryKind::ScreenProjection)
      .with_viewport(Viewport::new(854, 480))
      .with_vertical_fov(70.0);
    let ans70 = query_spatial_memory(&store, &q70, None);

    let q90 = SpatialMemoryQuery::new(viewpoint, LandmarkTarget::BlockPos(block), QueryKind::ScreenProjection)
      .with_viewport(Viewport::new(854, 480))
      .with_vertical_fov(90.0);
    let ans90 = query_spatial_memory(&store, &q90, None);

    assert_eq!(ans70.fov_source, FovSource::QueryParam);
    assert_eq!(ans70.effective_fov_deg, 70.0);
    assert_eq!(ans90.fov_source, FovSource::QueryParam);
    assert_eq!(ans90.effective_fov_deg, 90.0);

    let (_x70, y70) = ans70.screen_xy.expect("screen xy 70");
    let (_x90, y90) = ans90.screen_xy.expect("screen xy 90");
    assert!((y70 - y90).abs() > 5.0, "y70 ({y70}) and y90 ({y90}) should differ due to FOV");
  }

  #[test]
  fn answer_contains_explicit_limitations() {
    let mut store = SpatialMemoryStore::open("limits_mem.json").unwrap();
    let block = BlockPosition::new(0, 64, 10);
    let hit = RaycastHit {
      block_pos: block,
      face: BlockFace::North,
      block_id: "minecraft:stone".to_string(),
    };
    let obs = ObservationRef {
      observation_id: "obs-lim".to_string(),
      captured_at_millis: 1000,
    };
    store.upsert_from_raycast(&hit, &obs);

    let viewpoint = PlayerPose {
      eye_position: Vec3::new(0.0, 64.0, 0.0),
      yaw: 0.0,
      pitch: 0.0,
    };
    let query = SpatialMemoryQuery::new(viewpoint, LandmarkTarget::BlockPos(block), QueryKind::Visibility);
    let answer = query_spatial_memory(&store, &query, None);

    assert_eq!(answer.visibility, VisibilityClass::Visible);
    assert!(answer.limitations.iter().any(|lim| lim.contains("occlusion not assessed")));
    assert!(answer.limitations.iter().any(|lim| lim.contains("vertical fov defaulted to 70.0")));
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
    let answer = query_spatial_memory(&store, &query, None);

    assert_eq!(answer.status, AnswerStatus::Answered);
    assert_eq!(answer.visibility, VisibilityClass::Visible);
    let (sx, sy) = answer.screen_xy.expect("projected screen coords");

    // Reference from M3 geometric scoring (reacquisition) targeting the same face
    let m3_query = ReacquisitionQuery {
      observer_frame: v01_frame,
      target_block: block,
      target_face: Some(BlockFace::West),
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

  #[test]
  fn query_with_depth_map_detects_occlusion() {
    let mut store = SpatialMemoryStore::open("occl_mem.json").unwrap();
    // Target is 20m ahead at (0, 64, 20)
    let block = BlockPosition::new(0, 64, 20);
    let hit = RaycastHit {
      block_pos: block,
      face: BlockFace::North,
      block_id: "minecraft:chest".to_string(),
    };
    let obs = ObservationRef {
      observation_id: "obs-chest".to_string(),
      captured_at_millis: 1000,
    };
    store.upsert_from_raycast(&hit, &obs);

    let viewpoint = PlayerPose {
      eye_position: Vec3::new(0.0, 64.0, 0.0),
      yaw: 0.0,
      pitch: 0.0,
    };
    let query =
      SpatialMemoryQuery::new(viewpoint, LandmarkTarget::BlockPos(block), QueryKind::Visibility).with_viewport(Viewport::new(854, 480));

    // Depth map indicates a wall at 5.0m across all pixels
    let depth_map = MetricDepthMap::new(vec![5.0f32; 854 * 480], 854, 480);

    let answer = query_spatial_memory(&store, &query, Some(&depth_map));
    assert_eq!(answer.status, AnswerStatus::Answered);
    assert_eq!(answer.visibility, VisibilityClass::Occluded);
    assert!(answer.limitations.iter().any(|lim| lim.contains("target occluded")));
  }

  #[test]
  fn query_with_depth_map_confirms_visibility() {
    let mut store = SpatialMemoryStore::open("vis_mem.json").unwrap();
    // Target is 4m ahead at (0, 64, 4)
    let block = BlockPosition::new(0, 64, 4);
    let hit = RaycastHit {
      block_pos: block,
      face: BlockFace::North,
      block_id: "minecraft:crafting_table".to_string(),
    };
    let obs = ObservationRef {
      observation_id: "obs-ct".to_string(),
      captured_at_millis: 1000,
    };
    store.upsert_from_raycast(&hit, &obs);

    let viewpoint = PlayerPose {
      eye_position: Vec3::new(0.0, 64.0, 0.0),
      yaw: 0.0,
      pitch: 0.0,
    };
    let query =
      SpatialMemoryQuery::new(viewpoint, LandmarkTarget::BlockPos(block), QueryKind::Visibility).with_viewport(Viewport::new(854, 480));

    // Depth map indicates wall at 10.0m (behind the target)
    let depth_map = MetricDepthMap::new(vec![10.0f32; 854 * 480], 854, 480);

    let answer = query_spatial_memory(&store, &query, Some(&depth_map));
    assert_eq!(answer.status, AnswerStatus::Answered);
    assert_eq!(answer.visibility, VisibilityClass::Visible);
    assert!(answer.limitations.iter().any(|lim| lim.contains("target confirmed visible")));
  }
}
