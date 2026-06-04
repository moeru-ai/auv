use crate::convert::detection_set_from_result;
use crate::device::InferenceDevice;
use auv_inference_common::{
  DetectionOptions, DetectionSet, ImageFrame, InferenceError, InferenceResult, ModelConfig, ModelId,
};
use image::{DynamicImage, ImageReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use ultralytics_inference::{InferenceConfig, YOLOModel};

#[derive(Clone, Debug)]
pub struct UltralyticsModelConfig {
  pub model_id: ModelId,
  pub model_path: PathBuf,
  pub input_size: Option<u32>,
  pub options: DetectionOptions,
  pub device: InferenceDevice,
  pub class_names_override: Option<Vec<String>>,
}

impl From<ModelConfig> for UltralyticsModelConfig {
  fn from(value: ModelConfig) -> Self {
    Self {
      model_id: value.model_id,
      model_path: value.model_path,
      input_size: value.input_size,
      options: DetectionOptions::default(),
      device: InferenceDevice::Cpu,
      class_names_override: None,
    }
  }
}

pub struct UltralyticsDetector {
  model_id: ModelId,
  class_names_override: Option<Vec<String>>,
  model: Mutex<YOLOModel>,
}

impl std::fmt::Debug for UltralyticsDetector {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("UltralyticsDetector")
      .field("model_id", &self.model_id)
      .field("class_names_override", &self.class_names_override)
      .finish_non_exhaustive()
  }
}

impl UltralyticsDetector {
  pub fn load(config: UltralyticsModelConfig) -> InferenceResult<Self> {
    validate_config(&config)?;
    require_model_path(&config.model_path)?;

    let inference_config =
      build_inference_config(config.input_size, config.options, config.device.clone());
    let model =
      YOLOModel::load_with_config(&config.model_path, inference_config).map_err(backend_error)?;

    Ok(Self {
      model_id: config.model_id,
      class_names_override: config.class_names_override,
      model: Mutex::new(model),
    })
  }

  pub fn detect_path(&self, path: impl AsRef<Path>) -> InferenceResult<DetectionSet> {
    let path = path.as_ref();
    let image = load_image_path(path)?;
    let source = path.to_string_lossy().into_owned();
    let results = {
      let mut model = self.lock_model()?;
      model.predict_image(&image, source).map_err(backend_error)?
    };
    self.convert_first_result(results)
  }

  pub fn detect_frame(&self, frame: &ImageFrame) -> InferenceResult<DetectionSet> {
    validate_frame_size(frame)?;

    let image = DynamicImage::ImageRgb8(frame.image.clone());
    let results = {
      let mut model = self.lock_model()?;
      model
        .predict_image(&image, "<frame>".to_string())
        .map_err(backend_error)?
    };
    self.convert_first_result(results)
  }

  fn lock_model(&self) -> InferenceResult<std::sync::MutexGuard<'_, YOLOModel>> {
    self
      .model
      .lock()
      .map_err(|err| InferenceError::SessionUnavailable {
        reason: err.to_string(),
      })
  }

  fn convert_first_result(
    &self,
    results: Vec<ultralytics_inference::Results>,
  ) -> InferenceResult<DetectionSet> {
    let result = results
      .into_iter()
      .next()
      .ok_or(InferenceError::MissingResult)?;
    detection_set_from_result(
      &self.model_id,
      &result,
      self.class_names_override.as_deref(),
    )
  }
}

fn validate_config(config: &UltralyticsModelConfig) -> InferenceResult<()> {
  if let Some(input_size) = config.input_size
    && input_size == 0
  {
    return Err(InferenceError::InvalidInputSize { input_size });
  }

  validate_threshold("confidence", config.options.confidence_threshold)?;
  validate_threshold("iou", config.options.iou_threshold)?;

  if config.options.max_detections == 0 {
    return Err(InferenceError::InvalidMaxDetections {
      max_detections: config.options.max_detections,
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
  options: DetectionOptions,
  device: InferenceDevice,
) -> InferenceConfig {
  let mut config = InferenceConfig::new()
    .with_confidence(options.confidence_threshold)
    .with_iou(options.iou_threshold)
    .with_max_det(options.max_detections)
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
  use auv_inference_common::{DetectionOptions, ImageFrame, InferenceError, ModelId};
  use image::RgbImage;
  use std::path::PathBuf;

  fn valid_config() -> UltralyticsModelConfig {
    UltralyticsModelConfig {
      model_id: ModelId("test-model".to_string()),
      model_path: PathBuf::from("does-not-exist.onnx"),
      input_size: Some(640),
      options: DetectionOptions::default(),
      device: InferenceDevice::Cpu,
      class_names_override: None,
    }
  }

  #[test]
  fn missing_model_rejected_before_backend_load() {
    let err = UltralyticsDetector::load(valid_config()).unwrap_err();

    assert!(
      matches!(err, InferenceError::MissingModel { .. }),
      "expected MissingModel, got {err:?}"
    );
  }

  #[test]
  fn zero_input_size_rejected() {
    let err = UltralyticsDetector::load(UltralyticsModelConfig {
      input_size: Some(0),
      ..valid_config()
    })
    .unwrap_err();

    assert!(
      matches!(err, InferenceError::InvalidInputSize { input_size: 0 }),
      "expected InvalidInputSize, got {err:?}"
    );
  }

  #[test]
  fn nan_confidence_rejected() {
    let err = UltralyticsDetector::load(UltralyticsModelConfig {
      options: DetectionOptions {
        confidence_threshold: f32::NAN,
        ..DetectionOptions::default()
      },
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
    let err = UltralyticsDetector::load(UltralyticsModelConfig {
      options: DetectionOptions {
        max_detections: 0,
        ..DetectionOptions::default()
      },
      ..valid_config()
    })
    .unwrap_err();

    assert!(
      matches!(
        err,
        InferenceError::InvalidMaxDetections { max_detections: 0 }
      ),
      "expected InvalidMaxDetections, got {err:?}"
    );
  }

  #[test]
  fn empty_class_names_override_rejected() {
    let err = UltralyticsDetector::load(UltralyticsModelConfig {
      class_names_override: Some(Vec::new()),
      ..valid_config()
    })
    .unwrap_err();

    assert!(
      matches!(err, InferenceError::EmptyClassList),
      "expected EmptyClassList, got {err:?}"
    );
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

    assert!(
      matches!(err, InferenceError::Io { .. }),
      "expected Io, got {err:?}"
    );
  }

  #[test]
  fn path_image_decode_errors_stay_image_decode_layer() {
    let path = std::env::temp_dir().join(format!(
      "auv-ultralytics-invalid-image-{}.txt",
      std::process::id()
    ));
    std::fs::write(&path, b"not an image").unwrap();

    let err = load_image_path(&path).unwrap_err();
    std::fs::remove_file(&path).unwrap();

    assert!(
      matches!(err, InferenceError::ImageDecode { .. }),
      "expected ImageDecode, got {err:?}"
    );
  }
}
