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
      application_id: Some("com.apple.TextEdit".to_string()),
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
      application_id: None,
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
  assert_eq!(report.wide_tables[0].columns, ["REF", "APP", "TITLE", "FRAME", "APP ID", "PID", "FLAGS"]);
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
  let long_application_id = "com.example.fixture.application.identifier.with.extra.segments".to_string();
  let windows = vec![Window {
    reference: WindowRef {
      id: "window_long".to_string(),
    },
    title: Some(long_title.clone()),
    app_name: Some(long_app_name.clone()),
    application_id: Some(long_application_id.clone()),
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
  assert_eq!(report.wide_tables[0].rows[0].cells[4], long_application_id);
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn window_text_result_keeps_resolved_window_and_ocr_matches_together() {
  let recognition = WindowTextRecognition {
    window: Window {
      reference: WindowRef {
        id: "window_ocr".to_string(),
      },
      title: Some("Fixture".to_string()),
      app_name: Some("Fixture App".to_string()),
      application_id: Some("com.example.Fixture".to_string()),
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
      application_id: Some("com.example.Fixture".to_string()),
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
    // The fixture window sits at screen (10, 20), so the screen point below is
    // exactly the window point offset by the frame origin.
    point: auv_driver::geometry::ScreenPoint::new(85.0, 80.0),
    window_point: auv_driver::geometry::WindowPoint::new(75.0, 60.0),
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
    origin: None,
    image: image::RgbaImage::new(1, 1),
    bounds: Rect::new(10.0, 20.0, 1.0, 1.0),
    scale_factor: 1.0,
    backend: "fixture".to_string(),
    fallback_reason: None,
  };
  let output = recorded_window_text_click_output(&click, Some(&capture)).expect("window click result should serialize");
  let result = output.result().expect("click should have a result");

  assert_eq!(result["window"]["reference"]["id"], "window_click");
  assert_eq!(result["matches"]["matches"][0]["text"], "Pause");
  assert_eq!(result["selected_index"], 1);
  // ROOT CAUSE:
  //
  // The result exposed a single unlabelled point in window-local space, while
  // window.findText reports match bounds in screen space and the command's own
  // overlay draws the match rectangle in screen space. A caller comparing the
  // returned point against the match it came from was off by the window frame
  // origin.
  //
  // Both spaces are now reported under names that say which is which.
  assert_eq!(result["point"]["x"], 85.0);
  assert_eq!(result["point"]["y"], 80.0);
  assert_eq!(result["window_point"]["x"], 75.0);
  assert_eq!(result["window_point"]["y"], 60.0);
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
  assert_eq!(report_field(report, "Screen point"), "85.0,80.0");
  assert_eq!(report_field(report, "Window point"), "75.0,60.0");
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
  assert_eq!(error.code, crate::FailureCode::InvalidInput);
  assert_eq!(error.message, "window.clickText --index 2 is out of range for 2 text match(es)");
}

// ROOT CAUSE:
//
// The selected-Runner route consumed this helper's `String` errors, which
// `From<String>` downgraded to `command_failed`, while the direct local path
// answered the same conditions with `not_found` and `invalid_input`. A caller
// could not tell a text miss from a broken run when the command ran on a
// selected Runner.
//
// Before the fix, the helper carried plain strings and the Runner arm could
// not preserve a failure code. The fix types the helper's errors, so both
// routes propagate the same classification, and this test pins the codes.
#[test]
fn window_text_match_failures_carry_the_shared_failure_codes() {
  let matches = auv_driver::OcrMatches::default();

  let miss = selected_window_text_match(&matches, "Ready", 0).expect_err("an empty match set should fail");
  assert_eq!(miss.code, crate::FailureCode::NotFound);
  assert_eq!(miss.message, "window.clickText did not find text \"Ready\"");

  let candidate = auv_driver::OcrMatch {
    text: "Ready".to_string(),
    confidence: 0.9,
    bounds: auv_driver::Rect::new(10.0, 10.0, 20.0, 8.0),
  };
  let one_match = auv_driver::OcrMatches {
    matches: vec![candidate],
  };
  let out_of_range = selected_window_text_match(&one_match, "Ready", 1).expect_err("index 1 should be out of range for one match");
  assert_eq!(out_of_range.code, crate::FailureCode::InvalidInput);
  assert_eq!(out_of_range.message, "window.clickText --index 1 is out of range for 1 text match(es)");
}

#[test]
fn matched_window_point_maps_interior_and_rejects_out_of_bounds_before_delivery() {
  let window = Window {
    reference: WindowRef {
      id: "test_win".into(),
    },
    title: Some("Test".into()),
    app_name: Some("test.exe".into()),
    application_id: None,
    process_id: Some(100),
    frame: auv_driver::Rect::new(50.0, 150.0, 400.0, 300.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };

  let ocr_match_at = |center_x: f64, center_y: f64| auv_driver::OcrMatch {
    text: "Target".to_string(),
    confidence: 0.95,
    bounds: auv_driver::Rect::new(center_x - 10.0, center_y - 10.0, 20.0, 20.0),
  };

  // 1. Interior point: screen (100, 200) -> window (50, 50)
  let inside = ocr_match_at(100.0, 200.0);
  let point = matched_window_point(&window, &inside).expect("inside point should map");
  assert_eq!(point, auv_driver::geometry::WindowPoint::new(50.0, 50.0));

  // 2. Exact boundaries: top-left (0.0, 0.0) is inclusive; right and bottom [width, height] are excluded
  // Top-left boundary: local (0.0, 0.0)
  let top_left = ocr_match_at(50.0, 150.0);
  assert_eq!(
    matched_window_point(&window, &top_left).expect("top-left boundary should map"),
    auv_driver::geometry::WindowPoint::new(0.0, 0.0)
  );

  // Just inside bottom-right perimeter: local (399.9, 299.9)
  let just_inside = ocr_match_at(449.9, 449.9);
  assert_eq!(
    matched_window_point(&window, &just_inside).expect("point just inside right/bottom should map"),
    auv_driver::geometry::WindowPoint::new(399.9, 299.9)
  );

  // Exact right boundary x == width (400.0) is outside the half-open window pixel span [0, width)
  let exact_right = ocr_match_at(450.0, 200.0);
  let err_right = matched_window_point(&window, &exact_right).expect_err("exact right edge x==width must be rejected");
  assert_eq!(err_right.code, crate::FailureCode::InvalidTarget);
  assert!(err_right.message.contains("outside resolved window bounds; no input delivered"));

  // Exact bottom boundary y == height (300.0) is outside the half-open window pixel span [0, height)
  let exact_bottom = ocr_match_at(100.0, 450.0);
  let err_bottom = matched_window_point(&window, &exact_bottom).expect_err("exact bottom edge y==height must be rejected");
  assert_eq!(err_bottom.code, crate::FailureCode::InvalidTarget);

  // Exact bottom-right corner (400.0, 300.0) must be rejected
  let bottom_right = ocr_match_at(450.0, 450.0);
  let err_br = matched_window_point(&window, &bottom_right).expect_err("bottom-right corner must be rejected");
  assert_eq!(err_br.code, crate::FailureCode::InvalidTarget);

  // 3. Out of bounds beyond right/bottom: screen (600, 500) -> window (550, 350)
  let outside = ocr_match_at(600.0, 500.0);
  let error = matched_window_point(&window, &outside).expect_err("outside point must be rejected");
  assert_eq!(error.code, crate::FailureCode::InvalidTarget);
  assert!(error.message.contains("outside resolved window bounds; no input delivered"));

  // Negative coordinate relative to window
  let negative_x = ocr_match_at(49.9, 200.0);
  let err_neg = matched_window_point(&window, &negative_x).expect_err("negative x must be rejected");
  assert_eq!(err_neg.code, crate::FailureCode::InvalidTarget);

  // 4. Non-finite candidate coordinates (NaN and Inf) must be rejected
  let nan_match = ocr_match_at(f64::NAN, 200.0);
  let err_nan = matched_window_point(&window, &nan_match).expect_err("NaN point must be rejected");
  assert_eq!(err_nan.code, crate::FailureCode::InvalidTarget);

  let inf_match = ocr_match_at(f64::INFINITY, 200.0);
  let err_inf = matched_window_point(&window, &inf_match).expect_err("Infinity point must be rejected");
  assert_eq!(err_inf.code, crate::FailureCode::InvalidTarget);

  // 5. Raw float display precision in error message (no misleading rounding like 450.0001 -> 450.0)
  let float_precision_match = ocr_match_at(450.0001, 200.0);
  let err_precision = matched_window_point(&window, &float_precision_match).expect_err("micro out-of-bounds must be rejected");
  assert!(err_precision.message.contains("450.0001,200"));

  // 6. Zero-sized and negative-dimension windows must reject any click point
  let zero_window = Window {
    frame: auv_driver::Rect::new(50.0, 150.0, 0.0, 0.0),
    ..window.clone()
  };
  let err_zero = matched_window_point(&zero_window, &top_left).expect_err("zero-size window must reject clicks");
  assert_eq!(err_zero.code, crate::FailureCode::InvalidTarget);

  let neg_width_window = Window {
    frame: auv_driver::Rect::new(50.0, 150.0, -10.0, 300.0),
    ..window.clone()
  };
  assert_eq!(
    matched_window_point(&neg_width_window, &inside).expect_err("negative width must be rejected").code,
    crate::FailureCode::InvalidTarget
  );

  let neg_height_window = Window {
    frame: auv_driver::Rect::new(50.0, 150.0, 400.0, -10.0),
    ..window.clone()
  };
  assert_eq!(
    matched_window_point(&neg_height_window, &inside).expect_err("negative height must be rejected").code,
    crate::FailureCode::InvalidTarget
  );

  // 7. Non-finite window dimensions (NaN and Infinity) must be rejected
  let nan_width_window = Window {
    frame: auv_driver::Rect::new(50.0, 150.0, f64::NAN, 300.0),
    ..window.clone()
  };
  assert_eq!(
    matched_window_point(&nan_width_window, &inside).expect_err("NaN width must be rejected").code,
    crate::FailureCode::InvalidTarget
  );

  let nan_height_window = Window {
    frame: auv_driver::Rect::new(50.0, 150.0, 400.0, f64::NAN),
    ..window.clone()
  };
  assert_eq!(
    matched_window_point(&nan_height_window, &inside).expect_err("NaN height must be rejected").code,
    crate::FailureCode::InvalidTarget
  );

  let inf_width_window = Window {
    frame: auv_driver::Rect::new(50.0, 150.0, f64::INFINITY, 300.0),
    ..window.clone()
  };
  assert_eq!(
    matched_window_point(&inf_width_window, &inside).expect_err("Infinity width must be rejected").code,
    crate::FailureCode::InvalidTarget
  );

  let inf_height_window = Window {
    frame: auv_driver::Rect::new(50.0, 150.0, 400.0, f64::INFINITY),
    ..window.clone()
  };
  assert_eq!(
    matched_window_point(&inf_height_window, &inside).expect_err("Infinity height must be rejected").code,
    crate::FailureCode::InvalidTarget
  );
}

#[test]
fn matched_window_point_out_of_bounds_serializes_to_structured_invalid_target_failure() {
  let window = Window {
    reference: WindowRef {
      id: "test_win".into(),
    },
    title: Some("Test".into()),
    app_name: Some("test.exe".into()),
    application_id: None,
    process_id: Some(100),
    frame: auv_driver::Rect::new(50.0, 150.0, 400.0, 300.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };
  let outside = auv_driver::OcrMatch {
    text: "Target".to_string(),
    confidence: 0.95,
    bounds: auv_driver::Rect::new(445.0, 195.0, 10.0, 10.0), // center at (450.0, 200.0), exact right edge
  };
  let failure = matched_window_point(&window, &outside).expect_err("exact right edge is outside half-open window bounds");
  assert_eq!(failure.code, crate::FailureCode::InvalidTarget);

  let registry = crate::default_registry();
  let command = registry.resolve("window.clickText").expect("resolve window.clickText");
  let output = crate::models::InvokeResult::from_command_result(auv_tracing::RunId::new(), command, Err(failure.clone()));
  let json: serde_json::Value = serde_json::from_str(
    &output
      .render_to_string(crate::models::InvokeOutputOptions {
        json: true,
        ..Default::default()
      })
      .expect("render json"),
  )
  .expect("parse json");

  assert_eq!(json["status"], "failed");
  assert_eq!(json["command_id"], "window.clickText");
  assert_eq!(json["failure"], failure.message);
  assert_eq!(json["failure_details"]["code"], "invalid_target");
  assert_eq!(json["failure_details"]["message"], failure.message);
}

fn report_field<'a>(report: &'a InvokeReport, label: &str) -> &'a str {
  report.fields.iter().find(|field| field.label == label).map(|field| field.value.as_str()).expect("report field")
}

#[cfg(not(target_os = "macos"))]
fn dry_run_input(command_id: &str, target: Option<&str>, query: Option<&str>) -> crate::InvokeCommandInput {
  crate::InvokeCommandInput {
    command_id: command_id.to_string(),
    target: target.map(|id| crate::ExecutionTarget::Application { id: id.to_string() }),
    inputs: query.map(|query| ("query".to_string(), query.to_string())).into_iter().collect(),
    typed_args: None,
    dry_run: true,
    cancellation: crate::InvokeCancellation::new(),
  }
}

// ROOT CAUSE:
//
// If `window.list`, `window.capture`, `window.findText` or `window.waitForText` ran on Windows,
// each answered "only available on macOS and Linux" before reaching the driver, although the
// Windows driver already lists windows, captures them with PrintWindow and reads their text with
// the system OCR.
//
// Before the fix, the handlers were compiled out on Windows. The fix keeps them compiled in, so a
// dry run reaches the same validation that every other platform reaches. A dry run proves the
// gate only; what the commands do on a desktop is recorded in the Windows window commands
// reference.
#[cfg(target_os = "windows")]
#[test]
fn window_observation_commands_are_not_gated_out_on_windows() {
  let commands = [
    (list_windows_invoke_command(), dry_run_input("window.list", None, None)),
    (capture_window_invoke_command(), dry_run_input("window.capture", Some("notepad.exe"), None)),
    (find_window_text_invoke_command(), dry_run_input("window.findText", Some("notepad.exe"), Some("Save"))),
    (wait_for_window_text_invoke_command(), dry_run_input("window.waitForText", Some("notepad.exe"), Some("Saved"))),
    (click_window_text_invoke_command(), dry_run_input("window.clickText", Some("notepad.exe"), Some("Save"))),
  ];

  for (command, input) in commands {
    let id = input.command_id.clone();
    futures_executor::block_on(command.invoke(input)).unwrap_or_else(|error| panic!("{id} was refused on Windows: {}", error.message));
  }
}

// ROOT CAUSE:
//
// If `window.clickText` ran on Linux, where its cell is `Unsupported`, the refusal reached the
// caller as `command_failed`, because the handler returned a plain `String` and
// `InvokeFailure::from(String)` assigns that code.
//
// Before the fix, a caller could not tell a platform without a path from a click that failed. The
// fix keeps the refusal typed as `unsupported`, and the command still refuses instead of clicking
// through foreground input.
#[cfg(target_os = "linux")]
#[test]
fn window_click_text_refuses_as_unsupported_on_linux() {
  let input = dry_run_input("window.clickText", Some("notepad.exe"), Some("Save"));

  let outcome = futures_executor::block_on(click_window_text_invoke_command().invoke(input));

  // A dry run returns before platform access on macOS and Windows; elsewhere the handler is not compiled in.
  let error = outcome.expect_err("window.clickText has no path here");
  assert_eq!(error.code, crate::FailureCode::Unsupported, "unexpected refusal: {}", error.message);
  assert!(error.message.contains("only available on macOS and Windows"), "unexpected message: {}", error.message);
}

// Linux resolves and captures through AT-SPI and runs the shared OCR decoder.
// A dry run validates the arguments and returns before platform access.
#[cfg(target_os = "linux")]
#[test]
fn window_text_commands_dry_run_on_linux() {
  let commands = [
    (find_window_text_invoke_command(), dry_run_input("window.findText", Some("org.example.App"), Some("Save"))),
    (wait_for_window_text_invoke_command(), dry_run_input("window.waitForText", Some("org.example.App"), Some("Saved"))),
  ];

  for (command, input) in commands {
    let id = input.command_id.clone();
    let outcome = futures_executor::block_on(command.invoke(input));
    assert!(outcome.is_ok(), "{id}: {:?}", outcome.err());
  }
}

// On Windows an application is its executable file name, the `app_name` of `window.list`;
// `application_id` is always `None` (TODO(windows-application-id)).
#[cfg(target_os = "windows")]
#[test]
fn window_selector_names_the_executable_on_windows() {
  let selector = window_selector(&dry_run_input("window.capture", Some("Notepad.exe"), None), Some("Untitled")).expect("selector");

  assert_eq!(selector.app, Some(auv_driver::App::name("Notepad.exe")));
  assert_eq!(selector.title, Some(auv_driver::TextMatcher::Contains("Untitled".to_string())));
  assert!(selector.main_visible);
}
