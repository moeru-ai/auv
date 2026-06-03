use crate::{
  DetectionOptions, DetectionSet, ImageFrame, YoloError, YoloFamily, YoloModelConfig, YoloResult,
  decode::decode_ultralytics_v8_like, letterbox::prepare_input,
};
use ort::{session::Session, value::TensorRef};
use std::sync::{Mutex, PoisonError};

pub struct YoloDetector {
  config: YoloModelConfig,
  session: Mutex<Session>,
}

impl YoloDetector {
  pub fn load(config: YoloModelConfig) -> YoloResult<Self> {
    validate_config(&config)?;

    let session = Session::builder()?.commit_from_file(&config.model_path)?;

    Ok(Self {
      config,
      session: Mutex::new(session),
    })
  }

  pub fn detect(&self, frame: &ImageFrame, options: DetectionOptions) -> YoloResult<DetectionSet> {
    validate_options(options)?;
    validate_frame(frame)?;

    let (input, letterbox) = prepare_input(frame, self.config.input_size);
    let input = TensorRef::from_array_view(&input)?;
    let mut session = self.session.lock().map_err(session_poisoned)?;
    let outputs = session.run(ort::inputs![input])?;
    if outputs.len() == 0 {
      return Err(YoloError::UnsupportedOutputShape { shape: Vec::new() });
    }
    let output = outputs[0].try_extract_array::<f32>()?;

    let detections = match self.config.family {
      YoloFamily::UltralyticsV8Like => {
        decode_ultralytics_v8_like(output, &self.config.class_names, letterbox, options)?
      }
    };

    Ok(DetectionSet {
      model_id: self.config.model_id.clone(),
      image_size: frame.size(),
      detections,
    })
  }
}

fn validate_config(config: &YoloModelConfig) -> YoloResult<()> {
  if config.class_names.is_empty() {
    return Err(YoloError::EmptyClassList);
  }
  if config.input_size == 0 {
    return Err(YoloError::InvalidInputSize {
      input_size: config.input_size,
    });
  }
  if !config.model_path.exists() {
    return Err(YoloError::MissingModel {
      path: config.model_path.clone(),
    });
  }

  Ok(())
}

fn validate_frame(frame: &ImageFrame) -> YoloResult<()> {
  let size = frame.size();
  if size.width == 0 || size.height == 0 {
    return Err(YoloError::InvalidImageSize {
      width: size.width,
      height: size.height,
    });
  }

  Ok(())
}

fn validate_options(options: DetectionOptions) -> YoloResult<()> {
  validate_threshold("confidence", options.confidence_threshold)?;
  validate_threshold("iou", options.iou_threshold)
}

fn validate_threshold(name: &'static str, value: f32) -> YoloResult<()> {
  if !value.is_finite() || !(0.0..=1.0).contains(&value) {
    return Err(YoloError::InvalidThreshold { name, value });
  }

  Ok(())
}

fn session_poisoned<T>(_: PoisonError<T>) -> YoloError {
  YoloError::SessionUnavailable
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{DetectionOptions, ModelId, YoloError, YoloFamily, YoloModelConfig};
  use std::path::PathBuf;

  fn config_with_classes(class_names: Vec<String>) -> YoloModelConfig {
    YoloModelConfig {
      model_id: ModelId("test-model".to_string()),
      model_path: PathBuf::from("missing-test-model.onnx"),
      class_names,
      input_size: 640,
      family: YoloFamily::UltralyticsV8Like,
    }
  }

  #[test]
  fn load_rejects_empty_classes() {
    let result = YoloDetector::load(config_with_classes(Vec::new()));

    assert!(matches!(result, Err(YoloError::EmptyClassList)));
  }

  #[test]
  fn load_rejects_missing_model() {
    let config = config_with_classes(vec!["button".to_string()]);
    let expected_path = config.model_path.clone();

    assert!(matches!(
      YoloDetector::load(config),
      Err(YoloError::MissingModel { path }) if path == expected_path
    ));
  }

  #[test]
  fn load_rejects_zero_input_size_before_missing_model() {
    let mut config = config_with_classes(vec!["button".to_string()]);
    config.input_size = 0;

    assert!(matches!(
      YoloDetector::load(config),
      Err(YoloError::InvalidInputSize { input_size: 0 })
    ));
  }

  #[test]
  fn validate_frame_rejects_zero_width() {
    let frame = ImageFrame::new(image::RgbImage::new(0, 10));

    assert!(matches!(
      validate_frame(&frame),
      Err(YoloError::InvalidImageSize {
        width: 0,
        height: 10
      })
    ));
  }

  #[test]
  fn session_poisoned_maps_to_detector_error() {
    let error = session_poisoned(std::sync::PoisonError::new(()));

    assert!(matches!(error, YoloError::SessionUnavailable));
  }

  #[test]
  fn validate_options_rejects_negative_confidence() {
    let error = validate_options(DetectionOptions {
      confidence_threshold: -0.1,
      iou_threshold: 0.5,
    })
    .unwrap_err();

    assert!(matches!(
      error,
      YoloError::InvalidThreshold {
        name: "confidence",
        value
      } if value == -0.1
    ));
  }

  #[test]
  fn validate_options_rejects_nan_iou() {
    let error = validate_options(DetectionOptions {
      confidence_threshold: 0.5,
      iou_threshold: f32::NAN,
    })
    .unwrap_err();

    assert!(matches!(
      error,
      YoloError::InvalidThreshold {
        name: "iou",
        value
      } if value.is_nan()
    ));
  }

  #[test]
  fn validate_options_rejects_confidence_above_one() {
    let error = validate_options(DetectionOptions {
      confidence_threshold: 1.1,
      iou_threshold: 0.5,
    })
    .unwrap_err();

    assert!(matches!(
      error,
      YoloError::InvalidThreshold {
        name: "confidence",
        value
      } if value == 1.1
    ));
  }
}
