use super::*;
use image::{GrayImage, Luma};
use std::path::PathBuf;

fn test_png(name: &str) -> PathBuf {
  std::env::temp_dir().join(format!("auv_tmatch_test_{name}.png"))
}

fn write_gray_png(path: &Path, width: u32, height: u32, fill: u8) {
  let img = GrayImage::from_pixel(width, height, Luma([fill]));
  img.save(path).expect("should save png");
}

// Writes a non-uniform pattern (checkerboard of `lo` and `hi`) into a region.
fn write_pattern_png(path: &Path, width: u32, height: u32, bg: u8, px: u32, py: u32, pw: u32, ph: u32, lo: u8, hi: u8) {
  let mut img = GrayImage::from_pixel(width, height, Luma([bg]));
  for dy in 0..ph {
    for dx in 0..pw {
      let val = if (dx + dy) % 2 == 0 { hi } else { lo };
      img.put_pixel(px + dx, py + dy, Luma([val]));
    }
  }
  img.save(path).expect("should save png");
}

fn read_rgba(path: &Path) -> image::RgbaImage {
  image::open(path).expect("read test screenshot").into_rgba8()
}

#[test]
fn match_template_accepts_in_memory_screenshot() {
  let template_path = test_png("memory_tmpl");
  let mut screenshot = image::RgbaImage::from_pixel(80, 60, image::Rgba([50, 50, 50, 255]));
  for dy in 0..12 {
    for dx in 0..12 {
      let value = if (dx + dy) % 2 == 0 { 220 } else { 50 };
      screenshot.put_pixel(24 + dx, 18 + dy, image::Rgba([value, value, value, 255]));
    }
  }
  write_pattern_png(&template_path, 12, 12, 50, 0, 0, 12, 12, 50, 220);

  let output = match_template(&screenshot, &template_path, None, 0.9).expect("template match should use in-memory screenshot");

  assert_eq!((output.matches[0].x, output.matches[0].y), (24, 18));
}

#[test]
fn match_template_finds_exact_patch() {
  let screenshot_path = test_png("exact_ss");
  let template_path = test_png("exact_tmpl");

  // Non-uniform checkerboard pattern so NCC is well-defined
  write_pattern_png(&screenshot_path, 200, 150, 50, 80, 60, 20, 20, 50, 220);
  write_pattern_png(&template_path, 20, 20, 50, 0, 0, 20, 20, 50, 220);

  let output = match_template(&read_rgba(&screenshot_path), &template_path, None, 0.9).expect("template match should succeed");

  assert!(!output.matches.is_empty(), "should find at least one match");
  let best = &output.matches[0];
  assert_eq!(best.x, 80, "match x should align with patch");
  assert_eq!(best.y, 60, "match y should align with patch");
  assert!(best.score > 0.95, "score should be near 1.0 for exact match: {}", best.score);
}

#[test]
fn match_template_returns_empty_for_uniform_template() {
  let screenshot_path = test_png("uniform_ss");
  let template_path = test_png("uniform_tmpl");

  write_gray_png(&screenshot_path, 100, 100, 128);
  write_gray_png(&template_path, 10, 10, 200);

  let output = match_template(&read_rgba(&screenshot_path), &template_path, None, 0.5).expect("should handle uniform template");
  assert!(output.matches.is_empty(), "uniform template returns no matches");
}

#[test]
fn match_template_respects_search_region() {
  let screenshot_path = test_png("region_ss");
  let template_path = test_png("region_tmpl");

  // Pattern at (10,10) — outside the restricted search region (80,60,100,80)
  write_pattern_png(&screenshot_path, 200, 150, 50, 10, 10, 20, 20, 50, 220);
  write_pattern_png(&template_path, 20, 20, 50, 0, 0, 20, 20, 50, 220);

  let region = ObservedRect {
    x: 80,
    y: 60,
    width: 100,
    height: 80,
  };
  let output = match_template(&read_rgba(&screenshot_path), &template_path, Some(&region), 0.9).expect("should match with region");
  assert!(
    output.matches.is_empty(),
    "patch outside region should not be found: {:?}",
    output.matches.iter().map(|m| (m.x, m.y)).collect::<Vec<_>>()
  );
}

#[test]
fn match_template_errors_on_oversized_search() {
  let screenshot_path = test_png("oversize_ss");
  let template_path = test_png("oversize_tmpl");

  write_pattern_png(&screenshot_path, 3024, 1964, 50, 100, 100, 128, 128, 50, 220);
  write_pattern_png(&template_path, 128, 128, 50, 0, 0, 128, 128, 50, 220);

  let result = match_template(&read_rgba(&screenshot_path), &template_path, None, 0.9);
  assert!(result.is_err(), "should reject oversized search without region");
  assert!(result.unwrap_err().contains("too large"), "error should mention size");
}
