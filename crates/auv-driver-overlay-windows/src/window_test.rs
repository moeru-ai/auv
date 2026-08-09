use super::{SENTINEL_BGRA, key_sentinel_to_alpha};

#[test]
fn key_sentinel_to_alpha_clears_untouched_pixels_and_opaques_drawn_ones() {
  let mut pixels = vec![0u8; 4 * 3];
  pixels[0..4].copy_from_slice(&SENTINEL_BGRA);
  pixels[4..8].copy_from_slice(&[10, 20, 30, 0]);
  pixels[8..12].copy_from_slice(&SENTINEL_BGRA);

  key_sentinel_to_alpha(&mut pixels);

  assert_eq!(&pixels[0..4], &[0, 0, 0, 0]);
  assert_eq!(&pixels[4..8], &[10, 20, 30, 255]);
  assert_eq!(&pixels[8..12], &[0, 0, 0, 0]);
}

#[test]
fn key_sentinel_to_alpha_is_a_no_op_on_an_empty_buffer() {
  let mut pixels: Vec<u8> = Vec::new();
  key_sentinel_to_alpha(&mut pixels);
  assert!(pixels.is_empty());
}
