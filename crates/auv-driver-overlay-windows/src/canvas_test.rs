use auv_driver_overlay_common::style::{Color, Insets};
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
fn rounded_rect_corners_are_antialiased_with_partial_alpha_edges() {
  let canvas = canvas(48, 48);
  let (width, _) = canvas.size();
  canvas.fill_rounded_rect(rect(8.0, 8.0, 40.0, 40.0), 12.0, Color::rgb(1.0, 0.0, 0.0)).unwrap();
  let pixels = canvas.into_pixels().unwrap();

  assert_eq!(pixel(&pixels, width, 24, 24), [0, 0, 255, 255], "opaque red interior");
  assert_eq!(alpha(&pixels, width, 0, 0), 0, "untouched corner stays transparent");
  let corner = (8..20).flat_map(|y| (8..20).map(move |x| (x, y))).filter(|&(x, y)| {
    let a = alpha(&pixels, width, x, y);
    a > 0 && a < 255
  });
  assert!(corner.count() >= 2, "the rounded corner must blend with partial alpha");
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
