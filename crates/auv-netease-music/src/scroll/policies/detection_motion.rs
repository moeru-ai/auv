use image::RgbaImage;
use serde::{Deserialize, Serialize};

const DEFAULT_MAX_SHIFT_Y: i32 = 24;
const DEFAULT_NO_MOTION_DIFF_THRESHOLD: f64 = 0.01;
const SAMPLE_STEP: usize = 4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionEvidence {
  pub estimated_shift_y: i32,
  pub normalized_diff: f64,
  pub no_motion: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MotionDetectionPolicy {
  max_shift_y: i32,
  no_motion_diff_threshold: f64,
}

impl Default for MotionDetectionPolicy {
  fn default() -> Self {
    Self {
      max_shift_y: DEFAULT_MAX_SHIFT_Y,
      no_motion_diff_threshold: DEFAULT_NO_MOTION_DIFF_THRESHOLD,
    }
  }
}

impl MotionDetectionPolicy {
  pub fn compare(&self, before: &RgbaImage, after: &RgbaImage) -> MotionEvidence {
    // REVIEW(netease-scroll-motion): this bounded shift search is a first
    // motion-evidence implementation. Live NetEase testing showed it records
    // useful movement values, but it did not resolve completion for large
    // playlists before max_scrolls; prefer section-count or scroll bar state
    // evidence for the next completion-policy slice.
    if before.width() == 0 || before.height() == 0 || before.width() != after.width() || before.height() != after.height() {
      return MotionEvidence {
        estimated_shift_y: 0,
        normalized_diff: 1.0,
        no_motion: false,
      };
    }

    let max_shift_y = self.max_shift_y.min(before.height().saturating_sub(1) as i32);
    let mut best_shift_y = 0;
    let mut best_diff = f64::INFINITY;
    for shift_y in -max_shift_y..=max_shift_y {
      let diff = normalized_diff_for_shift(before, after, shift_y);
      if diff < best_diff {
        best_shift_y = shift_y;
        best_diff = diff;
      }
    }

    MotionEvidence {
      estimated_shift_y: best_shift_y,
      normalized_diff: best_diff,
      no_motion: best_shift_y == 0 && best_diff <= self.no_motion_diff_threshold,
    }
  }
}

fn normalized_diff_for_shift(before: &RgbaImage, after: &RgbaImage, shift_y: i32) -> f64 {
  let height = before.height() as i32;
  let width = before.width();
  let before_start_y = if shift_y >= 0 { 0 } else { -shift_y };
  let after_start_y = if shift_y >= 0 { shift_y } else { 0 };
  let rows = height - shift_y.abs();
  if rows <= 0 {
    return f64::INFINITY;
  }

  let mut total = 0.0;
  let mut samples = 0usize;
  for row in (0..rows).step_by(SAMPLE_STEP) {
    let before_y = (before_start_y + row) as u32;
    let after_y = (after_start_y + row) as u32;
    for x in (0..width).step_by(SAMPLE_STEP) {
      let before_pixel = before.get_pixel(x, before_y).0;
      let after_pixel = after.get_pixel(x, after_y).0;
      for channel in 0..3 {
        total += (before_pixel[channel] as f64 - after_pixel[channel] as f64).abs() / 255.0;
        samples += 1;
      }
    }
  }

  total / samples as f64
}

#[cfg(test)]
mod tests {
  use image::{Rgba, RgbaImage};

  use super::MotionDetectionPolicy;

  #[test]
  fn shifted_image_estimates_nonzero_vertical_shift() {
    let before = striped_image(24, 24);
    let after = shift_down(&before, 3);
    let evidence = MotionDetectionPolicy::default().compare(&before, &after);

    assert_eq!(evidence.estimated_shift_y, 3);
    assert!(!evidence.no_motion);
  }

  #[test]
  fn identical_image_is_no_motion() {
    let before = striped_image(24, 24);
    let evidence = MotionDetectionPolicy::default().compare(&before, &before);

    assert_eq!(evidence.estimated_shift_y, 0);
    assert!(evidence.no_motion);
  }

  #[test]
  fn tiny_noise_is_still_no_motion() {
    let before = striped_image(24, 24);
    let mut after = before.clone();
    for y in (0..24).step_by(8) {
      let pixel = after.get_pixel_mut(0, y);
      pixel.0[0] = pixel.0[0].saturating_add(1);
    }
    let evidence = MotionDetectionPolicy::default().compare(&before, &after);

    assert_eq!(evidence.estimated_shift_y, 0);
    assert!(evidence.no_motion);
  }

  fn striped_image(width: u32, height: u32) -> RgbaImage {
    let mut image = RgbaImage::new(width, height);
    for y in 0..height {
      for x in 0..width {
        let value = ((y * 13 + x * 3) % 255) as u8;
        image.put_pixel(x, y, Rgba([value, value.saturating_add(17), 255 - value, 255]));
      }
    }
    image
  }

  fn shift_down(image: &RgbaImage, shift: u32) -> RgbaImage {
    let mut shifted = RgbaImage::new(image.width(), image.height());
    for y in shift..image.height() {
      for x in 0..image.width() {
        shifted.put_pixel(x, y, *image.get_pixel(x, y - shift));
      }
    }
    shifted
  }
}
