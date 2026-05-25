// File: src/driver/macos/support/template_match.rs
use std::path::Path;

use crate::types::{AuvResult, ObservedRect};

#[derive(Debug)]
pub struct TemplateMatchItem {
  pub x: i64,
  pub y: i64,
  pub width: i64,
  pub height: i64,
  pub score: f64,
}

#[derive(Debug)]
pub struct TemplateMatchOutput {
  pub matches: Vec<TemplateMatchItem>,
  pub template_width: u32,
  pub template_height: u32,
  pub search_x: i64,
  pub search_y: i64,
  pub search_width: u32,
  pub search_height: u32,
}

const MAX_SEARCH_PIXELS: u64 = 10_000_000;
const MAX_RESULTS: usize = 16;

/// Normalized cross-correlation template matching on grayscale images.
/// Returns at most MAX_RESULTS matches above `threshold` after non-maximum suppression.
pub fn match_template(
  screenshot_path: &Path,
  template_path: &Path,
  search_region: Option<&ObservedRect>,
  threshold: f64,
) -> AuvResult<TemplateMatchOutput> {
  let screenshot = image::open(screenshot_path)
    .map_err(|e| {
      format!(
        "failed to open screenshot {}: {e}",
        screenshot_path.display()
      )
    })?
    .to_luma8();
  let template = image::open(template_path)
    .map_err(|e| format!("failed to open template {}: {e}", template_path.display()))?
    .to_luma8();

  let (img_w, img_h) = screenshot.dimensions();
  let (tw, th) = template.dimensions();

  let (sx, sy, sw, sh) = if let Some(r) = search_region {
    let x = r.x.max(0) as u32;
    let y = r.y.max(0) as u32;
    let max_x = ((r.x + r.width) as u32).min(img_w);
    let max_y = ((r.y + r.height) as u32).min(img_h);
    let w = max_x.saturating_sub(x);
    let h = max_y.saturating_sub(y);
    (x, y, w, h)
  } else {
    (0, 0, img_w, img_h)
  };

  let search_pixels = sw as u64 * sh as u64;
  let template_pixels = tw as u64 * th as u64;
  if search_pixels * template_pixels > MAX_SEARCH_PIXELS * tw.max(th) as u64 {
    return Err(format!(
      "search region {}x{} with template {}x{} is too large ({}M pixel-ops); \
       provide --region to restrict the search area",
      sw,
      sh,
      tw,
      th,
      search_pixels * template_pixels / 1_000_000
    ));
  }

  if tw > sw || th > sh {
    return Ok(TemplateMatchOutput {
      matches: vec![],
      template_width: tw,
      template_height: th,
      search_x: sx as i64,
      search_y: sy as i64,
      search_width: sw,
      search_height: sh,
    });
  }

  let t_pixels: Vec<f32> = template.pixels().map(|p| p[0] as f32).collect();
  let n = (tw * th) as f32;
  let t_mean = t_pixels.iter().sum::<f32>() / n;
  let t_centered: Vec<f32> = t_pixels.iter().map(|&p| p - t_mean).collect();
  let t_norm = {
    let sq: f32 = t_centered.iter().map(|&p| p * p).sum();
    sq.sqrt()
  };

  if t_norm < 1e-6 {
    return Ok(TemplateMatchOutput {
      matches: vec![],
      template_width: tw,
      template_height: th,
      search_x: sx as i64,
      search_y: sy as i64,
      search_width: sw,
      search_height: sh,
    });
  }

  let mut candidates: Vec<(f64, u32, u32)> = Vec::new();

  for dy in 0..=(sh - th) {
    for dx in 0..=(sw - tw) {
      let px = sx + dx;
      let py = sy + dy;

      let mut patch_sum: f32 = 0.0;
      for ti in 0..th {
        for tj in 0..tw {
          patch_sum += screenshot.get_pixel(px + tj, py + ti)[0] as f32;
        }
      }
      let patch_mean = patch_sum / n;

      let mut num: f32 = 0.0;
      let mut patch_norm_sq: f32 = 0.0;
      for ti in 0..th {
        for tj in 0..tw {
          let t_val = t_centered[(ti * tw + tj) as usize];
          let p_val = screenshot.get_pixel(px + tj, py + ti)[0] as f32 - patch_mean;
          num += t_val * p_val;
          patch_norm_sq += p_val * p_val;
        }
      }

      let patch_norm = patch_norm_sq.sqrt();
      let denom = t_norm * patch_norm;
      let ncc = if denom < 1e-6 {
        0.0
      } else {
        (num / denom) as f64
      };

      if ncc >= threshold {
        candidates.push((ncc, px, py));
      }
    }
  }

  candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

  // Non-maximum suppression: skip if within half-template of a higher-score match.
  let half_w = (tw as i64) / 2;
  let half_h = (th as i64) / 2;
  let mut selected: Vec<TemplateMatchItem> = Vec::new();

  'outer: for &(score, x, y) in candidates.iter().take(MAX_RESULTS * 8) {
    let xi = x as i64;
    let yi = y as i64;
    for existing in &selected {
      if (existing.x - xi).abs() < half_w && (existing.y - yi).abs() < half_h {
        continue 'outer;
      }
    }
    selected.push(TemplateMatchItem {
      x: xi,
      y: yi,
      width: tw as i64,
      height: th as i64,
      score,
    });
    if selected.len() >= MAX_RESULTS {
      break;
    }
  }

  Ok(TemplateMatchOutput {
    matches: selected,
    template_width: tw,
    template_height: th,
    search_x: sx as i64,
    search_y: sy as i64,
    search_width: sw,
    search_height: sh,
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use image::{GrayImage, Luma};
  use std::path::PathBuf;

  fn test_png(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("auv_tmatch_test_{name}.png"))
  }

  fn write_gray_png(path: &std::path::Path, width: u32, height: u32, fill: u8) {
    let img = GrayImage::from_pixel(width, height, Luma([fill]));
    img.save(path).expect("should save png");
  }

  // Writes a non-uniform pattern (checkerboard of `lo` and `hi`) into a region.
  fn write_pattern_png(
    path: &std::path::Path,
    width: u32,
    height: u32,
    bg: u8,
    px: u32,
    py: u32,
    pw: u32,
    ph: u32,
    lo: u8,
    hi: u8,
  ) {
    let mut img = GrayImage::from_pixel(width, height, Luma([bg]));
    for dy in 0..ph {
      for dx in 0..pw {
        let val = if (dx + dy) % 2 == 0 { hi } else { lo };
        img.put_pixel(px + dx, py + dy, Luma([val]));
      }
    }
    img.save(path).expect("should save png");
  }

  #[test]
  fn match_template_finds_exact_patch() {
    let screenshot_path = test_png("exact_ss");
    let template_path = test_png("exact_tmpl");

    // Non-uniform checkerboard pattern so NCC is well-defined
    write_pattern_png(&screenshot_path, 200, 150, 50, 80, 60, 20, 20, 50, 220);
    write_pattern_png(&template_path, 20, 20, 50, 0, 0, 20, 20, 50, 220);

    let output = match_template(&screenshot_path, &template_path, None, 0.9)
      .expect("template match should succeed");

    assert!(!output.matches.is_empty(), "should find at least one match");
    let best = &output.matches[0];
    assert_eq!(best.x, 80, "match x should align with patch");
    assert_eq!(best.y, 60, "match y should align with patch");
    assert!(
      best.score > 0.95,
      "score should be near 1.0 for exact match: {}",
      best.score
    );
  }

  #[test]
  fn match_template_returns_empty_for_uniform_template() {
    let screenshot_path = test_png("uniform_ss");
    let template_path = test_png("uniform_tmpl");

    write_gray_png(&screenshot_path, 100, 100, 128);
    write_gray_png(&template_path, 10, 10, 200);

    let output = match_template(&screenshot_path, &template_path, None, 0.5)
      .expect("should handle uniform template");
    assert!(
      output.matches.is_empty(),
      "uniform template returns no matches"
    );
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
    let output = match_template(&screenshot_path, &template_path, Some(&region), 0.9)
      .expect("should match with region");
    assert!(
      output.matches.is_empty(),
      "patch outside region should not be found: {:?}",
      output
        .matches
        .iter()
        .map(|m| (m.x, m.y))
        .collect::<Vec<_>>()
    );
  }

  #[test]
  fn match_template_errors_on_oversized_search() {
    let screenshot_path = test_png("oversize_ss");
    let template_path = test_png("oversize_tmpl");

    write_pattern_png(
      &screenshot_path,
      3024,
      1964,
      50,
      100,
      100,
      128,
      128,
      50,
      220,
    );
    write_pattern_png(&template_path, 128, 128, 50, 0, 0, 128, 128, 50, 220);

    let result = match_template(&screenshot_path, &template_path, None, 0.9);
    assert!(
      result.is_err(),
      "should reject oversized search without region"
    );
    assert!(
      result.unwrap_err().contains("too large"),
      "error should mention size"
    );
  }
}
