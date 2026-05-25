// File: src/driver/macos/native/ocr.rs
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use super::ffi::ffi::{
  NativeOcrTextRequest, NativeOcrTextResponse, NativeVisualRowsRequest, NativeVisualRowsResponse,
  find_ocr_text, find_visual_rows,
};
use crate::driver::macos::{
  DetectedScreenRows, ObservedOcrRow, ObservedRect, OcrTextMatch, OcrTextSnapshot,
};
use crate::model::AuvResult;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeOcrTextCapture {
  pub(crate) snapshot: OcrTextSnapshot,
  pub(crate) normalized_query: String,
  pub(crate) crop_rect: Option<ObservedRect>,
  pub(crate) ocr_scale_factor: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeVisualRowsCapture {
  pub(crate) rows: DetectedScreenRows,
  pub(crate) detected_at: String,
  pub(crate) image_path: PathBuf,
  pub(crate) image_width: i64,
  pub(crate) image_height: i64,
  pub(crate) crop_rect: Option<ObservedRect>,
  pub(crate) analysis_strip: ObservedRect,
  pub(crate) peak_densities: Vec<f64>,
}

#[cfg(target_os = "macos")]
pub(crate) fn find_text(
  image_path: &Path,
  query: &str,
  exact: bool,
  case_sensitive: bool,
  max_observations: i64,
  crop_region: Option<&ObservedRect>,
) -> AuvResult<NativeOcrTextCapture> {
  let crop = crop_region.cloned().unwrap_or(ObservedRect {
    x: 0,
    y: 0,
    width: 0,
    height: 0,
  });
  decode_ocr_text_response(DecodedOcrTextResponse::from(find_ocr_text(
    NativeOcrTextRequest {
      image_path: image_path.display().to_string(),
      query: query.to_string(),
      exact,
      case_sensitive,
      max_observations,
      crop_enabled: crop_region.is_some(),
      crop_x: crop.x,
      crop_y: crop.y,
      crop_width: crop.width,
      crop_height: crop.height,
    },
  )))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn find_text(
  _image_path: &Path,
  _query: &str,
  _exact: bool,
  _case_sensitive: bool,
  _max_observations: i64,
  _crop_region: Option<&ObservedRect>,
) -> AuvResult<NativeOcrTextCapture> {
  Err("macOS native OCR text detection is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn find_rows(
  image_path: &Path,
  crop_region: Option<&ObservedRect>,
) -> AuvResult<NativeVisualRowsCapture> {
  let crop = crop_region.cloned().unwrap_or(ObservedRect {
    x: 0,
    y: 0,
    width: 0,
    height: 0,
  });
  decode_visual_rows_response(DecodedVisualRowsResponse::from(find_visual_rows(
    NativeVisualRowsRequest {
      image_path: image_path.display().to_string(),
      crop_enabled: crop_region.is_some(),
      crop_x: crop.x,
      crop_y: crop.y,
      crop_width: crop.width,
      crop_height: crop.height,
    },
  )))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn find_rows(
  _image_path: &Path,
  _crop_region: Option<&ObservedRect>,
) -> AuvResult<NativeVisualRowsCapture> {
  Err("macOS native visual row detection is unsupported on this target".to_string())
}

pub(crate) fn decode_ocr_text_response(
  response: DecodedOcrTextResponse,
) -> AuvResult<NativeOcrTextCapture> {
  if response.error_message.is_some() {
    return super::error::native_result(
      "find_ocr_text",
      None,
      response.error_message,
      response.recovery_hint,
    );
  }

  let count = response.match_indices.len();
  let lengths = [
    response.texts.len(),
    response.confidences.len(),
    response.x_values.len(),
    response.y_values.len(),
    response.width_values.len(),
    response.height_values.len(),
  ];
  if lengths.iter().any(|length| *length != count) {
    return Err("native OCR text response had mismatched OCR match vector lengths".to_string());
  }

  let matches = (0..count)
    .map(|index| {
      let match_index = usize::try_from(response.match_indices[index]).map_err(|error| {
        format!(
          "native OCR text response had invalid match index {}: {error}",
          response.match_indices[index]
        )
      })?;
      Ok(OcrTextMatch {
        match_index,
        text: response.texts[index].clone(),
        confidence: response.confidences[index],
        bounds: ObservedRect {
          x: response.x_values[index],
          y: response.y_values[index],
          width: response.width_values[index],
          height: response.height_values[index],
        },
      })
    })
    .collect::<AuvResult<Vec<_>>>()?;

  let crop_rect = response.crop_enabled.then_some(ObservedRect {
    x: response.crop_x,
    y: response.crop_y,
    width: response.crop_width,
    height: response.crop_height,
  });

  Ok(NativeOcrTextCapture {
    snapshot: OcrTextSnapshot {
      recognized_at: response.recognized_at,
      image_path: PathBuf::from(response.image_path),
      image_width: response.image_width,
      image_height: response.image_height,
      query: response.query,
      exact: response.exact,
      case_sensitive: response.case_sensitive,
      matches,
    },
    normalized_query: response.normalized_query,
    crop_rect,
    ocr_scale_factor: response.ocr_scale_factor,
  })
}

pub(crate) fn decode_visual_rows_response(
  response: DecodedVisualRowsResponse,
) -> AuvResult<NativeVisualRowsCapture> {
  if response.error_message.is_some() {
    return super::error::native_result(
      "find_visual_rows",
      None,
      response.error_message,
      response.recovery_hint,
    );
  }

  let count = response.row_indices.len();
  let lengths = [
    response.x_values.len(),
    response.y_values.len(),
    response.width_values.len(),
    response.height_values.len(),
    response.peak_densities.len(),
  ];
  if lengths.iter().any(|length| *length != count) {
    return Err("native visual rows response had mismatched row vector lengths".to_string());
  }

  let rows = (0..count)
    .map(|index| {
      let row_index = usize::try_from(response.row_indices[index]).map_err(|error| {
        format!(
          "native visual rows response had invalid row index {}: {error}",
          response.row_indices[index]
        )
      })?;
      Ok(ObservedOcrRow {
        row_index,
        source: "visual-bands".to_string(),
        bounds: ObservedRect {
          x: response.x_values[index],
          y: response.y_values[index],
          width: response.width_values[index],
          height: response.height_values[index],
        },
        text_fragments: vec![],
      })
    })
    .collect::<AuvResult<Vec<_>>>()?;

  let crop_rect = response.crop_enabled.then_some(ObservedRect {
    x: response.crop_x,
    y: response.crop_y,
    width: response.crop_width,
    height: response.crop_height,
  });
  let analysis_strip = ObservedRect {
    x: response.analysis_strip_x,
    y: response.analysis_strip_y,
    width: response.analysis_strip_width,
    height: response.analysis_strip_height,
  };
  let report_rows = rows.clone();

  Ok(
    NativeVisualRowsCapture {
      rows: DetectedScreenRows {
        strategy: "visual-bands".to_string(),
        raw_match_count: 0,
        filtered_match_count: 0,
        rows,
        report: String::new(),
      },
      detected_at: response.detected_at,
      image_path: PathBuf::from(response.image_path),
      image_width: response.image_width,
      image_height: response.image_height,
      crop_rect,
      analysis_strip,
      peak_densities: response.peak_densities,
    }
    .with_report(report_rows),
  )
}

impl NativeVisualRowsCapture {
  fn with_report(mut self, report_rows: Vec<ObservedOcrRow>) -> Self {
    self.rows.report = render_visual_rows_report(self.clone(), &report_rows);
    self
  }
}

pub(crate) fn render_ocr_text_report(capture: &NativeOcrTextCapture) -> String {
  let snapshot = &capture.snapshot;
  let mut lines = vec![
    format!("recognizedAt={}", snapshot.recognized_at),
    format!("imagePath={}", snapshot.image_path.display()),
    format!("imageWidth={}", snapshot.image_width),
    format!("imageHeight={}", snapshot.image_height),
    format!("query={}", snapshot.query),
    format!("exact={}", snapshot.exact),
    format!("caseSensitive={}", snapshot.case_sensitive),
    format!("normalizedQuery={}", capture.normalized_query),
  ];
  if let Some(crop) = capture.crop_rect.as_ref() {
    lines.push(format!(
      "cropRect={},{},{},{}",
      crop.x, crop.y, crop.width, crop.height
    ));
    lines.push(format!("ocrScaleFactor={:.3}", capture.ocr_scale_factor));
  }
  for matched in &snapshot.matches {
    lines.push(format!(
      "match\t{}\t{}\t{:.6}\t{}\t{}\t{}\t{}",
      matched.match_index,
      matched.text,
      matched.confidence,
      matched.bounds.x,
      matched.bounds.y,
      matched.bounds.width,
      matched.bounds.height
    ));
  }
  lines.push(format!("matchCount={}", snapshot.matches.len()));
  lines.join("\n") + "\n"
}

pub(crate) fn render_visual_rows_report(
  capture: NativeVisualRowsCapture,
  rows: &[ObservedOcrRow],
) -> String {
  let mut lines = vec![
    format!("detectedAt={}", capture.detected_at),
    format!("imagePath={}", capture.image_path.display()),
    format!("imageWidth={}", capture.image_width),
    format!("imageHeight={}", capture.image_height),
    "rowStrategy=visual-bands".to_string(),
  ];
  if let Some(crop) = capture.crop_rect.as_ref() {
    lines.push(format!(
      "cropRect={},{},{},{}",
      crop.x, crop.y, crop.width, crop.height
    ));
  }
  lines.push(format!(
    "analysisStrip={},{},{},{}",
    capture.analysis_strip.x,
    capture.analysis_strip.y,
    capture.analysis_strip.width,
    capture.analysis_strip.height
  ));
  for (index, row) in rows.iter().enumerate() {
    let peak_density = capture.peak_densities.get(index).copied().unwrap_or(0.0);
    lines.push(format!(
      "row\t{}\t{}\t{}\t{}\t{}\t{:.6}",
      row.row_index, row.bounds.x, row.bounds.y, row.bounds.width, row.bounds.height, peak_density
    ));
  }
  lines.push(format!("rowCount={}", rows.len()));
  lines.join("\n") + "\n"
}

#[derive(Clone, Debug)]
pub(crate) struct DecodedOcrTextResponse {
  pub(crate) recognized_at: String,
  pub(crate) image_path: String,
  pub(crate) image_width: i64,
  pub(crate) image_height: i64,
  pub(crate) query: String,
  pub(crate) exact: bool,
  pub(crate) case_sensitive: bool,
  pub(crate) normalized_query: String,
  pub(crate) crop_enabled: bool,
  pub(crate) crop_x: i64,
  pub(crate) crop_y: i64,
  pub(crate) crop_width: i64,
  pub(crate) crop_height: i64,
  pub(crate) ocr_scale_factor: f64,
  pub(crate) match_indices: Vec<i64>,
  pub(crate) texts: Vec<String>,
  pub(crate) confidences: Vec<f64>,
  pub(crate) x_values: Vec<i64>,
  pub(crate) y_values: Vec<i64>,
  pub(crate) width_values: Vec<i64>,
  pub(crate) height_values: Vec<i64>,
  pub(crate) error_message: Option<String>,
  pub(crate) recovery_hint: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct DecodedVisualRowsResponse {
  pub(crate) detected_at: String,
  pub(crate) image_path: String,
  pub(crate) image_width: i64,
  pub(crate) image_height: i64,
  pub(crate) crop_enabled: bool,
  pub(crate) crop_x: i64,
  pub(crate) crop_y: i64,
  pub(crate) crop_width: i64,
  pub(crate) crop_height: i64,
  pub(crate) analysis_strip_x: i64,
  pub(crate) analysis_strip_y: i64,
  pub(crate) analysis_strip_width: i64,
  pub(crate) analysis_strip_height: i64,
  pub(crate) row_indices: Vec<i64>,
  pub(crate) x_values: Vec<i64>,
  pub(crate) y_values: Vec<i64>,
  pub(crate) width_values: Vec<i64>,
  pub(crate) height_values: Vec<i64>,
  pub(crate) peak_densities: Vec<f64>,
  pub(crate) error_message: Option<String>,
  pub(crate) recovery_hint: Option<String>,
}

#[cfg(target_os = "macos")]
impl From<NativeOcrTextResponse> for DecodedOcrTextResponse {
  fn from(value: NativeOcrTextResponse) -> Self {
    Self {
      recognized_at: value.recognized_at,
      image_path: value.image_path,
      image_width: value.image_width,
      image_height: value.image_height,
      query: value.query,
      exact: value.exact,
      case_sensitive: value.case_sensitive,
      normalized_query: value.normalized_query,
      crop_enabled: value.crop_enabled,
      crop_x: value.crop_x,
      crop_y: value.crop_y,
      crop_width: value.crop_width,
      crop_height: value.crop_height,
      ocr_scale_factor: value.ocr_scale_factor,
      match_indices: value.match_indices,
      texts: value.texts,
      confidences: value.confidences,
      x_values: value.x_values,
      y_values: value.y_values,
      width_values: value.width_values,
      height_values: value.height_values,
      error_message: value.error_message,
      recovery_hint: value.recovery_hint,
    }
  }
}

#[cfg(target_os = "macos")]
impl From<NativeVisualRowsResponse> for DecodedVisualRowsResponse {
  fn from(value: NativeVisualRowsResponse) -> Self {
    Self {
      detected_at: value.detected_at,
      image_path: value.image_path,
      image_width: value.image_width,
      image_height: value.image_height,
      crop_enabled: value.crop_enabled,
      crop_x: value.crop_x,
      crop_y: value.crop_y,
      crop_width: value.crop_width,
      crop_height: value.crop_height,
      analysis_strip_x: value.analysis_strip_x,
      analysis_strip_y: value.analysis_strip_y,
      analysis_strip_width: value.analysis_strip_width,
      analysis_strip_height: value.analysis_strip_height,
      row_indices: value.row_indices,
      x_values: value.x_values,
      y_values: value.y_values,
      width_values: value.width_values,
      height_values: value.height_values,
      peak_densities: value.peak_densities,
      error_message: value.error_message,
      recovery_hint: value.recovery_hint,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn decode_ocr_text_rejects_mismatched_match_vectors() {
    let error = decode_ocr_text_response(DecodedOcrTextResponse {
      recognized_at: "2026-05-20T00:00:00Z".to_string(),
      image_path: "/tmp/sample.png".to_string(),
      image_width: 100,
      image_height: 100,
      query: "play".to_string(),
      exact: false,
      case_sensitive: false,
      normalized_query: "play".to_string(),
      crop_enabled: false,
      crop_x: 0,
      crop_y: 0,
      crop_width: 0,
      crop_height: 0,
      ocr_scale_factor: 1.0,
      match_indices: vec![0, 1],
      texts: vec!["Play".to_string(), "Pause".to_string()],
      confidences: vec![0.99],
      x_values: vec![1, 2],
      y_values: vec![3, 4],
      width_values: vec![5, 6],
      height_values: vec![7, 8],
      error_message: None,
      recovery_hint: None,
    })
    .unwrap_err();

    assert!(error.contains("mismatched OCR match vector lengths"));
  }

  #[test]
  fn decode_visual_rows_preserves_row_order() {
    let capture = decode_visual_rows_response(DecodedVisualRowsResponse {
      detected_at: "2026-05-20T00:00:00Z".to_string(),
      image_path: "/tmp/sample.png".to_string(),
      image_width: 300,
      image_height: 300,
      crop_enabled: false,
      crop_x: 0,
      crop_y: 0,
      crop_width: 0,
      crop_height: 0,
      analysis_strip_x: 0,
      analysis_strip_y: 0,
      analysis_strip_width: 100,
      analysis_strip_height: 300,
      row_indices: vec![0, 1],
      x_values: vec![10, 10],
      y_values: vec![100, 200],
      width_values: vec![80, 80],
      height_values: vec![30, 30],
      peak_densities: vec![0.2, 0.3],
      error_message: None,
      recovery_hint: None,
    })
    .unwrap();

    assert_eq!(capture.rows.rows[0].bounds.y, 100);
    assert_eq!(capture.rows.rows[1].bounds.y, 200);
  }
}
