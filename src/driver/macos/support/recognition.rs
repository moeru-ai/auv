// File: src/driver/macos/support/recognition.rs
use std::path::Path;

use serde_json::json;

use super::super::*;
use crate::contract::{
  ArtifactRef, RatioRegion, RecognitionBox, RecognitionResult, RecognitionScope, RecognitionSource,
  RecognitionSurface, RecognizedItem,
};
use crate::driver::macos::capture::types::{CaptureContract, CaptureSource};
use crate::model::AuvResult;

#[derive(Clone, Debug)]
pub(crate) struct RowRecognitionArtifactRequest<'a> {
  pub(crate) recognition_id: String,
  pub(crate) source: RecognitionSource,
  pub(crate) surface: RecognitionSurface,
  pub(crate) rows: &'a [ObservedOcrRow],
  pub(crate) strategy: &'a str,
  pub(crate) raw_match_count: usize,
  pub(crate) filtered_match_count: usize,
  pub(crate) screenshot_path: &'a Path,
  pub(crate) screenshot_dimensions: &'a ScreenshotDimensions,
  pub(crate) display_ref: Option<&'a str>,
  pub(crate) native_display_id: Option<&'a str>,
  pub(crate) app_bundle_id: Option<&'a str>,
  pub(crate) window_title: Option<&'a str>,
  pub(crate) window_number: Option<i64>,
  pub(crate) region_hint: Option<RatioRegion>,
  pub(crate) capture_contract: Option<&'a CaptureContract>,
  /// `ArtifactRef` pointing at the screenshot artifact this recognition is
  /// derived from. When provided, it is set on `scope.capture_artifact` and
  /// pushed into `evidence` so consumers can traverse from the recognition
  /// back to the source capture. `None` produces the legacy "evidence-empty"
  /// shape for callers that haven't been wired through `DriverArtifactBuilder`
  /// yet.
  pub(crate) capture_artifact: Option<ArtifactRef>,
  pub(crate) additional_detail: serde_json::Value,
  pub(crate) known_limits: Vec<String>,
}

pub(crate) fn row_recognition_artifact(
  kind: &str,
  label: &str,
  note: &str,
  request: RowRecognitionArtifactRequest<'_>,
) -> AuvResult<ProducedArtifact> {
  let json = render_row_recognition_result(request)?;
  build_text_artifact(kind, "json", label, json, note)
}

pub(crate) fn render_row_recognition_result(
  request: RowRecognitionArtifactRequest<'_>,
) -> AuvResult<String> {
  let best = if request.rows.len() == 1 {
    request.rows.first().map(|row| recognized_row_item(row))
  } else {
    None
  };
  let filtered = request
    .rows
    .iter()
    .map(recognized_row_item)
    .collect::<Vec<_>>();
  let all = filtered.clone();
  let detail = json!({
    "provider": "macos.row_detection",
    "strategy": request.strategy,
    "raw_match_count": request.raw_match_count,
    "filtered_match_count": request.filtered_match_count,
    "row_count": request.rows.len(),
    "screenshot": {
      "path": request.screenshot_path.display().to_string(),
      "width": request.screenshot_dimensions.width,
      "height": request.screenshot_dimensions.height,
    },
    "capture_contract": request.capture_contract.map(capture_contract_detail),
    "provider_detail": request.additional_detail,
  });
  let evidence = match request.capture_artifact.as_ref() {
    Some(reference) => vec![reference.clone()],
    None => Vec::new(),
  };
  let result = RecognitionResult {
    recognition_id: request.recognition_id,
    source: request.source,
    scope: RecognitionScope {
      surface: request.surface,
      display_ref: request.display_ref.map(str::to_string),
      native_display_id: request.native_display_id.map(str::to_string),
      app_bundle_id: request.app_bundle_id.map(str::to_string),
      window_title: request.window_title.map(str::to_string),
      window_number: request.window_number,
      region_hint: request.region_hint,
      capture_artifact: request.capture_artifact,
      capture_contract_artifact: None,
    },
    best: best.clone(),
    filtered,
    all,
    detail,
    evidence,
    known_limits: request.known_limits,
  };

  serde_json::to_string_pretty(&result)
    .map(|mut rendered| {
      rendered.push('\n');
      rendered
    })
    .map_err(|error| format!("failed to encode row recognition result JSON: {error}"))
}

pub(crate) fn recognition_source_for_rows(
  strategy: &str,
  rows: &[ObservedOcrRow],
) -> RecognitionSource {
  if strategy == "ocr-text" || rows.iter().all(|row| row.source == "ocr-text") {
    RecognitionSource::OcrRow
  } else {
    RecognitionSource::VisualRow
  }
}

pub(crate) fn observed_rect_to_ratio_region(
  rect: &ObservedRect,
  dimensions: &ScreenshotDimensions,
) -> RatioRegion {
  RatioRegion {
    left: rect.x as f64 / dimensions.width as f64,
    top: rect.y as f64 / dimensions.height as f64,
    right: (rect.x + rect.width) as f64 / dimensions.width as f64,
    bottom: (rect.y + rect.height) as f64 / dimensions.height as f64,
  }
}

fn recognized_row_item(row: &ObservedOcrRow) -> RecognizedItem {
  RecognizedItem {
    item_id: format!("row#{}", row.row_index + 1),
    kind: "row".to_string(),
    box_: recognition_box(&row.bounds),
    text: joined_row_text(row),
    provider_score: None,
    detail: json!({
      "row_index": row.row_index,
      "source": row.source,
      "text_fragments": row.text_fragments,
    }),
  }
}

fn joined_row_text(row: &ObservedOcrRow) -> Option<String> {
  let joined = row
    .text_fragments
    .iter()
    .map(|fragment| fragment.trim())
    .filter(|fragment| !fragment.is_empty())
    .collect::<Vec<_>>()
    .join(" | ");
  if joined.is_empty() {
    None
  } else {
    Some(joined)
  }
}

fn recognition_box(bounds: &ObservedRect) -> RecognitionBox {
  RecognitionBox {
    x: bounds.x,
    y: bounds.y,
    width: bounds.width,
    height: bounds.height,
  }
}

fn capture_contract_detail(contract: &CaptureContract) -> serde_json::Value {
  match &contract.capture_source {
    CaptureSource::Display {
      display_ref,
      native_display_id,
    } => json!({
      "source_kind": "display",
      "display_ref": display_ref,
      "native_display_id": native_display_id,
      "source_global_logical_bounds": contract.source_global_logical_bounds,
      "source_physical_pixel_bounds": contract.source_physical_pixel_bounds,
      "screenshot_pixel_size": contract.screenshot_pixel_size,
      "captured_at_unix_ms": contract.captured_at_unix_ms,
    }),
    CaptureSource::Region {
      display_ref,
      native_display_id,
      input_space,
    } => json!({
      "source_kind": "region",
      "display_ref": display_ref,
      "native_display_id": native_display_id,
      "input_space": input_space,
      "source_global_logical_bounds": contract.source_global_logical_bounds,
      "source_physical_pixel_bounds": contract.source_physical_pixel_bounds,
      "screenshot_pixel_size": contract.screenshot_pixel_size,
      "captured_at_unix_ms": contract.captured_at_unix_ms,
    }),
    CaptureSource::Window {
      window_ref,
      display_ref,
      native_window_id,
      native_display_id,
    } => json!({
      "source_kind": "window",
      "window_ref": window_ref,
      "display_ref": display_ref,
      "native_window_id": native_window_id,
      "native_display_id": native_display_id,
      "source_global_logical_bounds": contract.source_global_logical_bounds,
      "source_physical_pixel_bounds": contract.source_physical_pixel_bounds,
      "screenshot_pixel_size": contract.screenshot_pixel_size,
      "captured_at_unix_ms": contract.captured_at_unix_ms,
    }),
  }
}

pub(crate) fn window_number_from_ref(window_ref: &str) -> Option<i64> {
  let trimmed = window_ref.trim();
  if trimmed.is_empty() {
    return None;
  }
  if let Ok(number) = trimmed.parse::<i64>() {
    return Some(number);
  }
  trimmed
    .strip_prefix("window_")
    .and_then(|suffix| suffix.parse::<i64>().ok())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::driver::macos::capture::types::{
    CaptureBackend, CoordinateSpace, Rect, Scale2D, Size,
  };

  fn sample_rows() -> Vec<ObservedOcrRow> {
    vec![
      ObservedOcrRow {
        row_index: 0,
        source: "ocr-text".to_string(),
        bounds: ObservedRect {
          x: 100,
          y: 200,
          width: 640,
          height: 96,
        },
        text_fragments: vec!["Song A".to_string(), "Artist A".to_string()],
      },
      ObservedOcrRow {
        row_index: 1,
        source: "ocr-text".to_string(),
        bounds: ObservedRect {
          x: 100,
          y: 320,
          width: 640,
          height: 96,
        },
        text_fragments: vec!["Song B".to_string()],
      },
    ]
  }

  fn sample_capture_contract() -> CaptureContract {
    CaptureContract {
      coordinate_contract_version: 1,
      capture_source: CaptureSource::Display {
        display_ref: "display_1".to_string(),
        native_display_id: "2".to_string(),
      },
      capture_backend: CaptureBackend::XcapMacos,
      include_shadow: false,
      source_global_logical_bounds: Rect {
        x: 0.0,
        y: 0.0,
        width: 1512.0,
        height: 982.0,
      },
      source_physical_pixel_bounds: Rect {
        x: 0.0,
        y: 0.0,
        width: 3024.0,
        height: 1964.0,
      },
      screenshot_pixel_size: Size {
        width: 3024.0,
        height: 1964.0,
      },
      pixel_to_logical_scale: Scale2D { x: 0.5, y: 0.5 },
      logical_to_pixel_scale: Scale2D { x: 2.0, y: 2.0 },
      captured_at_unix_ms: 1779090000000,
    }
  }

  #[test]
  fn render_row_recognition_result_encodes_best_filtered_all_and_scope() {
    let rows = vec![sample_rows()[0].clone()];
    let json = render_row_recognition_result(RowRecognitionArtifactRequest {
      recognition_id: "recognition_screen_rows".to_string(),
      source: RecognitionSource::OcrRow,
      surface: RecognitionSurface::Display,
      rows: &rows,
      strategy: "ocr-text",
      raw_match_count: 3,
      filtered_match_count: 2,
      screenshot_path: Path::new("/tmp/screen.png"),
      screenshot_dimensions: &ScreenshotDimensions {
        width: 3024,
        height: 1964,
      },
      display_ref: Some("display_1"),
      native_display_id: Some("2"),
      app_bundle_id: None,
      window_title: None,
      window_number: None,
      region_hint: None,
      capture_contract: Some(&sample_capture_contract()),
      capture_artifact: None,
      additional_detail: json!({ "note": "sample" }),
      known_limits: vec!["single row sample".to_string()],
    })
    .expect("json should render");

    let value: serde_json::Value = serde_json::from_str(&json).expect("json should parse");
    assert_eq!(value["source"], json!("ocr_row"));
    assert_eq!(value["scope"]["surface"], json!("display"));
    assert_eq!(value["best"]["box"]["x"], json!(100));
    assert_eq!(value["filtered"][0]["detail"]["row_index"], json!(0));
    assert_eq!(value["all"][0]["text"], json!("Song A | Artist A"));
    assert_eq!(value["detail"]["provider"], json!("macos.row_detection"));
    assert_eq!(
      value["detail"]["capture_contract"]["source_kind"],
      json!("display")
    );
  }

  #[test]
  fn render_row_recognition_result_populates_evidence_when_capture_artifact_given() {
    use crate::trace::{ArtifactId, RunId, SpanId};

    let capture_ref = ArtifactRef {
      run_id: RunId::new("run_42"),
      artifact_id: ArtifactId::new("artifact_0001"),
      span_id: SpanId::new("span_7"),
      captured_event_id: None,
    };
    let rows = sample_rows();
    let json = render_row_recognition_result(RowRecognitionArtifactRequest {
      recognition_id: "recognition_with_evidence".to_string(),
      source: RecognitionSource::OcrRow,
      surface: RecognitionSurface::Window,
      rows: &rows,
      strategy: "ocr-text",
      raw_match_count: 2,
      filtered_match_count: 2,
      screenshot_path: Path::new("/tmp/screen.png"),
      screenshot_dimensions: &ScreenshotDimensions {
        width: 1440,
        height: 900,
      },
      display_ref: None,
      native_display_id: None,
      app_bundle_id: None,
      window_title: None,
      window_number: None,
      region_hint: None,
      capture_contract: None,
      capture_artifact: Some(capture_ref.clone()),
      additional_detail: json!({}),
      known_limits: Vec::new(),
    })
    .expect("json should render");

    let value: serde_json::Value = serde_json::from_str(&json).expect("json should parse");
    assert_eq!(
      value["scope"]["capture_artifact"]["run_id"],
      json!("run_42")
    );
    assert_eq!(
      value["scope"]["capture_artifact"]["artifact_id"],
      json!("artifact_0001")
    );
    let evidence = value["evidence"].as_array().expect("evidence is array");
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0]["artifact_id"], json!("artifact_0001"));
  }

  #[test]
  fn render_row_recognition_result_leaves_best_empty_for_multi_row_observation() {
    let rows = sample_rows();
    let json = render_row_recognition_result(RowRecognitionArtifactRequest {
      recognition_id: "recognition_window_rows".to_string(),
      source: RecognitionSource::VisualRow,
      surface: RecognitionSurface::Window,
      rows: &rows,
      strategy: "visual-bands+ocr-text",
      raw_match_count: 5,
      filtered_match_count: 5,
      screenshot_path: Path::new("/tmp/window.png"),
      screenshot_dimensions: &ScreenshotDimensions {
        width: 1440,
        height: 900,
      },
      display_ref: Some("display_1"),
      native_display_id: Some("2"),
      app_bundle_id: Some("com.example.music"),
      window_title: None,
      window_number: Some(42),
      region_hint: Some(RatioRegion {
        left: 0.2,
        top: 0.3,
        right: 0.8,
        bottom: 0.9,
      }),
      capture_contract: None,
      capture_artifact: None,
      additional_detail: json!({
        "region_semantics": "window_rows"
      }),
      known_limits: Vec::new(),
    })
    .expect("json should render");

    let value: serde_json::Value = serde_json::from_str(&json).expect("json should parse");
    assert!(value["best"].is_null());
    assert_eq!(value["filtered"].as_array().unwrap().len(), 2);
    assert_eq!(value["all"].as_array().unwrap().len(), 2);
    assert_eq!(value["scope"]["window_number"], json!(42));
    assert_eq!(value["scope"]["region_hint"]["left"], json!(0.2));
  }

  #[test]
  fn recognition_source_for_rows_preserves_visual_rows_with_attached_text() {
    let rows = vec![ObservedOcrRow {
      row_index: 0,
      source: "visual-bands+ocr-text".to_string(),
      bounds: ObservedRect {
        x: 0,
        y: 0,
        width: 10,
        height: 10,
      },
      text_fragments: vec!["row".to_string()],
    }];

    assert_eq!(
      recognition_source_for_rows("visual-bands+ocr-text", &rows),
      RecognitionSource::VisualRow
    );
    assert_eq!(
      recognition_source_for_rows("ocr-text", &rows),
      RecognitionSource::OcrRow
    );
  }

  #[test]
  fn observed_rect_to_ratio_region_projects_pixels_back_to_ratios() {
    let ratio = observed_rect_to_ratio_region(
      &ObservedRect {
        x: 100,
        y: 50,
        width: 400,
        height: 150,
      },
      &ScreenshotDimensions {
        width: 1000,
        height: 500,
      },
    );

    assert_eq!(ratio.left, 0.1);
    assert_eq!(ratio.top, 0.1);
    assert_eq!(ratio.right, 0.5);
    assert_eq!(ratio.bottom, 0.4);
  }

  #[test]
  fn window_number_from_ref_accepts_plain_and_prefixed_window_ids() {
    assert_eq!(window_number_from_ref("42"), Some(42));
    assert_eq!(window_number_from_ref("window_42"), Some(42));
    assert_eq!(window_number_from_ref("window_main"), None);
    assert_eq!(window_number_from_ref(""), None);
  }

  #[test]
  fn capture_contract_detail_preserves_region_source_metadata() {
    let value = capture_contract_detail(&CaptureContract {
      coordinate_contract_version: 1,
      capture_source: CaptureSource::Region {
        display_ref: "display_2".to_string(),
        native_display_id: "7".to_string(),
        input_space: CoordinateSpace::DisplayLogical,
      },
      capture_backend: CaptureBackend::XcapMacos,
      include_shadow: false,
      source_global_logical_bounds: Rect {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
      },
      source_physical_pixel_bounds: Rect {
        x: 5.0,
        y: 6.0,
        width: 7.0,
        height: 8.0,
      },
      screenshot_pixel_size: Size {
        width: 9.0,
        height: 10.0,
      },
      pixel_to_logical_scale: Scale2D { x: 1.0, y: 1.0 },
      logical_to_pixel_scale: Scale2D { x: 1.0, y: 1.0 },
      captured_at_unix_ms: 1,
    });

    assert_eq!(value["source_kind"], json!("region"));
    assert_eq!(value["input_space"], json!("display_logical"));
  }
}
