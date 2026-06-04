// File: src/driver/macos/support/ocr.rs
use std::path::Path;

#[cfg(test)]
use auv_driver_macos::support::{parse_i64, report_value};

use super::super::{DetectedScreenRows, DriverCall, ObservedOcrRow, OcrTextMatch};
use super::call::optional_f64;
use super::geometry::{ocr_match_center, render_rect_compact};
use crate::model::AuvResult;
use auv_driver_macos::support::group_ocr_matches_into_rows;
use auv_driver_macos::types::ObservedRect;

#[cfg(test)]
pub(crate) fn parse_visual_rows_snapshot(report: &str) -> AuvResult<DetectedScreenRows> {
  let rows = report
    .lines()
    .filter(|line| line.starts_with("row\t"))
    .map(parse_visual_row_line)
    .collect::<AuvResult<Vec<_>>>()?;
  Ok(DetectedScreenRows {
    strategy: report_value(report, "rowStrategy=")
      .unwrap_or("visual-bands")
      .to_string(),
    raw_match_count: 0,
    filtered_match_count: 0,
    rows,
    report: report.to_string(),
  })
}

#[cfg(test)]
fn parse_visual_row_line(line: &str) -> AuvResult<ObservedOcrRow> {
  let columns = line.split('\t').collect::<Vec<_>>();
  if columns.len() != 7 {
    return Err(format!(
      "invalid visual-row report line; expected 7 columns but got {}: {}",
      columns.len(),
      line
    ));
  }

  Ok(ObservedOcrRow {
    row_index: columns[1]
      .parse::<usize>()
      .map_err(|error| format!("invalid visualRow.index value {}: {}", columns[1], error))?,
    source: "visual-bands".to_string(),
    bounds: ObservedRect {
      x: parse_i64(columns[2], "visualRow.bounds.x")?,
      y: parse_i64(columns[3], "visualRow.bounds.y")?,
      width: parse_i64(columns[4], "visualRow.bounds.width")?,
      height: parse_i64(columns[5], "visualRow.bounds.height")?,
    },
    text_fragments: vec![],
  })
}

pub(crate) fn parse_ocr_region_constraint(
  call: &DriverCall,
  image_width: i64,
  image_height: i64,
) -> AuvResult<Option<ObservedRect>> {
  let left_ratio = optional_f64(call, "region_left_ratio")?;
  let top_ratio = optional_f64(call, "region_top_ratio")?;
  let right_ratio = optional_f64(call, "region_right_ratio")?;
  let bottom_ratio = optional_f64(call, "region_bottom_ratio")?;

  match (left_ratio, top_ratio, right_ratio, bottom_ratio) {
    (None, None, None, None) => Ok(None),
    (Some(left), Some(top), Some(right), Some(bottom)) => {
      for (label, value) in [
        ("region_left_ratio", left),
        ("region_top_ratio", top),
        ("region_right_ratio", right),
        ("region_bottom_ratio", bottom),
      ] {
        if !(0.0..=1.0).contains(&value) {
          return Err(format!(
            "invalid --{} value {:.3}: expected a ratio within 0.0..=1.0",
            label, value
          ));
        }
      }
      if left >= right {
        return Err(format!(
          "invalid OCR region: left ratio {:.3} must be smaller than right ratio {:.3}",
          left, right
        ));
      }
      if top >= bottom {
        return Err(format!(
          "invalid OCR region: top ratio {:.3} must be smaller than bottom ratio {:.3}",
          top, bottom
        ));
      }

      Ok(Some(ObservedRect {
        x: (left * image_width as f64).round() as i64,
        y: (top * image_height as f64).round() as i64,
        width: ((right - left) * image_width as f64).round() as i64,
        height: ((bottom - top) * image_height as f64).round() as i64,
      }))
    }
    _ => Err(
      "OCR region ratio mode requires --region_left_ratio, --region_top_ratio, --region_right_ratio, and --region_bottom_ratio together"
        .to_string(),
    ),
  }
}

pub(crate) fn filter_ocr_matches<'a>(
  matches: &'a [OcrTextMatch],
  min_confidence: f64,
  region: Option<&ObservedRect>,
) -> Vec<&'a OcrTextMatch> {
  matches
    .iter()
    .filter(|matched| matched.confidence >= min_confidence)
    .filter(|matched| {
      region.is_none_or(|region| {
        let (center_x, center_y) = ocr_match_center(matched);
        center_x >= region.x as f64
          && center_y >= region.y as f64
          && center_x < (region.x + region.width) as f64
          && center_y < (region.y + region.height) as f64
      })
    })
    .collect()
}

pub(crate) fn detect_screen_rows(
  image_path: &Path,
  min_confidence: f64,
  max_observations: i64,
  region: Option<&ObservedRect>,
) -> AuvResult<DetectedScreenRows> {
  let ocr_capture = auv_driver_macos::native::ocr::find_text(
    image_path,
    "",
    false,
    false,
    max_observations,
    &[],
    None,
    region,
  )?;
  let ocr_report = auv_driver_macos::native::ocr::render_ocr_text_report(&ocr_capture);
  let ocr_snapshot = ocr_capture.snapshot;
  let filtered_matches = filter_ocr_matches(&ocr_snapshot.matches, min_confidence, region);
  let ocr_rows = group_ocr_matches_into_rows(&filtered_matches);

  let mut visual_detection = auv_driver_macos::native::ocr::find_rows(image_path, region)?.rows;
  if visual_detection.rows.is_empty() && !ocr_rows.is_empty() {
    return Ok(DetectedScreenRows {
      strategy: "ocr-text".to_string(),
      raw_match_count: ocr_snapshot.matches.len(),
      filtered_match_count: filtered_matches.len(),
      rows: ocr_rows,
      report: ocr_report,
    });
  }
  attach_ocr_fragments_to_visual_rows(&mut visual_detection.rows, &filtered_matches);
  let mut row_ocr_match_count = 0;
  if visual_detection
    .rows
    .iter()
    .any(|row| row.text_fragments.is_empty())
  {
    row_ocr_match_count = attach_row_crop_ocr_fragments(
      image_path,
      &mut visual_detection.rows,
      min_confidence,
      max_observations,
      ocr_snapshot.image_width,
      ocr_snapshot.image_height,
    )?;
  }
  if visual_detection
    .rows
    .iter()
    .any(|row| !row.text_fragments.is_empty())
  {
    visual_detection.strategy = "visual-bands+ocr-text".to_string();
    visual_detection.raw_match_count = ocr_snapshot.matches.len() + row_ocr_match_count;
    visual_detection.filtered_match_count = filtered_matches.len() + row_ocr_match_count;
  }

  Ok(visual_detection)
}

// REVIEW: Visual row bands and Vision text boxes do not share a stable list
// model yet. This joins text by row containment/centerline as a conservative
// bridge so scroll-scan artifacts expose readable row content before a richer
// item extractor contract exists.
pub(crate) fn attach_ocr_fragments_to_visual_rows(
  rows: &mut [ObservedOcrRow],
  matches: &[&OcrTextMatch],
) -> usize {
  let mut attached = 0;
  for row in rows.iter_mut() {
    for matched in matches {
      let (_, center_y) = ocr_match_center(matched);
      if !row_contains_y(&row.bounds, center_y) || !rects_overlap_x(&row.bounds, &matched.bounds) {
        continue;
      }
      if !row
        .text_fragments
        .iter()
        .any(|fragment| fragment == &matched.text)
      {
        row.text_fragments.push(matched.text.clone());
        attached += 1;
      }
    }
    if !row.text_fragments.is_empty() && row.source == "visual-bands" {
      row.source = "visual-bands+ocr-text".to_string();
    }
  }
  attached
}

fn attach_row_crop_ocr_fragments(
  image_path: &Path,
  rows: &mut [ObservedOcrRow],
  min_confidence: f64,
  max_observations: i64,
  image_width: i64,
  image_height: i64,
) -> AuvResult<usize> {
  let mut attached = 0;
  for row in rows.iter_mut() {
    if !row.text_fragments.is_empty() {
      continue;
    }
    let row_region = expand_rect(&row.bounds, 12, 8, image_width, image_height);
    let row_capture = auv_driver_macos::native::ocr::find_text(
      image_path,
      "",
      false,
      false,
      max_observations.min(32),
      &[],
      None,
      Some(&row_region),
    )?;
    let row_matches = filter_ocr_matches(&row_capture.snapshot.matches, min_confidence, None);
    for matched in row_matches {
      if row
        .text_fragments
        .iter()
        .any(|fragment| fragment == &matched.text)
      {
        continue;
      }
      row.text_fragments.push(matched.text.clone());
      attached += 1;
    }
    if !row.text_fragments.is_empty() {
      row.source = "visual-bands+row-ocr".to_string();
    }
  }
  Ok(attached)
}

fn row_contains_y(row: &ObservedRect, y: f64) -> bool {
  let padding = (row.height as f64 * 0.25).max(8.0);
  y >= row.y as f64 - padding && y <= (row.y + row.height) as f64 + padding
}

fn rects_overlap_x(left: &ObservedRect, right: &ObservedRect) -> bool {
  let left_max = left.x + left.width;
  let right_max = right.x + right.width;
  left.x < right_max && right.x < left_max
}

fn expand_rect(
  rect: &ObservedRect,
  pad_x: i64,
  pad_y: i64,
  image_width: i64,
  image_height: i64,
) -> ObservedRect {
  // Guard against a non-positive image size before clamping. With i64,
  // `image_width.saturating_sub(1)` is -1 when image_width is 0, which
  // makes `clamp(_, 0, -1)` panic (min > max). Skip expansion in that
  // case and return the original rect — the caller's rect is already
  // a well-formed observation.
  if image_width <= 0 || image_height <= 0 {
    return rect.clone();
  }
  let x = (rect.x - pad_x).clamp(0, image_width.saturating_sub(1));
  let y = (rect.y - pad_y).clamp(0, image_height.saturating_sub(1));
  let max_x = (rect.x + rect.width + pad_x).clamp(x + 1, image_width);
  let max_y = (rect.y + rect.height + pad_y).clamp(y + 1, image_height);
  ObservedRect {
    x,
    y,
    width: max_x - x,
    height: max_y - y,
  }
}

pub(crate) fn render_ocr_row_note(row: &ObservedOcrRow) -> String {
  if row.text_fragments.is_empty() {
    return format!(
      "row[{}] source={} bounds={}",
      row.row_index,
      row.source,
      render_rect_compact(&row.bounds)
    );
  }

  let preview = row
    .text_fragments
    .iter()
    .take(3)
    .cloned()
    .collect::<Vec<_>>()
    .join(" | ");
  format!(
    "row[{}] source={} bounds={} text={}",
    row.row_index,
    row.source,
    render_rect_compact(&row.bounds),
    preview
  )
}

pub(crate) fn render_ocr_region_note(region: &ObservedRect) -> String {
  format!("ocrRegion={}", render_rect_compact(region))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn attaches_ocr_fragments_to_overlapping_visual_rows() {
    let mut rows = vec![
      ObservedOcrRow {
        row_index: 0,
        source: "visual-bands".to_string(),
        bounds: ObservedRect {
          x: 100,
          y: 200,
          width: 400,
          height: 80,
        },
        text_fragments: Vec::new(),
      },
      ObservedOcrRow {
        row_index: 1,
        source: "visual-bands".to_string(),
        bounds: ObservedRect {
          x: 100,
          y: 320,
          width: 400,
          height: 80,
        },
        text_fragments: Vec::new(),
      },
    ];
    let first = OcrTextMatch {
      match_index: 0,
      text: "Song A".to_string(),
      confidence: 0.9,
      bounds: ObservedRect {
        x: 140,
        y: 220,
        width: 120,
        height: 28,
      },
    };
    let second = OcrTextMatch {
      match_index: 1,
      text: "Artist B".to_string(),
      confidence: 0.9,
      bounds: ObservedRect {
        x: 150,
        y: 340,
        width: 120,
        height: 28,
      },
    };
    let outside = OcrTextMatch {
      match_index: 2,
      text: "Outside".to_string(),
      confidence: 0.9,
      bounds: ObservedRect {
        x: 700,
        y: 220,
        width: 120,
        height: 28,
      },
    };
    let matches = vec![&first, &second, &outside];

    let attached = attach_ocr_fragments_to_visual_rows(&mut rows, &matches);

    assert_eq!(attached, 2);
    assert_eq!(rows[0].source, "visual-bands+ocr-text");
    assert_eq!(rows[0].text_fragments, vec!["Song A"]);
    assert_eq!(rows[1].text_fragments, vec!["Artist B"]);
  }

  #[test]
  fn expand_rect_returns_input_when_image_dimensions_are_zero() {
    let rect = ObservedRect {
      x: 0,
      y: 0,
      width: 10,
      height: 10,
    };
    let expanded = expand_rect(&rect, 12, 8, 0, 0);
    assert_eq!(expanded, rect);
  }

  #[test]
  fn expand_rect_returns_input_when_image_dimensions_are_negative() {
    let rect = ObservedRect {
      x: 0,
      y: 0,
      width: 10,
      height: 10,
    };
    let expanded = expand_rect(&rect, 12, 8, -1, -1);
    assert_eq!(expanded, rect);
  }

  #[test]
  fn expand_rect_pads_within_image_bounds() {
    let rect = ObservedRect {
      x: 100,
      y: 200,
      width: 400,
      height: 80,
    };
    let expanded = expand_rect(&rect, 12, 8, 1024, 768);
    assert_eq!(
      expanded,
      ObservedRect {
        x: 88,
        y: 192,
        width: 424,
        height: 96,
      }
    );
  }
}
