use crate::device::InferenceDevice;
use auv_inference_common::{ImageFrame, InferenceError, InferenceResult, ModelConfig, ModelId};
use image::{DynamicImage, ImageReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use ultralytics_inference::{Boxes, InferenceConfig, Results, YOLOModel};

#[derive(Clone, Debug)]
pub struct UltralyticsModelConfig {
  pub model_id: ModelId,
  pub model_path: PathBuf,
  pub input_size: Option<u32>,
  pub confidence_threshold: f32,
  pub iou_threshold: f32,
  pub max_detections: usize,
  pub device: InferenceDevice,
  pub class_names_override: Option<Vec<String>>,
}

impl From<ModelConfig> for UltralyticsModelConfig {
  fn from(value: ModelConfig) -> Self {
    Self {
      model_id: value.model_id,
      model_path: value.model_path,
      input_size: value.input_size,
      confidence_threshold: 0.25,
      iou_threshold: 0.45,
      max_detections: 300,
      device: InferenceDevice::Cpu,
      class_names_override: None,
    }
  }
}

pub struct UltralyticsSession {
  model_id: ModelId,
  class_names_override: Option<Vec<String>>,
  model: Mutex<YOLOModel>,
}

impl std::fmt::Debug for UltralyticsSession {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("UltralyticsSession")
      .field("model_id", &self.model_id)
      .field("class_names_override", &self.class_names_override)
      .finish_non_exhaustive()
  }
}

#[derive(Clone, Debug)]
pub struct UltralyticsPrediction {
  model_id: ModelId,
  class_names_override: Option<Vec<String>>,
  // TODO(ultralytics-batch-results): This backend stores the upstream result
  // vector but exposes only the first image result because the current public
  // session API accepts one image at a time. Re-open if batch prediction is
  // added to `UltralyticsSession`.
  results: Vec<Results>,
}

#[derive(Debug)]
pub struct UltralyticsResult<'a> {
  result: &'a Results,
  class_names_override: Option<&'a [String]>,
}

#[derive(Debug)]
pub struct UltralyticsBoxes<'a> {
  boxes: &'a Boxes,
  result: &'a Results,
  class_names_override: Option<&'a [String]>,
}

impl UltralyticsSession {
  pub fn load(config: UltralyticsModelConfig) -> InferenceResult<Self> {
    validate_config(&config)?;
    require_model_path(&config.model_path)?;

    let inference_config = build_inference_config(
      config.input_size,
      config.confidence_threshold,
      config.iou_threshold,
      config.max_detections,
      config.device.clone(),
    );
    let model = YOLOModel::load_with_config(&config.model_path, inference_config).map_err(backend_error)?;

    Ok(Self {
      model_id: config.model_id,
      class_names_override: config.class_names_override,
      model: Mutex::new(model),
    })
  }

  pub fn predict_path(&self, path: impl AsRef<Path>) -> InferenceResult<UltralyticsPrediction> {
    let path = path.as_ref();
    let image = load_image_path(path)?;
    let source = path.to_string_lossy().into_owned();
    let results = {
      let mut model = self.lock_model()?;
      model.predict_image(&image, source).map_err(backend_error)?
    };

    Ok(UltralyticsPrediction {
      model_id: self.model_id.clone(),
      class_names_override: self.class_names_override.clone(),
      results,
    })
  }

  pub fn predict_frame(&self, frame: &ImageFrame) -> InferenceResult<UltralyticsPrediction> {
    validate_frame_size(frame)?;

    let image = DynamicImage::ImageRgb8(frame.image.clone());
    let results = {
      let mut model = self.lock_model()?;
      model.predict_image(&image, "<frame>".to_string()).map_err(backend_error)?
    };

    Ok(UltralyticsPrediction {
      model_id: self.model_id.clone(),
      class_names_override: self.class_names_override.clone(),
      results,
    })
  }

  fn lock_model(&self) -> InferenceResult<std::sync::MutexGuard<'_, YOLOModel>> {
    self.model.lock().map_err(|err| InferenceError::SessionUnavailable {
      reason: err.to_string(),
    })
  }
}

impl UltralyticsPrediction {
  pub fn model_id(&self) -> &ModelId {
    &self.model_id
  }

  pub fn first_result(&self) -> InferenceResult<UltralyticsResult<'_>> {
    let result = self.results.first().ok_or(InferenceError::MissingResult)?;

    Ok(UltralyticsResult {
      result,
      class_names_override: self.class_names_override.as_deref(),
    })
  }

  pub fn first_boxes(&self) -> InferenceResult<Option<UltralyticsBoxes<'_>>> {
    let result = self.results.first().ok_or(InferenceError::MissingResult)?;
    let boxes = result.boxes.as_ref();

    Ok(boxes.map(|boxes| UltralyticsBoxes {
      boxes,
      result,
      class_names_override: self.class_names_override.as_deref(),
    }))
  }
}

impl UltralyticsResult<'_> {
  pub fn image_width(&self) -> u32 {
    self.result.orig_shape.1
  }

  pub fn image_height(&self) -> u32 {
    self.result.orig_shape.0
  }

  pub fn boxes(&self) -> Option<UltralyticsBoxes<'_>> {
    let boxes = self.result.boxes.as_ref()?;

    Some(UltralyticsBoxes {
      boxes,
      result: self.result,
      class_names_override: self.class_names_override,
    })
  }
}

impl UltralyticsBoxes<'_> {
  fn checked_index(&self, index: usize) -> InferenceResult<usize> {
    if index < self.len() {
      return Ok(index);
    }

    Err(InferenceError::Backend {
      message: format!("ultralytics box index {index} out of range for {} boxes", self.len()),
    })
  }

  pub fn len(&self) -> usize {
    self.boxes.len()
  }

  pub fn image_width(&self) -> u32 {
    self.result.orig_shape.1
  }

  pub fn image_height(&self) -> u32 {
    self.result.orig_shape.0
  }

  pub fn class_id(&self, index: usize) -> InferenceResult<usize> {
    let index = self.checked_index(index)?;
    Ok(self.boxes.cls()[index] as usize)
  }

  pub fn confidence(&self, index: usize) -> InferenceResult<f32> {
    let index = self.checked_index(index)?;
    Ok(self.boxes.conf()[index])
  }

  pub fn xyxy(&self, index: usize) -> InferenceResult<[f32; 4]> {
    let index = self.checked_index(index)?;
    let xyxy = self.boxes.xyxy();
    Ok([
      xyxy[[index, 0]],
      xyxy[[index, 1]],
      xyxy[[index, 2]],
      xyxy[[index, 3]],
    ])
  }

  pub fn label(&self, index: usize) -> InferenceResult<String> {
    let class_id = self.class_id(index)?;
    if let Some(class_names) = self.class_names_override {
      return class_names.get(class_id).cloned().ok_or(InferenceError::MissingClassLabel { class_id });
    }

    self.result.names.get(&class_id).cloned().ok_or(InferenceError::MissingClassLabel { class_id })
  }
}

fn validate_config(config: &UltralyticsModelConfig) -> InferenceResult<()> {
  if let Some(input_size) = config.input_size
    && input_size == 0
  {
    return Err(InferenceError::InvalidInputSize { input_size });
  }

  validate_threshold("confidence", config.confidence_threshold)?;
  validate_threshold("iou", config.iou_threshold)?;

  if config.max_detections == 0 {
    return Err(InferenceError::InvalidMaxDetections {
      max_detections: config.max_detections,
    });
  }

  if matches!(&config.class_names_override, Some(class_names) if class_names.is_empty()) {
    return Err(InferenceError::EmptyClassList);
  }

  Ok(())
}

fn validate_threshold(name: &'static str, value: f32) -> InferenceResult<()> {
  if value.is_finite() && (0.0..=1.0).contains(&value) {
    return Ok(());
  }

  Err(InferenceError::InvalidThreshold { name, value })
}

fn require_model_path(path: &Path) -> InferenceResult<()> {
  if path.exists() {
    return Ok(());
  }

  Err(InferenceError::MissingModel {
    path: path.to_path_buf(),
  })
}

fn build_inference_config(
  input_size: Option<u32>,
  confidence_threshold: f32,
  iou_threshold: f32,
  max_detections: usize,
  device: InferenceDevice,
) -> InferenceConfig {
  let mut config = InferenceConfig::new()
    .with_confidence(confidence_threshold)
    .with_iou(iou_threshold)
    .with_max_det(max_detections)
    .with_device(device.into())
    .with_save(false);

  if let Some(input_size) = input_size {
    config = config.with_imgsz(input_size as usize, input_size as usize);
  }

  config
}

fn backend_error(error: ultralytics_inference::InferenceError) -> InferenceError {
  InferenceError::Backend {
    message: error.to_string(),
  }
}

fn load_image_path(path: impl AsRef<Path>) -> InferenceResult<DynamicImage> {
  Ok(ImageReader::open(path)?.decode()?)
}

fn validate_frame_size(frame: &ImageFrame) -> InferenceResult<()> {
  let size = frame.size();
  if size.width > 0 && size.height > 0 {
    return Ok(());
  }

  Err(InferenceError::InvalidImageSize {
    width: size.width,
    height: size.height,
  })
}

#[cfg(test)]
mod tests {
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
}
