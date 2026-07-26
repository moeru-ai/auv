use super::super::types::{CaptureBackend, Rect, Scale2D, Size};
use super::*;

fn sample_display_contract() -> CaptureContract {
  CaptureContract {
    coordinate_contract_version: 1,
    capture_source: CaptureSource::Display {
      display_ref: "display_0".to_string(),
      native_display_id: "main-native-display".to_string(),
    },
    capture_backend: CaptureBackend::XcapMacos,
    include_shadow: false,
    source_global_logical_bounds: Rect {
      x: 0.0,
      y: 0.0,
      width: 3008.0,
      height: 1692.0,
    },
    source_physical_pixel_bounds: Rect {
      x: 0.0,
      y: 0.0,
      width: 6016.0,
      height: 3384.0,
    },
    screenshot_pixel_size: Size {
      width: 6016.0,
      height: 3384.0,
    },
    pixel_to_logical_scale: Scale2D { x: 0.5, y: 0.5 },
    logical_to_pixel_scale: Scale2D { x: 2.0, y: 2.0 },
    captured_at_unix_ms: 1779090000000,
  }
}

#[test]
fn json_contains_contract_fields() {
  let rendered = render_capture_contract_json(&sample_display_contract()).unwrap();

  assert!(rendered.contains("\"coordinate_contract_version\": 1"));
  assert!(rendered.contains("\"display_ref\": \"display_0\""));
  assert!(rendered.contains("\"native_display_id\": \"main-native-display\""));
  assert!(rendered.contains("\"pixel_to_logical_scale\""));
}

#[test]
fn text_report_is_human_readable() {
  let rendered = render_capture_contract_text(&sample_display_contract());

  assert!(rendered.contains("captureSource=display:display_0"));
  assert!(rendered.contains("nativeDisplayId=main-native-display"));
  assert!(rendered.contains("includeShadow=false"));
  assert!(rendered.contains("screenshotPixels=6016x3384"));
}
