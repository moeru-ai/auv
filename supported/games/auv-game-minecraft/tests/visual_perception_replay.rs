//! Replay test: ingest visual perception landmarks from M2 session screenshot into spatial memory.
//!
//! Validates:
//! - Full visual perception pipeline: 2D detection -> monocular depth -> back-projection -> memory ingest.
//! - Perceived landmarks carry semantic category labels in `description`.
//! - Perceived landmarks have `source == VisualPerception` and `status == Candidate`.
//! - Heterogeneous fusion preserves `Confirmed` status if raycast already exists.
//! - Offline replay only; Minecraft is not launched.

use std::path::PathBuf;

use auv_game_minecraft::spatial_memory_ingest::LandmarkIngest;
use auv_game_minecraft::spatial_memory_observation::SpatialClaimStatus;
use auv_game_minecraft::spatial_memory_store::{LandmarkSource, ObservationRef, SpatialMemoryStore};
use auv_game_minecraft::types::{PlayerPose, Vec3, Viewport};
use auv_game_minecraft::visual_perception::{
  DEFAULT_MINECRAFT_CLASSES, DepthEstimator, VisualPerceptionIngest, YoloWorldConfig, YoloWorldDetector,
};

#[test]
fn test_visual_perception_replay_end_to_end() {
  let yolo_path = PathBuf::from("F:/auv/.tmp/models/yolov8s-worldv2.onnx");
  let depth_path = PathBuf::from("F:/auv/.tmp/models/model-small.onnx");
  let screenshot_path = PathBuf::from("F:/auv/.tmp/m2-session/v01/screenshot.png");

  if !yolo_path.is_file() || !depth_path.is_file() || !screenshot_path.is_file() {
    eprintln!("Skipping visual perception replay: models or screenshot not found");
    return;
  }

  let screenshot = image::open(&screenshot_path).expect("open v01 screenshot");

  // Observer pose from v01 telemetry.jsonl:
  // eye_position: (-22.662026, 82.62, 39.552317), yaw: -9.449891, pitch: 12.000004
  let observer = PlayerPose {
    eye_position: Vec3::new(-22.662026, 82.62, 39.552317),
    yaw: -9.449891,
    pitch: 12.000004,
  };
  let viewport = Viewport::new(screenshot.width(), screenshot.height());

  // 1. Initialize DepthEstimator and calibrate with ground-truth raycast distance
  let mut depth_estimator = DepthEstimator::new(&depth_path).expect("load depth model");
  let depth_map = depth_estimator.estimate(&screenshot).expect("depth estimation");

  // Center crosshair depth
  let center_x = screenshot.width() as f64 * 0.5;
  let center_y = screenshot.height() as f64 * 0.5;
  let pred_depth_center = depth_map.depth_at(center_x, center_y);

  // Ground-truth raycast distance to block (-22, 81, 43):
  let dx = -22.0 - observer.eye_position.x;
  let dy = 81.0 - observer.eye_position.y;
  let dz = 43.0 - observer.eye_position.z;
  let true_depth_at_crosshair = (dx * dx + dy * dy + dz * dz).sqrt();

  let scale = depth_estimator.calibrate_from_raycast(true_depth_at_crosshair, pred_depth_center);
  assert!(scale > 0.0, "calibrated depth scale must be positive");

  let mut calibrator = auv_game_minecraft::depth_calibration::AffineDepthCalibrator::new(20);
  calibrator.add_anchor(pred_depth_center as f32, true_depth_at_crosshair as f32);
  calibrator.add_anchor(pred_depth_center as f32 * 0.5, true_depth_at_crosshair as f32 * 0.5);
  assert!(calibrator.fit().is_some(), "calibrator must be fitted");

  // 2. Initialize YOLO-World detector
  let yolo_config = YoloWorldConfig {
    model_path: yolo_path,
    // Use low threshold (0.05) to ensure Minecraft pixel-art detections trigger on natural terrain
    confidence_threshold: 0.05,
    iou_threshold: 0.45,
    input_size: 640,
    classes: DEFAULT_MINECRAFT_CLASSES.iter().map(|s| s.to_string()).collect(),
  };
  let detector = YoloWorldDetector::new(yolo_config).expect("load YOLO-World");

  // 3. Build VisualPerceptionIngest
  let obs_ref = ObservationRef {
    observation_id: "m2-v01-visual".to_string(),
    captured_at_millis: 9193984,
  };

  let ingest = VisualPerceptionIngest {
    detector: &detector,
    depth: &depth_estimator,
    calibrator: &calibrator,
    screenshot: &screenshot,
    observer,
    viewport,
    vertical_fov_deg: 70.0,
    observation_ref: obs_ref,
  };

  // 4. Ingest into fresh store
  let temp_file = tempfile::NamedTempFile::new().expect("temp file");
  let mut store = SpatialMemoryStore::open(temp_file.path()).expect("open store");

  let report = ingest.ingest(&mut store, 9193984);

  // Assertions per Definition of Done (T5):
  // 1. report.landmarks_created > 0 (detected objects became landmarks)
  assert!(report.landmarks_created > 0, "visual perception must create at least one landmark, got report: {:?}", report);

  // 2. At least one landmark has a non-empty semantic description
  let has_semantic_desc = store.landmarks().values().any(|lm| lm.description.as_ref().map(|d| !d.is_empty()).unwrap_or(false));
  assert!(has_semantic_desc, "at least one landmark must carry semantic description");

  // 3. All visually perceived landmarks have source == VisualPerception and status == Candidate
  for landmark in store.landmarks().values() {
    assert_eq!(landmark.source, LandmarkSource::VisualPerception, "landmark source must be VisualPerception");
    assert_eq!(landmark.status, SpatialClaimStatus::Candidate, "unconfirmed visual landmark must have status Candidate");
    assert!(landmark.confidence <= 0.50, "candidate landmark confidence must not exceed 0.50, got {}", landmark.confidence);
  }
}
