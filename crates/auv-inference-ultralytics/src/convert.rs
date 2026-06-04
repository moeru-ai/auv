use auv_inference_common::{
  BoundingBox, Detection, DetectionSet, ImageSize, InferenceError, InferenceResult, ModelId,
};
use ultralytics_inference::Results;

pub fn detection_set_from_result(
  model_id: &ModelId,
  result: &Results,
  class_names_override: Option<&[String]>,
) -> InferenceResult<DetectionSet> {
  let boxes = result.boxes.as_ref().ok_or(InferenceError::MissingBoxes)?;
  let xyxy = boxes.xyxy();
  let conf = boxes.conf();
  let cls = boxes.cls();

  let mut detections = Vec::with_capacity(boxes.len());
  for index in 0..boxes.len() {
    let class_id = cls[index] as usize;
    let label = resolve_label(class_id, result, class_names_override)?;
    detections.push(Detection {
      class_id,
      label,
      confidence: conf[index],
      bbox: BoundingBox {
        x1: xyxy[[index, 0]],
        y1: xyxy[[index, 1]],
        x2: xyxy[[index, 2]],
        y2: xyxy[[index, 3]],
      },
    });
  }

  Ok(DetectionSet {
    model_id: model_id.clone(),
    image_size: ImageSize {
      width: result.orig_shape.1,
      height: result.orig_shape.0,
    },
    detections,
  })
}

fn resolve_label(
  class_id: usize,
  result: &Results,
  class_names_override: Option<&[String]>,
) -> InferenceResult<String> {
  if let Some(class_names) = class_names_override {
    return class_names
      .get(class_id)
      .cloned()
      .ok_or(InferenceError::MissingClassLabel { class_id });
  }

  result
    .names
    .get(&class_id)
    .cloned()
    .ok_or(InferenceError::MissingClassLabel { class_id })
}

#[cfg(test)]
mod tests {
  use super::*;
  use ndarray::{Array2, Array3};
  use std::collections::HashMap;
  use std::sync::Arc;
  use ultralytics_inference::{Boxes, Speed};

  #[test]
  fn override_missing_class_id_does_not_fall_back_to_backend_names() {
    // ROOT CAUSE:
    //
    // If a caller supplied class names but omitted a detected class id,
    // conversion silently fell back to the backend result label because
    // the override branch only returned on a match.
    //
    // Before the fix, class id 1 resolved to `backend-one`.
    // The fix keeps override labels authoritative whenever they are supplied.
    let mut result = result_with_box_for_class(1);
    result.names = Arc::new(HashMap::from([(1, "backend-one".to_string())]));
    let override_names = vec!["override-zero".to_string()];

    let error = detection_set_from_result(
      &ModelId("test-model".to_string()),
      &result,
      Some(&override_names),
    )
    .expect_err("missing override label should fail");

    assert!(matches!(
      error,
      InferenceError::MissingClassLabel { class_id: 1 }
    ));
  }

  fn result_with_box_for_class(class_id: usize) -> Results {
    let mut result = Results::new(
      Array3::zeros((8, 8, 3)),
      "test.png".to_string(),
      Arc::new(HashMap::new()),
      Speed::default(),
      (8, 8),
    );
    result.boxes = Some(Boxes::new(
      Array2::from_shape_vec((1, 6), vec![1.0, 2.0, 3.0, 4.0, 0.9, class_id as f32])
        .expect("test box shape should be valid"),
      result.orig_shape,
    ));
    result
  }
}
