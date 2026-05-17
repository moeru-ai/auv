use std::path::{Path, PathBuf};

use super::super::*;
use super::{
  build_find_visual_rows_script, build_ocr_find_text_script, ocr_match_center, optional_f64,
  parse_bool_flag, parse_f64, parse_i64, render_rect_compact, report_value, run_swift_script,
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
  let ocr_report = run_swift_script(&build_ocr_find_text_script(
    image_path,
    "",
    false,
    false,
    max_observations,
    region,
  ))?;
  let ocr_snapshot = parse_ocr_text_snapshot(&ocr_report)?;
  let filtered_matches = filter_ocr_matches(&ocr_snapshot.matches, min_confidence, region);
  let rows = group_ocr_matches_into_rows(&filtered_matches);
  if !rows.is_empty() {
    return Ok(DetectedScreenRows {
      strategy: "ocr-text".to_string(),
      raw_match_count: ocr_snapshot.matches.len(),
      filtered_match_count: filtered_matches.len(),
      rows,
      report: ocr_report,
    });
  }

  let visual_report = run_swift_script(&build_find_visual_rows_script(image_path, region))?;
  parse_visual_rows_snapshot(&visual_report)
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
