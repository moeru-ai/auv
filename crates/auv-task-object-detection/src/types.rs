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

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn detection_result_json_is_minimal() {
    let result = DetectionResult {
      image_size: ImageSize {
        width: 640,
        height: 480,
      },
      detections: vec![Detection {
        class_id: 1,
        label: "hit_circle".to_string(),
        confidence: 0.91,
        bbox: BoundingBox {
          x1: 100.0,
          y1: 120.0,
          x2: 140.0,
          y2: 160.0,
        },
      }],
    };

    let value = serde_json::to_value(result).expect("detection result should serialize");
    let object = value.as_object().expect("detection result should serialize as an object");

    assert_eq!(value["image_size"]["width"], json!(640));
    assert_eq!(value["detections"][0]["label"], json!("hit_circle"));
    for forbidden in [
      "model_id",
      "source_image",
      "model_run",
      "known_limits",
      "coordinate_space",
      "projection_basis",
      "evidence",
    ] {
      assert!(!object.contains_key(forbidden), "DetectionResult must not carry wrapper field `{forbidden}`");
    }
  }

  #[test]
  fn detection_options_default_matches_object_detection_defaults() {
    let options = DetectionOptions::default();

    assert_eq!(options.confidence_threshold, 0.25);
    assert_eq!(options.iou_threshold, 0.45);
    assert_eq!(options.max_detections, 300);
  }
}
