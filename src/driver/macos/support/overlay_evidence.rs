// File: src/driver/macos/support/overlay_evidence.rs
//! Overlay evidence rendering.
//!
//! Builds "what happened" PNG + JSON annotation artifacts by compositing
//! cursors, target boxes (OCR/row/AX), and decision panels on top of captured
//! screenshots.
//!
//! Boundary: this module renders evidence only; capture lives in `capture::*`
//! and control logic lives in `control::*`.

use std::fs;

use image::{Rgba, RgbaImage};
use serde::Serialize;

use super::super::*;
use crate::driver::macos::capture::types::CaptureContract;
use crate::driver::macos::capture::xcap_backend::save_rgba_image;
use auv_driver_macos::native::pointer::current_mouse_logical_point;

const AUV_OUTLINE: [u8; 4] = [21, 23, 26, 255];
const AUV_BODY: [u8; 4] = [0, 196, 210, 255];
const AUV_HIGHLIGHT: [u8; 4] = [0, 224, 224, 255];
const AUV_CLICK_HIGHLIGHT: [u8; 4] = [207, 244, 247, 255];
const AUV_ACCENT: [u8; 4] = [127, 208, 48, 255];
const AUV_SPARK: [u8; 4] = [160, 224, 32, 255];
const YOU_OUTLINE: [u8; 4] = [14, 16, 19, 255];
const YOU_BODY: [u8; 4] = [90, 98, 112, 255];
const YOU_HIGHLIGHT: [u8; 4] = [154, 163, 178, 255];
const OCR_BOX: [u8; 4] = [255, 196, 0, 255];
const TARGET_BOX: [u8; 4] = [0, 155, 166, 255];
const ROW_BOX: [u8; 4] = [127, 208, 48, 255];
const AX_BOX: [u8; 4] = [105, 168, 255, 255];
const LABEL_BG: [u8; 4] = [0, 155, 166, 238];
const LABEL_YOU_BG: [u8; 4] = [42, 58, 82, 238];
const LABEL_FG: [u8; 4] = [255, 255, 255, 255];
const PANEL_BG: [u8; 4] = [15, 18, 21, 210];

const CURSOR_SPRITE_PX: i64 = 24;
const CURSOR_OFFSET_X: i64 = 4;
const CURSOR_OFFSET_Y: i64 = 4;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct OverlayEvidencePoint {
  pub(crate) logical_x: f64,
  pub(crate) logical_y: f64,
  pub(crate) screenshot_x: f64,
  pub(crate) screenshot_y: f64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct OverlayEvidenceCursor {
  pub(crate) cursor_id: String,
  pub(crate) label: String,
  pub(crate) variant: String,
  pub(crate) point: OverlayEvidencePoint,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct OverlayEvidenceMatch {
  pub(crate) text: String,
  pub(crate) confidence: f64,
  pub(crate) bounds: ObservedRect,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct OverlayEvidenceRow {
  pub(crate) row_index: usize,
  pub(crate) source: String,
  pub(crate) bounds: ObservedRect,
  pub(crate) text_fragments: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct OverlayEvidenceAxTarget {
  pub(crate) role: String,
  pub(crate) label: String,
  pub(crate) bounds: ObservedRect,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct OverlayEvidenceDecision {
  pub(crate) operation: String,
  pub(crate) primary_strategy: String,
  pub(crate) selected_strategy: String,
  pub(crate) fallback_used: bool,
  pub(crate) primary_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct OverlayEvidencePayload {
  pub(crate) version: String,
  pub(crate) kind: String,
  pub(crate) screenshot_path: String,
  pub(crate) screenshot_width: i64,
  pub(crate) screenshot_height: i64,
  pub(crate) query: Option<String>,
  pub(crate) strategy: Option<String>,
  pub(crate) fallback_used: Option<bool>,
  pub(crate) cursor_disturbance: Option<String>,
  pub(crate) press_mechanism: Option<String>,
  pub(crate) overlay_presentation: Option<String>,
  pub(crate) action_point: Option<OverlayEvidencePoint>,
  pub(crate) expected_target: Option<OverlayEvidencePoint>,
  pub(crate) ocr_match: Option<OverlayEvidenceMatch>,
  pub(crate) row: Option<OverlayEvidenceRow>,
  pub(crate) ax_target: Option<OverlayEvidenceAxTarget>,
  pub(crate) decision: Option<OverlayEvidenceDecision>,
  pub(crate) user_cursor: Option<OverlayEvidenceCursor>,
  pub(crate) auv_cursor: Option<OverlayEvidenceCursor>,
}

#[derive(Clone, Debug)]
pub(crate) struct RowObservationOverlayRequest {
  pub(crate) label: String,
  pub(crate) screenshot_path: std::path::PathBuf,
  pub(crate) screenshot_dimensions: ScreenshotDimensions,
  pub(crate) strategy: String,
  pub(crate) rows: Vec<OverlayEvidenceRow>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RowObservationOverlayPayload {
  pub(crate) version: String,
  pub(crate) kind: String,
  pub(crate) screenshot_path: String,
  pub(crate) screenshot_width: i64,
  pub(crate) screenshot_height: i64,
  pub(crate) strategy: String,
  pub(crate) rows: Vec<OverlayEvidenceRow>,
}

#[derive(Clone, Debug)]
pub(crate) struct OverlayEvidenceRequest {
  pub(crate) kind: &'static str,
  pub(crate) label: String,
  pub(crate) screenshot_path: std::path::PathBuf,
  pub(crate) screenshot_dimensions: ScreenshotDimensions,
  pub(crate) capture_contract: CaptureContract,
  pub(crate) query: Option<String>,
  pub(crate) strategy: Option<String>,
  pub(crate) fallback_used: Option<bool>,
  pub(crate) cursor_disturbance: Option<String>,
  pub(crate) press_mechanism: Option<String>,
  pub(crate) overlay_presentation: Option<String>,
  pub(crate) action_point: OverlayEvidencePoint,
  pub(crate) expected_target: Option<OverlayEvidencePoint>,
  pub(crate) ocr_match: Option<OverlayEvidenceMatch>,
  pub(crate) row: Option<OverlayEvidenceRow>,
  pub(crate) ax_target: Option<OverlayEvidenceAxTarget>,
  pub(crate) decision: Option<OverlayEvidenceDecision>,
  pub(crate) include_user_cursor: bool,
  pub(crate) auv_cursor_variant: &'static str,
}

pub(crate) fn capture_pixel_to_logical(
  contract: &CaptureContract,
  screenshot_x: f64,
  screenshot_y: f64,
) -> OverlayEvidencePoint {
  OverlayEvidencePoint {
    logical_x: contract.source_global_logical_bounds.x
      + screenshot_x * contract.pixel_to_logical_scale.x,
    logical_y: contract.source_global_logical_bounds.y
      + screenshot_y * contract.pixel_to_logical_scale.y,
    screenshot_x,
    screenshot_y,
  }
}

pub(crate) fn logical_to_capture_pixel(
  contract: &CaptureContract,
  logical_x: f64,
  logical_y: f64,
) -> OverlayEvidencePoint {
  let screenshot_x =
    (logical_x - contract.source_global_logical_bounds.x) * contract.logical_to_pixel_scale.x;
  let screenshot_y =
    (logical_y - contract.source_global_logical_bounds.y) * contract.logical_to_pixel_scale.y;
  OverlayEvidencePoint {
    logical_x,
    logical_y,
    screenshot_x,
    screenshot_y,
  }
}

pub(crate) fn build_overlay_evidence_artifacts(
  request: OverlayEvidenceRequest,
) -> AuvResult<Vec<ProducedArtifact>> {
  let bytes = fs::read(&request.screenshot_path).map_err(|error| {
    format!(
      "failed to read screenshot source {} for overlay evidence: {error}",
      request.screenshot_path.display()
    )
  })?;
  let mut image = image::load_from_memory(&bytes)
    .map_err(|error| format!("failed to decode screenshot PNG for overlay evidence: {error}"))?
    .to_rgba8();

  if let Some(matched) = request.ocr_match.as_ref() {
    draw_rect_outline(&mut image, &matched.bounds, OCR_BOX, 2);
  }
  if let Some(row) = request.row.as_ref() {
    draw_rect_outline(&mut image, &row.bounds, ROW_BOX, 2);
  }
  if let Some(ax_target) = request.ax_target.as_ref() {
    draw_rect_outline(&mut image, &ax_target.bounds, AX_BOX, 2);
  }
  if let Some(target) = request.expected_target.as_ref() {
    draw_target_marker(
      &mut image,
      target.screenshot_x,
      target.screenshot_y,
      TARGET_BOX,
    );
  }
  draw_target_marker(
    &mut image,
    request.action_point.screenshot_x,
    request.action_point.screenshot_y,
    AUV_ACCENT,
  );

  let auv_cursor = OverlayEvidenceCursor {
    cursor_id: "auv".to_string(),
    label: "auv".to_string(),
    variant: request.auv_cursor_variant.to_string(),
    point: request.action_point.clone(),
  };
  draw_cursor_with_label(
    &mut image,
    auv_cursor.point.screenshot_x,
    auv_cursor.point.screenshot_y,
    auv_cursor.variant.as_str(),
    auv_cursor.label.as_str(),
  );

  let user_cursor = if request.include_user_cursor {
    current_mouse_logical_point()
      .ok()
      .map(|(x, y)| logical_to_capture_pixel(&request.capture_contract, x, y))
      .filter(|point| point_is_visible(point, &request.screenshot_dimensions))
      .map(|point| OverlayEvidenceCursor {
        cursor_id: "you".to_string(),
        label: "you".to_string(),
        variant: "you".to_string(),
        point,
      })
  } else {
    None
  };
  if let Some(cursor) = user_cursor.as_ref() {
    draw_cursor_with_label(
      &mut image,
      cursor.point.screenshot_x,
      cursor.point.screenshot_y,
      "you",
      cursor.label.as_str(),
    );
  }

  draw_signal_panel(
    &mut image,
    build_signal_lines(
      request.query.as_deref(),
      request.strategy.as_deref(),
      request.fallback_used,
      request.cursor_disturbance.as_deref(),
      request.press_mechanism.as_deref(),
      request.overlay_presentation.as_deref(),
      request.ocr_match.as_ref(),
      request.row.as_ref(),
      request.ax_target.as_ref(),
      request.decision.as_ref(),
    ),
  );

  let json_payload = OverlayEvidencePayload {
    version: "v1alpha1".to_string(),
    kind: request.kind.to_string(),
    screenshot_path: request.screenshot_path.display().to_string(),
    screenshot_width: request.screenshot_dimensions.width,
    screenshot_height: request.screenshot_dimensions.height,
    query: request.query.clone(),
    strategy: request.strategy.clone(),
    fallback_used: request.fallback_used,
    cursor_disturbance: request.cursor_disturbance.clone(),
    press_mechanism: request.press_mechanism.clone(),
    overlay_presentation: request.overlay_presentation.clone(),
    action_point: Some(request.action_point.clone()),
    expected_target: request.expected_target.clone(),
    ocr_match: request.ocr_match.clone(),
    row: request.row.clone(),
    ax_target: request.ax_target.clone(),
    decision: request.decision.clone(),
    user_cursor,
    auv_cursor: Some(auv_cursor),
  };

  let overlay_png_path = temp_file_path(
    &format!("{}-click-overlay", sanitize_file_component(&request.label)),
    "png",
  );
  save_rgba_image(image, overlay_png_path.as_path())?;

  let overlay_json = serde_json::to_string_pretty(&json_payload)
    .map_err(|error| format!("failed to encode click overlay annotation JSON: {error}"))?
    + "\n";
  let overlay_json_artifact = build_text_artifact(
    "click.overlay.annotation",
    "json",
    &format!("{}-click-overlay", sanitize_file_component(&request.label)),
    overlay_json,
    "Structured annotation payload for the click overlay evidence image.",
  )?;

  Ok(vec![
    ProducedArtifact {
      kind: "click.overlay".to_string(),
      source_path: overlay_png_path,
      preferred_name: format!(
        "{}-click-overlay.png",
        sanitize_file_component(&request.label)
      ),
      note: Some(
        "Evidence screenshot with dual-cursor and interaction overlay annotations.".to_string(),
      ),
    },
    overlay_json_artifact,
  ])
}

pub(crate) fn build_row_observation_overlay_artifacts(
  request: RowObservationOverlayRequest,
) -> AuvResult<Vec<ProducedArtifact>> {
  let bytes = fs::read(&request.screenshot_path).map_err(|error| {
    format!(
      "failed to read screenshot source {} for row observation overlay: {error}",
      request.screenshot_path.display()
    )
  })?;
  let mut image = image::load_from_memory(&bytes)
    .map_err(|error| {
      format!("failed to decode screenshot PNG for row observation overlay: {error}")
    })?
    .to_rgba8();

  for row in &request.rows {
    draw_rect_outline(&mut image, &row.bounds, ROW_BOX, 2);
    draw_label_chip(
      &mut image,
      row.bounds.x,
      row.bounds.y.saturating_sub(18),
      &format!("#{} {}", row.row_index + 1, row.source),
      LABEL_BG,
    );
  }
  draw_signal_panel(
    &mut image,
    vec![
      "row observation".to_string(),
      format!("strategy: {}", request.strategy),
      format!("rows: {}", request.rows.len()),
    ],
  );

  let overlay_png_path = temp_file_path(
    &format!(
      "{}-row-observation-overlay",
      sanitize_file_component(&request.label)
    ),
    "png",
  );
  save_rgba_image(image, overlay_png_path.as_path())?;

  let payload = RowObservationOverlayPayload {
    version: "v1alpha1".to_string(),
    kind: "row_observation_overlay".to_string(),
    screenshot_path: request.screenshot_path.display().to_string(),
    screenshot_width: request.screenshot_dimensions.width,
    screenshot_height: request.screenshot_dimensions.height,
    strategy: request.strategy,
    rows: request.rows,
  };
  let overlay_json = serde_json::to_string_pretty(&payload)
    .map_err(|error| format!("failed to encode row observation overlay JSON: {error}"))?
    + "\n";
  let overlay_json_artifact = build_text_artifact(
    "row-observation.overlay.annotation",
    "json",
    &format!(
      "{}-row-observation-overlay",
      sanitize_file_component(&request.label)
    ),
    overlay_json,
    "Structured annotation payload for the row observation overlay image.",
  )?;

  Ok(vec![
    ProducedArtifact {
      kind: "row-observation.overlay".to_string(),
      source_path: overlay_png_path,
      preferred_name: format!(
        "{}-row-observation-overlay.png",
        sanitize_file_component(&request.label)
      ),
      note: Some("Evidence screenshot with observed row bounds overlay.".to_string()),
    },
    overlay_json_artifact,
  ])
}

fn point_is_visible(point: &OverlayEvidencePoint, dimensions: &ScreenshotDimensions) -> bool {
  point.screenshot_x >= 0.0
    && point.screenshot_y >= 0.0
    && point.screenshot_x < dimensions.width as f64
    && point.screenshot_y < dimensions.height as f64
}

fn draw_rect_outline(image: &mut RgbaImage, rect: &ObservedRect, color: [u8; 4], stroke: i64) {
  for offset in 0..stroke {
    let x0 = rect.x - offset;
    let y0 = rect.y - offset;
    let x1 = rect.x + rect.width - 1 + offset;
    let y1 = rect.y + rect.height - 1 + offset;
    draw_line(image, x0, y0, x1, y0, color);
    draw_line(image, x0, y1, x1, y1, color);
    draw_line(image, x0, y0, x0, y1, color);
    draw_line(image, x1, y0, x1, y1, color);
  }
}

fn draw_target_marker(image: &mut RgbaImage, x: f64, y: f64, color: [u8; 4]) {
  let cx = x.round() as i64;
  let cy = y.round() as i64;
  draw_line(image, cx - 8, cy, cx + 8, cy, color);
  draw_line(image, cx, cy - 8, cx, cy + 8, color);
  draw_line(image, cx - 5, cy - 5, cx + 5, cy + 5, color);
  draw_line(image, cx + 5, cy - 5, cx - 5, cy + 5, color);
}

fn draw_cursor_with_label(image: &mut RgbaImage, x: f64, y: f64, variant: &str, label: &str) {
  let origin_x = x.round() as i64 + CURSOR_OFFSET_X;
  let origin_y = y.round() as i64 + CURSOR_OFFSET_Y;
  for (px, py, color) in cursor_pixels(variant) {
    put_pixel(image, origin_x + px, origin_y + py, color);
  }
  let label_bg = if variant == "you" {
    LABEL_YOU_BG
  } else {
    LABEL_BG
  };
  draw_label_chip(
    image,
    origin_x + CURSOR_SPRITE_PX + 6,
    origin_y + 6,
    label,
    label_bg,
  );
}

fn cursor_pixels(variant: &str) -> Vec<(i64, i64, [u8; 4])> {
  let mut pixels = Vec::new();
  let cells = match variant {
    "you" => you_cells(),
    "auv-click" => auv_click_cells(),
    _ => auv_cells(),
  };
  for (x, y, w, h, color) in cells {
    for dx in 0..w {
      for dy in 0..h {
        pixels.push((x + dx, y + dy, color));
      }
    }
  }
  pixels
}

fn auv_cells() -> Vec<(i64, i64, i64, i64, [u8; 4])> {
  rect_cells(&[
    (0, 0, 2, 2, AUV_OUTLINE),
    (0, 2, 2, 2, AUV_OUTLINE),
    (0, 4, 2, 2, AUV_OUTLINE),
    (0, 6, 2, 2, AUV_OUTLINE),
    (0, 8, 2, 2, AUV_OUTLINE),
    (0, 10, 2, 2, AUV_OUTLINE),
    (0, 12, 2, 2, AUV_OUTLINE),
    (0, 14, 2, 2, AUV_OUTLINE),
    (0, 16, 2, 2, AUV_OUTLINE),
    (2, 2, 2, 2, AUV_OUTLINE),
    (2, 16, 2, 2, AUV_OUTLINE),
    (4, 4, 2, 2, AUV_OUTLINE),
    (4, 16, 2, 2, AUV_OUTLINE),
    (6, 6, 2, 2, AUV_OUTLINE),
    (6, 14, 2, 2, AUV_OUTLINE),
    (8, 8, 2, 2, AUV_OUTLINE),
    (8, 12, 2, 2, AUV_OUTLINE),
    (10, 10, 2, 2, AUV_OUTLINE),
    (10, 14, 2, 2, AUV_OUTLINE),
    (12, 10, 2, 2, AUV_OUTLINE),
    (12, 14, 2, 2, AUV_OUTLINE),
    (14, 14, 2, 2, AUV_OUTLINE),
    (14, 16, 2, 2, AUV_OUTLINE),
    (2, 4, 2, 12, AUV_BODY),
    (4, 6, 2, 10, AUV_BODY),
    (6, 8, 2, 6, AUV_BODY),
    (8, 10, 2, 2, AUV_BODY),
    (2, 4, 2, 2, AUV_HIGHLIGHT),
    (2, 6, 2, 2, AUV_HIGHLIGHT),
    (10, 12, 2, 2, AUV_ACCENT),
    (12, 12, 2, 2, AUV_ACCENT),
  ])
}

fn auv_click_cells() -> Vec<(i64, i64, i64, i64, [u8; 4])> {
  rect_cells(&[
    (6, -2, 2, 2, AUV_ACCENT),
    (14, 2, 2, 2, AUV_ACCENT),
    (-2, 6, 2, 2, AUV_ACCENT),
    (-4, 12, 2, 2, AUV_ACCENT),
    (16, 10, 2, 2, AUV_ACCENT),
    (6, 0, 2, 2, AUV_SPARK),
    (12, 0, 2, 2, AUV_SPARK),
    (14, 6, 2, 2, AUV_SPARK),
    (-2, 10, 2, 2, AUV_SPARK),
    (0, 0, 2, 2, AUV_OUTLINE),
    (0, 2, 2, 2, AUV_OUTLINE),
    (0, 4, 2, 2, AUV_OUTLINE),
    (0, 6, 2, 2, AUV_OUTLINE),
    (0, 8, 2, 2, AUV_OUTLINE),
    (0, 10, 2, 2, AUV_OUTLINE),
    (0, 12, 2, 2, AUV_OUTLINE),
    (0, 14, 2, 2, AUV_OUTLINE),
    (0, 16, 2, 2, AUV_OUTLINE),
    (2, 2, 2, 2, AUV_OUTLINE),
    (2, 16, 2, 2, AUV_OUTLINE),
    (4, 4, 2, 2, AUV_OUTLINE),
    (4, 16, 2, 2, AUV_OUTLINE),
    (6, 6, 2, 2, AUV_OUTLINE),
    (6, 14, 2, 2, AUV_OUTLINE),
    (8, 8, 2, 2, AUV_OUTLINE),
    (8, 12, 2, 2, AUV_OUTLINE),
    (10, 10, 2, 2, AUV_OUTLINE),
    (10, 14, 2, 2, AUV_OUTLINE),
    (12, 10, 2, 2, AUV_OUTLINE),
    (12, 14, 2, 2, AUV_OUTLINE),
    (14, 14, 2, 2, AUV_OUTLINE),
    (14, 16, 2, 2, AUV_OUTLINE),
    (2, 4, 2, 4, AUV_CLICK_HIGHLIGHT),
    (4, 6, 2, 4, AUV_CLICK_HIGHLIGHT),
    (2, 8, 2, 8, AUV_BODY),
    (4, 10, 2, 6, AUV_BODY),
    (6, 8, 2, 6, AUV_BODY),
    (8, 10, 2, 2, AUV_BODY),
    (10, 12, 2, 2, AUV_ACCENT),
    (12, 12, 2, 2, AUV_ACCENT),
  ])
}

fn you_cells() -> Vec<(i64, i64, i64, i64, [u8; 4])> {
  rect_cells(&[
    (0, 0, 2, 2, YOU_OUTLINE),
    (0, 2, 2, 2, YOU_OUTLINE),
    (0, 4, 2, 2, YOU_OUTLINE),
    (0, 6, 2, 2, YOU_OUTLINE),
    (0, 8, 2, 2, YOU_OUTLINE),
    (0, 10, 2, 2, YOU_OUTLINE),
    (0, 12, 2, 2, YOU_OUTLINE),
    (0, 14, 2, 2, YOU_OUTLINE),
    (0, 16, 2, 2, YOU_OUTLINE),
    (2, 2, 2, 2, YOU_OUTLINE),
    (2, 16, 2, 2, YOU_OUTLINE),
    (4, 4, 2, 2, YOU_OUTLINE),
    (4, 16, 2, 2, YOU_OUTLINE),
    (6, 6, 2, 2, YOU_OUTLINE),
    (6, 14, 2, 2, YOU_OUTLINE),
    (8, 8, 2, 2, YOU_OUTLINE),
    (8, 12, 2, 2, YOU_OUTLINE),
    (10, 10, 2, 2, YOU_OUTLINE),
    (10, 14, 2, 2, YOU_OUTLINE),
    (12, 10, 2, 2, YOU_OUTLINE),
    (12, 14, 2, 2, YOU_OUTLINE),
    (14, 14, 2, 2, YOU_OUTLINE),
    (14, 16, 2, 2, YOU_OUTLINE),
    (2, 4, 2, 12, YOU_BODY),
    (4, 6, 2, 10, YOU_BODY),
    (6, 8, 2, 6, YOU_BODY),
    (8, 10, 2, 2, YOU_BODY),
    (2, 4, 2, 2, YOU_HIGHLIGHT),
    (2, 6, 2, 2, YOU_HIGHLIGHT),
  ])
}

fn rect_cells(values: &[(i64, i64, i64, i64, [u8; 4])]) -> Vec<(i64, i64, i64, i64, [u8; 4])> {
  values.to_vec()
}

fn draw_signal_panel(image: &mut RgbaImage, lines: Vec<String>) {
  if lines.is_empty() {
    return;
  }
  let char_w = 6i64;
  let char_h = 8i64;
  let padding_x = 8i64;
  let padding_y = 6i64;
  let line_gap = 4i64;
  let max_chars = lines
    .iter()
    .map(|line| line.chars().count() as i64)
    .max()
    .unwrap_or(0);
  let panel_width = padding_x * 2 + max_chars * char_w;
  let panel_height =
    padding_y * 2 + (lines.len() as i64 * char_h) + ((lines.len() as i64 - 1).max(0) * line_gap);
  fill_rect(image, 12, 12, panel_width, panel_height, PANEL_BG);
  for (index, line) in lines.iter().enumerate() {
    draw_mono_text(
      image,
      20,
      18 + index as i64 * (char_h + line_gap),
      line,
      LABEL_FG,
    );
  }
}

fn build_signal_lines(
  query: Option<&str>,
  strategy: Option<&str>,
  fallback_used: Option<bool>,
  cursor_disturbance: Option<&str>,
  press_mechanism: Option<&str>,
  overlay_presentation: Option<&str>,
  ocr_match: Option<&OverlayEvidenceMatch>,
  row: Option<&OverlayEvidenceRow>,
  ax_target: Option<&OverlayEvidenceAxTarget>,
  decision: Option<&OverlayEvidenceDecision>,
) -> Vec<String> {
  let mut lines = Vec::new();
  if let Some(query) = query {
    lines.push(format!("query: {query}"));
  }
  if let Some(strategy) = strategy {
    lines.push(format!("strategy: {strategy}"));
  }
  if let Some(value) = fallback_used {
    lines.push(format!("fallback: {value}"));
  }
  if let Some(value) = cursor_disturbance {
    lines.push(format!("cursor: {value}"));
  }
  if let Some(value) = press_mechanism {
    lines.push(format!("press: {value}"));
  }
  if let Some(value) = overlay_presentation {
    lines.push(format!("overlay: {value}"));
  }
  if let Some(matched) = ocr_match {
    lines.push(format!("ocr: {} ({:.2})", matched.text, matched.confidence));
  }
  if let Some(row) = row {
    lines.push(format!("row: #{} {}", row.row_index + 1, row.source));
  }
  if let Some(ax_target) = ax_target {
    let label = if ax_target.label.is_empty() {
      ax_target.role.as_str()
    } else {
      ax_target.label.as_str()
    };
    lines.push(format!("ax: {} {}", ax_target.role, label));
  }
  if let Some(decision) = decision {
    lines.push(format!(
      "decision: {} -> {}",
      decision.primary_strategy, decision.selected_strategy
    ));
    if decision.fallback_used {
      lines.push("fallback_reason: primary_error".to_string());
    }
    if let Some(primary_error) = decision.primary_error.as_deref() {
      lines.push(format!(
        "error: {}",
        abbreviate_signal_line(primary_error, 44)
      ));
    }
  }
  lines
}

fn abbreviate_signal_line(value: &str, max_chars: usize) -> String {
  let chars = value.chars().collect::<Vec<_>>();
  if chars.len() <= max_chars {
    return value.to_string();
  }
  chars[..max_chars.saturating_sub(1)]
    .iter()
    .collect::<String>()
    + "…"
}

fn fill_rect(image: &mut RgbaImage, x: i64, y: i64, width: i64, height: i64, color: [u8; 4]) {
  for dy in 0..height {
    for dx in 0..width {
      put_pixel(image, x + dx, y + dy, color);
    }
  }
}

fn draw_label_chip(image: &mut RgbaImage, x: i64, y: i64, label: &str, bg: [u8; 4]) {
  let char_w = 6i64;
  let char_h = 8i64;
  let padding_x = 6i64;
  let padding_y = 4i64;
  let text_w = label.chars().count() as i64 * char_w;
  let width = text_w + padding_x * 2;
  let height = char_h + padding_y * 2;
  fill_rect(image, x, y, width, height, bg);
  draw_mono_text(image, x + padding_x, y + padding_y, label, LABEL_FG);
}

fn draw_mono_text(image: &mut RgbaImage, x: i64, y: i64, text: &str, color: [u8; 4]) {
  let mut cursor_x = x;
  for ch in text.chars() {
    if ch == ' ' {
      cursor_x += 6;
      continue;
    }
    let glyph = glyph_rows(ch);
    for (row, bits) in glyph.iter().enumerate() {
      for col in 0..5 {
        if bits & (1 << (4 - col)) != 0 {
          put_pixel(image, cursor_x + col as i64, y + row as i64, color);
        }
      }
    }
    cursor_x += 6;
  }
}

fn glyph_rows(ch: char) -> [u8; 7] {
  match ch.to_ascii_lowercase() {
    'a' => [
      0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
    ],
    'b' => [
      0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
    ],
    'c' => [
      0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
    ],
    'd' => [
      0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
    ],
    'e' => [
      0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
    ],
    'f' => [
      0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
    ],
    'g' => [
      0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
    ],
    'h' => [
      0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
    ],
    'i' => [
      0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
    ],
    'j' => [
      0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
    ],
    'k' => [
      0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
    ],
    'l' => [
      0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
    ],
    'm' => [
      0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
    ],
    'n' => [
      0b10001, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001,
    ],
    'o' => [
      0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
    ],
    'p' => [
      0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
    ],
    'q' => [
      0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
    ],
    'r' => [
      0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
    ],
    's' => [
      0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
    ],
    't' => [
      0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
    ],
    'u' => [
      0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
    ],
    'v' => [
      0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
    ],
    'w' => [
      0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
    ],
    'x' => [
      0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
    ],
    'y' => [
      0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
    ],
    'z' => [
      0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
    ],
    '0' => [
      0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
    ],
    '1' => [
      0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
    ],
    '2' => [
      0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
    ],
    '3' => [
      0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
    ],
    '4' => [
      0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
    ],
    '5' => [
      0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
    ],
    '6' => [
      0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
    ],
    '7' => [
      0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
    ],
    '8' => [
      0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
    ],
    '9' => [
      0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
    ],
    ':' => [
      0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
    ],
    '.' => [
      0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110,
    ],
    ',' => [
      0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110, 0b00100,
    ],
    '-' => [
      0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
    ],
    '_' => [
      0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
    ],
    '#' => [
      0b01010, 0b11111, 0b01010, 0b01010, 0b11111, 0b01010, 0b01010,
    ],
    '(' => [
      0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
    ],
    ')' => [
      0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
    ],
    '/' => [
      0b00001, 0b00010, 0b00100, 0b00100, 0b01000, 0b10000, 0b00000,
    ],
    '"' => [
      0b01010, 0b01010, 0b00100, 0b00000, 0b00000, 0b00000, 0b00000,
    ],
    '=' => [
      0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
    ],
    '+' => [
      0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
    ],
    _ => [
      0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
    ],
  }
}

fn draw_line(image: &mut RgbaImage, x0: i64, y0: i64, x1: i64, y1: i64, color: [u8; 4]) {
  let dx = (x1 - x0).abs();
  let sx = if x0 < x1 { 1 } else { -1 };
  let dy = -(y1 - y0).abs();
  let sy = if y0 < y1 { 1 } else { -1 };
  let mut err = dx + dy;
  let mut x = x0;
  let mut y = y0;
  loop {
    put_pixel(image, x, y, color);
    if x == x1 && y == y1 {
      break;
    }
    let e2 = err * 2;
    if e2 >= dy {
      err += dy;
      x += sx;
    }
    if e2 <= dx {
      err += dx;
      y += sy;
    }
  }
}

fn put_pixel(image: &mut RgbaImage, x: i64, y: i64, color: [u8; 4]) {
  if x < 0 || y < 0 || x >= image.width() as i64 || y >= image.height() as i64 {
    return;
  }
  image.put_pixel(x as u32, y as u32, Rgba(color));
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::driver::macos::capture::types::{
    CaptureBackend, CaptureContract, CaptureSource, Rect, Scale2D, Size,
  };

  fn sample_capture_contract() -> CaptureContract {
    CaptureContract {
      coordinate_contract_version: 1,
      capture_source: CaptureSource::Display {
        display_ref: "display-main".to_string(),
        native_display_id: "69733248".to_string(),
      },
      capture_backend: CaptureBackend::XcapMacos,
      include_shadow: false,
      source_global_logical_bounds: Rect {
        x: 100.0,
        y: 200.0,
        width: 400.0,
        height: 300.0,
      },
      source_physical_pixel_bounds: Rect {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
      },
      screenshot_pixel_size: Size {
        width: 800.0,
        height: 600.0,
      },
      pixel_to_logical_scale: Scale2D { x: 0.5, y: 0.5 },
      logical_to_pixel_scale: Scale2D { x: 2.0, y: 2.0 },
      captured_at_unix_ms: 1_717_000_000_000,
    }
  }

  fn write_test_screenshot() -> std::path::PathBuf {
    let path = temp_file_path("overlay-evidence-test", "png");
    let image = RgbaImage::from_pixel(64, 48, Rgba([12, 18, 24, 255]));
    save_rgba_image(image, path.as_path()).expect("test screenshot should write");
    path
  }

  #[test]
  fn logical_capture_conversion_round_trips() {
    let contract = sample_capture_contract();

    let point = logical_to_capture_pixel(&contract, 160.0, 260.0);

    assert_eq!(point.screenshot_x, 120.0);
    assert_eq!(point.screenshot_y, 120.0);

    let round_trip = capture_pixel_to_logical(&contract, point.screenshot_x, point.screenshot_y);
    assert_eq!(round_trip.logical_x, 160.0);
    assert_eq!(round_trip.logical_y, 260.0);
  }

  #[test]
  fn build_overlay_evidence_artifacts_writes_png_and_annotation_json() {
    let screenshot_path = write_test_screenshot();
    let request = OverlayEvidenceRequest {
      kind: "window-text-click",
      label: "Window Text Click".to_string(),
      screenshot_path: screenshot_path.clone(),
      screenshot_dimensions: ScreenshotDimensions {
        width: 64,
        height: 48,
      },
      capture_contract: sample_capture_contract(),
      query: Some("Play".to_string()),
      strategy: Some("ocr-text".to_string()),
      fallback_used: Some(false),
      cursor_disturbance: Some("warp-visible".to_string()),
      press_mechanism: Some("pointer-click".to_string()),
      overlay_presentation: Some("off".to_string()),
      action_point: OverlayEvidencePoint {
        logical_x: 120.0,
        logical_y: 212.0,
        screenshot_x: 40.0,
        screenshot_y: 24.0,
      },
      expected_target: Some(OverlayEvidencePoint {
        logical_x: 118.0,
        logical_y: 210.0,
        screenshot_x: 36.0,
        screenshot_y: 20.0,
      }),
      ocr_match: Some(OverlayEvidenceMatch {
        text: "Play".to_string(),
        confidence: 0.93,
        bounds: ObservedRect {
          x: 24,
          y: 12,
          width: 16,
          height: 8,
        },
      }),
      row: Some(OverlayEvidenceRow {
        row_index: 0,
        source: "title_band".to_string(),
        bounds: ObservedRect {
          x: 12,
          y: 16,
          width: 36,
          height: 12,
        },
        text_fragments: vec!["Play".to_string(), "Song".to_string()],
      }),
      ax_target: Some(OverlayEvidenceAxTarget {
        role: "AXButton".to_string(),
        label: "Play".to_string(),
        bounds: ObservedRect {
          x: 20,
          y: 10,
          width: 24,
          height: 14,
        },
      }),
      decision: Some(OverlayEvidenceDecision {
        operation: "smart_press".to_string(),
        primary_strategy: "ax-action".to_string(),
        selected_strategy: "pointer-click".to_string(),
        fallback_used: true,
        primary_error: Some("no matching AX action".to_string()),
      }),
      include_user_cursor: false,
      auv_cursor_variant: "auv-click",
    };

    let artifacts =
      build_overlay_evidence_artifacts(request).expect("overlay evidence should build");

    assert_eq!(artifacts.len(), 2);
    assert_eq!(artifacts[0].kind, "click.overlay");
    assert_eq!(artifacts[1].kind, "click.overlay.annotation");
    assert!(artifacts[0].source_path.exists());
    assert!(artifacts[1].source_path.exists());

    let annotation = fs::read_to_string(&artifacts[1].source_path)
      .expect("annotation artifact should be readable");
    let payload: serde_json::Value =
      serde_json::from_str(&annotation).expect("annotation JSON should parse");

    assert_eq!(
      payload.get("kind").and_then(|value| value.as_str()),
      Some("window-text-click")
    );
    assert_eq!(
      payload.get("query").and_then(|value| value.as_str()),
      Some("Play")
    );
    assert_eq!(
      payload.get("strategy").and_then(|value| value.as_str()),
      Some("ocr-text")
    );
    assert_eq!(
      payload
        .get("cursor_disturbance")
        .and_then(|value| value.as_str()),
      Some("warp-visible")
    );
    assert_eq!(
      payload
        .get("press_mechanism")
        .and_then(|value| value.as_str()),
      Some("pointer-click")
    );
    assert_eq!(
      payload
        .get("ocr_match")
        .and_then(|value| value.get("text"))
        .and_then(|value| value.as_str()),
      Some("Play")
    );
    assert_eq!(
      payload
        .get("row")
        .and_then(|value| value.get("text_fragments"))
        .and_then(|value| value.as_array())
        .map(|items| {
          items
            .iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect::<Vec<_>>()
        }),
      Some(vec!["Play".to_string(), "Song".to_string()])
    );
    assert_eq!(
      payload
        .get("ax_target")
        .and_then(|value| value.get("role"))
        .and_then(|value| value.as_str()),
      Some("AXButton")
    );
    assert_eq!(
      payload
        .get("decision")
        .and_then(|value| value.get("selected_strategy"))
        .and_then(|value| value.as_str()),
      Some("pointer-click")
    );
    assert!(
      payload
        .get("user_cursor")
        .is_some_and(|value| value.is_null())
    );
    assert_eq!(
      payload
        .get("auv_cursor")
        .and_then(|value| value.get("variant"))
        .and_then(|value| value.as_str()),
      Some("auv-click")
    );
  }

  #[test]
  fn build_signal_lines_include_query_strategy_and_row_context() {
    let lines = build_signal_lines(
      Some("晴天"),
      Some("smart-press"),
      Some(true),
      Some("none"),
      Some("ax-action"),
      Some("dual-cursor-visual-only"),
      Some(&OverlayEvidenceMatch {
        text: "天空仍灿烂".to_string(),
        confidence: 0.81,
        bounds: ObservedRect {
          x: 1,
          y: 2,
          width: 3,
          height: 4,
        },
      }),
      Some(&OverlayEvidenceRow {
        row_index: 1,
        source: "row_ratio".to_string(),
        bounds: ObservedRect {
          x: 1,
          y: 2,
          width: 3,
          height: 4,
        },
        text_fragments: vec!["天空仍灿烂".to_string()],
      }),
      Some(&OverlayEvidenceAxTarget {
        role: "AXButton".to_string(),
        label: "播放".to_string(),
        bounds: ObservedRect {
          x: 1,
          y: 2,
          width: 3,
          height: 4,
        },
      }),
      Some(&OverlayEvidenceDecision {
        operation: "smart_press".to_string(),
        primary_strategy: "ax-action".to_string(),
        selected_strategy: "pointer-click".to_string(),
        fallback_used: true,
        primary_error: Some("no action".to_string()),
      }),
    );

    assert!(lines.contains(&"query: 晴天".to_string()));
    assert!(lines.contains(&"strategy: smart-press".to_string()));
    assert!(lines.contains(&"fallback: true".to_string()));
    assert!(lines.contains(&"cursor: none".to_string()));
    assert!(lines.contains(&"press: ax-action".to_string()));
    assert!(lines.contains(&"overlay: dual-cursor-visual-only".to_string()));
    assert!(lines.contains(&"ocr: 天空仍灿烂 (0.81)".to_string()));
    assert!(lines.contains(&"row: #2 row_ratio".to_string()));
    assert!(lines.contains(&"ax: AXButton 播放".to_string()));
    assert!(lines.contains(&"decision: ax-action -> pointer-click".to_string()));
    assert!(lines.contains(&"fallback_reason: primary_error".to_string()));
  }
}
