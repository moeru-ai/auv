use auv_driver::{CoordinateSpace, Rect, Window, WindowRef};
use image::RgbaImage;

use super::*;

#[test]
fn window_list_report_uses_human_first_table_and_wide_diagnostic_columns() {
  let windows = vec![
    Window {
      reference: WindowRef {
        id: "window_10".to_string(),
      },
      title: Some("Project Notes".to_string()),
      app_name: Some("TextEdit".to_string()),
      app_bundle_id: Some("com.apple.TextEdit".to_string()),
      process_id: Some(1234),
      frame: Rect::new(12.0, 34.0, 640.0, 480.0),
      coordinate_space: CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    },
    Window {
      reference: WindowRef {
        id: "window_11".to_string(),
      },
      title: None,
      app_name: None,
      app_bundle_id: None,
      process_id: None,
      frame: Rect::new(-100.0, 20.0, 300.0, 200.0),
      coordinate_space: CoordinateSpace::Screen,
      is_main: false,
      is_visible: false,
    },
  ];

  let output = InvokeCommandOutput::from_result(&windows).expect("window result should serialize").with_report(window_list_report(&windows));
  let report = output.report.as_ref().expect("window.list should expose a human-readable report");

  assert_eq!(report.fields[0].value, "2 window(s)");
  assert!(report.sections.is_empty());
  assert_eq!(report.tables[0].columns, ["REF", "APP", "TITLE", "FRAME"]);
  assert_eq!(report.tables[0].display_max_chars, [None, Some(18), Some(40), None]);
  assert_eq!(report.tables[0].rows[0].cells, ["window_10", "TextEdit", "Project Notes", "12,34 640x480"]);
  assert_eq!(report.tables[0].rows[1].cells, ["window_11", "unknown", "untitled", "-100,20 300x200"]);
  assert_eq!(report.wide_tables[0].columns, ["REF", "APP", "TITLE", "FRAME", "BUNDLE", "PID", "FLAGS"]);
  assert_eq!(report.wide_tables[0].display_max_chars, [None, Some(18), Some(40), None, Some(32), None, None]);
  assert_eq!(report.wide_tables[0].rows[0].cells[4], "com.apple.TextEdit");
  assert_eq!(report.wide_tables[0].rows[0].cells[5], "1234");
  assert_eq!(report.wide_tables[0].rows[0].cells[6], "main,visible");
  assert_eq!(report.wide_tables[0].rows[1].cells[6], "hidden");
  assert_eq!(output.result(), Some(&serde_json::to_value(&windows).expect("fixture should serialize")));
}

#[test]
fn window_list_report_preserves_full_cell_values_for_human_rendering() {
  let long_title = "Fixture Window Title With Enough Words To Exceed The Human Display Limit".to_string();
  let long_app_name = "Fixture Application Name Beyond Human Display Limit".to_string();
  let long_bundle_id = "com.example.fixture.application.identifier.with.extra.segments".to_string();
  let windows = vec![Window {
    reference: WindowRef {
      id: "window_long".to_string(),
    },
    title: Some(long_title.clone()),
    app_name: Some(long_app_name.clone()),
    app_bundle_id: Some(long_bundle_id.clone()),
    process_id: Some(4321),
    frame: Rect::new(1.0, 2.0, 3.0, 4.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: false,
    is_visible: true,
  }];

  let output = InvokeCommandOutput::from_result(&windows).expect("window result should serialize").with_report(window_list_report(&windows));
  let report = output.report.as_ref().expect("window.list should expose a report");

  assert_eq!(report.tables[0].rows[0].cells[1], long_app_name);
  assert_eq!(report.tables[0].rows[0].cells[2], long_title);
  assert_eq!(report.wide_tables[0].rows[0].cells[4], long_bundle_id);
}

#[test]
fn window_capture_result_keeps_pixels_out_of_json() {
  let capture = WindowCapture {
    window: Window {
      reference: WindowRef {
        id: "window_capture".to_string(),
      },
      title: Some("Fixture".to_string()),
      app_name: Some("Fixture App".to_string()),
      app_bundle_id: Some("com.example.Fixture".to_string()),
      process_id: Some(42),
      frame: Rect::new(10.0, 20.0, 640.0, 480.0),
      coordinate_space: CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    },
    capture: auv_driver::Capture {
      image: RgbaImage::new(1280, 960),
      bounds: Rect::new(10.0, 20.0, 640.0, 480.0),
      scale_factor: 2.0,
      backend: "fixture-window".to_string(),
      fallback_reason: None,
    },
  };

  let output = window_capture_output(&capture).expect("window capture result should serialize");
  let result = output.result().expect("capture should have a result");

  assert_eq!(result["window"]["reference"]["id"], "window_capture");
  assert_eq!(result["capture"]["pixel_dimensions"]["width"], 1280);
  assert_eq!(result["capture"]["backend"], "fixture-window");
  assert!(result["capture"].get("image").is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn window_text_result_keeps_resolved_window_and_ocr_matches_together() {
  let recognition = WindowTextRecognition {
    window: Window {
      reference: WindowRef {
        id: "window_ocr".to_string(),
      },
      title: Some("Fixture".to_string()),
      app_name: Some("Fixture App".to_string()),
      app_bundle_id: Some("com.example.Fixture".to_string()),
      process_id: Some(42),
      frame: Rect::new(10.0, 20.0, 640.0, 480.0),
      coordinate_space: CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    },
    matches: auv_driver::OcrMatches {
      matches: vec![auv_driver::OcrMatch {
        text: "Pause".to_string(),
        confidence: 0.98,
        bounds: Rect::new(40.0, 50.0, 70.0, 20.0),
      }],
    },
  };

  let output = window_text_matches_output("window.findText", &recognition).expect("window OCR result should serialize");
  let result = output.result().expect("recognition should have a result");

  assert_eq!(result["window"]["reference"]["id"], "window_ocr");
  assert_eq!(result["matches"]["matches"][0]["text"], "Pause");
  assert_eq!(result["matches"]["matches"][0]["confidence"], 0.98);
}

#[cfg(target_os = "macos")]
#[test]
fn window_text_click_result_keeps_resolution_and_delivery_together() {
  let click = WindowTextClick {
    window: Window {
      reference: WindowRef {
        id: "window_click".to_string(),
      },
      title: Some("Fixture".to_string()),
      app_name: Some("Fixture App".to_string()),
      app_bundle_id: Some("com.example.Fixture".to_string()),
      process_id: Some(42),
      frame: Rect::new(10.0, 20.0, 640.0, 480.0),
      coordinate_space: CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    },
    matches: auv_driver::OcrMatches {
      matches: vec![auv_driver::OcrMatch {
        text: "Pause".to_string(),
        confidence: 0.98,
        bounds: Rect::new(40.0, 50.0, 70.0, 20.0),
      }],
    },
    point: auv_driver::geometry::WindowPoint::new(75.0, 60.0),
    action: auv_driver::InputActionResult::single_success(auv_driver::InputDeliveryPath::WindowTargetedMouse),
  };

  let output = window_text_click_output(&click).expect("window click result should serialize");
  let result = output.result().expect("click should have a result");

  assert_eq!(result["window"]["reference"]["id"], "window_click");
  assert_eq!(result["matches"]["matches"][0]["text"], "Pause");
  assert_eq!(result["point"]["x"], 75.0);
  assert_eq!(result["action"]["selected_path"], "window_targeted_mouse");
}
