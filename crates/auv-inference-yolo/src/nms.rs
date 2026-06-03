use crate::{BoundingBox, Detection};
use std::cmp::Ordering;

pub fn class_aware_nms(mut detections: Vec<Detection>, iou_threshold: f32) -> Vec<Detection> {
  detections.sort_by(|left, right| {
    right
      .confidence
      .partial_cmp(&left.confidence)
      .unwrap_or(Ordering::Equal)
  });

  let mut kept: Vec<Detection> = Vec::new();
  for detection in detections {
    let suppressed = kept.iter().any(|kept_detection| {
      kept_detection.class_id == detection.class_id
        && iou(kept_detection.bbox, detection.bbox) >= iou_threshold
    });

    if !suppressed {
      kept.push(detection);
    }
  }

  kept
}

pub fn iou(a: BoundingBox, b: BoundingBox) -> f32 {
  let intersection_width = (a.x2.min(b.x2) - a.x1.max(b.x1)).max(0.0);
  let intersection_height = (a.y2.min(b.y2) - a.y1.max(b.y1)).max(0.0);
  let intersection_area = intersection_width * intersection_height;
  if intersection_area <= 0.0 {
    return 0.0;
  }

  let a_area = positive_area(a);
  let b_area = positive_area(b);
  let union_area = a_area + b_area - intersection_area;
  if union_area <= 0.0 {
    return 0.0;
  }

  intersection_area / union_area
}

fn positive_area(bbox: BoundingBox) -> f32 {
  bbox.width().max(0.0) * bbox.height().max(0.0)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{BoundingBox, Detection};

  fn detection(class_id: usize, confidence: f32, bbox: BoundingBox) -> Detection {
    Detection {
      class_id,
      label: format!("class-{class_id}"),
      confidence,
      bbox,
    }
  }

  fn overlap_box() -> BoundingBox {
    BoundingBox {
      x1: 0.0,
      y1: 0.0,
      x2: 10.0,
      y2: 10.0,
    }
  }

  #[test]
  fn suppresses_lower_confidence_same_class_overlap() {
    let detections = vec![
      detection(0, 0.6, overlap_box()),
      detection(
        0,
        0.9,
        BoundingBox {
          x1: 1.0,
          y1: 1.0,
          x2: 11.0,
          y2: 11.0,
        },
      ),
    ];

    let kept = class_aware_nms(detections, 0.5);

    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].confidence, 0.9);
  }

  #[test]
  fn keeps_overlapping_different_classes() {
    let detections = vec![
      detection(0, 0.8, overlap_box()),
      detection(1, 0.7, overlap_box()),
    ];

    let kept = class_aware_nms(detections, 0.5);

    assert_eq!(kept.len(), 2);
  }

  #[test]
  fn iou_returns_zero_for_non_overlapping_or_degenerate_boxes() {
    let disjoint = iou(
      overlap_box(),
      BoundingBox {
        x1: 20.0,
        y1: 20.0,
        x2: 30.0,
        y2: 30.0,
      },
    );
    let degenerate = iou(
      overlap_box(),
      BoundingBox {
        x1: 5.0,
        y1: 5.0,
        x2: 5.0,
        y2: 8.0,
      },
    );

    assert_eq!(disjoint, 0.0);
    assert_eq!(degenerate, 0.0);
  }
}
