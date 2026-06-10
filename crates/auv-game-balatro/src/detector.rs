use std::path::Path;

use auv_inference_common::{DetectionOptions, DetectionSet, InferenceResult, ModelId};
use auv_inference_ultralytics::{UltralyticsDetector, UltralyticsModelConfig};

use crate::config::{BalatroModelConfig, load_class_names};

#[derive(Debug)]
pub struct BalatroDetectors {
  entities: UltralyticsDetector,
  ui: UltralyticsDetector,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BalatroDetectionSets {
  pub entities: DetectionSet,
  pub ui: DetectionSet,
}

impl BalatroDetectors {
  pub fn load(config: BalatroModelConfig) -> InferenceResult<Self> {
    let config =
      config
        .resolve()
        .map_err(|error| auv_inference_common::InferenceError::Backend {
          message: error.to_string(),
        })?;
    let entities = UltralyticsDetector::load(UltralyticsModelConfig {
      model_id: ModelId("balatro-entities".to_owned()),
      model_path: config.entities_model,
      input_size: Some(640),
      options: balatro_detection_options(),
      device: config.device.clone(),
      class_names_override: Some(load_class_names(&config.entities_classes)?),
    })?;
    let ui = UltralyticsDetector::load(UltralyticsModelConfig {
      model_id: ModelId("balatro-ui".to_owned()),
      model_path: config.ui_model,
      input_size: Some(640),
      options: balatro_detection_options(),
      device: config.device,
      class_names_override: Some(load_class_names(&config.ui_classes)?),
    })?;

    Ok(Self { entities, ui })
  }

  pub fn detect_path(&self, image: impl AsRef<Path>) -> InferenceResult<BalatroDetectionSets> {
    let image = image.as_ref();
    let entities = self.entities.detect_path(image)?;
    let ui = self.ui.detect_path(image)?;
    Ok(BalatroDetectionSets { entities, ui })
  }
}

fn balatro_detection_options() -> DetectionOptions {
  DetectionOptions {
    confidence_threshold: 0.25,
    iou_threshold: 0.45,
    max_detections: 300,
  }
}
