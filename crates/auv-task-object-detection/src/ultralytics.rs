use crate::{BoundingBox, Detection, DetectionOptions, DetectionResult};
use auv_inference_common::{ImageFrame, ImageSize, InferenceResult, ModelConfig, ModelId};
use auv_inference_ultralytics::{InferenceDevice, UltralyticsModelConfig, UltralyticsPrediction, UltralyticsSession};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct UltralyticsObjectDetectorConfig {
  pub model_id: ModelId,
  pub model_path: PathBuf,
  pub input_size: Option<u32>,
  pub options: DetectionOptions,
  pub device: InferenceDevice,
  pub class_names_override: Option<Vec<String>>,
}

impl From<ModelConfig> for UltralyticsObjectDetectorConfig {
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

pub struct UltralyticsObjectDetector {
  session: UltralyticsSession,
}

impl std::fmt::Debug for UltralyticsObjectDetector {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.debug_struct("UltralyticsObjectDetector").finish_non_exhaustive()
  }
}

impl UltralyticsObjectDetector {
  pub fn load(config: UltralyticsObjectDetectorConfig) -> InferenceResult<Self> {
    let session = UltralyticsSession::load(UltralyticsModelConfig {
      model_id: config.model_id,
      model_path: config.model_path,
      input_size: config.input_size,
      confidence_threshold: config.options.confidence_threshold,
      iou_threshold: config.options.iou_threshold,
      max_detections: config.options.max_detections,
      device: config.device,
      class_names_override: config.class_names_override,
    })?;

    Ok(Self { session })
  }

  pub fn detect_path(&self, path: impl AsRef<Path>) -> InferenceResult<DetectionResult> {
    detection_result_from_prediction(self.session.predict_path(path)?)
  }

  pub fn detect_frame(&self, frame: &ImageFrame) -> InferenceResult<DetectionResult> {
    detection_result_from_prediction(self.session.predict_frame(frame)?)
  }
}

pub fn detection_result_from_prediction(prediction: UltralyticsPrediction) -> InferenceResult<DetectionResult> {
  let result = prediction.first_result()?;
  let image_size = ImageSize {
    width: result.image_width(),
    height: result.image_height(),
  };
  let Some(boxes) = result.boxes() else {
    return Ok(DetectionResult {
      image_size,
      detections: Vec::new(),
    });
  };

  let mut detections = Vec::with_capacity(boxes.len());
  for index in 0..boxes.len() {
    let [x1, y1, x2, y2] = boxes.xyxy(index)?;
    detections.push(Detection {
      class_id: boxes.class_id(index)?,
      label: boxes.label(index)?,
      confidence: boxes.confidence(index)?,
      bbox: BoundingBox { x1, y1, x2, y2 },
    });
  }

  Ok(DetectionResult {
    image_size,
    detections,
  })
}
