//! Pixel evidence that a scrolled viewport moved between two captures.
//!
//! A bounded shift search along the scroll axis finds the offset with the
//! smallest sampled difference. "No motion" means the best offset is zero and
//! the remaining difference is below a threshold. This generalizes the NetEase
//! sidebar policy (`supported/apps/auv-netease-music/src/scroll/policies/detection_motion.rs`)
//! to both axes. Unlike [`crate::motion`], which reads scan-frame bounds
//! metadata, this compares the captured pixels themselves.

use image::RgbaImage;
use serde::{Deserialize, Serialize};

/// Axis along which a scroll moves the viewport content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollAxis {
  Vertical,
  Horizontal,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewportPixelMotion {
  /// Best content shift in image pixels along the axis; zero when unchanged.
  pub estimated_shift: i32,
  /// Mean absolute RGB difference in `[0, 1]` at that shift.
  pub normalized_diff: f64,
  pub no_motion: bool,
}

/// Tunables for [`compare_viewport_pixels`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewportPixelPolicy {
  /// Largest shift searched in image pixels.
  pub max_shift: u32,
  /// Highest normalized difference still counted as "no motion".
  pub no_motion_threshold: f64,
  /// Pixel sampling stride on both axes.
  pub sample_step: u32,
}

impl Default for ViewportPixelPolicy {
  // NOTICE(viewport-pixel-defaults): the NetEase sidebar policy validated a
  // ±24 px search, a 0.01 threshold, and a stride of 4 on live captures. Large
  // scroll steps exceed the search window; that still counts as motion,
  // because the zero-shift difference stays high.
  fn default() -> Self {
    Self {
      max_shift: 24,
      no_motion_threshold: 0.01,
      sample_step: 4,
    }
  }
}

/// Compares two equally sized viewport images. Images with different sizes are
/// reported as motion, so a resize never looks like a stuck viewport.
pub fn compare_viewport_pixels(before: &RgbaImage, after: &RgbaImage, axis: ScrollAxis, policy: ViewportPixelPolicy) -> ViewportPixelMotion {
  if before.width() == 0 || before.height() == 0 || before.dimensions() != after.dimensions() {
    return ViewportPixelMotion {
      estimated_shift: 0,
      normalized_diff: 1.0,
      no_motion: false,
    };
  }
  let extent = match axis {
    ScrollAxis::Vertical => before.height(),
    ScrollAxis::Horizontal => before.width(),
  };
  let max_shift = policy.max_shift.min(extent.saturating_sub(1)) as i32;
  let step = policy.sample_step.max(1);
  let (mut best_shift, mut best_diff) = (0, f64::INFINITY);
  for shift in -max_shift..=max_shift {
    let diff = shifted_diff(before, after, axis, shift, step);
    // Prefer zero on ties so an unchanged viewport reports no shift.
    if diff < best_diff || (diff == best_diff && shift == 0) {
      best_shift = shift;
      best_diff = diff;
    }
  }
  ViewportPixelMotion {
    estimated_shift: best_shift,
    normalized_diff: best_diff,
    no_motion: best_shift == 0 && best_diff <= policy.no_motion_threshold,
  }
}

fn shifted_diff(before: &RgbaImage, after: &RgbaImage, axis: ScrollAxis, shift: i32, step: u32) -> f64 {
  let (width, height) = (before.width() as i32, before.height() as i32);
  let (dx, dy) = match axis {
    ScrollAxis::Vertical => (0, shift),
    ScrollAxis::Horizontal => (shift, 0),
  };
  let (overlap_width, overlap_height) = (width - dx.abs(), height - dy.abs());
  if overlap_width <= 0 || overlap_height <= 0 {
    return f64::INFINITY;
  }
  let (before_x, after_x) = if dx >= 0 { (0, dx) } else { (-dx, 0) };
  let (before_y, after_y) = if dy >= 0 { (0, dy) } else { (-dy, 0) };
  let mut total = 0.0;
  let mut samples = 0usize;
  for row in (0..overlap_height).step_by(step as usize) {
    for column in (0..overlap_width).step_by(step as usize) {
      let a = before.get_pixel((before_x + column) as u32, (before_y + row) as u32).0;
      let b = after.get_pixel((after_x + column) as u32, (after_y + row) as u32).0;
      for channel in 0..3 {
        total += (f64::from(a[channel]) - f64::from(b[channel])).abs() / 255.0;
        samples += 1;
      }
    }
  }
  total / samples as f64
}

/// Crops `image` to a normalized region; `None` keeps the whole image.
pub(crate) fn crop_ratio(image: &RgbaImage, region: Option<auv_driver::RelativeRect>) -> RgbaImage {
  let Some(region) = region else {
    return image.clone();
  };
  let (width, height) = (f64::from(image.width()), f64::from(image.height()));
  // Round edges so float noise (0.6 * 50 = 30.000000000000004) cannot add a row.
  let x = (region.x * width).round().clamp(0.0, width) as u32;
  let y = (region.y * height).round().clamp(0.0, height) as u32;
  let right = ((region.x + region.width) * width).round().clamp(0.0, width) as u32;
  let bottom = ((region.y + region.height) * height).round().clamp(0.0, height) as u32;
  image::imageops::crop_imm(image, x, y, right.saturating_sub(x), bottom.saturating_sub(y)).to_image()
}

#[cfg(test)]
#[path = "viewport_pixels_test.rs"]
mod tests;
