use std::collections::BTreeMap;

use image::RgbaImage;

use super::*;
use crate::InvokeCancellation;

fn inputs(values: [(&str, &str); 4]) -> BTreeMap<String, String> {
  values.into_iter().map(|(name, value)| (name.to_string(), value.to_string())).collect()
}

#[test]
fn capture_region_validates_the_same_region_before_dry_and_live_branches() {
  let valid_dry_run = InvokeCommandInput {
    command_id: "screen.captureRegion".to_string(),
    target_application_id: None,
    inputs: inputs([("x", "1"), ("y", "2"), ("width", "3"), ("height", "4")]),
    typed_args: None,
    dry_run: true,
    cancellation: InvokeCancellation::new(),
  };
  assert!(futures_executor::block_on(capture_region_invoke_command().invoke(valid_dry_run)).is_ok());

  let invalid_live = InvokeCommandInput {
    command_id: "screen.captureRegion".to_string(),
    target_application_id: None,
    inputs: inputs([("x", "1"), ("y", "2"), ("width", "0"), ("height", "4")]),
    typed_args: None,
    dry_run: false,
    cancellation: InvokeCancellation::new(),
  };
  let error = futures_executor::block_on(capture_region_invoke_command().invoke(invalid_live))
    .expect_err("invalid live region must fail before capture");
  assert!(error.contains("greater than zero"));
}

#[cfg(target_os = "macos")]
#[test]
fn screen_text_output_returns_typed_ocr_matches() {
  let matches = auv_driver::OcrMatches {
    matches: vec![auv_driver::OcrMatch {
      text: "Pause".to_string(),
      confidence: 0.95,
      bounds: auv_driver::Rect::new(10.0, 20.0, 80.0, 24.0),
    }],
  };

  let output = screen_text_matches_output(&matches).expect("OCR result should serialize");

  assert_eq!(output.result(), Some(&serde_json::to_value(&matches).expect("fixture should serialize")));
}

#[test]
fn region_capture_result_keeps_pixels_out_of_json() {
  let capture = auv_driver::RegionCapture {
    display: auv_driver::Display {
      id: "display_1".to_string(),
      name: None,
      frame: auv_driver::Rect::new(0.0, 0.0, 1920.0, 1080.0),
      coordinate_space: auv_driver::CoordinateSpace::Screen,
      scale_factor: 1.0,
      is_primary: false,
      is_builtin: Some(false),
    },
    capture: auv_driver::Capture {
      image: RgbaImage::new(320, 180),
      bounds: auv_driver::Rect::new(100.0, 120.0, 320.0, 180.0),
      scale_factor: 1.0,
      backend: "fixture-region".to_string(),
      fallback_reason: Some("fixture fallback".to_string()),
    },
  };

  let output = region_capture_output(&capture, None).expect("region result should serialize");
  let result = output.result().expect("capture should have a result");

  assert_eq!(result["display"]["id"], "display_1");
  assert_eq!(result["capture"]["bounds"]["origin"]["x"], 100.0);
  assert_eq!(result["capture"]["pixel_dimensions"]["width"], 320);
  assert_eq!(result["capture"]["backend"], "fixture-region");
  assert_eq!(result["capture"]["fallback_reason"], "fixture fallback");
  assert!(result.get("image").is_none());
}

#[test]
fn screen_text_click_result_keeps_resolution_and_delivery_together() {
  let click = ScreenTextClick {
    matches: auv_driver::OcrMatches {
      matches: vec![auv_driver::OcrMatch {
        text: "Pause".to_string(),
        confidence: 0.97,
        bounds: auv_driver::Rect::new(40.0, 50.0, 70.0, 20.0),
      }],
    },
    point: auv_driver::Point::new(75.0, 60.0),
    action: auv_driver::InputActionResult::single_success(auv_driver::InputDeliveryPath::ForegroundSystemEvents),
  };

  let output = screen_text_click_output(&click).expect("screen click result should serialize");
  let result = output.result().expect("click should have a result");

  assert_eq!(result["matches"]["matches"][0]["text"], "Pause");
  assert_eq!(result["point"]["x"], 75.0);
  assert_eq!(result["action"]["selected_path"], "foreground_system_events");
  let report = output.report.as_ref().expect("screen click report");
  assert_eq!(report_field(report, "Delivery"), "delivered");
  assert_eq!(report_field(report, "Verification"), "delivery_only");
  assert_eq!(report_field(report, "Path"), "foreground_system_events");
}

fn report_field<'a>(report: &'a InvokeReport, label: &str) -> &'a str {
  report.fields.iter().find(|field| field.label == label).map(|field| field.value.as_str()).expect("report field")
}
