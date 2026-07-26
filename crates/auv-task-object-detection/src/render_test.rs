use super::clamped_bbox;
use crate::BoundingBox;

#[test]
fn clamped_bbox_rounds_and_clamps_to_drawable_pixels() {
  assert_eq!(
    clamped_bbox(
      BoundingBox {
        x1: -0.5,
        y1: 1.2,
        x2: 3.1,
        y2: 2.2,
      },
      3,
      3,
    ),
    Some((0, 1, 2, 2))
  );
}

#[test]
fn clamped_bbox_rejects_fractional_boxes_outside_each_image_edge() {
  assert_eq!(
    clamped_bbox(
      BoundingBox {
        x1: -0.9,
        y1: 1.0,
        x2: -0.1,
        y2: 2.0,
      },
      3,
      3,
    ),
    None
  );
  assert_eq!(
    clamped_bbox(
      BoundingBox {
        x1: 3.1,
        y1: 1.0,
        x2: 3.9,
        y2: 2.0,
      },
      3,
      3,
    ),
    None
  );
  assert_eq!(
    clamped_bbox(
      BoundingBox {
        x1: 1.0,
        y1: 3.1,
        x2: 2.0,
        y2: 3.9,
      },
      3,
      3,
    ),
    None
  );
}

#[test]
fn clamped_bbox_rejects_non_finite_coordinates() {
  assert_eq!(
    clamped_bbox(
      BoundingBox {
        x1: f32::NAN,
        y1: 1.0,
        x2: 2.0,
        y2: 2.0,
      },
      3,
      3,
    ),
    None
  );
}
