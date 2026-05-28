// File: src/driver/macos/support/ocr.rs
use std::path::{Path, PathBuf};

use super::super::*;
use super::{
  ocr_match_center, optional_f64, parse_bool_flag, parse_f64, parse_i64, render_rect_compact,
  report_value,
};

pub(crate) fn parse_ocr_text_snapshot(report: &str) -> AuvResult<OcrTextSnapshot> {
  let recognized_at = report_value(report, "recognizedAt=")
    .unwrap_or("")
    .to_string();
  let image_path = PathBuf::from(report_value(report, "imagePath=").unwrap_or(""));
  let image_width = parse_i64(
    report_value(report, "imageWidth=").unwrap_or("0"),
    "ocr.imageWidth",
  )?;
  let image_height = parse_i64(
    report_value(report, "imageHeight=").unwrap_or("0"),
    "ocr.imageHeight",
  )?;
  let query = report_value(report, "query=").unwrap_or("").to_string();
  let exact = parse_bool_flag(
    report_value(report, "exact=").unwrap_or("false"),
    "ocr.exact",
  )?;
  let case_sensitive = parse_bool_flag(
    report_value(report, "caseSensitive=").unwrap_or("false"),
    "ocr.caseSensitive",
  )?;
  let matches = report
    .lines()
    .filter(|line| line.starts_with("match\t"))
    .map(parse_ocr_text_line)
    .collect::<AuvResult<Vec<_>>>()?;
  Ok(OcrTextSnapshot {
    recognized_at,
    image_path,
    image_width,
    image_height,
    query,
    exact,
    case_sensitive,
    matches,
  })
}

pub(crate) fn parse_ocr_text_line(line: &str) -> AuvResult<OcrTextMatch> {
  let columns = line.split('\t').collect::<Vec<_>>();
  if columns.len() != 8 {
    return Err(format!(
      "invalid OCR report line; expected 8 columns but got {}: {}",
      columns.len(),
      line
    ));
  }

  Ok(OcrTextMatch {
    match_index: columns[1]
      .parse::<usize>()
      .map_err(|error| format!("invalid ocr.matchIndex value {}: {}", columns[1], error))?,
    text: columns[2].to_string(),
    confidence: parse_f64(columns[3], "ocr.confidence")?,
    bounds: ObservedRect {
      x: parse_i64(columns[4], "ocr.bounds.x")?,
      y: parse_i64(columns[5], "ocr.bounds.y")?,
      width: parse_i64(columns[6], "ocr.bounds.width")?,
      height: parse_i64(columns[7], "ocr.bounds.height")?,
    },
  })
}

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

pub(crate) fn ocr_text_fragments_in_image(
  image_path: &Path,
  min_confidence: f64,
  max_observations: i64,
) -> AuvResult<Vec<String>> {
  let capture = auv_driver_macos::native::ocr::find_text(
    image_path,
    "",
    false,
    false,
    max_observations.min(64),
    None,
  )?;
  let mut matches = filter_ocr_matches(&capture.snapshot.matches, min_confidence, None);
  matches.sort_by(|left, right| {
    left
      .bounds
      .y
      .cmp(&right.bounds.y)
      .then_with(|| left.bounds.x.cmp(&right.bounds.x))
  });
  let mut fragments = Vec::new();
  for matched in matches {
    if !fragments.iter().any(|fragment| fragment == &matched.text) {
      fragments.push(matched.text.clone());
    }
  }
  Ok(fragments)
}

pub(crate) fn group_ocr_matches_into_rows(matches: &[&OcrTextMatch]) -> Vec<ObservedOcrRow> {
  let mut sorted = matches.to_vec();
  sorted.sort_by(|left, right| {
    let (_, left_center_y) = ocr_match_center(left);
    let (_, right_center_y) = ocr_match_center(right);
    left_center_y
      .partial_cmp(&right_center_y)
      .unwrap_or(std::cmp::Ordering::Equal)
      .then_with(|| left.bounds.x.cmp(&right.bounds.x))
  });

  let mut rows = Vec::<ObservedOcrRow>::new();
  for matched in sorted {
    let (_, center_y) = ocr_match_center(matched);
    if let Some(existing) = rows.last_mut() {
      let existing_center_y = existing.bounds.y as f64 + (existing.bounds.height as f64 / 2.0);
      let vertical_threshold =
        ((existing.bounds.height.max(matched.bounds.height)) as f64 * 1.5).max(36.0);
      if (center_y - existing_center_y).abs() <= vertical_threshold {
        existing.bounds = union_rects(&existing.bounds, &matched.bounds);
        if !existing
          .text_fragments
          .iter()
          .any(|value| value == &matched.text)
        {
          existing.text_fragments.push(matched.text.clone());
        }
        continue;
      }
    }

    rows.push(ObservedOcrRow {
      row_index: rows.len(),
      source: "ocr-text".to_string(),
      bounds: matched.bounds.clone(),
      text_fragments: vec![matched.text.clone()],
    });
  }

  for (index, row) in rows.iter_mut().enumerate() {
    row.row_index = index;
  }
  rows
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

fn union_rects(left: &ObservedRect, right: &ObservedRect) -> ObservedRect {
  let min_x = left.x.min(right.x);
  let min_y = left.y.min(right.y);
  let max_x = (left.x + left.width).max(right.x + right.width);
  let max_y = (left.y + left.height).max(right.y + right.height);
  ObservedRect {
    x: min_x,
    y: min_y,
    width: max_x - min_x,
    height: max_y - min_y,
  }
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
}
