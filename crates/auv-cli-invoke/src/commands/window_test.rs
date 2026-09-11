use auv_driver::{CoordinateSpace, Rect, Window, WindowRef};

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

  let output = window_text_matches_output("window.findText", &recognition, crate::commands::overlay::OverlayStatus::Disabled)
    .expect("window OCR result should serialize");
  let result = output.result().expect("recognition should have a result");

  assert_eq!(result["window"]["reference"]["id"], "window_ocr");
  assert_eq!(result["matches"]["matches"][0]["text"], "Pause");
  assert_eq!(result["matches"]["matches"][0]["confidence"], 0.98);
  let report = output.report.as_ref().expect("window text report");

  // ROOT CAUSE:
  //
  // If window OCR commands returned a recognized rectangle, no overlay was
  // requested because their handlers only built the result/report.
  //
  // Before the fix, the report had no Overlay field. The fix constructs a
  // typed outline scene and records presentation independently from OCR.
  assert_eq!(report_field(report, "Overlay"), "disabled");

  let scene = window_text_overlay(&recognition.matches, None);
  let auv_driver::overlay::Layer::Outline(outline) = &scene.layers()[0] else {
    panic!("text match should produce an outline layer");
  };
  assert_eq!(outline.rect(), Rect::new(40.0, 50.0, 70.0, 20.0));
}

#[test]
fn recorded_window_text_click_result_keeps_resolution_and_delivery_together() {
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
      matches: vec![
        auv_driver::OcrMatch {
          text: "Pause".to_string(),
          confidence: 0.98,
          bounds: Rect::new(40.0, 50.0, 70.0, 20.0),
        },
        auv_driver::OcrMatch {
          text: "Pause".to_string(),
          confidence: 0.96,
          bounds: Rect::new(140.0, 150.0, 70.0, 20.0),
        },
      ],
    },
    selected_index: 1,
    point: auv_driver::geometry::WindowPoint::new(75.0, 60.0),
    options: auv_driver::ClickOptions {
      policy: auv_driver::InputPolicy::ForegroundPreferred,
      click: auv_driver::Click::Repeated {
        count: 3,
        interval: std::time::Duration::from_millis(60),
      },
      ..auv_driver::ClickOptions::default()
    },
    action: auv_driver::InputActionResult::single_success(auv_driver::InputDeliveryPath::WindowTargetedMouse),
  };

  let capture = auv_driver::Capture {
    image: image::RgbaImage::new(1, 1),
    bounds: Rect::new(10.0, 20.0, 1.0, 1.0),
    scale_factor: 1.0,
    backend: "fixture".to_string(),
    fallback_reason: None,
  };
  let output = recorded_window_text_click_output(&click, &capture).expect("window click result should serialize");
  let result = output.result().expect("click should have a result");

  assert_eq!(result["window"]["reference"]["id"], "window_click");
  assert_eq!(result["matches"]["matches"][0]["text"], "Pause");
  assert_eq!(result["selected_index"], 1);
  assert_eq!(result["point"]["x"], 75.0);
  assert_eq!(result["options"]["policy"], "foreground_preferred");
  assert_eq!(result["options"]["click"]["repeated"]["count"], 3);
  assert_eq!(result["action"]["selected_path"], "window_targeted_mouse");
  let report = output.report.as_ref().expect("window click report");
  assert_eq!(report_field(report, "Delivery"), "delivered");
  assert_eq!(report_field(report, "Verification"), "delivery_only");
  assert_eq!(report_field(report, "Path"), "window_targeted_mouse");
  assert_eq!(report_field(report, "Input policy"), "foreground_preferred");
  assert_eq!(report_field(report, "Click count"), "3");
  assert_eq!(report_field(report, "Click interval"), "60 ms");
  assert_eq!(report.tables[0].rows[0].cells[0], "");
  assert_eq!(report.tables[0].rows[1].cells[0], "*");
}

#[test]
fn window_text_click_selects_the_requested_candidate_and_rejects_out_of_range_indexes() {
  let matches = auv_driver::OcrMatches {
    matches: vec![
      auv_driver::OcrMatch {
        text: "AGENTS.md".to_string(),
        confidence: 0.98,
        bounds: auv_driver::Rect::new(468.0, 108.0, 71.0, 10.0),
      },
      auv_driver::OcrMatch {
        text: "AGENTS.md".to_string(),
        confidence: 0.96,
        bounds: auv_driver::Rect::new(100.0, 720.0, 71.0, 10.0),
      },
    ],
  };

  // ROOT CAUSE:
  //
  // If OCR returned multiple matching anchors, window.clickText always used
  // the first candidate because it called OcrMatches::best_match directly.
  //
  // Before the fix, the IDX column was informational only. The fix lets the
  // caller select that zero-based index and rejects invalid indexes before
  // input delivery.
  let selected = selected_window_text_match(&matches, "AGENTS.md", 1).expect("second candidate should be selectable");
  assert_eq!(selected.bounds, auv_driver::Rect::new(100.0, 720.0, 71.0, 10.0));

  let error = selected_window_text_match(&matches, "AGENTS.md", 2).expect_err("out-of-range index should fail");
  assert_eq!(error, "window.clickText --index 2 is out of range for 2 text match(es)");
}

fn report_field<'a>(report: &'a InvokeReport, label: &str) -> &'a str {
  report.fields.iter().find(|field| field.label == label).map(|field| field.value.as_str()).expect("report field")
}
