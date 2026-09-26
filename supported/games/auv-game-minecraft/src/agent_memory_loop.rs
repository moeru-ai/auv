//! Agent memory loop: 1Hz execution cycle integrating telemetry raycast,
//! visual perception, and memory lifecycle maintenance.
//!
//! Enforces the 4 operational rules in executable code:
//! 1. Telemetry Required: If mod telemetry pose is missing, `tick()` rejects immediately
//!    with `Err(LoopError::TelemetryRequired)`.
//! 2. Zero-Anchor Depth Gate: If depth calibrator has no fitted anchors, visual back-projection
//!    is skipped without guessing or fabricating coordinates.
//! 3. High Precision Perception: YOLO confidence threshold defaults to 0.50, filtering out
//!    noisy Minecraft texture false positives.
//! 4. Bounded Ingest Cycle: 1Hz tick rate (`tick_interval_millis = 1000`) for stable memory ingest.

use std::fmt;
use std::time::Instant;

use image::DynamicImage;
use serde::{Deserialize, Serialize};

use crate::depth_calibration::AffineDepthCalibrator;
use crate::memory_maintenance::MemoryMaintenance;
use crate::spatial_memory_ingest::LandmarkIngest;
use crate::spatial_memory_store::{ObservationRef, SpatialMemoryStore};
use crate::types::{PlayerPose, RaycastHit, Viewport};
use crate::visual_perception::{DepthEstimator, VisualPerceptionIngest, YoloWorldDetector};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentMemoryLoopConfig {
  /// Tick interval in milliseconds (default: 1000ms / 1Hz, Rule #4).
  pub tick_interval_millis: u64,
  /// YOLO-World confidence threshold (default: 0.50, Rule #3).
  pub yolo_confidence_threshold: f64,
  /// Whitelist of static categories to ingest into durable memory.
  pub static_whitelist: Vec<String>,
  /// Enforce telemetry requirement (default: true, Rule #1).
  pub require_mod_telemetry: bool,
}

impl Default for AgentMemoryLoopConfig {
  fn default() -> Self {
    Self {
      tick_interval_millis: 1000,
      yolo_confidence_threshold: 0.50,
      static_whitelist: vec![
        "chest".to_string(),
        "furnace".to_string(),
        "crafting table".to_string(),
        "door".to_string(),
        "bed".to_string(),
        "torch".to_string(),
      ],
      require_mod_telemetry: true,
    }
  }
}

/// Unified live capture packet for one agent tick.
#[derive(Clone, Debug)]
pub struct LiveCapture {
  pub screenshot: Option<DynamicImage>,
  pub player_pose: Option<PlayerPose>,
  pub raycast_hit: Option<RaycastHit>,
  pub monotonic_timestamp_ms: u64,
  pub observation_id: String,
  pub viewport: Option<Viewport>,
  pub vertical_fov_deg: Option<f64>,
}

impl LiveCapture {
  pub fn new(
    observation_id: impl Into<String>,
    monotonic_timestamp_ms: u64,
    player_pose: Option<PlayerPose>,
    raycast_hit: Option<RaycastHit>,
    screenshot: Option<DynamicImage>,
  ) -> Self {
    Self {
      screenshot,
      player_pose,
      raycast_hit,
      monotonic_timestamp_ms,
      observation_id: observation_id.into(),
      viewport: None,
      vertical_fov_deg: None,
    }
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

/// Report produced at the end of every tick.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TickReport {
  pub landmarks_created: usize,
  pub landmarks_merged: usize,
  pub observations_skipped: usize,
  pub pruned_count: usize,
  pub visual_skipped_reason: Option<String>,
  pub elapsed_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoopError {
  TelemetryRequired,
  ModelInference(String),
  StoreError(String),
}

impl fmt::Display for LoopError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::TelemetryRequired => write!(f, "Telemetry required: agent memory loop requires mod telemetry pose"),
      Self::ModelInference(err) => write!(f, "Model inference error: {err}"),
      Self::StoreError(err) => write!(f, "Store error: {err}"),
    }
  }
}

impl std::error::Error for LoopError {}

pub struct AgentMemoryLoop {
  store: SpatialMemoryStore,
  calibrator: AffineDepthCalibrator,
  maintenance: MemoryMaintenance,
  detector: Option<YoloWorldDetector>,
  depth_estimator: Option<DepthEstimator>,
  config: AgentMemoryLoopConfig,
}

impl AgentMemoryLoop {
  pub fn new(store: SpatialMemoryStore, config: AgentMemoryLoopConfig) -> Self {
    Self {
      store,
      calibrator: AffineDepthCalibrator::new(20),
      maintenance: MemoryMaintenance::default(),
      detector: None,
      depth_estimator: None,
      config,
    }
  }

  pub fn with_calibrator(mut self, calibrator: AffineDepthCalibrator) -> Self {
    self.calibrator = calibrator;
    self
  }

  pub fn with_maintenance(mut self, maintenance: MemoryMaintenance) -> Self {
    self.maintenance = maintenance;
    self
  }

  pub fn with_models(mut self, detector: YoloWorldDetector, depth: DepthEstimator) -> Self {
    self.detector = Some(detector);
    self.depth_estimator = Some(depth);
    self
  }

  pub fn store(&self) -> &SpatialMemoryStore {
    &self.store
  }

  pub fn store_mut(&mut self) -> &mut SpatialMemoryStore {
    &mut self.store
  }

  pub fn calibrator(&self) -> &AffineDepthCalibrator {
    &self.calibrator
  }

  pub fn calibrator_mut(&mut self) -> &mut AffineDepthCalibrator {
    &mut self.calibrator
  }

  pub fn maintenance(&self) -> &MemoryMaintenance {
    &self.maintenance
  }

  pub fn maintenance_mut(&mut self) -> &mut MemoryMaintenance {
    &mut self.maintenance
  }

  pub fn config(&self) -> &AgentMemoryLoopConfig {
    &self.config
  }

  pub fn config_mut(&mut self) -> &mut AgentMemoryLoopConfig {
    &mut self.config
  }

  /// Execute one agent tick: capture -> raycast ingest & calibration -> visual ingest -> maintenance -> tick report.
  pub fn tick(&mut self, capture: &LiveCapture) -> Result<TickReport, LoopError> {
    // 1. Enforce Rule #1: Mod telemetry is required if configured
    if self.config.require_mod_telemetry && capture.player_pose.is_none() {
      return Err(LoopError::TelemetryRequired);
    }

    let start = Instant::now();
    let mut report = TickReport::default();

    // 2. Lifecycle maintenance
    let pruned = self.maintenance.maybe_prune(&mut self.store, capture.monotonic_timestamp_ms);
    report.pruned_count = pruned;

    let obs_ref = ObservationRef {
      observation_id: capture.observation_id.clone(),
      captured_at_millis: capture.monotonic_timestamp_ms,
    };

    // 3. Telemetry Raycast Ingest
    if let Some(hit) = &capture.raycast_hit {
      let before_len = self.store.len();
      self.store.upsert_from_raycast(hit, &obs_ref);
      if self.store.len() > before_len {
        report.landmarks_created += 1;
      } else {
        report.landmarks_merged += 1;
      }

      // Negative evidence around raycast hit
      if let Some(pose) = &capture.player_pose {
        crate::memory_maintenance::apply_raycast_negative_evidence(
          &mut self.store,
          (pose.eye_position.x, pose.eye_position.y, pose.eye_position.z),
          hit.block_pos,
          0.5,
        );
      }

      // Depth calibrator anchor generation from raycast ground truth
      if let Some(pose) = &capture.player_pose {
        if let Some(img) = &capture.screenshot {
          if let Some(depth_est) = &mut self.depth_estimator {
            if let Ok(dm) = depth_est.estimate(img) {
              let cx = img.width() as f64 * 0.5;
              let cy = img.height() as f64 * 0.5;
              let pred_center = dm.depth_at(cx, cy);
              let hit_center = hit.block_pos.center();
              let dx = hit_center.x - pose.eye_position.x;
              let dy = hit_center.y - pose.eye_position.y;
              let dz = hit_center.z - pose.eye_position.z;
              let true_dist = (dx * dx + dy * dy + dz * dz).sqrt() as f32;
              self.calibrator.add_anchor(pred_center as f32, true_dist);
            }
          }
        }
      }
    } else {
      // No raycast in this tick
      report.observations_skipped += 1;
    }

    // 4. Visual Perception Ingest
    if let Some(img) = &capture.screenshot {
      if let (Some(detector), Some(depth)) = (&self.detector, &self.depth_estimator) {
        if let Some(pose) = &capture.player_pose {
          let viewport = capture.viewport.unwrap_or_else(|| Viewport::new(img.width(), img.height()));
          let vertical_fov_deg = capture.vertical_fov_deg.unwrap_or(70.0);

          let visual_ingest = VisualPerceptionIngest {
            detector,
            depth,
            calibrator: &self.calibrator,
            screenshot: img,
            observer: *pose,
            viewport,
            vertical_fov_deg,
            observation_ref: obs_ref,
            static_whitelist: Some(self.config.static_whitelist.clone()),
            confidence_threshold: Some(self.config.yolo_confidence_threshold),
          };

          let visual_report = visual_ingest.ingest(&mut self.store, capture.monotonic_timestamp_ms);
          report.landmarks_created += visual_report.landmarks_created;
          report.landmarks_merged += visual_report.landmarks_merged;
          report.observations_skipped += visual_report.observations_skipped;
          report.visual_skipped_reason = visual_report.skipped_reason;
        }
      }
    }

    report.elapsed_millis = start.elapsed().as_millis() as u64;
    Ok(report)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::types::{BlockFace, BlockPosition, Vec3};
  use std::path::PathBuf;

  #[test]
  fn test_telemetry_required_error() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let store = SpatialMemoryStore::open(tmp.path()).unwrap();
    let mut agent_loop = AgentMemoryLoop::new(store, AgentMemoryLoopConfig::default());

    let capture = LiveCapture::new("obs-no-telem", 1000, None, None, None);
    let result = agent_loop.tick(&capture);
    assert_eq!(result, Err(LoopError::TelemetryRequired));
  }

  #[test]
  fn test_replay_3_ticks_with_gate() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let store = SpatialMemoryStore::open(tmp.path()).unwrap();
    let mut agent_loop = AgentMemoryLoop::new(store, AgentMemoryLoopConfig::default());

    let pose_v01 = PlayerPose {
      eye_position: Vec3::new(-22.662, 82.62, 39.552),
      yaw: -9.45,
      pitch: 12.0,
    };
    let hit_v01 = RaycastHit {
      block_pos: BlockPosition::new(-22, 81, 43),
      face: BlockFace::West,
      block_id: "minecraft:grass_block".to_string(),
    };

    // Tick 1: v01 with raycast -> landmark created
    let capture_1 = LiveCapture::new("obs-v01", 1000, Some(pose_v01), Some(hit_v01.clone()), None);
    let report_1 = agent_loop.tick(&capture_1).expect("tick 1 succeeds");
    assert_eq!(report_1.landmarks_created, 1);
    assert_eq!(report_1.landmarks_merged, 0);
    assert_eq!(agent_loop.store().len(), 1);

    // Tick 2: v02 with same block raycast -> landmark merged
    let pose_v02 = PlayerPose {
      eye_position: Vec3::new(-22.346, 82.62, 41.446),
      yaw: -11.1,
      pitch: 15.0,
    };
    let hit_v02 = RaycastHit {
      block_pos: BlockPosition::new(-22, 81, 43),
      face: BlockFace::Up,
      block_id: "minecraft:grass_block".to_string(),
    };
    let capture_2 = LiveCapture::new("obs-v02", 2000, Some(pose_v02), Some(hit_v02), None);
    let report_2 = agent_loop.tick(&capture_2).expect("tick 2 succeeds");
    assert_eq!(report_2.landmarks_created, 0);
    assert_eq!(report_2.landmarks_merged, 1);
    assert_eq!(agent_loop.store().len(), 1);

    // Tick 3: v03 with NO raycast hit
    let pose_v03 = PlayerPose {
      eye_position: Vec3::new(-22.089, 82.62, 36.979),
      yaw: 5.7,
      pitch: 1.2,
    };
    let capture_3 = LiveCapture::new("obs-v03", 3000, Some(pose_v03), None, None);
    let report_3 = agent_loop.tick(&capture_3).expect("tick 3 succeeds");
    assert_eq!(report_3.landmarks_created, 0);
    assert_eq!(report_3.landmarks_merged, 0);
    assert_eq!(report_3.observations_skipped, 1);
    assert_eq!(agent_loop.store().len(), 1);
  }

  #[test]
  fn test_v03_visual_perception_zero_anchor_gate() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let store = SpatialMemoryStore::open(tmp.path()).unwrap();
    let mut agent_loop = AgentMemoryLoop::new(store, AgentMemoryLoopConfig::default());

    let yolo_path = PathBuf::from("F:/auv/.tmp/models/yolov8s-worldv2.onnx");
    let depth_path = PathBuf::from("F:/auv/.tmp/models/model-small.onnx");
    let screenshot_path = PathBuf::from("F:/auv/.tmp/m2-session/v03/screenshot.png");
    if !yolo_path.is_file() || !depth_path.is_file() || !screenshot_path.is_file() {
      return;
    }

    let detector = YoloWorldDetector::new(crate::visual_perception::YoloWorldConfig {
      model_path: yolo_path,
      confidence_threshold: 0.50,
      iou_threshold: 0.45,
      input_size: 640,
      classes: crate::visual_perception::DEFAULT_MINECRAFT_CLASSES.iter().map(|s| s.to_string()).collect(),
    })
    .unwrap();
    let depth = DepthEstimator::new(&depth_path).unwrap();
    agent_loop = agent_loop.with_models(detector, depth);

    // Ensure calibrator has zero anchors
    assert_eq!(agent_loop.calibrator().fit(), None);

    let img = image::open(&screenshot_path).unwrap();
    let pose_v03 = PlayerPose {
      eye_position: Vec3::new(-22.089, 82.62, 36.979),
      yaw: 5.7,
      pitch: 1.2,
    };
    let capture = LiveCapture::new("obs-v03", 3000, Some(pose_v03), None, Some(img));
    let report = agent_loop.tick(&capture).expect("tick succeeds");

    assert_eq!(report.landmarks_created, 0);
    assert_eq!(report.visual_skipped_reason, Some("no depth calibration anchors".to_string()));
  }
}
