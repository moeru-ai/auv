use super::*;
use image::RgbImage;
use ndarray::{Array2, Array3};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use ultralytics_inference::Speed;

fn valid_config() -> UltralyticsModelConfig {
  UltralyticsModelConfig {
    model_id: ModelId("test-model".to_string()),
    model_path: PathBuf::from("does-not-exist.onnx"),
    input_size: Some(640),
    confidence_threshold: 0.25,
    iou_threshold: 0.45,
    max_detections: 300,
    device: InferenceDevice::Cpu,
    class_names_override: None,
  }
}

#[test]
fn missing_model_rejected_before_backend_load() {
  let err = UltralyticsSession::load(valid_config()).unwrap_err();

  assert!(matches!(err, InferenceError::MissingModel { .. }), "expected MissingModel, got {err:?}");
}

#[test]
fn zero_input_size_rejected() {
  let err = UltralyticsSession::load(UltralyticsModelConfig {
    input_size: Some(0),
    ..valid_config()
  })
  .unwrap_err();

  assert!(matches!(err, InferenceError::InvalidInputSize { input_size: 0 }), "expected InvalidInputSize, got {err:?}");
}

#[test]
fn nan_confidence_rejected() {
  let err = UltralyticsSession::load(UltralyticsModelConfig {
    confidence_threshold: f32::NAN,
    ..valid_config()
  })
  .unwrap_err();

  assert!(
    matches!(
      err,
      InferenceError::InvalidThreshold {
        name: "confidence",
        ..
      }
    ),
    "expected InvalidThreshold(confidence), got {err:?}"
  );
}

#[test]
fn zero_max_detections_rejected() {
  let err = UltralyticsSession::load(UltralyticsModelConfig {
    max_detections: 0,
    ..valid_config()
  })
  .unwrap_err();

  assert!(matches!(err, InferenceError::InvalidMaxDetections { max_detections: 0 }), "expected InvalidMaxDetections, got {err:?}");
}

#[test]
fn empty_class_names_override_rejected() {
  let err = UltralyticsSession::load(UltralyticsModelConfig {
    class_names_override: Some(Vec::new()),
    ..valid_config()
  })
  .unwrap_err();

  assert!(matches!(err, InferenceError::EmptyClassList), "expected EmptyClassList, got {err:?}");
}

#[test]
fn zero_sized_frame_rejected() {
  let frame = ImageFrame::new(RgbImage::new(0, 1));
  let err = validate_frame_size(&frame).unwrap_err();

  assert!(
    matches!(
      err,
      InferenceError::InvalidImageSize {
        width: 0,
        height: 1
      }
    ),
    "expected InvalidImageSize, got {err:?}"
  );
}

#[test]
fn path_image_open_errors_stay_io_layer() {
  let err = load_image_path("missing-input-image.png").unwrap_err();

  assert!(matches!(err, InferenceError::Io { .. }), "expected Io, got {err:?}");
}

#[test]
fn path_image_decode_errors_stay_image_decode_layer() {
  let path = std::env::temp_dir().join(format!("auv-ultralytics-invalid-image-{}.txt", std::process::id()));
  std::fs::write(&path, b"not an image").unwrap();

  let err = load_image_path(&path).unwrap_err();
  std::fs::remove_file(&path).unwrap();

  assert!(matches!(err, InferenceError::ImageDecode { .. }), "expected ImageDecode, got {err:?}");
}

#[test]
fn first_boxes_requires_result() {
  let prediction = UltralyticsPrediction {
    model_id: ModelId("test-model".to_string()),
    class_names_override: None,
    results: Vec::new(),
  };

  let err = prediction.first_boxes().unwrap_err();

  assert!(matches!(err, InferenceError::MissingResult), "expected MissingResult, got {err:?}");
}

#[test]
fn first_result_allows_empty_detection_output() {
  let prediction = UltralyticsPrediction {
    model_id: ModelId("test-model".to_string()),
    class_names_override: None,
    results: vec![Results::new(
      Array3::zeros((8, 8, 3)),
      "test.png".to_string(),
      Arc::new(HashMap::new()),
      Speed::default(),
      (8, 8),
    )],
  };

  let result = prediction.first_result().expect("result should exist");

  assert_eq!(result.image_width(), 8);
  assert_eq!(result.image_height(), 8);
  assert!(result.boxes().is_none(), "empty detections should not be an error");
  assert!(prediction.first_boxes().unwrap().is_none(), "empty detections should return no boxes");
}

#[test]
fn override_missing_class_id_does_not_fall_back_to_backend_names() {
  // ROOT CAUSE:
  //
  // If a caller supplied class names but omitted a detected class id,
  // label lookup could silently fall back to backend names instead of
  // preserving the authoritative override list.
  //
  // Before the fix, class id 1 resolved to `backend-one`.
  // The fix keeps override labels authoritative whenever they are supplied.
  let prediction = UltralyticsPrediction {
    model_id: ModelId("test-model".to_string()),
    class_names_override: Some(vec!["override-zero".to_string()]),
    results: vec![result_with_box_for_class(1, Some("backend-one"))],
  };

  let boxes = prediction.first_boxes().expect("result should exist").expect("boxes should exist");
  let error = boxes.label(0).expect_err("missing override label should fail");

  assert!(matches!(error, InferenceError::MissingClassLabel { class_id: 1 }));
}

#[test]
fn uses_override_label_when_present() {
  let prediction = UltralyticsPrediction {
    model_id: ModelId("test-model".to_string()),
    class_names_override: Some(vec!["override-zero".to_string(), "override-one".to_string()]),
    results: vec![result_with_box_for_class(1, Some("backend-one"))],
  };

  let boxes = prediction.first_boxes().expect("result should exist").expect("boxes should exist");

  assert_eq!(boxes.len(), 1);
  assert_eq!(boxes.image_width(), 8);
  assert_eq!(boxes.image_height(), 8);
  assert_eq!(boxes.class_id(0).unwrap(), 1);
  assert_eq!(boxes.confidence(0).unwrap(), 0.9);
  assert_eq!(boxes.xyxy(0).unwrap(), [1.0, 2.0, 3.0, 4.0]);
  assert_eq!(boxes.label(0).unwrap(), "override-one");
}

#[test]
fn uses_backend_names_without_override() {
  let prediction = UltralyticsPrediction {
    model_id: ModelId("test-model".to_string()),
    class_names_override: None,
    results: vec![result_with_box_for_class(1, Some("backend-one"))],
  };

  let boxes = prediction.first_boxes().expect("result should exist").expect("boxes should exist");

  assert_eq!(prediction.model_id(), &ModelId("test-model".to_string()));
  assert_eq!(boxes.label(0).unwrap(), "backend-one");
}

#[test]
fn out_of_range_box_accessors_return_backend_error() {
  let prediction = UltralyticsPrediction {
    model_id: ModelId("test-model".to_string()),
    class_names_override: None,
    results: vec![result_with_box_for_class(1, Some("backend-one"))],
  };

  let boxes = prediction.first_boxes().expect("result should exist").expect("boxes should exist");

  for error in [
    boxes.class_id(1).unwrap_err(),
    boxes.confidence(1).unwrap_err(),
    boxes.xyxy(1).unwrap_err(),
    boxes.label(1).unwrap_err(),
  ] {
    match error {
      InferenceError::Backend { message } => {
        assert_eq!(message, "ultralytics box index 1 out of range for 1 boxes");
      }
      other => panic!("expected Backend error, got {other:?}"),
    }
  }
}

fn result_with_box_for_class(class_id: usize, backend_name: Option<&str>) -> Results {
  let mut names = HashMap::new();
  if let Some(backend_name) = backend_name {
    names.insert(class_id, backend_name.to_string());
  }

  let mut result = Results::new(Array3::zeros((8, 8, 3)), "test.png".to_string(), Arc::new(names), Speed::default(), (8, 8));
  result.boxes = Some(Boxes::new(
    Array2::from_shape_vec((1, 6), vec![1.0, 2.0, 3.0, 4.0, 0.9, class_id as f32]).expect("test box shape should be valid"),
    result.orig_shape,
  ));
  result
}
