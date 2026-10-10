use auv_driver_overlay_common::style::{Color, Insets, Shadow};
use windows::Win32::Foundation::RECT;

use super::{Canvas, LabelPill, point, rect};

fn canvas(width: i32, height: i32) -> Canvas {
  Canvas::new(RECT {
    left: 0,
    top: 0,
    right: width,
    bottom: height,
  })
  .expect("Direct2D canvas")
}

/// Premultiplied BGRA pixel at `(x, y)`.
fn pixel(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
  let index = (y * width + x) * 4;
  [
    pixels[index],
    pixels[index + 1],
    pixels[index + 2],
    pixels[index + 3],
  ]
}

fn alpha(pixels: &[u8], width: usize, x: usize, y: usize) -> u8 {
  pixel(pixels, width, x, y)[3]
}

#[test]
fn a_new_canvas_is_fully_transparent() {
  let pixels = canvas(16, 16).into_pixels().unwrap();
  assert!(pixels.iter().all(|&byte| byte == 0));
}

#[test]
fn circles_are_antialiased_with_partial_alpha_edges() {
  let canvas = canvas(48, 48);
  let (width, _) = canvas.size();
  canvas.fill_circle(point(24.0, 24.0), 12.0, Color::rgb(1.0, 0.0, 0.0)).unwrap();
  let pixels = canvas.into_pixels().unwrap();

  assert_eq!(pixel(&pixels, width, 24, 24), [0, 0, 255, 255], "opaque red interior");
  assert_eq!(alpha(&pixels, width, 0, 0), 0, "untouched corner stays transparent");
  let edge = (0..48).map(|x| alpha(&pixels, width, x, 24)).filter(|&a| a > 0 && a < 255).count();
  assert!(edge >= 2, "the disc's left and right edges must blend with partial alpha");
}

#[test]
fn translucent_fills_are_stored_as_premultiplied_alpha() {
  let canvas = canvas(32, 32);
  let (width, _) = canvas.size();
  canvas.fill_rounded_rect(rect(4.0, 4.0, 28.0, 28.0), 4.0, Color::rgba(1.0, 0.0, 0.0, 0.5)).unwrap();
  let pixels = canvas.into_pixels().unwrap();

  let [blue, green, red, a] = pixel(&pixels, width, 16, 16);
  assert!((126..=129).contains(&a), "half alpha, got {a}");
  assert_eq!(red, a, "a premultiplied full-red channel equals alpha");
  assert_eq!((blue, green), (0, 0));
}

#[test]
fn glow_peaks_at_the_shadow_alpha_and_fades_past_the_silhouette() {
  let canvas = canvas(80, 80);
  let (width, _) = canvas.size();
  let shadow = Shadow {
    offset_x: 0.0,
    offset_y: 0.0,
    ..Shadow::auv()
  };
  canvas.draw_glow(point(40.0, 40.0), 12.0, &shadow).unwrap();
  let pixels = canvas.into_pixels().unwrap();

  let ray = (0..40).map(|distance| alpha(&pixels, width, 40 + distance, 40)).collect::<Vec<_>>();
  let peak = (Shadow::auv().color.alpha * 255.0).round() as u8;
  assert!(ray[0].abs_diff(peak) <= 3, "center alpha {} should be the shadow alpha {peak}", ray[0]);
  assert!(ray.windows(2).all(|pair| pair[1] <= pair[0] + 1), "alpha must not grow away from the center: {ray:?}");
  assert!(ray[12] > 0 && ray[12] < ray[0], "the silhouette edge is half-covered: {ray:?}");
  assert!(ray[16] > 0, "the glow reaches past the silhouette: {ray:?}");
  assert_eq!(ray[39], 0, "the glow ends within three blur sigmas: {ray:?}");
}

#[test]
fn a_transparent_shadow_draws_nothing() {
  let canvas = canvas(32, 32);
  let shadow = Shadow {
    color: Color::CLEAR,
    ..Shadow::auv()
  };
  canvas.draw_glow(point(16.0, 16.0), 8.0, &shadow).unwrap();
  assert!(canvas.into_pixels().unwrap().iter().all(|&byte| byte == 0));
}

#[test]
fn shadow_offset_moves_the_glow() {
  let canvas = canvas(80, 80);
  let (width, _) = canvas.size();
  let shadow = Shadow {
    offset_x: 0.0,
    offset_y: 10.0,
    ..Shadow::auv()
  };
  canvas.draw_glow(point(40.0, 30.0), 8.0, &shadow).unwrap();
  let pixels = canvas.into_pixels().unwrap();
  assert!(alpha(&pixels, width, 40, 40) > alpha(&pixels, width, 40, 30), "the brightest point follows the offset");
}

/// Columns covered by any non-transparent pixel.
fn covered_width(pixels: &[u8], width: usize, height: usize) -> usize {
  (0..width).filter(|&x| (0..height).any(|y| alpha(pixels, width, x, y) > 0)).count()
}

fn pill(text: &str) -> LabelPill<'_> {
  LabelPill {
    text,
    foreground: Color::WHITE,
    background: Color::rgb(0.0, 0.0, 0.0),
    padding: Insets::symmetric(3.0, 8.0),
    corner_radius: 999.0,
    font_size: 11.0,
  }
}

#[test]
fn label_pills_are_sized_from_measured_text() {
  let short = canvas(400, 40);
  let (width, height) = short.size();
  short.draw_label_pill(point(10.0, 20.0), &pill("ab")).unwrap();
  let short_width = covered_width(&short.into_pixels().unwrap(), width, height);

  let long = canvas(400, 40);
  long.draw_label_pill(point(10.0, 20.0), &pill("abcdefghij")).unwrap();
  let long_pixels = long.into_pixels().unwrap();
  let long_width = covered_width(&long_pixels, width, height);

  // Consolas is monospaced: eight more glyphs widen the pill by eight advances.
  assert!(long_width > short_width + 8 * 5, "short {short_width}px vs long {long_width}px");
  let text_pixels = long_pixels.as_chunks::<4>().0.iter().filter(|px| px[3] == 255 && px[0] > 128).count();
  assert!(text_pixels > 0, "white glyphs must be drawn over the black pill");
}
