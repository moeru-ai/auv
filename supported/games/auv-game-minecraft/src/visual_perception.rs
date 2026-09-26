//! Visual perception pipeline: YOLO-World 2D detection, monocular depth estimation,
//! and 3D back-projection into structured spatial landmarks.
//!
//! Addresses Architectural Boundary 3 (keyhole perception) and Boundary 4 (2D->3D gap).
//! Perceived landmarks carry semantic category labels and start as `Candidate` status
//! in spatial memory.

use auv_inference_ort::{ExecutionProvider, F32Tensor, OrtModelConfig, OrtSession};
use image::DynamicImage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::depth_calibration::AffineDepthCalibrator;
use crate::spatial_memory_ingest::{IngestReport, LandmarkIngest};
use crate::spatial_memory_store::{LandmarkKind, ObservationRef, SpatialMemoryStore};
use crate::types::{BlockPosition, PlayerPose, Vec3, Viewport};

pub const DEFAULT_MINECRAFT_CLASSES: [&str; 10] = [
  "chest",
  "furnace",
  "crafting table",
  "door",
  "bed",
  "torch",
  "tree",
  "sheep",
  "pig",
  "cow",
];

const EMBEDDINGS_JSON: &str = include_str!("../assets/minecraft_classes_embeds.json");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Detection {
  pub bbox: (f64, f64, f64, f64),
  pub label: String,
  pub confidence: f64,
}

#[derive(Clone, Debug)]
pub struct YoloWorldConfig {
  pub model_path: PathBuf,
  pub confidence_threshold: f64,
  pub iou_threshold: f64,
  pub input_size: u32,
  pub classes: Vec<String>,
}

impl Default for YoloWorldConfig {
  fn default() -> Self {
    Self {
      model_path: PathBuf::from("F:/.auv/.tmp/models/yolov8s-worldv2.onnx"),
      confidence_threshold: 0.30,
      iou_threshold: 0.45,
      input_size: 640,
      classes: DEFAULT_MINECRAFT_CLASSES.iter().map(|s| s.to_string()).collect(),
    }
  }
}

#[derive(Deserialize)]
struct StoredEmbeddings {
  classes: Vec<String>,
  embeds: Vec<Vec<f32>>,
}

pub struct YoloWorldDetector {
  session: OrtSession,
  config: YoloWorldConfig,
  text_embeds_flat: Vec<f32>,
  num_classes: usize,
}

impl YoloWorldDetector {
  pub fn new(config: YoloWorldConfig) -> Result<Self, String> {
    let session = OrtSession::load(OrtModelConfig {
      model_path: config.model_path.clone(),
      execution_provider: ExecutionProvider::Cpu,
    })
    .map_err(|err| format!("failed to load YOLO-World ONNX model: {err}"))?;

    let parsed: StoredEmbeddings =
      serde_json::from_str(EMBEDDINGS_JSON).map_err(|err| format!("failed to parse embedded Minecraft CLIP embeddings: {err}"))?;

    let num_classes = parsed.classes.len();
    let mut flat = Vec::with_capacity(num_classes * 512);
    for row in parsed.embeds {
      if row.len() != 512 {
        return Err(format!("expected 512-dim embedding, got {}", row.len()));
      }
      flat.extend(row);
    }

    Ok(Self {
      session,
      config,
      text_embeds_flat: flat,
      num_classes,
    })
  }

  pub fn config(&self) -> &YoloWorldConfig {
    &self.config
  }

  pub fn set_confidence_threshold(&mut self, threshold: f64) {
    self.config.confidence_threshold = threshold;
  }

  pub fn detect(&self, image: &DynamicImage) -> Result<Vec<Detection>, String> {
    let orig_w = image.width() as f64;
    let orig_h = image.height() as f64;
    if orig_w == 0.0 || orig_h == 0.0 {
      return Ok(Vec::new());
    }

    let input_sz = self.config.input_size;
    let resized = image.resize_exact(input_sz, input_sz, image::imageops::FilterType::Triangle);
    let rgb = resized.to_rgb8();

    // Construct images tensor: [1, 3, H, W] in [0.0, 1.0]
    let mut img_data = vec![0.0f32; 3 * (input_sz as usize) * (input_sz as usize)];
    let stride = (input_sz as usize) * (input_sz as usize);
    for (i, pixel) in rgb.pixels().enumerate() {
      img_data[i] = f32::from(pixel[0]) / 255.0;
      img_data[i + stride] = f32::from(pixel[1]) / 255.0;
      img_data[i + stride * 2] = f32::from(pixel[2]) / 255.0;
    }

    let images_tensor = F32Tensor {
      name: "images".to_string(),
      shape: vec![1, 3, input_sz as usize, input_sz as usize],
      data: img_data,
    };

    let txt_tensor = F32Tensor {
      name: "txt_feats".to_string(),
      shape: vec![1, self.num_classes, 512],
      data: self.text_embeds_flat.clone(),
    };

    let outputs = self.session.run_tensors(vec![images_tensor, txt_tensor]).map_err(|err| format!("YOLO-World inference failed: {err}"))?;

    let out = outputs
      .into_iter()
      .find(|t| t.name == "output0" || t.name == "output")
      .ok_or_else(|| "missing output0 tensor in YOLO-World result".to_string())?;

    // out.shape is [1, 4 + num_classes, num_anchors]
    if out.shape.len() != 3 {
      return Err(format!("unexpected output0 shape: {:?}", out.shape));
    }
    let channels = out.shape[1];
    let num_anchors = out.shape[2];
    if channels < 4 + self.num_classes {
      return Err(format!("output channels ({channels}) smaller than 4 + {}", self.num_classes));
    }

    let scale_x = orig_w / f64::from(input_sz);
    let scale_y = orig_h / f64::from(input_sz);

    let mut raw_candidates: Vec<Detection> = Vec::new();

    for a in 0..num_anchors {
      let mut best_cls = 0;
      let mut best_score = 0.0f32;
      for c in 0..self.num_classes {
        let score = out.data[(4 + c) * num_anchors + a];
        if score > best_score {
          best_score = score;
          best_cls = c;
        }
      }

      let conf = f64::from(best_score);
      if conf >= self.config.confidence_threshold {
        let cx = f64::from(out.data[a]);
        let cy = f64::from(out.data[num_anchors + a]);
        let w = f64::from(out.data[2 * num_anchors + a]);
        let h = f64::from(out.data[3 * num_anchors + a]);

        let x1 = ((cx - w * 0.5) * scale_x).clamp(0.0, orig_w);
        let y1 = ((cy - h * 0.5) * scale_y).clamp(0.0, orig_h);
        let x2 = ((cx + w * 0.5) * scale_x).clamp(0.0, orig_w);
        let y2 = ((cy + h * 0.5) * scale_y).clamp(0.0, orig_h);

        let label = self.config.classes.get(best_cls).cloned().unwrap_or_else(|| format!("class_{best_cls}"));

        raw_candidates.push(Detection {
          bbox: (x1, y1, x2, y2),
          label,
          confidence: conf,
        });
      }
    }

    // Apply Non-Maximum Suppression (NMS)
    raw_candidates.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    let mut kept: Vec<Detection> = Vec::new();
    for cand in raw_candidates {
      let dominated = kept.iter().any(|k| k.label == cand.label && calculate_iou(k.bbox, cand.bbox) >= self.config.iou_threshold);
      if !dominated {
        kept.push(cand);
      }
    }

    Ok(kept)
  }
}

fn calculate_iou(b1: (f64, f64, f64, f64), b2: (f64, f64, f64, f64)) -> f64 {
  let inter_x1 = b1.0.max(b2.0);
  let inter_y1 = b1.1.max(b2.1);
  let inter_x2 = b1.2.min(b2.2);
  let inter_y2 = b1.3.min(b2.3);

  let inter_w = (inter_x2 - inter_x1).max(0.0);
  let inter_h = (inter_y2 - inter_y1).max(0.0);
  let inter_area = inter_w * inter_h;

  let area1 = (b1.2 - b1.0).max(0.0) * (b1.3 - b1.1).max(0.0);
  let area2 = (b2.2 - b2.0).max(0.0) * (b2.3 - b2.1).max(0.0);
  let union_area = area1 + area2 - inter_area;

  if union_area <= 0.0 {
    0.0
  } else {
    inter_area / union_area
  }
}

#[derive(Clone, Debug)]
pub struct DepthMap {
  pub width: u32,
  pub height: u32,
  pub values: Vec<f32>,
}

impl DepthMap {
  pub fn depth_at(&self, x: f64, y: f64) -> f64 {
    let xi = (x.round() as usize).clamp(0, self.width.saturating_sub(1) as usize);
    let yi = (y.round() as usize).clamp(0, self.height.saturating_sub(1) as usize);
    f64::from(self.values[yi * (self.width as usize) + xi])
  }
}

pub struct DepthEstimator {
  session: OrtSession,
  input_name: String,
  input_size: u32,
  scale: f64,
}

impl DepthEstimator {
  pub fn new(model_path: impl AsRef<Path>) -> Result<Self, String> {
    let session = OrtSession::load(OrtModelConfig {
      model_path: model_path.as_ref().to_path_buf(),
      execution_provider: ExecutionProvider::Cpu,
    })
    .map_err(|err| format!("failed to load depth ONNX model: {err}"))?;

    let path_str = model_path.as_ref().to_string_lossy();
    let (input_name, input_size) = if path_str.contains("depth_anything") {
      ("pixel_values".to_string(), 518)
    } else {
      ("0".to_string(), 256)
    };

    Ok(Self {
      session,
      input_name,
      input_size,
      scale: 1.0,
    })
  }

  pub fn scale(&self) -> f64 {
    self.scale
  }

  pub fn calibrate(&mut self, scale: f64) {
    self.scale = scale;
  }

  /// Calibrate depth scale using a known ground-truth distance at crosshair.
  /// NOTICE: This is testbed scaffolding using telemetry raycast truth.
  /// In black-box deployments, replace with multi-view stereo or known object size.
  pub fn calibrate_from_raycast(&mut self, true_depth_at_crosshair: f64, pred_depth_at_crosshair: f64) -> f64 {
    let scale = true_depth_at_crosshair / pred_depth_at_crosshair.max(1e-6);
    self.scale = scale;
    scale
  }

  pub fn estimate(&self, image: &DynamicImage) -> Result<DepthMap, String> {
    let orig_w = image.width();
    let orig_h = image.height();
    if orig_w == 0 || orig_h == 0 {
      return Ok(DepthMap {
        width: 0,
        height: 0,
        values: Vec::new(),
      });
    }

    let sz = self.input_size;
    let resized = image.resize_exact(sz, sz, image::imageops::FilterType::Triangle);
    let rgb = resized.to_rgb8();

    let mean = [0.485f32, 0.456f32, 0.406f32];
    let std = [0.229f32, 0.224f32, 0.225f32];

    let mut norm_data = vec![0.0f32; 3 * (sz as usize) * (sz as usize)];
    let stride = (sz as usize) * (sz as usize);
    for (i, pixel) in rgb.pixels().enumerate() {
      norm_data[i] = ((f32::from(pixel[0]) / 255.0) - mean[0]) / std[0];
      norm_data[i + stride] = ((f32::from(pixel[1]) / 255.0) - mean[1]) / std[1];
      norm_data[i + stride * 2] = ((f32::from(pixel[2]) / 255.0) - mean[2]) / std[2];
    }

    let input_tensor = F32Tensor {
      name: self.input_name.clone(),
      shape: vec![1, 3, sz as usize, sz as usize],
      data: norm_data,
    };

    let outputs = self.session.run_tensors(vec![input_tensor]).map_err(|err| format!("depth inference failed: {err}"))?;

    let out = outputs.into_iter().next().ok_or_else(|| "no output from depth model".to_string())?;

    let sz_usize = sz as usize;
    let out_h = *out.shape.get(1).unwrap_or(&sz_usize);
    let out_w = *out.shape.get(2).unwrap_or(&sz_usize);

    // Resample back to original width and height using nearest neighbor / bilinear
    let mut full_values = vec![0.0f32; (orig_w as usize) * (orig_h as usize)];
    for y in 0..(orig_h as usize) {
      let src_y = (y * out_h / (orig_h as usize)).min(out_h.saturating_sub(1));
      for x in 0..(orig_w as usize) {
        let src_x = (x * out_w / (orig_w as usize)).min(out_w.saturating_sub(1));
        full_values[y * (orig_w as usize) + x] = out.data[src_y * out_w + src_x];
      }
    }

    Ok(DepthMap {
      width: orig_w,
      height: orig_h,
      values: full_values,
    })
  }

  pub fn metric_depth_at(&self, depth_map: &DepthMap, x: f64, y: f64) -> f64 {
    depth_map.depth_at(x, y) * self.scale
  }
}

/// Back-projects a 2D screen coordinate and metric depth into 3D world coordinates.
///
/// Implements the exact inverse of `MinecraftProjector` / `build_frame_from_pose`.
///
/// Mathematical formulation:
/// 1. NDC coordinates from viewport:
///    `ndc_x = (2.0 * px / width) - 1.0`
///    `ndc_y = 1.0 - (2.0 * py / height)`
/// 2. Camera-space ray with vertical FOV:
///    `f = 1.0 / tan(fov_y / 2.0)`
///    `v_cam = (ndc_x * aspect / f, ndc_y / f, -1.0)`
/// 3. Transform to world-space ray via transpose of view rotation R^T:
///    `v_world = cam_x * R_col0 + cam_y * R_col1 - cam_z * R_forward`
/// 4. Scale by metric depth from eye position:
///    `position = eye_position + metric_depth * normalized(v_world)`
pub fn back_project(screen_point: (f64, f64), metric_depth: f64, viewport: Viewport, observer: &PlayerPose, vertical_fov_deg: f64) -> Vec3 {
  let width = f64::from(viewport.width.max(1));
  let height = f64::from(viewport.height.max(1));
  let ndc_x = (2.0 * screen_point.0 / width) - 1.0;
  let ndc_y = 1.0 - (2.0 * screen_point.1 / height);

  let aspect = width / height;
  let half_fov_rad = (vertical_fov_deg / 2.0).to_radians();
  let f = 1.0 / half_fov_rad.tan().max(1e-6);

  let cam_x = ndc_x * aspect / f;
  let cam_y = ndc_y / f;

  let psi = observer.yaw.to_radians();
  let theta = observer.pitch.to_radians();
  let cos_yaw = psi.cos();
  let sin_yaw = psi.sin();
  let cos_pitch = theta.cos();
  let sin_pitch = theta.sin();

  // Minecraft camera coordinate axes in world space:
  // Forward look vector: (-sin_yaw * cos_pitch, -sin_pitch, cos_yaw * cos_pitch)
  // Right vector: (-cos_yaw, 0.0, -sin_yaw)
  // Up vector: (-sin_yaw * sin_pitch, cos_pitch, cos_yaw * sin_pitch)
  let fwd_x = -sin_yaw * cos_pitch;
  let fwd_y = -sin_pitch;
  let fwd_z = cos_yaw * cos_pitch;

  let right_x = -cos_yaw;
  let right_y = 0.0;
  let right_z = -sin_yaw;

  let up_x = -sin_yaw * sin_pitch;
  let up_y = cos_pitch;
  let up_z = cos_yaw * sin_pitch;

  let ray_x = cam_x * right_x + cam_y * up_x + fwd_x;
  let ray_y = cam_x * right_y + cam_y * up_y + fwd_y;
  let ray_z = cam_x * right_z + cam_y * up_z + fwd_z;

  let len = (ray_x * ray_x + ray_y * ray_y + ray_z * ray_z).sqrt().max(1e-6);
  let norm_x = ray_x / len;
  let norm_y = ray_y / len;
  let norm_z = ray_z / len;

  Vec3::new(
    observer.eye_position.x + norm_x * metric_depth,
    observer.eye_position.y + norm_y * metric_depth,
    observer.eye_position.z + norm_z * metric_depth,
  )
}

/// 对 bbox 内缩 inset_ratio（默认 0.2）后的区域取深度中位数。
/// 返回 None 当且仅当内缩后区域为空（退化 bbox）或无有效像素。
pub fn robust_bbox_depth(
  depth_map: &[f32],
  width: usize,
  height: usize,
  bbox: (f32, f32, f32, f32), // (x1, y1, x2, y2)
  inset_ratio: f32,
) -> Option<f32> {
  let (x1, y1, x2, y2) = bbox;
  let w = x2 - x1;
  let h = y2 - y1;
  if w <= 0.0 || h <= 0.0 {
    return None;
  }
  let inset_x = w * inset_ratio;
  let inset_y = h * inset_ratio;
  let in_x1 = (x1 + inset_x).max(0.0).min(width as f32);
  let in_x2 = (x2 - inset_x).max(0.0).min(width as f32);
  let in_y1 = (y1 + inset_y).max(0.0).min(height as f32);
  let in_y2 = (y2 - inset_y).max(0.0).min(height as f32);

  if in_x2 <= in_x1 || in_y2 <= in_y1 {
    return None;
  }

  let min_x = in_x1.floor() as usize;
  let max_x = (in_x2.ceil() as usize).min(width);
  let min_y = in_y1.floor() as usize;
  let max_y = (in_y2.ceil() as usize).min(height);

  let mut values = Vec::new();
  for y in min_y..max_y {
    for x in min_x..max_x {
      let val = depth_map[y * width + x];
      if val.is_finite() && val > 0.0 {
        values.push(val);
      }
    }
  }

  if values.is_empty() {
    return None;
  }

  values.sort_by(|a, b| a.total_cmp(b));
  Some(values[values.len() / 2])
}

#[derive(Clone, Debug, PartialEq)]
pub struct PerceivedLandmark {
  pub position: Vec3,
  pub block_pos: BlockPosition,
  pub label: String,
  pub confidence: f64,
  pub bbox_2d: (f64, f64, f64, f64),
  pub metric_depth: f64,
}

/// Pluggable LandmarkIngest implementation using YOLO-World 2D detection and monocular depth.
pub struct VisualPerceptionIngest<'a> {
  pub detector: &'a YoloWorldDetector,
  pub depth: &'a DepthEstimator,
  pub calibrator: &'a AffineDepthCalibrator,
  pub screenshot: &'a DynamicImage,
  pub observer: PlayerPose,
  pub viewport: Viewport,
  pub vertical_fov_deg: f64,
  pub observation_ref: ObservationRef,
  pub static_whitelist: Option<Vec<String>>,
  pub confidence_threshold: Option<f64>,
}

impl<'a> VisualPerceptionIngest<'a> {
  pub fn new(
    detector: &'a YoloWorldDetector,
    depth: &'a DepthEstimator,
    calibrator: &'a AffineDepthCalibrator,
    screenshot: &'a DynamicImage,
    observer: PlayerPose,
    viewport: Viewport,
    vertical_fov_deg: f64,
    observation_ref: ObservationRef,
  ) -> Self {
    Self {
      detector,
      depth,
      calibrator,
      screenshot,
      observer,
      viewport,
      vertical_fov_deg,
      observation_ref,
      static_whitelist: None,
      confidence_threshold: None,
    }
  }

  pub fn with_whitelist(mut self, whitelist: Vec<String>) -> Self {
    self.static_whitelist = Some(whitelist);
    self
  }

  pub fn with_confidence_threshold(mut self, threshold: f64) -> Self {
    self.confidence_threshold = Some(threshold);
    self
  }
}

/// Category routing for visual perception landmarks:
/// - Static: blocks, containers, furniture with durable world positions:
///   `["chest", "furnace", "crafting table", "crafting_table", "door", "bed", "torch"]`
/// - Dynamic: mobs, animals, players, or high-false-positive categories like "tree"
///   (which uses dynamic TTL cleanup to avoid cluttering static memory).
/// - Unknown categories: defaulted to Dynamic as a conservative fallback.
pub fn is_static_category(label: &str) -> bool {
  let lower = label.to_lowercase();
  matches!(lower.as_str(), "chest" | "furnace" | "crafting table" | "crafting_table" | "door" | "bed" | "torch")
}

impl<'a> LandmarkIngest for VisualPerceptionIngest<'a> {
  fn ingest(&self, store: &mut SpatialMemoryStore, _now_millis: u64) -> IngestReport {
    let mut report = IngestReport::default();

    // P2c: Zero-anchor gate - if calibrator has no fit, abort and skip
    if self.calibrator.fit().is_none() {
      report.observations_skipped += 1;
      report.skipped_reason = Some("no depth calibration anchors".to_string());
      return report;
    }

    let detections = match self.detector.detect(self.screenshot) {
      Ok(d) => d,
      Err(e) => {
        report.observations_skipped += 1;
        report.skipped_reason = Some(format!("detector failure: {e}"));
        return report;
      }
    };

    if detections.is_empty() {
      return report;
    }

    let depth_map = match self.depth.estimate(self.screenshot) {
      Ok(dm) => dm,
      Err(e) => {
        report.observations_skipped += 1;
        report.skipped_reason = Some(format!("depth estimator failure: {e}"));
        return report;
      }
    };

    let mut next_track_id = store
      .landmarks()
      .values()
      .filter_map(|lm| {
        if let LandmarkKind::Dynamic { track_id, .. } = lm.kind {
          Some(track_id)
        } else {
          None
        }
      })
      .max()
      .unwrap_or(0)
      + 1;

    for det in detections {
      if let Some(min_conf) = self.confidence_threshold {
        if det.confidence < min_conf {
          report.observations_skipped += 1;
          continue;
        }
      }

      let cx = (det.bbox.0 + det.bbox.2) * 0.5;
      let cy = (det.bbox.1 + det.bbox.3) * 0.5;

      let raw_depth_opt = robust_bbox_depth(
        &depth_map.values,
        depth_map.width as usize,
        depth_map.height as usize,
        (det.bbox.0 as f32, det.bbox.1 as f32, det.bbox.2 as f32, det.bbox.3 as f32),
        0.2,
      );

      let Some(raw_depth) = raw_depth_opt else {
        report.observations_skipped += 1;
        continue;
      };

      let Some(metric_depth) = self.calibrator.calibrate(raw_depth) else {
        report.observations_skipped += 1;
        continue;
      };
      let metric_depth = metric_depth as f64;

      let world_pos = back_project((cx, cy), metric_depth, self.viewport, &self.observer, self.vertical_fov_deg);

      let block_pos = BlockPosition::new(world_pos.x.round() as i32, world_pos.y.round() as i32, world_pos.z.round() as i32);

      let label_desc = format!("{} ({:.2})", det.label, det.confidence);
      let len_before = store.len();

      let is_static = match &self.static_whitelist {
        Some(whitelist) => whitelist.iter().any(|item| item.eq_ignore_ascii_case(&det.label)),
        None => is_static_category(&det.label),
      };

      if is_static {
        store.upsert_from_perception(block_pos, &label_desc, det.confidence, &self.observation_ref);
      } else {
        // NOTICE (Naive Tracking Heuristic):
        // This is a naive heuristic association (same class name prefix and inter-frame
        // 3D displacement < 2.0m), NOT a full multi-target tracker (MOT / Kalman filter).
        // It provides lightweight track continuity for dynamic entities in single sessions.
        let mut matched_track_id = None;
        for lm in store.landmarks().values() {
          if let LandmarkKind::Dynamic { track_id, .. } = lm.kind {
            let same_class = lm.description.as_deref().map(|d| d.starts_with(&det.label)).unwrap_or(false);
            if same_class {
              let lm_pos = lm
                .continuous_position
                .unwrap_or_else(|| (f64::from(lm.position.x) + 0.5, f64::from(lm.position.y) + 0.5, f64::from(lm.position.z) + 0.5));
              let dx = world_pos.x - lm_pos.0;
              let dy = world_pos.y - lm_pos.1;
              let dz = world_pos.z - lm_pos.2;
              let dist = (dx * dx + dy * dy + dz * dz).sqrt();
              if dist < 2.0 {
                matched_track_id = Some(track_id);
                break;
              }
            }
          }
        }

        let track_id = if let Some(tid) = matched_track_id {
          tid
        } else {
          let tid = next_track_id;
          next_track_id += 1;
          tid
        };

        const DYNAMIC_TTL_MILLIS: u64 = 10_000;
        store.upsert_dynamic_landmark(
          track_id,
          DYNAMIC_TTL_MILLIS,
          block_pos,
          Some((world_pos.x, world_pos.y, world_pos.z)),
          &label_desc,
          det.confidence,
          &self.observation_ref,
        );
      }

      let len_after = store.len();
      if len_after > len_before {
        report.landmarks_created += 1;
      } else {
        report.landmarks_merged += 1;
      }
    }

    report
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::spatial_memory_observation::SpatialClaimStatus;
  use crate::spatial_memory_store::{LandmarkKind, LandmarkSource, SpatialMemoryConfig};
  use crate::types::{BlockFace, RaycastHit};

  #[test]
  fn test_back_project_center_ray_accuracy() {
    let observer = PlayerPose {
      eye_position: Vec3::new(-22.662026, 82.62, 39.552317),
      yaw: -9.449891,
      pitch: 12.000004,
    };
    let viewport = Viewport::new(870, 519);
    let crosshair = (870.0 * 0.5, 519.0 * 0.5);
    // True distance to block (-22, 81, 43):
    // dx = -22.0 - (-22.662026) = 0.662
    // dy = 81.0 - 82.62 = -1.62
    // dz = 43.0 - 39.552317 = 3.448
    let true_distance = (0.662026f64.powi(2) + (-1.62f64).powi(2) + 3.447683f64.powi(2)).sqrt();

    let projected_pos = back_project(crosshair, true_distance, viewport, &observer, 70.0);

    let err_x = projected_pos.x - (-22.0);
    let err_y = projected_pos.y - 81.0;
    let err_z = projected_pos.z - 43.0;
    let total_error = (err_x * err_x + err_y * err_y + err_z * err_z).sqrt();

    // Brief requirement T3: error within 2.0m
    assert!(total_error < 2.0, "back-projection error must be within 2.0m, got {total_error:.3}m (pos: {projected_pos:?})");
  }

  #[test]
  fn test_visual_perception_ingest_fusion_logic() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let mut store = SpatialMemoryStore::open_with_config(tmp.path(), SpatialMemoryConfig::default()).unwrap();
    let obs_ray = ObservationRef {
      observation_id: "obs-telemetry-1".to_string(),
      captured_at_millis: 1000,
    };
    let obs_vis = ObservationRef {
      observation_id: "obs-visual-1".to_string(),
      captured_at_millis: 2000,
    };

    // 1. Raycast creates Confirmed landmark with mechanical coordinates
    let hit = RaycastHit {
      block_pos: BlockPosition::new(-22, 81, 43),
      face: BlockFace::West,
      block_id: "minecraft:grass_block".to_string(),
    };
    let lm_id = store.upsert_from_raycast(&hit, &obs_ray);
    let initial = store.get(&lm_id).unwrap();
    assert_eq!(initial.status, SpatialClaimStatus::Confirmed);
    assert_eq!(initial.source, LandmarkSource::TelemetryRaycast);
    assert_eq!(initial.description, None);

    // 2. Visual perception detects an object at the same position
    store.upsert_from_perception(BlockPosition::new(-22, 81, 43), "chest (0.87)", 0.87, &obs_vis);
    let merged = store.get(&lm_id).unwrap();

    // Multi-engine fusion invariant:
    // Confirmed status preserved; description enriched with semantic label
    assert_eq!(merged.status, SpatialClaimStatus::Confirmed);
    assert_eq!(merged.description, Some("chest (0.87)".to_string()));
    assert_eq!(merged.observations.len(), 2);
    assert_eq!(merged.observations[1].source, LandmarkSource::VisualPerception);
  }

  #[test]
  fn test_visual_perception_candidate_creation() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let mut store = SpatialMemoryStore::open_with_config(tmp.path(), SpatialMemoryConfig::default()).unwrap();
    let obs = ObservationRef {
      observation_id: "obs-visual-only".to_string(),
      captured_at_millis: 3000,
    };

    let lm_id = store.upsert_from_perception(BlockPosition::new(10, 64, 10), "tree (0.75)", 0.75, &obs);
    let lm = store.get(&lm_id).unwrap();

    assert_eq!(lm.status, SpatialClaimStatus::Candidate);
    assert_eq!(lm.source, LandmarkSource::VisualPerception);
    assert_eq!(lm.kind, LandmarkKind::Static);
    assert_eq!(lm.description, Some("tree (0.75)".to_string()));
    assert!(lm.confidence <= 0.50);
  }

  #[test]
  fn test_visual_perception_category_routing() {
    assert!(is_static_category("chest"));
    assert!(is_static_category("Furnace"));
    assert!(is_static_category("crafting table"));
    assert!(is_static_category("door"));
    assert!(is_static_category("bed"));
    assert!(is_static_category("torch"));

    assert!(!is_static_category("sheep"));
    assert!(!is_static_category("pig"));
    assert!(!is_static_category("cow"));
    assert!(!is_static_category("tree"));
    assert!(!is_static_category("unknown_entity"));
  }

  #[test]
  fn test_live_inference_if_models_present() {
    let yolo_path = PathBuf::from("F:/.auv/.tmp/models/yolov8s-worldv2.onnx")
      .canonicalize()
      .unwrap_or_else(|_| PathBuf::from("F:/auv/.tmp/models/yolov8s-worldv2.onnx"));
    let depth_path = PathBuf::from("F:/.auv/.tmp/models/model-small.onnx")
      .canonicalize()
      .unwrap_or_else(|_| PathBuf::from("F:/auv/.tmp/models/model-small.onnx"));
    let screenshot_path = PathBuf::from("F:/auv/.tmp/m2-session/v01/screenshot.png");

    if !yolo_path.exists() || !depth_path.exists() || !screenshot_path.exists() {
      eprintln!("skipping live inference test: models or screenshot not present");
      return;
    }

    let img = image::open(&screenshot_path).expect("failed to open test screenshot");

    // Test Depth Estimator
    let mut depth_est = DepthEstimator::new(&depth_path).expect("failed to load depth estimator");
    let depth_map = depth_est.estimate(&img).expect("depth estimation failed");
    assert_eq!(depth_map.width, img.width());
    assert_eq!(depth_map.height, img.height());

    let center_val = depth_map.depth_at(img.width() as f64 * 0.5, img.height() as f64 * 0.5);
    assert!(center_val > 0.0);

    // Calibrate scale
    let true_dist = 3.866;
    let scale = depth_est.calibrate_from_raycast(true_dist, center_val);
    assert!(scale > 0.0);
    let pred_dist = depth_est.metric_depth_at(&depth_map, img.width() as f64 * 0.5, img.height() as f64 * 0.5);
    assert!((pred_dist - true_dist).abs() < 1e-4, "expected ~{true_dist}, got {pred_dist}");

    // Test YOLO-World detector
    let yolo_config = YoloWorldConfig {
      model_path: yolo_path,
      confidence_threshold: 0.05, // test with low threshold to assert detection structure
      iou_threshold: 0.45,
      input_size: 640,
      classes: DEFAULT_MINECRAFT_CLASSES.iter().map(|s| s.to_string()).collect(),
    };
    let detector = YoloWorldDetector::new(yolo_config).expect("failed to load YOLO-World");
    let detections = detector.detect(&img).expect("YOLO detection failed");
    assert!(!detections.is_empty(), "expected at least one detection with threshold 0.05");
    for det in &detections {
      assert!(!det.label.is_empty());
      assert!(det.confidence >= 0.05);
      assert!(det.bbox.2 >= det.bbox.0);
      assert!(det.bbox.3 >= det.bbox.1);
    }
  }

  #[test]
  fn test_robust_bbox_depth_ignores_center_hole() {
    // 100x100 depth map, everywhere is 3.0m, but center (45..55, 45..55) is 60.0m (sky hole)
    let mut data = vec![3.0f32; 100 * 100];
    for y in 45..55 {
      for x in 45..55 {
        data[y * 100 + x] = 60.0f32;
      }
    }

    // Bbox covering 30..70, with 20% inset it covers 38..62
    // Inside the 24x24 inset region (576 px), the 10x10 hole (100 px) is only 17% of pixels.
    // The median is guaranteed to remain 3.0m (ignoring the 60m center hole).
    let bbox = (30.0, 30.0, 70.0, 70.0);
    let depth = robust_bbox_depth(&data, 100, 100, bbox, 0.2).expect("depth computed");
    assert!((depth - 3.0).abs() < 1e-4, "expected robust depth 3.0m, got {}", depth);
  }

  #[test]
  fn test_zero_anchor_gate_skips_ingest() {
    let empty_calibrator = AffineDepthCalibrator::new(20);
    assert_eq!(empty_calibrator.fit(), None);

    // Any detector / depth estimator path will be short-circuited before inference
    // We construct a mock-like or real instance if models exist, or test gate directly
    let yolo_path = PathBuf::from("F:/auv/.tmp/models/yolov8s-worldv2.onnx");
    let depth_path = PathBuf::from("F:/auv/.tmp/models/model-small.onnx");
    let screenshot_path = PathBuf::from("F:/auv/.tmp/m2-session/v01/screenshot.png");
    if !yolo_path.is_file() || !depth_path.is_file() || !screenshot_path.is_file() {
      return;
    }

    let img = image::open(&screenshot_path).unwrap();
    let detector = YoloWorldDetector::new(YoloWorldConfig {
      model_path: yolo_path,
      confidence_threshold: 0.05,
      iou_threshold: 0.45,
      input_size: 640,
      classes: DEFAULT_MINECRAFT_CLASSES.iter().map(|s| s.to_string()).collect(),
    })
    .unwrap();
    let depth = DepthEstimator::new(&depth_path).unwrap();

    let ingest = VisualPerceptionIngest {
      detector: &detector,
      depth: &depth,
      calibrator: &empty_calibrator,
      screenshot: &img,
      observer: PlayerPose {
        eye_position: Vec3::new(0.0, 64.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      viewport: Viewport::new(854, 480),
      vertical_fov_deg: 70.0,
      observation_ref: ObservationRef {
        observation_id: "obs-gate".to_string(),
        captured_at_millis: 1000,
      },
      static_whitelist: None,
      confidence_threshold: None,
    };

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let mut store = SpatialMemoryStore::open(tmp.path()).unwrap();
    let report = ingest.ingest(&mut store, 1000);

    assert_eq!(report.landmarks_created, 0);
    assert_eq!(report.observations_skipped, 1);
    assert_eq!(report.skipped_reason, Some("no depth calibration anchors".to_string()));
  }
}
