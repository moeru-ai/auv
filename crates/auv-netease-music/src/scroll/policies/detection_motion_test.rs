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
