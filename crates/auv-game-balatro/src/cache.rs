use auv_inference_common::{BoundingBox, Detection};
use image::RgbImage;

use crate::model::CacheHint;

// TODO(balatro-cache-v1): Reading cache storage and invalidation policy are
// deferred until observation/read commands need cached enrichment.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReadingCache;

pub fn cache_hint_for_detection(detection: &Detection, image: &RgbImage, _no_cache: bool) -> CacheHint {
  // `no_cache` bypasses persisted/semantic reading reuse, not frame evidence.
  // Live action verification still needs the current-frame fingerprint to
  // prove that hand contents changed when Balatro refills to the same count.
  CacheHint {
    needs_reading: true,
    visual_fingerprint: visual_fingerprint(detection, image),
    changed_since_last_read: true,
  }
}

pub fn visual_fingerprint(detection: &Detection, image: &RgbImage) -> Option<String> {
  let bounds = bounded_crop(detection.bbox, image)?;
  let mut hash = 0xcbf29ce484222325_u64;

  for value in [
    detection.bbox.x1.to_bits(),
    detection.bbox.y1.to_bits(),
    detection.bbox.x2.to_bits(),
    detection.bbox.y2.to_bits(),
  ] {
    hash = fnv1a(hash, value);
  }

  let width = bounds.x2 - bounds.x1;
  let height = bounds.y2 - bounds.y1;
  let x_step = (width / 8).max(1);
  let y_step = (height / 8).max(1);

  let mut y = bounds.y1;
  while y < bounds.y2 {
    let mut x = bounds.x1;
    while x < bounds.x2 {
      for channel in image.get_pixel(x, y).0 {
        hash = fnv1a(hash, channel as u32);
      }
      x = x.saturating_add(x_step);
    }
    y = y.saturating_add(y_step);
  }

  Some(format!("{hash:016x}"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CropBounds {
  x1: u32,
  y1: u32,
  x2: u32,
  y2: u32,
}

fn bounded_crop(bbox: BoundingBox, image: &RgbImage) -> Option<CropBounds> {
  let image_width = image.width();
  let image_height = image.height();
  if image_width == 0 || image_height == 0 {
    return None;
  }

  let x1 = clamp_lower(bbox.x1.floor(), image_width);
  let y1 = clamp_lower(bbox.y1.floor(), image_height);
  let x2 = clamp_upper(bbox.x2.ceil(), image_width);
  let y2 = clamp_upper(bbox.y2.ceil(), image_height);

  if x1 >= x2 || y1 >= y2 {
    return None;
  }

  Some(CropBounds { x1, y1, x2, y2 })
}

fn clamp_lower(value: f32, upper: u32) -> u32 {
  if !value.is_finite() || value <= 0.0 {
    return 0;
  }

  (value as u32).min(upper)
}

fn clamp_upper(value: f32, upper: u32) -> u32 {
  if !value.is_finite() {
    return 0;
  }

  if value <= 0.0 {
    0
  } else {
    (value as u32).min(upper)
  }
}

fn fnv1a(hash: u64, value: u32) -> u64 {
  let mut hash = hash;
  for byte in value.to_le_bytes() {
    hash ^= byte as u64;
    hash = hash.wrapping_mul(0x100000001b3);
  }
  hash
}
