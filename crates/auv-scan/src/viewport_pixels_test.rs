use image::{Rgba, RgbaImage};

use super::*;

/// Rows (or columns) carry distinct colors so a shift is measurable.
fn striped(width: u32, height: u32, offset: i32, axis: ScrollAxis) -> RgbaImage {
  RgbaImage::from_fn(width, height, |x, y| {
    let index = match axis {
      ScrollAxis::Vertical => y as i32 + offset,
      ScrollAxis::Horizontal => x as i32 + offset,
    };
    let value = ((index * 37).rem_euclid(251)) as u8;
    Rgba([value, value.wrapping_mul(3), value.wrapping_add(90), 255])
  })
}

#[test]
fn identical_viewports_report_no_motion_at_zero_shift() {
  let image = striped(40, 60, 0, ScrollAxis::Vertical);
  let motion = compare_viewport_pixels(&image, &image, ScrollAxis::Vertical, ViewportPixelPolicy::default());
  assert_eq!(motion.estimated_shift, 0);
  assert!(motion.no_motion);
}

#[test]
fn shifted_viewports_report_motion_and_the_shift_on_either_axis() {
  for axis in [ScrollAxis::Vertical, ScrollAxis::Horizontal] {
    let before = striped(60, 60, 0, axis);
    let after = striped(60, 60, 10, axis);
    let motion = compare_viewport_pixels(&before, &after, axis, ViewportPixelPolicy::default());
    assert!(!motion.no_motion, "{axis:?}");
    assert_eq!(motion.estimated_shift.abs(), 10, "{axis:?}");
    assert!(motion.normalized_diff < 1e-9, "{axis:?}");
  }
  // A shift beyond the search window is still motion.
  let far = compare_viewport_pixels(
    &striped(40, 200, 0, ScrollAxis::Vertical),
    &striped(40, 200, 120, ScrollAxis::Vertical),
    ScrollAxis::Vertical,
    ViewportPixelPolicy::default(),
  );
  assert!(!far.no_motion);
}

#[test]
fn resized_or_empty_viewports_never_look_stuck() {
  let motion = compare_viewport_pixels(
    &striped(40, 60, 0, ScrollAxis::Vertical),
    &striped(40, 61, 0, ScrollAxis::Vertical),
    ScrollAxis::Vertical,
    ViewportPixelPolicy::default(),
  );
  assert!(!motion.no_motion);
  assert!(
    !compare_viewport_pixels(&RgbaImage::new(0, 0), &RgbaImage::new(0, 0), ScrollAxis::Vertical, ViewportPixelPolicy::default()).no_motion
  );
}

#[test]
fn crop_ratio_selects_the_normalized_region() {
  let image = striped(100, 50, 0, ScrollAxis::Vertical);
  let cropped = crop_ratio(&image, Some(auv_driver::NormalizedRect::new(0.5, 0.2, 0.5, 0.4)));
  assert_eq!(cropped.dimensions(), (50, 20));
  assert_eq!(cropped.get_pixel(0, 0), image.get_pixel(50, 10));
  assert_eq!(crop_ratio(&image, None).dimensions(), (100, 50));
}
