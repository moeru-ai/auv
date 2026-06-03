use crate::{
  BoundingBox, Detection, DetectionOptions, YoloError, YoloResult,
  letterbox::{Letterbox, project_model_bbox_to_source},
};
use ndarray::{ArrayViewD, Ix3};

pub fn decode_ultralytics_v8_like(
  output: ArrayViewD<'_, f32>,
  classes: &[String],
  letterbox: Letterbox,
  options: DetectionOptions,
) -> YoloResult<Vec<Detection>> {
  if classes.is_empty() {
    return Err(YoloError::EmptyClassList);
  }
  validate_threshold("confidence", options.confidence_threshold)?;
  validate_threshold("iou", options.iou_threshold)?;

  let shape = output.shape().to_vec();
  if shape.len() != 3 || shape[0] != 1 {
    return Err(YoloError::UnsupportedOutputShape { shape });
  }

  let expected_channels = 4 + classes.len();
  let actual_channels = shape[1];
  if actual_channels != expected_channels {
    return Err(YoloError::ClassCountMismatch {
      expected_channels,
      actual_channels,
    });
  }

  let output =
    output
      .into_dimensionality::<Ix3>()
      .map_err(|_| YoloError::UnsupportedOutputShape {
        shape: shape.clone(),
      })?;
  let anchor_count = output.shape()[2];
  let mut detections = Vec::new();

  for anchor_index in 0..anchor_count {
    let mut best_class_id = 0;
    let mut best_score = f32::NEG_INFINITY;
    for class_id in 0..classes.len() {
      let score = output[[0, 4 + class_id, anchor_index]];
      if score > best_score {
        best_class_id = class_id;
        best_score = score;
      }
    }

    if best_score < options.confidence_threshold {
      continue;
    }

    let center_x = output[[0, 0, anchor_index]];
    let center_y = output[[0, 1, anchor_index]];
    let width = output[[0, 2, anchor_index]];
    let height = output[[0, 3, anchor_index]];
    let half_width = width / 2.0;
    let half_height = height / 2.0;
    let model_bbox = BoundingBox {
      x1: center_x - half_width,
      y1: center_y - half_height,
      x2: center_x + half_width,
      y2: center_y + half_height,
    };

    detections.push(Detection {
      class_id: best_class_id,
      label: classes[best_class_id].clone(),
      confidence: best_score,
      bbox: project_model_bbox_to_source(model_bbox, letterbox),
    });
  }

  Ok(crate::nms::class_aware_nms(
    detections,
    options.iou_threshold,
  ))
}

fn validate_threshold(name: &'static str, value: f32) -> YoloResult<()> {
  if !value.is_finite() || !(0.0..=1.0).contains(&value) {
    return Err(YoloError::InvalidThreshold { name, value });
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{BoundingBox, DetectionOptions, YoloError, letterbox::Letterbox};
  use ndarray::{Array2, Array3};

  fn identity_letterbox() -> Letterbox {
    Letterbox {
      source_width: 640,
      source_height: 640,
      input_size: 640,
      scale: 1.0,
      pad_left: 0,
      pad_top: 0,
    }
  }

  #[test]
  fn decodes_highest_class_from_channel_first_output() {
    let classes = vec!["button".to_string(), "field".to_string()];
    let mut output = Array3::<f32>::zeros((1, 6, 1));
    output[[0, 0, 0]] = 100.0;
    output[[0, 1, 0]] = 110.0;
    output[[0, 2, 0]] = 20.0;
    output[[0, 3, 0]] = 30.0;
    output[[0, 4, 0]] = 0.2;
    output[[0, 5, 0]] = 0.9;

    let detections = decode_ultralytics_v8_like(
      output.view().into_dyn(),
      &classes,
      identity_letterbox(),
      DetectionOptions {
        confidence_threshold: 0.5,
        iou_threshold: 0.5,
      },
    )
    .unwrap();

    assert_eq!(detections.len(), 1);
    assert_eq!(detections[0].class_id, 1);
    assert_eq!(detections[0].label, "field");
    assert_eq!(detections[0].confidence, 0.9);
    assert_eq!(
      detections[0].bbox,
      BoundingBox {
        x1: 90.0,
        y1: 95.0,
        x2: 110.0,
        y2: 125.0,
      }
    );
  }

  #[test]
  fn rejects_wrong_channel_count() {
    let classes = vec!["button".to_string(), "field".to_string()];
    let output = Array3::<f32>::zeros((1, 5, 1));

    let error = decode_ultralytics_v8_like(
      output.view().into_dyn(),
      &classes,
      identity_letterbox(),
      DetectionOptions::default(),
    )
    .unwrap_err();

    assert!(matches!(
      error,
      YoloError::ClassCountMismatch {
        expected_channels: 6,
        actual_channels: 5
      }
    ));
  }

  #[test]
  fn rejects_empty_class_list_before_decoding() {
    let classes = Vec::new();
    let output = Array3::<f32>::zeros((1, 4, 1));

    let error = decode_ultralytics_v8_like(
      output.view().into_dyn(),
      &classes,
      identity_letterbox(),
      DetectionOptions::default(),
    )
    .unwrap_err();

    assert!(matches!(error, YoloError::EmptyClassList));
  }

  #[test]
  fn rejects_nan_confidence_threshold() {
    let classes = vec!["button".to_string()];
    let output = Array3::<f32>::zeros((1, 5, 1));

    let error = decode_ultralytics_v8_like(
      output.view().into_dyn(),
      &classes,
      identity_letterbox(),
      DetectionOptions {
        confidence_threshold: f32::NAN,
        iou_threshold: 0.5,
      },
    )
    .unwrap_err();

    assert!(matches!(
      error,
      YoloError::InvalidThreshold {
        name: "confidence",
        value
      } if value.is_nan()
    ));
  }

  #[test]
  fn rejects_out_of_range_iou_threshold() {
    let classes = vec!["button".to_string()];
    let output = Array3::<f32>::zeros((1, 5, 1));

    let error = decode_ultralytics_v8_like(
      output.view().into_dyn(),
      &classes,
      identity_letterbox(),
      DetectionOptions {
        confidence_threshold: 0.5,
        iou_threshold: 1.1,
      },
    )
    .unwrap_err();

    assert!(matches!(
      error,
      YoloError::InvalidThreshold {
        name: "iou",
        value: 1.1
      }
    ));
  }

  #[test]
  fn rejects_unsupported_rank() {
    let classes = vec!["button".to_string()];
    let output = Array2::<f32>::zeros((5, 1));

    let error = decode_ultralytics_v8_like(
      output.view().into_dyn(),
      &classes,
      identity_letterbox(),
      DetectionOptions::default(),
    )
    .unwrap_err();

    assert!(matches!(
      error,
      YoloError::UnsupportedOutputShape { shape } if shape == vec![5, 1]
    ));
  }

  #[test]
  fn rejects_unsupported_batch() {
    let classes = vec!["button".to_string()];
    let output = Array3::<f32>::zeros((2, 5, 1));

    let error = decode_ultralytics_v8_like(
      output.view().into_dyn(),
      &classes,
      identity_letterbox(),
      DetectionOptions::default(),
    )
    .unwrap_err();

    assert!(matches!(
      error,
      YoloError::UnsupportedOutputShape { shape } if shape == vec![2, 5, 1]
    ));
  }
}
