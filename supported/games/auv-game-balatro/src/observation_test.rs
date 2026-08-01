use auv_api_proto::auv::api::{driver::v1 as driver_proto, image::v1 as image_proto};
use auv_inference_common::ImageSize;
use auv_task_object_detection::{BoundingBox, Detection, DetectionResult};
use image::{Rgb, RgbImage};

use crate::cache::cache_hint_for_detection;

use super::{
  BalatroDetectionSets, build_state_from_detections, driver_runner_options, enrich_ui_numeric_readings_from_recognition,
  inference_runner_options, rgba_frame_to_rgb,
};

#[test]
fn observation_claims_distinct_typed_driver_and_inference_runners() {
  let driver = driver_runner_options();
  let inference = inference_runner_options();

  assert_eq!(driver.runner_class, "auv.core.local");
  assert_eq!(inference.runner_class, "auv.inference.ultralytics");
  assert!(driver.required_capabilities.iter().any(|capability| capability.service == "auv.api.driver.v1.CaptureService"));
  assert_eq!(inference.required_capabilities.len(), 1);
  assert_eq!(inference.required_capabilities[0].service, "auv.api.inference.v1.ObjectDetectionService");
}

#[test]
fn rgba_capture_is_converted_to_packed_rgb_for_inference() {
  let frame = image_proto::RgbaFrame {
    width: 2,
    height: 1,
    data: vec![1, 2, 3, 4, 5, 6, 7, 8],
  };

  let rgb = rgba_frame_to_rgb(&frame).expect("valid RGBA frame");

  assert_eq!(rgb.width, 2);
  assert_eq!(rgb.height, 1);
  assert_eq!(rgb.data, vec![1, 2, 3, 5, 6, 7]);
}

#[test]
fn cache_hint_handles_partially_out_of_bounds_bbox() {
  let image = RgbImage::from_fn(160, 160, |x, y| Rgb([(x % 251) as u8, (y % 251) as u8, ((x + y) % 251) as u8]));
  let detection = Detection {
    class_id: 0,
    label: "joker_card".to_owned(),
    confidence: 0.9,
    bbox: BoundingBox {
      x1: -5.0,
      y1: 2.0,
      x2: 8.0,
      y2: 20.0,
    },
  };

  let hint = cache_hint_for_detection(&detection, &image, false);

  assert!(hint.needs_reading);
  assert!(hint.visual_fingerprint.is_some());
}

#[test]
fn driver_runner_ocr_logical_bounds_enrich_pixel_space_numeric_detection() {
  let image = RgbImage::new(200, 100);
  let image_size = ImageSize {
    width: 200,
    height: 100,
  };
  let ui = DetectionResult {
    image_size,
    detections: vec![
      Detection {
        class_id: 0,
        label: "ui_score_chips".to_string(),
        confidence: 0.9,
        bbox: BoundingBox {
          x1: 90.0,
          y1: 20.0,
          x2: 130.0,
          y2: 60.0,
        },
      },
      Detection {
        class_id: 1,
        label: "button_play".to_string(),
        confidence: 0.9,
        bbox: BoundingBox {
          x1: 10.0,
          y1: 70.0,
          x2: 40.0,
          y2: 90.0,
        },
      },
    ],
  };
  let mut state = build_state_from_detections(
    "daemon://window/test",
    image_size,
    &image,
    BalatroDetectionSets {
      entities: DetectionResult {
        image_size,
        detections: Vec::new(),
      },
      ui,
    },
    true,
  );
  let capture = driver_proto::CapturedFrame {
    image: Some(image_proto::RgbaFrame {
      width: 200,
      height: 100,
      data: vec![0; 200 * 100 * 4],
    }),
    bounds: Some(driver_proto::ScreenRect {
      x: 10.0,
      y: 20.0,
      width: 20.0,
      height: 10.0,
    }),
    scale_factor: 10.0,
    backend: "fixture".to_string(),
    fallback_reason: None,
  };
  let recognition = driver_proto::RecognizeTextResponse {
    text: "$1,234".to_string(),
    regions: vec![driver_proto::RecognizedText {
      text: "$1,234".to_string(),
      bounds: Some(driver_proto::ScreenRect {
        x: 20.0,
        y: 23.0,
        width: 2.0,
        height: 1.0,
      }),
      confidence: Some(0.99),
    }],
  };

  enrich_ui_numeric_readings_from_recognition(&mut state, &capture, recognition);

  assert_eq!(state.scores.chips.as_deref(), Some("$1234"));
}
