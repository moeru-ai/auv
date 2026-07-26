// File: src/driver/macos/native/ocr.rs
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use super::binding::ffi::{
  NativeOcrRgbaRequest, NativeOcrTextRequest, NativeOcrTextResponse, NativeVisualRowsRequest, NativeVisualRowsResponse, find_ocr_text,
  find_ocr_text_rgba, find_visual_rows,
};
use super::types::{AuvResult, DetectedScreenRows, ObservedOcrRow, ObservedRect, OcrTextMatch, OcrTextSnapshot};

#[derive(Clone, Debug, PartialEq)]
pub struct NativeOcrTextCapture {
  pub snapshot: OcrTextSnapshot,
  pub normalized_query: String,
  pub crop_rect: Option<ObservedRect>,
  pub ocr_scale_factor: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeVisualRowsCapture {
  pub rows: DetectedScreenRows,
  pub detected_at: String,
  pub image_path: PathBuf,
  pub image_width: i64,
  pub image_height: i64,
  pub crop_rect: Option<ObservedRect>,
  pub analysis_strip: ObservedRect,
  pub peak_densities: Vec<f64>,
}

#[cfg(target_os = "macos")]
#[allow(clippy::too_many_arguments)]
pub fn find_text(
  image_path: &Path,
  query: &str,
  exact: bool,
  case_sensitive: bool,
  max_observations: i64,
  custom_words: &[String],
  recognition_languages: Option<&[String]>,
  crop_region: Option<&ObservedRect>,
) -> AuvResult<NativeOcrTextCapture> {
  let crop = crop_region.cloned().unwrap_or(ObservedRect {
    x: 0,
    y: 0,
    width: 0,
    height: 0,
  });
  decode_ocr_text_response(DecodedOcrTextResponse::from(find_ocr_text(NativeOcrTextRequest {
    image_path: image_path.display().to_string(),
    query: query.to_string(),
    exact,
    case_sensitive,
    max_observations,
    custom_words: custom_words.to_vec(),
    recognition_languages: recognition_languages.map(<[String]>::to_vec).unwrap_or_default(),
    crop_enabled: crop_region.is_some(),
    crop_x: crop.x,
    crop_y: crop.y,
    crop_width: crop.width,
    crop_height: crop.height,
  })))
}

#[cfg(target_os = "macos")]
#[allow(clippy::too_many_arguments)]
pub fn find_text_in_rgba(
  rgba_bytes: Vec<u8>,
  image_width: i64,
  image_height: i64,
  query: &str,
  exact: bool,
  case_sensitive: bool,
  max_observations: i64,
  custom_words: &[String],
  recognition_languages: Option<&[String]>,
  crop_region: Option<&ObservedRect>,
) -> AuvResult<NativeOcrTextCapture> {
  let crop = crop_region.cloned().unwrap_or(ObservedRect {
    x: 0,
    y: 0,
    width: 0,
    height: 0,
  });
  decode_ocr_text_response(DecodedOcrTextResponse::from(find_ocr_text_rgba(NativeOcrRgbaRequest {
    image_width,
    image_height,
    rgba_bytes,
    query: query.to_string(),
    exact,
    case_sensitive,
    max_observations,
    custom_words: custom_words.to_vec(),
    recognition_languages: recognition_languages.map(<[String]>::to_vec).unwrap_or_default(),
    crop_enabled: crop_region.is_some(),
    crop_x: crop.x,
    crop_y: crop.y,
    crop_width: crop.width,
    crop_height: crop.height,
  })))
}

#[cfg(not(target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub fn find_text_in_rgba(
  _rgba_bytes: Vec<u8>,
  _image_width: i64,
  _image_height: i64,
  _query: &str,
  _exact: bool,
  _case_sensitive: bool,
  _max_observations: i64,
  _custom_words: &[String],
  _recognition_languages: Option<&[String]>,
  _crop_region: Option<&ObservedRect>,
) -> AuvResult<NativeOcrTextCapture> {
  Err("macOS native OCR text detection is unsupported on this target".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn find_text(
  _image_path: &Path,
  _query: &str,
  _exact: bool,
  _case_sensitive: bool,
  _max_observations: i64,
  _custom_words: &[String],
  _recognition_languages: Option<&[String]>,
  _crop_region: Option<&ObservedRect>,
) -> AuvResult<NativeOcrTextCapture> {
  Err("macOS native OCR text detection is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn find_rows(image_path: &Path, crop_region: Option<&ObservedRect>) -> AuvResult<NativeVisualRowsCapture> {
  let crop = crop_region.cloned().unwrap_or(ObservedRect {
    x: 0,
    y: 0,
    width: 0,
    height: 0,
  });
  decode_visual_rows_response(DecodedVisualRowsResponse::from(find_visual_rows(NativeVisualRowsRequest {
    image_path: image_path.display().to_string(),
    crop_enabled: crop_region.is_some(),
    crop_x: crop.x,
    crop_y: crop.y,
    crop_width: crop.width,
    crop_height: crop.height,
  })))
}

#[cfg(not(target_os = "macos"))]
pub fn find_rows(_image_path: &Path, _crop_region: Option<&ObservedRect>) -> AuvResult<NativeVisualRowsCapture> {
  Err("macOS native visual row detection is unsupported on this target".to_string())
}

pub fn decode_ocr_text_response(response: DecodedOcrTextResponse) -> AuvResult<NativeOcrTextCapture> {
  if response.error_message.is_some() {
    return super::error::native_result("find_ocr_text", None, response.error_message, response.recovery_hint);
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
      let match_index = usize::try_from(response.match_indices[index])
        .map_err(|error| format!("native OCR text response had invalid match index {}: {error}", response.match_indices[index]))?;
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

pub fn decode_visual_rows_response(response: DecodedVisualRowsResponse) -> AuvResult<NativeVisualRowsCapture> {
  if response.error_message.is_some() {
    return super::error::native_result("find_visual_rows", None, response.error_message, response.recovery_hint);
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
      let row_index = usize::try_from(response.row_indices[index])
        .map_err(|error| format!("native visual rows response had invalid row index {}: {error}", response.row_indices[index]))?;
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
  Ok(NativeVisualRowsCapture {
    rows: DetectedScreenRows {
      strategy: "visual-bands".to_string(),
      raw_match_count: 0,
      filtered_match_count: 0,
      rows,
    },
    detected_at: response.detected_at,
    image_path: PathBuf::from(response.image_path),
    image_width: response.image_width,
    image_height: response.image_height,
    crop_rect,
    analysis_strip,
    peak_densities: response.peak_densities,
  })
}

#[derive(Clone, Debug)]
pub struct DecodedOcrTextResponse {
  pub recognized_at: String,
  pub image_path: String,
  pub image_width: i64,
  pub image_height: i64,
  pub query: String,
  pub exact: bool,
  pub case_sensitive: bool,
  pub normalized_query: String,
  pub crop_enabled: bool,
  pub crop_x: i64,
  pub crop_y: i64,
  pub crop_width: i64,
  pub crop_height: i64,
  pub ocr_scale_factor: f64,
  pub match_indices: Vec<i64>,
  pub texts: Vec<String>,
  pub confidences: Vec<f64>,
  pub x_values: Vec<i64>,
  pub y_values: Vec<i64>,
  pub width_values: Vec<i64>,
  pub height_values: Vec<i64>,
  pub error_message: Option<String>,
  pub recovery_hint: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DecodedVisualRowsResponse {
  pub detected_at: String,
  pub image_path: String,
  pub image_width: i64,
  pub image_height: i64,
  pub crop_enabled: bool,
  pub crop_x: i64,
  pub crop_y: i64,
  pub crop_width: i64,
  pub crop_height: i64,
  pub analysis_strip_x: i64,
  pub analysis_strip_y: i64,
  pub analysis_strip_width: i64,
  pub analysis_strip_height: i64,
  pub row_indices: Vec<i64>,
  pub x_values: Vec<i64>,
  pub y_values: Vec<i64>,
  pub width_values: Vec<i64>,
  pub height_values: Vec<i64>,
  pub peak_densities: Vec<f64>,
  pub error_message: Option<String>,
  pub recovery_hint: Option<String>,
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
#[path = "ocr_test.rs"]
mod tests;
