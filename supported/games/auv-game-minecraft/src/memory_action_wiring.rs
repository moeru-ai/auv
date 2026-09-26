//! Memory-to-action wiring: transforms spatial memory queries into
//! verified executable window clicks.
//!
//! Provides the execution bridge between SpatialMemoryStore and real-world actions:
//! 1. Resolves queried labels (e.g. "chest", "crafting table") against structured landmarks.
//! 2. Evaluates projective visibility and 3D->2D window point coordinates.
//! 3. Enforces P1 forward occlusion gating: if metric depth indicates foreground occlusion,
//!    the click is defensively refused before dispatch.
//! 4. Dispatches clicks via typed `ActionExecutor`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use auv_driver::geometry::WindowPoint;

use crate::occlusion::MetricDepthMap;
use crate::spatial_memory_query::{LandmarkTarget, QueryKind, SpatialMemoryQuery, VisibilityClass, query_spatial_memory};
use crate::spatial_memory_store::SpatialMemoryStore;
use crate::types::{MinecraftSpatialFrame, PlayerPose, Viewport};

/// Abstraction for delivering window clicks.
///
/// Implemented by `DirectWindowPointClickExecutor` in production live mode,
/// and by `MockActionExecutor` in testing and verification harnesses.
pub trait ActionExecutor {
  fn click(&self, point: WindowPoint) -> Result<auv_driver::InputActionResult, String>;
}

/// Mock executor for testing and offline harness validation.
#[derive(Default)]
pub struct MockActionExecutor {
  pub clicked_points: Mutex<Vec<WindowPoint>>,
  pub should_fail: AtomicBool,
}

impl MockActionExecutor {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn fail_with_error(&self) {
    self.should_fail.store(true, Ordering::SeqCst);
  }

  pub fn clicks(&self) -> Vec<WindowPoint> {
    self.clicked_points.lock().unwrap().clone()
  }
}

impl ActionExecutor for MockActionExecutor {
  fn click(&self, point: WindowPoint) -> Result<auv_driver::InputActionResult, String> {
    if self.should_fail.load(Ordering::SeqCst) {
      return Err("mock click failed".to_string());
    }
    self.clicked_points.lock().unwrap().push(point);
    Ok(auv_driver::InputActionResult::single_success(auv_driver::InputDeliveryPath::WindowTargetedMouse))
  }
}

/// Typed query asking memory for a semantic target from an observer viewpoint.
#[derive(Clone, Debug)]
pub struct MemoryActionQuery<'a> {
  pub label: String,
  pub observer: PlayerPose,
  pub observer_frame: Option<MinecraftSpatialFrame>,
  pub viewport: Option<Viewport>,
  pub vertical_fov_deg: Option<f64>,
  pub depth_map: Option<&'a MetricDepthMap>,
}

impl<'a> MemoryActionQuery<'a> {
  pub fn new(label: impl Into<String>, observer: PlayerPose) -> Self {
    Self {
      label: label.into(),
      observer,
      observer_frame: None,
      viewport: None,
      vertical_fov_deg: None,
      depth_map: None,
    }
  }

  pub fn with_depth_map(mut self, depth_map: &'a MetricDepthMap) -> Self {
    self.depth_map = Some(depth_map);
    self
  }

  pub fn with_frame(mut self, frame: MinecraftSpatialFrame) -> Self {
    self.observer_frame = Some(frame);
    self
  }

  pub fn with_viewport(mut self, viewport: Viewport) -> Self {
    self.viewport = Some(viewport);
    self
  }

  pub fn with_vertical_fov(mut self, fov: f64) -> Self {
    self.vertical_fov_deg = Some(fov);
    self
  }
}

/// Outcome of attempting to wire a memory query to a physical click action.
#[derive(Clone, Debug, PartialEq)]
pub struct MemoryActionOutcome {
  pub attempted: bool,
  pub window_point: Option<WindowPoint>,
  pub refusal_reason: Option<String>,
  pub known_limits: Vec<String>,
}

/// Agent queries spatial memory and executes a click action if target is visible and unoccluded.
///
/// Steps:
/// 1. Finds landmark matching label in store.
/// 2. Performs geometric query (projection + occlusion check).
/// 3. If occluded, out-of-frustum, or unknown, refuses action without dispatching input.
/// 4. If visible, dispatches click to executor at projected window coordinates.
pub fn wire_memory_query_to_action(
  store: &SpatialMemoryStore,
  query: &MemoryActionQuery,
  executor: &impl ActionExecutor,
) -> MemoryActionOutcome {
  let lower_label = query.label.to_lowercase();
  let matched_landmark = store.landmarks().values().find(|lm| {
    if let Some(desc) = &lm.description {
      if desc.to_lowercase().contains(&lower_label) {
        return true;
      }
    }
    for obs in &lm.observations {
      if let Some(bid) = &obs.block_id {
        if bid.to_lowercase().contains(&lower_label) {
          return true;
        }
      }
    }
    if lm.landmark_id.to_lowercase().contains(&lower_label) {
      return true;
    }
    false
  });

  let Some(landmark) = matched_landmark else {
    return MemoryActionOutcome {
      attempted: false,
      window_point: None,
      refusal_reason: Some("no such landmark".to_string()),
      known_limits: vec![],
    };
  };

  let mut spatial_query =
    SpatialMemoryQuery::new(query.observer, LandmarkTarget::LandmarkId(landmark.landmark_id.clone()), QueryKind::ScreenProjection);
  if let Some(frame) = &query.observer_frame {
    spatial_query.observer_frame = Some(frame.clone());
  }
  if let Some(vp) = query.viewport {
    spatial_query = spatial_query.with_viewport(vp);
  }
  if let Some(fov) = query.vertical_fov_deg {
    spatial_query = spatial_query.with_vertical_fov(fov);
  }

  let answer = query_spatial_memory(store, &spatial_query, query.depth_map);

  match answer.visibility {
    VisibilityClass::Occluded => MemoryActionOutcome {
      attempted: false,
      window_point: None,
      refusal_reason: Some("target occluded".to_string()),
      known_limits: answer.limitations,
    },
    VisibilityClass::OutOfFrustum => MemoryActionOutcome {
      attempted: false,
      window_point: None,
      refusal_reason: Some("target out of frustum".to_string()),
      known_limits: answer.limitations,
    },
    VisibilityClass::Unknown => MemoryActionOutcome {
      attempted: false,
      window_point: None,
      refusal_reason: Some("target visibility unknown".to_string()),
      known_limits: answer.limitations,
    },
    VisibilityClass::Visible => {
      let Some((sx, sy)) = answer.screen_xy else {
        return MemoryActionOutcome {
          attempted: false,
          window_point: None,
          refusal_reason: Some("visible target missing screen projection".to_string()),
          known_limits: answer.limitations,
        };
      };

      let window_point = WindowPoint::new(sx, sy);
      match executor.click(window_point) {
        Ok(_) => MemoryActionOutcome {
          attempted: true,
          window_point: Some(window_point),
          refusal_reason: None,
          known_limits: answer.limitations,
        },
        Err(err) => MemoryActionOutcome {
          attempted: true,
          window_point: Some(window_point),
          refusal_reason: Some(err),
          known_limits: answer.limitations,
        },
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::occlusion::MetricDepthMap;
  use crate::spatial_memory_store::{ObservationRef, SpatialMemoryStore};
  use crate::types::{BlockFace, BlockPosition, RaycastHit, Vec3};

  fn sample_frame(pose: PlayerPose, hit: Option<RaycastHit>) -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: "test-frame".to_string(),
      world_tick: 100,
      monotonic_timestamp_ms: 1000,
      telemetry_session_id: Some("test-session".to_string()),
      viewport: Viewport::new(854, 480),
      view_matrix: [
        -0.98643, 0.034136, -0.160596, 0.0, 0.0, 0.978148, 0.207912, 0.0, 0.164184, 0.20509, -0.964874, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      projection_matrix: [
        0.802706, 0.0, -0.0, -0.0, 0.0, 1.428148, -0.0, -0.0, 0.0, 0.0, -1.00013, -1.0, 0.0, -0.0, -0.100007, -0.0,
      ],
      player_pose: pose,
      raycast_hit: hit,
      nearby_blocks: vec![],
      nearby_entities: vec![],
      inventory_summary: vec![],
      resource_pack_ids: vec![],
      screen_state: Some("in_game".to_string()),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
    }
  }

  #[test]
  fn test_visible_target_clicks_executor() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let mut store = SpatialMemoryStore::open(tmp.path()).unwrap();

    let hit = RaycastHit {
      block_pos: BlockPosition::new(-22, 81, 43),
      face: BlockFace::West,
      block_id: "minecraft:chest".to_string(),
    };
    let obs_ref = ObservationRef {
      observation_id: "obs-chest".to_string(),
      captured_at_millis: 1000,
    };
    store.upsert_from_raycast(&hit, &obs_ref);

    let pose = PlayerPose {
      eye_position: Vec3::new(-22.662026, 82.62, 39.552317),
      yaw: -9.449891,
      pitch: 12.000004,
    };
    let frame = sample_frame(pose, Some(hit));

    let executor = MockActionExecutor::new();
    let query = MemoryActionQuery::new("chest", pose).with_frame(frame).with_viewport(Viewport::new(854, 480));

    let outcome = wire_memory_query_to_action(&store, &query, &executor);

    assert!(outcome.attempted);
    assert_eq!(outcome.refusal_reason, None);
    assert!(outcome.window_point.is_some());

    let clicks = executor.clicks();
    assert_eq!(clicks.len(), 1);
    assert_eq!(Some(clicks[0]), outcome.window_point);

    let pt = clicks[0];
    // Screen center is (427, 240); raycast target West face center is slightly offset from crosshair
    assert!((pt.0.x - 435.0).abs() < 15.0);
    assert!((pt.0.y - 253.0).abs() < 15.0);
  }

  #[test]
  fn test_occluded_target_refuses_action() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let mut store = SpatialMemoryStore::open(tmp.path()).unwrap();

    let hit = RaycastHit {
      block_pos: BlockPosition::new(-22, 81, 43),
      face: BlockFace::West,
      block_id: "minecraft:chest".to_string(),
    };
    let obs_ref = ObservationRef {
      observation_id: "obs-chest".to_string(),
      captured_at_millis: 1000,
    };
    store.upsert_from_raycast(&hit, &obs_ref);

    let pose = PlayerPose {
      eye_position: Vec3::new(-22.662026, 82.62, 39.552317),
      yaw: -9.449891,
      pitch: 12.000004,
    };
    let frame = sample_frame(pose, Some(hit));

    // Target block is at ~3.8m. We provide a depth map where screen depth is 1.0m (a wall is in front).
    let depth_map = MetricDepthMap::new(vec![1.0f32; 854 * 480], 854, 480);

    let executor = MockActionExecutor::new();
    let query = MemoryActionQuery::new("chest", pose).with_frame(frame).with_viewport(Viewport::new(854, 480)).with_depth_map(&depth_map);

    let outcome = wire_memory_query_to_action(&store, &query, &executor);

    assert!(!outcome.attempted);
    assert_eq!(outcome.refusal_reason, Some("target occluded".to_string()));
    assert_eq!(outcome.window_point, None);

    // Assert click was NOT dispatched to executor
    assert_eq!(executor.clicks().len(), 0);
  }

  #[test]
  fn test_missing_landmark_refuses_action() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let store = SpatialMemoryStore::open(tmp.path()).unwrap();

    let pose = PlayerPose {
      eye_position: Vec3::new(0.0, 64.0, 0.0),
      yaw: 0.0,
      pitch: 0.0,
    };

    let executor = MockActionExecutor::new();
    let query = MemoryActionQuery::new("diamond_block", pose);

    let outcome = wire_memory_query_to_action(&store, &query, &executor);

    assert!(!outcome.attempted);
    assert_eq!(outcome.refusal_reason, Some("no such landmark".to_string()));
    assert_eq!(outcome.window_point, None);
    assert_eq!(executor.clicks().len(), 0);
  }

  #[test]
  fn test_out_of_frustum_refuses_action() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let mut store = SpatialMemoryStore::open(tmp.path()).unwrap();

    // Target is behind camera: position (0, 64, -10) while camera looks towards positive Z (yaw = 180)
    let hit = RaycastHit {
      block_pos: BlockPosition::new(0, 64, -10),
      face: BlockFace::North,
      block_id: "minecraft:chest".to_string(),
    };
    let obs_ref = ObservationRef {
      observation_id: "obs-behind".to_string(),
      captured_at_millis: 1000,
    };
    store.upsert_from_raycast(&hit, &obs_ref);

    let pose = PlayerPose {
      eye_position: Vec3::new(0.0, 64.0, 0.0),
      yaw: 0.0, // looking towards (0, 0, 1) in MC coordinate axes
      pitch: 0.0,
    };
    let frame = sample_frame(pose, None);

    let executor = MockActionExecutor::new();
    let query = MemoryActionQuery::new("chest", pose).with_frame(frame).with_viewport(Viewport::new(854, 480));

    let outcome = wire_memory_query_to_action(&store, &query, &executor);

    assert!(!outcome.attempted);
    assert_eq!(outcome.refusal_reason, Some("target out of frustum".to_string()));
    assert_eq!(outcome.window_point, None);
    assert_eq!(executor.clicks().len(), 0);
  }
}
