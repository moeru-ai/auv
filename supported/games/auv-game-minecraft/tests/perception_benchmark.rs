//! Empirical stress and benchmark verification for auv-game-minecraft
//! visual perception and spatial memory store.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use auv_game_minecraft::depth_calibration::AffineDepthCalibrator;
use auv_game_minecraft::spatial_memory_ingest::LandmarkIngest;
use auv_game_minecraft::spatial_memory_store::{ObservationRef, SpatialMemoryConfig, SpatialMemoryStore};
use auv_game_minecraft::types::{BlockPosition, PlayerPose, Vec3, Viewport};
use auv_game_minecraft::visual_perception::{
  DEFAULT_MINECRAFT_CLASSES, DepthEstimator, VisualPerceptionIngest, YoloWorldConfig, YoloWorldDetector, back_project,
};

fn stats(times_ms: &[f64]) -> (f64, f64, f64, f64) {
  let mut sorted = times_ms.to_vec();
  sorted.sort_by(|a, b| a.total_cmp(b));
  let min = sorted[0];
  let max = *sorted.last().unwrap();
  let sum: f64 = sorted.iter().sum();
  let mean = sum / (sorted.len() as f64);
  let median = if sorted.len() % 2 == 1 {
    sorted[sorted.len() / 2]
  } else {
    (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) * 0.5
  };
  (min, max, mean, median)
}

#[test]
#[ignore = "benchmark suite for manual stress testing"]
fn test_perception_and_spatial_memory_benchmarks() {
  let yolo_path = PathBuf::from("F:/auv/.tmp/models/yolov8s-worldv2.onnx");
  let depth_midas_path = PathBuf::from("F:/auv/.tmp/models/model-small.onnx");
  let depth_da2_path = PathBuf::from("F:/auv/.tmp/models/depth_anything_v2_vits.onnx");

  let frames = ["v01", "v02", "v03"];

  println!("\n=======================================================");
  println!("PERCEPTION & SPATIAL MEMORY EMPIRICAL BENCHMARK REPORT");
  println!("=======================================================\n");

  // Load models
  println!("Loading YOLO-World detector...");
  let yolo_config = YoloWorldConfig {
    model_path: yolo_path.clone(),
    confidence_threshold: 0.05,
    iou_threshold: 0.45,
    input_size: 640,
    classes: DEFAULT_MINECRAFT_CLASSES.iter().map(|s| s.to_string()).collect(),
  };
  let mut detector = YoloWorldDetector::new(yolo_config).expect("load yolo-world");

  println!("Loading Depth Estimator (MiDaS v2.1 Small)...");
  let midas_estimator = DepthEstimator::new(&depth_midas_path).expect("load midas");

  println!("Loading Depth Estimator (Depth Anything V2 Small)...");
  let da2_estimator = DepthEstimator::new(&depth_da2_path).expect("load da2");

  // Load screenshots & telemetry
  let mut screenshots = HashMap::new();
  let mut telemetry_data = HashMap::new();

  for frame in &frames {
    let img_path = format!("F:/auv/.tmp/m2-session/{frame}/screenshot.png");
    let img = image::open(&img_path).unwrap_or_else(|_| panic!("open {img_path}"));
    let telem_path = format!("F:/auv/.tmp/m2-session/{frame}/telemetry.jsonl");
    let telem_str = std::fs::read_to_string(&telem_path).unwrap();
    let telem_line = telem_str.lines().next().unwrap();
    let telem_json: serde_json::Value = serde_json::from_str(telem_line).unwrap();

    screenshots.insert(*frame, img);
    telemetry_data.insert(*frame, telem_json);
  }

  // -------------------------------------------------------------
  // SECTION 1: YOLO-WORLD DETECTION AT DIFFERENT THRESHOLDS
  // -------------------------------------------------------------
  println!("\n-------------------------------------------------------------");
  println!("1. YOLO-World Detections across thresholds (0.05, 0.25, 0.50)");
  println!("-------------------------------------------------------------");

  let thresholds = [0.05, 0.25, 0.50];

  for frame in &frames {
    println!("\n>>> Frame: {} <<<", frame);
    let img = screenshots.get(frame).unwrap();
    let telem = telemetry_data.get(frame).unwrap();
    let pose = &telem["player_pose"];
    let raycast = &telem["raycast_hit"];
    println!(
      "  Observer Pose: eye=({}, {}, {}), yaw={}, pitch={}",
      pose["eye_position"]["x"], pose["eye_position"]["y"], pose["eye_position"]["z"], pose["yaw"], pose["pitch"]
    );
    println!(
      "  Telemetry Raycast Hit: {}",
      if raycast.is_null() {
        "None".to_string()
      } else {
        raycast.to_string()
      }
    );

    for &thresh in &thresholds {
      detector.set_confidence_threshold(thresh);
      let detections = detector.detect(img).expect("detect");
      println!("  [Conf Threshold >= {:.2}]: {} detections", thresh, detections.len());
      for (idx, det) in detections.iter().enumerate() {
        println!(
          "    #{}: label=\"{}\", conf={:.4}, bbox=({:.1}, {:.1}, {:.1}, {:.1}), center=({:.1}, {:.1})",
          idx + 1,
          det.label,
          det.confidence,
          det.bbox.0,
          det.bbox.1,
          det.bbox.2,
          det.bbox.3,
          (det.bbox.0 + det.bbox.2) * 0.5,
          (det.bbox.1 + det.bbox.3) * 0.5
        );
      }
    }
  }

  // -------------------------------------------------------------
  // SECTION 2: DEPTH ESTIMATION & V03 MISSING RAYCAST ANALYSIS
  // -------------------------------------------------------------
  println!("\n-------------------------------------------------------------");
  println!("2. Depth Estimation & v03 Missing Raycast Analysis");
  println!("-------------------------------------------------------------");

  let mut calibrated_midas_scale_v01 = 1.0;
  let mut calibrated_da2_scale_v01 = 1.0;

  for frame in &frames {
    println!("\n>>> Frame: {} <<<", frame);
    let img = screenshots.get(frame).unwrap();
    let telem = telemetry_data.get(frame).unwrap();
    let cx = img.width() as f64 * 0.5;
    let cy = img.height() as f64 * 0.5;

    let midas_map = midas_estimator.estimate(img).expect("midas estimate");
    let da2_map = da2_estimator.estimate(img).expect("da2 estimate");

    let midas_center_val = midas_map.depth_at(cx, cy);
    let da2_center_val = da2_map.depth_at(cx, cy);

    let raycast = &telem["raycast_hit"];
    if !raycast.is_null() {
      let eye_x = telem["player_pose"]["eye_position"]["x"].as_f64().unwrap();
      let eye_y = telem["player_pose"]["eye_position"]["y"].as_f64().unwrap();
      let eye_z = telem["player_pose"]["eye_position"]["z"].as_f64().unwrap();
      let hit_x = raycast["block_pos"]["x"].as_f64().unwrap();
      let hit_y = raycast["block_pos"]["y"].as_f64().unwrap();
      let hit_z = raycast["block_pos"]["z"].as_f64().unwrap();
      let true_dist = ((hit_x - eye_x).powi(2) + (hit_y - eye_y).powi(2) + (hit_z - eye_z).powi(2)).sqrt();

      let midas_scale = true_dist / midas_center_val;
      let da2_scale = true_dist / da2_center_val;

      if *frame == "v01" {
        calibrated_midas_scale_v01 = midas_scale;
        calibrated_da2_scale_v01 = da2_scale;
      }

      println!("  Raycast Ground Truth Hit: ({}, {}, {}), True Distance = {:.4}m", hit_x, hit_y, hit_z, true_dist);
      println!("  MiDaS Raw Crosshair Value: {:.4}, Calibrated Scale = {:.6}", midas_center_val, midas_scale);
      println!("  DA2 Raw Crosshair Value: {:.4}, Calibrated Scale = {:.6}", da2_center_val, da2_scale);
    } else {
      println!("  Raycast Ground Truth: NONE (Ray missed / player looking at open horizon)");
      println!("  MiDaS Raw Crosshair Value: {:.4}", midas_center_val);
      println!("  DA2 Raw Crosshair Value: {:.4}", da2_center_val);

      let eye = Vec3::new(
        telem["player_pose"]["eye_position"]["x"].as_f64().unwrap(),
        telem["player_pose"]["eye_position"]["y"].as_f64().unwrap(),
        telem["player_pose"]["eye_position"]["z"].as_f64().unwrap(),
      );
      let pose = PlayerPose {
        eye_position: eye,
        yaw: telem["player_pose"]["yaw"].as_f64().unwrap(),
        pitch: telem["player_pose"]["pitch"].as_f64().unwrap(),
      };
      let viewport = Viewport::new(img.width(), img.height());

      // If scale = 1.0 (uncalibrated default):
      let bp_midas_uncal = back_project((cx, cy), midas_center_val * 1.0, viewport, &pose, 70.0);
      let bp_da2_uncal = back_project((cx, cy), da2_center_val * 1.0, viewport, &pose, 70.0);
      println!("  [Uncalibrated scale=1.0]:");
      println!(
        "    MiDaS: predicted metric depth = {:.2}m, back-projected pos = ({:.1}, {:.1}, {:.1})",
        midas_center_val, bp_midas_uncal.x, bp_midas_uncal.y, bp_midas_uncal.z
      );
      println!(
        "    DA2:   predicted metric depth = {:.2}m, back-projected pos = ({:.1}, {:.1}, {:.1})",
        da2_center_val, bp_da2_uncal.x, bp_da2_uncal.y, bp_da2_uncal.z
      );

      // If transferring scale calibrated from v01:
      let bp_midas_trans = back_project((cx, cy), midas_center_val * calibrated_midas_scale_v01, viewport, &pose, 70.0);
      let bp_da2_trans = back_project((cx, cy), da2_center_val * calibrated_da2_scale_v01, viewport, &pose, 70.0);
      println!("  [Transferred scale from v01 (MiDaS={:.6}, DA2={:.6})]:", calibrated_midas_scale_v01, calibrated_da2_scale_v01);
      println!(
        "    MiDaS: predicted metric depth = {:.2}m, back-projected pos = ({:.1}, {:.1}, {:.1})",
        midas_center_val * calibrated_midas_scale_v01,
        bp_midas_trans.x,
        bp_midas_trans.y,
        bp_midas_trans.z
      );
      println!(
        "    DA2:   predicted metric depth = {:.2}m, back-projected pos = ({:.1}, {:.1}, {:.1})",
        da2_center_val * calibrated_da2_scale_v01,
        bp_da2_trans.x,
        bp_da2_trans.y,
        bp_da2_trans.z
      );
    }
  }

  // -------------------------------------------------------------
  // SECTION 3: REAL INFERENCE LATENCY BENCHMARKS (ON CPU)
  // -------------------------------------------------------------
  println!("\n-------------------------------------------------------------");
  println!("3. Real Inference Latency Benchmarks (CPU)");
  println!("-------------------------------------------------------------");

  let test_img = screenshots.get("v01").unwrap();
  let rounds = 20;

  // Warmup YOLO-World
  detector.set_confidence_threshold(0.05);
  for _ in 0..5 {
    let _ = detector.detect(test_img).unwrap();
  }
  let mut yolo_times = Vec::with_capacity(rounds);
  for _ in 0..rounds {
    let start = Instant::now();
    let _ = detector.detect(test_img).unwrap();
    yolo_times.push(start.elapsed().as_secs_f64() * 1000.0);
  }
  let (y_min, y_max, y_mean, y_med) = stats(&yolo_times);
  println!("  YOLO-World (640x640, 10 classes):");
  println!("    Min: {:.2} ms | Max: {:.2} ms | Mean: {:.2} ms | Median: {:.2} ms", y_min, y_max, y_mean, y_med);

  // Warmup MiDaS
  for _ in 0..5 {
    let _ = midas_estimator.estimate(test_img).unwrap();
  }
  let mut midas_times = Vec::with_capacity(rounds);
  for _ in 0..rounds {
    let start = Instant::now();
    let _ = midas_estimator.estimate(test_img).unwrap();
    midas_times.push(start.elapsed().as_secs_f64() * 1000.0);
  }
  let (m_min, m_max, m_mean, m_med) = stats(&midas_times);
  println!("  Depth Estimator - MiDaS v2.1 Small (256x256):");
  println!("    Min: {:.2} ms | Max: {:.2} ms | Mean: {:.2} ms | Median: {:.2} ms", m_min, m_max, m_mean, m_med);

  // Warmup Depth Anything V2
  for _ in 0..3 {
    let _ = da2_estimator.estimate(test_img).unwrap();
  }
  let mut da2_times = Vec::with_capacity(rounds);
  for _ in 0..rounds {
    let start = Instant::now();
    let _ = da2_estimator.estimate(test_img).unwrap();
    da2_times.push(start.elapsed().as_secs_f64() * 1000.0);
  }
  let (da_min, da_max, da_mean, da_med) = stats(&da2_times);
  println!("  Depth Estimator - Depth Anything V2 Small (518x518):");
  println!("    Min: {:.2} ms | Max: {:.2} ms | Mean: {:.2} ms | Median: {:.2} ms", da_min, da_max, da_mean, da_med);

  // End-to-end Ingest with MiDaS
  let observer = PlayerPose {
    eye_position: Vec3::new(-22.662026, 82.62, 39.552317),
    yaw: -9.449891,
    pitch: 12.000004,
  };
  let viewport = Viewport::new(test_img.width(), test_img.height());
  let obs_ref = ObservationRef {
    observation_id: "bench-obs".to_string(),
    captured_at_millis: 1000,
  };
  let mut calibrator = AffineDepthCalibrator::new(20);
  calibrator.add_anchor(441.0, 3.866);
  calibrator.add_anchor(482.0, 2.271);

  let ingest_midas = VisualPerceptionIngest {
    detector: &detector,
    depth: &midas_estimator,
    calibrator: &calibrator,
    screenshot: test_img,
    observer: observer.clone(),
    viewport,
    vertical_fov_deg: 70.0,
    observation_ref: obs_ref.clone(),
    static_whitelist: None,
    confidence_threshold: None,
  };

  let mut temp_store =
    SpatialMemoryStore::open_with_config(tempfile::NamedTempFile::new().unwrap().path(), SpatialMemoryConfig::default()).unwrap();

  let mut e2e_midas_times = Vec::with_capacity(rounds);
  for _ in 0..rounds {
    let start = Instant::now();
    let _ = ingest_midas.ingest(&mut temp_store, 1000);
    e2e_midas_times.push(start.elapsed().as_secs_f64() * 1000.0);
  }
  let (em_min, em_max, em_mean, em_med) = stats(&e2e_midas_times);
  println!("  End-to-End Ingest (YOLO-World + MiDaS + Back-proj + Upsert):");
  println!("    Min: {:.2} ms | Max: {:.2} ms | Mean: {:.2} ms | Median: {:.2} ms", em_min, em_max, em_mean, em_med);

  let ingest_da2 = VisualPerceptionIngest {
    detector: &detector,
    depth: &da2_estimator,
    calibrator: &calibrator,
    screenshot: test_img,
    observer,
    viewport,
    vertical_fov_deg: 70.0,
    observation_ref: obs_ref.clone(),
    static_whitelist: None,
    confidence_threshold: None,
  };
  let mut e2e_da2_times = Vec::with_capacity(rounds);
  for _ in 0..rounds {
    let start = Instant::now();
    let _ = ingest_da2.ingest(&mut temp_store, 1000);
    e2e_da2_times.push(start.elapsed().as_secs_f64() * 1000.0);
  }
  let (eda_min, eda_max, eda_mean, eda_med) = stats(&e2e_da2_times);
  println!("  End-to-End Ingest (YOLO-World + DA2 + Back-proj + Upsert):");
  println!("    Min: {:.2} ms | Max: {:.2} ms | Mean: {:.2} ms | Median: {:.2} ms", eda_min, eda_max, eda_mean, eda_med);

  // -------------------------------------------------------------
  // SECTION 4: SPATIAL MEMORY STORE SCALABILITY BENCHMARK
  // -------------------------------------------------------------
  println!("\n-------------------------------------------------------------");
  println!("4. SpatialMemoryStore Scalability Benchmark (1k vs 10k)");
  println!("-------------------------------------------------------------");

  for &count in &[1_000, 10_000] {
    println!("\n>>> Benchmark with N = {} Landmarks <<<", count);
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let mut store = SpatialMemoryStore::open_with_config(tmp.path(), SpatialMemoryConfig::default()).unwrap();

    let obs = ObservationRef {
      observation_id: "synth-obs".to_string(),
      captured_at_millis: 1000,
    };

    // 1. Benchmark Sequential Upsert
    let upsert_start = Instant::now();
    for i in 0..count {
      let x = (i % 100) as i32 * 2;
      let y = 64 + (i / 10000) as i32;
      let z = (i / 100) as i32 * 2;
      let pos = BlockPosition::new(x, y, z);
      store.upsert_from_perception(pos, "synthetic_object", 0.8, &obs);
    }
    let upsert_total = upsert_start.elapsed();
    let per_upsert_us = (upsert_total.as_secs_f64() * 1_000_000.0) / (count as f64);
    println!("  Sequential Upsert ({} landmarks):", count);
    println!("    Total time: {:.2} ms ({:.4} s)", upsert_total.as_secs_f64() * 1000.0, upsert_total.as_secs_f64());
    println!("    Per upsert avg: {:.2} µs (throughput: {:.1} upserts/sec)", per_upsert_us, (count as f64) / upsert_total.as_secs_f64());

    // 2. Benchmark query_radius
    let num_queries = 1000;
    let query_start = Instant::now();
    let mut total_hits = 0;
    for q in 0..num_queries {
      let qx = (q % 100) as i32 * 2;
      let qz = (q / 100) as i32 * 2;
      let hits = store.query_radius(BlockPosition::new(qx, 64, qz), 15.0);
      total_hits += hits.len();
    }
    let query_total = query_start.elapsed();
    let per_query_us = (query_total.as_secs_f64() * 1_000_000.0) / (num_queries as f64);
    println!("  query_radius (r=15m, {} queries across {} landmarks):", num_queries, count);
    println!("    Total time: {:.2} ms", query_total.as_secs_f64() * 1000.0);
    println!(
      "    Per query avg: {:.2} µs ({:.4} ms) (throughput: {:.1} queries/sec)",
      per_query_us,
      per_query_us / 1000.0,
      (num_queries as f64) / query_total.as_secs_f64()
    );
    println!("    Average hits per query: {:.1}", (total_hits as f64) / (num_queries as f64));

    // 3. Benchmark persistence save (serialization)
    let save_start = Instant::now();
    store.save().expect("store save");
    let save_time = save_start.elapsed();
    let file_size_bytes = std::fs::metadata(tmp.path()).unwrap().len();
    println!("  Persistence Save (JSON Serialization):");
    println!("    Time: {:.2} ms", save_time.as_secs_f64() * 1000.0);
    println!(
      "    File size on disk: {} bytes ({:.2} KB / {:.2} MB)",
      file_size_bytes,
      (file_size_bytes as f64) / 1024.0,
      (file_size_bytes as f64) / (1024.0 * 1024.0)
    );

    // 4. Benchmark persistence open (deserialization)
    let load_start = Instant::now();
    let loaded_store = SpatialMemoryStore::open(tmp.path()).expect("store open");
    let load_time = load_start.elapsed();
    assert_eq!(loaded_store.len(), count);
    println!("  Persistence Open (JSON Deserialization):");
    println!("    Time: {:.2} ms", load_time.as_secs_f64() * 1000.0);
  }

  println!("\n=======================================================");
  println!("BENCHMARK COMPLETE");
  println!("=======================================================\n");
}
