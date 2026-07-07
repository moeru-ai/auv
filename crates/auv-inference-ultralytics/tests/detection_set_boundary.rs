use auv_inference_common::{BoundingBox, Detection, DetectionSet, ImageSize, ModelId};
use serde_json::json;

fn sample_detection_set() -> DetectionSet {
  DetectionSet {
    model_id: ModelId("games-balatro-2024-yolo-ui-detection".to_string()),
    image_size: ImageSize {
      width: 1280,
      height: 660,
    },
    detections: vec![Detection {
      class_id: 28,
      label: "ui_score_chips".to_string(),
      confidence: 0.9867,
      bbox: BoundingBox {
        x1: 108.67,
        y1: 357.69,
        x2: 213.18,
        y2: 411.45,
      },
    }],
  }
}

#[test]
fn detection_set_json_stays_inference_only() {
  let value = serde_json::to_value(sample_detection_set()).expect("DetectionSet should serialize");
  assert_eq!(value["model_id"], json!("games-balatro-2024-yolo-ui-detection"), "DetectionSet JSON should preserve model identity");
  assert_eq!(
    value["image_size"],
    json!({
      "width": 1280,
      "height": 660
    }),
    "DetectionSet JSON should preserve source image size"
  );
  assert_eq!(value["detections"][0]["class_id"], json!(28), "DetectionSet JSON should preserve class id");
  assert_eq!(value["detections"][0]["label"], json!("ui_score_chips"), "DetectionSet JSON should preserve class label");

  let object = value.as_object().expect("DetectionSet JSON should serialize as an object");
  for forbidden in [
    "source_artifact",
    "capture_artifact",
    "capture_contract_artifact",
    "artifact_ref",
    "evidence",
    "known_limits",
    "freshness_basis",
    "liveness",
    "projection",
    "coordinate_space",
    "candidate",
    "candidate_ref",
    "recognition_id",
  ] {
    assert!(!object.contains_key(forbidden), "DetectionSet must not grow runtime bridge field `{forbidden}`");
  }

  let detection = value["detections"][0].as_object().expect("detection should serialize as an object");
  let bbox = detection["bbox"].as_object().expect("bbox should serialize as an object");
  assert_eq!(bbox.len(), 4, "bbox should stay a raw inference rectangle");
  for forbidden in [
    "known_limits",
    "source_artifact",
    "window_ref",
    "display_ref",
    "screen_point",
    "projection",
    "candidate_ref",
  ] {
    assert!(!detection.contains_key(forbidden), "Detection must not grow runtime bridge field `{forbidden}`");
  }
}

#[test]
fn detection_set_roundtrip_does_not_require_runtime_bridge_fields() {
  let value = json!({
    "model_id": "games-balatro-2024-yolo-entities-detection",
    "image_size": {
      "width": 1280,
      "height": 660
    },
    "detections": [
      {
        "class_id": 0,
        "label": "card_description",
        "confidence": 0.9706,
        "bbox": {
          "x1": 798.98,
          "y1": 213.76,
          "x2": 963.18,
          "y2": 350.84
        }
      }
    ]
  });

  let parsed: DetectionSet = serde_json::from_value(value).expect("DetectionSet JSON should parse without bridge fields");

  assert_eq!(parsed.model_id.0, "games-balatro-2024-yolo-entities-detection");
  assert_eq!(parsed.image_size.width, 1280);
  assert_eq!(parsed.image_size.height, 660);
  assert_eq!(parsed.detections.len(), 1);
  assert_eq!(parsed.detections[0].label, "card_description");
}
