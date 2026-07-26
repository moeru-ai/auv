use auv_task_object_detection::{BoundingBox, Detection};
use image::{Rgb, RgbImage};

use crate::cache::cache_hint_for_detection;

#[test]
fn cache_hint_handles_partially_out_of_bounds_bbox() {
  let image = RgbImage::from_fn(160, 160, |x, y| Rgb([(x % 251) as u8, (y % 251) as u8, ((x + y) % 251) as u8]));
  let detection = Detection {
    class_id: 0,
    label: "joker_card".to_owned(),
    confidence: 0.9,
    bbox: BoundingBox {
      x1: -5.0,
      y1: 2.0,
      x2: 8.0,
      y2: 20.0,
    },
  };

  let hint = cache_hint_for_detection(&detection, &image, false);

  assert!(hint.needs_reading);
  assert!(hint.visual_fingerprint.is_some());
}
