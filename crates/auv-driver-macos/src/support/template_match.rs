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

fn compute_search_window(search_region: Option<&ObservedRect>, img_w: u32, img_h: u32) -> (u32, u32, u32, u32) {
  if let Some(region) = search_region {
    let x = region.x.max(0) as u32;
    let y = region.y.max(0) as u32;
    let max_x = ((region.x + region.width) as u32).min(img_w);
    let max_y = ((region.y + region.height) as u32).min(img_h);
    let width = max_x.saturating_sub(x);
    let height = max_y.saturating_sub(y);
    (x, y, width, height)
  } else {
    (0, 0, img_w, img_h)
  }
}

/// Normalized cross-correlation template matching on grayscale images.
///
/// Required workflow:
/// 1. Capture the target surface first and pass its in-memory RGBA image here.
/// 2. Pass a small template cropped at the same pixel scale as the screenshot.
/// 3. Restrict `search_region` whenever possible; this implementation is a
///    straightforward sliding-window matcher and intentionally favors clear
///    traceability over speed.
/// 4. Treat returned boxes as recognition evidence. Higher-level code decides
///    whether and how to click, verify, or fall back.
///
/// Matching behavior:
/// - Both screenshot and template are converted to grayscale with `to_luma8`.
/// - Each candidate patch is mean-centered before scoring, so uniform brightness
///   shifts are partly tolerated.
/// - Template background still participates in the score. For theme-independent
///   icons, a future masked/edge matcher should ignore non-icon pixels.
/// - There is no scale or rotation invariance. The template must match the
///   screenshot resolution.
///
/// Returns at most MAX_RESULTS matches above `threshold` after non-maximum suppression.
pub fn match_template(
  screenshot: &image::RgbaImage,
  template_path: &Path,
  search_region: Option<&ObservedRect>,
  threshold: f64,
) -> AuvResult<TemplateMatchOutput> {
  let screenshot = image::imageops::grayscale(screenshot);
  let template = image::open(template_path).map_err(|e| format!("failed to open template {}: {e}", template_path.display()))?.to_luma8();

  let (img_w, img_h) = screenshot.dimensions();
  let (tw, th) = template.dimensions();

  let (sx, sy, sw, sh) = compute_search_window(search_region, img_w, img_h);

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
#[path = "template_match_test.rs"]
mod tests;
