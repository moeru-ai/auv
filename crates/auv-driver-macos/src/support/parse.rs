use std::path::PathBuf;

use crate::types::{AuvResult, ObservedRect, ObservedWindow, OcrTextMatch, OcrTextSnapshot};

pub fn report_value<'a>(report: &'a str, prefix: &str) -> Option<&'a str> {
  report.lines().find_map(|line| line.strip_prefix(prefix)).map(str::trim)
}

pub fn parse_bool_flag(raw: &str, label: &str) -> AuvResult<bool> {
  match raw {
    "1" | "true" => Ok(true),
    "0" | "false" => Ok(false),
    other => Err(format!("invalid {} value {}: expected 0/1", label, other)),
  }
}

pub fn parse_i64(raw: &str, label: &str) -> AuvResult<i64> {
  raw.parse::<i64>().map_err(|error| format!("invalid {} value {}: {}", label, raw, error))
}

pub fn parse_u32(raw: &str, label: &str) -> AuvResult<u32> {
  raw.parse::<u32>().map_err(|error| format!("invalid {} value {}: {}", label, raw, error))
}

pub fn parse_f64(raw: &str, label: &str) -> AuvResult<f64> {
  let value = raw.parse::<f64>().map_err(|error| format!("invalid {} value {}: {}", label, raw, error))?;
  if !value.is_finite() {
    return Err(format!("invalid {} value {}: expected a finite number", label, raw));
  }
  Ok(value)
}

pub fn parse_window_line(line: &str) -> AuvResult<ObservedWindow> {
  let columns = line.split('\t').collect::<Vec<_>>();
  if columns.len() != 11 {
    return Err(format!("invalid window report line; expected 11 columns but got {}: {}", columns.len(), line));
  }

  Ok(ObservedWindow {
    window_number: parse_i64(columns[4], "window.number")?,
    app_name: columns[1].to_string(),
    owner_pid: parse_i64(columns[2], "window.ownerPid")?,
    owner_bundle_id: columns[3].to_string(),
    layer: parse_i64(columns[5], "window.layer")?,
    title: columns[6].to_string(),
    bounds: ObservedRect {
      x: parse_i64(columns[7], "window.bounds.x")?,
      y: parse_i64(columns[8], "window.bounds.y")?,
      width: parse_i64(columns[9], "window.bounds.width")?,
      height: parse_i64(columns[10], "window.bounds.height")?,
    },
  })
}

pub fn parse_ocr_text_snapshot(report: &str) -> AuvResult<OcrTextSnapshot> {
  let recognized_at = report_value(report, "recognizedAt=").unwrap_or("").to_string();
  let image_path = PathBuf::from(report_value(report, "imagePath=").unwrap_or(""));
  let image_width = parse_i64(report_value(report, "imageWidth=").unwrap_or("0"), "ocr.imageWidth")?;
  let image_height = parse_i64(report_value(report, "imageHeight=").unwrap_or("0"), "ocr.imageHeight")?;
  let query = report_value(report, "query=").unwrap_or("").to_string();
  let exact = parse_bool_flag(report_value(report, "exact=").unwrap_or("false"), "ocr.exact")?;
  let case_sensitive = parse_bool_flag(report_value(report, "caseSensitive=").unwrap_or("false"), "ocr.caseSensitive")?;
  let matches = report.lines().filter(|line| line.starts_with("match\t")).map(parse_ocr_text_line).collect::<AuvResult<Vec<_>>>()?;
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

pub fn parse_ocr_text_line(line: &str) -> AuvResult<OcrTextMatch> {
  let columns = line.split('\t').collect::<Vec<_>>();
  if columns.len() != 8 {
    return Err(format!("invalid OCR report line; expected 8 columns but got {}: {}", columns.len(), line));
  }

  Ok(OcrTextMatch {
    match_index: columns[1].parse::<usize>().map_err(|error| format!("invalid ocr.matchIndex value {}: {}", columns[1], error))?,
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
