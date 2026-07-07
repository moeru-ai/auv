use crate::{BoundingBox, Detection, DetectionOptions, DetectionResult};
use auv_inference_common::{ImageFrame, ImageSize, InferenceResult, ModelConfig, ModelId};
use auv_inference_ultralytics::{InferenceDevice, UltralyticsBoxes, UltralyticsModelConfig, UltralyticsPrediction, UltralyticsSession};
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

  detection_result_from_boxes(image_size, Some(&boxes))
}

trait DetectionBoxes {
  fn len(&self) -> usize;
  fn class_id(&self, index: usize) -> InferenceResult<usize>;
  fn confidence(&self, index: usize) -> InferenceResult<f32>;
  fn xyxy(&self, index: usize) -> InferenceResult<[f32; 4]>;
  fn label(&self, index: usize) -> InferenceResult<String>;
}

impl DetectionBoxes for UltralyticsBoxes<'_> {
  fn len(&self) -> usize {
    self.len()
  }

  fn class_id(&self, index: usize) -> InferenceResult<usize> {
    self.class_id(index)
  }

  fn confidence(&self, index: usize) -> InferenceResult<f32> {
    self.confidence(index)
  }

  fn xyxy(&self, index: usize) -> InferenceResult<[f32; 4]> {
    self.xyxy(index)
  }

  fn label(&self, index: usize) -> InferenceResult<String> {
    self.label(index)
  }
}

fn detection_result_from_boxes(image_size: ImageSize, boxes: Option<&impl DetectionBoxes>) -> InferenceResult<DetectionResult> {
  let Some(boxes) = boxes else {
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

#[cfg(test)]
mod tests {
  use super::*;
  use auv_inference_common::InferenceError;
  use ndarray::{Array2, Array3};
  use std::collections::HashMap;
  use std::sync::Arc;
  use ultralytics_inference::{Boxes, Results, Speed};

  #[test]
  fn converts_backend_boxes_to_detection_result() {
    let result = result_with_box_for_class(1, Some("backend-one"));
    let detections = detection_result_from_fixture(&result, None).expect("prediction should convert");

    assert_eq!(
      detections,
      DetectionResult {
        image_size: ImageSize {
          width: 8,
          height: 8,
        },
        detections: vec![Detection {
          class_id: 1,
          label: "backend-one".to_string(),
          confidence: 0.9,
          bbox: BoundingBox {
            x1: 1.0,
            y1: 2.0,
            x2: 3.0,
            y2: 4.0,
          },
        }],
      }
    );
  }

  #[test]
  fn conversion_uses_override_labels_when_present() {
    let result = result_with_box_for_class(1, Some("backend-one"));
    let override_labels = vec!["override-zero".to_string(), "override-one".to_string()];
    let detections = detection_result_from_fixture(&result, Some(&override_labels)).expect("prediction should convert");

    assert_eq!(detections.detections[0].label, "override-one");
  }

  #[test]
  fn conversion_allows_zero_detections_with_image_size() {
    let result = empty_result();
    let detections = detection_result_from_fixture(&result, None).expect("empty detection output should convert");

    assert_eq!(
      detections,
      DetectionResult {
        image_size: ImageSize {
          width: 8,
          height: 8,
        },
        detections: Vec::new(),
      }
    );
  }

  struct FixtureBoxes<'a> {
    boxes: &'a Boxes,
    result: &'a Results,
    class_names_override: Option<&'a [String]>,
  }

  impl DetectionBoxes for FixtureBoxes<'_> {
    fn len(&self) -> usize {
      self.boxes.len()
    }

    fn class_id(&self, index: usize) -> InferenceResult<usize> {
      let index = self.checked_index(index)?;
      Ok(self.boxes.cls()[index] as usize)
    }

    fn confidence(&self, index: usize) -> InferenceResult<f32> {
      let index = self.checked_index(index)?;
      Ok(self.boxes.conf()[index])
    }

    fn xyxy(&self, index: usize) -> InferenceResult<[f32; 4]> {
      let index = self.checked_index(index)?;
      let xyxy = self.boxes.xyxy();
      Ok([
        xyxy[[index, 0]],
        xyxy[[index, 1]],
        xyxy[[index, 2]],
        xyxy[[index, 3]],
      ])
    }

    fn label(&self, index: usize) -> InferenceResult<String> {
      let class_id = self.class_id(index)?;
      if let Some(class_names) = self.class_names_override {
        return class_names.get(class_id).cloned().ok_or(InferenceError::MissingClassLabel { class_id });
      }

      self.result.names.get(&class_id).cloned().ok_or(InferenceError::MissingClassLabel { class_id })
    }
  }

  impl FixtureBoxes<'_> {
    fn checked_index(&self, index: usize) -> InferenceResult<usize> {
      if index < self.len() {
        return Ok(index);
      }

      Err(InferenceError::Backend {
        message: format!("ultralytics box index {index} out of range for {} boxes", self.len()),
      })
    }
  }

  fn detection_result_from_fixture(result: &Results, class_names_override: Option<&[String]>) -> InferenceResult<DetectionResult> {
    let image_size = ImageSize {
      width: result.orig_shape.1,
      height: result.orig_shape.0,
    };
    let boxes = result.boxes.as_ref().map(|boxes| FixtureBoxes {
      boxes,
      result,
      class_names_override,
    });

    detection_result_from_boxes(image_size, boxes.as_ref())
  }

  fn empty_result() -> Results {
    Results::new(Array3::zeros((8, 8, 3)), "test.png".to_string(), Arc::new(HashMap::new()), Speed::default(), (8, 8))
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
