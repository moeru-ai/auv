use auv_inference_common::{BoundingBox, ImageSize};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Detection {
  pub class_id: usize,
  pub label: String,
  pub confidence: f32,
  /// Bounding box in source-image pixel coordinates after backend
  /// preprocessing and postprocessing have been mapped back to the source
  /// image.
  pub bbox: BoundingBox,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DetectionResult {
  pub image_size: ImageSize,
  pub detections: Vec<Detection>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct DetectionOptions {
  pub confidence_threshold: f32,
  pub iou_threshold: f32,
  pub max_detections: usize,
}

impl Default for DetectionOptions {
  fn default() -> Self {
    Self {
      confidence_threshold: 0.25,
      iou_threshold: 0.45,
      max_detections: 300,
    }
  }
}
