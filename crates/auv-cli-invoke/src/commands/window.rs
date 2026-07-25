use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportTable,
  InvokeReportTableRow, InvokeReportValue, OptionalReportText,
  arg::{NO_ARGS, WINDOW_ARGS, WINDOW_TEXT_ARGS, WINDOW_VERIFY_TEXT_ARGS},
  artifact::emit_png,
  invoke_command,
};

pub fn group() -> CommandGroup {
  CommandGroup::new("window", "WINDOW")
    .command(list_windows_invoke_command())
    .command(capture_window_invoke_command())
    .command(capture_ax_tree_invoke_command())
    .command(find_window_text_invoke_command())
    .command(wait_for_window_text_invoke_command())
    .command(find_window_rows_invoke_command())
    .command(wait_for_window_rows_invoke_command())
    .command(observe_window_region_invoke_command())
    .command(find_icon_match_invoke_command())
    .command(scroll_window_region_invoke_command())
    .command(verify_ax_text_invoke_command())
    .command(click_window_text_invoke_command())
    .command(click_window_row_invoke_command())
}

#[invoke_command(
  id = "window.list",
  group = "window",
  description = "List visible macOS window candidates using the normalized AUV window selector model.",
  args = NO_ARGS,
)]
async fn list_windows(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let windows = observe_windows().await?;
    Ok(InvokeCommandOutput::from_result(&windows)?.with_report(window_list_report(&windows)))
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("window.list is only available on macOS".to_string())
  }
}

pub async fn observe_windows() -> Result<Vec<auv_driver::Window>, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    session.window().list().map_err(|error| error.to_string())
  }
  #[cfg(not(target_os = "macos"))]
  {
    Err("window.list is only available on macOS".to_string())
  }
}

#[invoke_command(
  id = "window.capture",
  group = "window",
  description = "Capture one single-display window and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = WINDOW_ARGS,
)]
async fn capture_window(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let result = capture_selected_window(window_selector(&input)).await?;
    window_capture_output(&result)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("window.capture is only available on macOS".to_string())
  }
}

#[derive(Clone, Debug)]
pub struct WindowCapture {
  pub window: auv_driver::Window,
  pub capture: auv_driver::Capture,
}

#[derive(serde::Serialize)]
pub struct WindowCaptureResult<'a> {
  window: &'a auv_driver::Window,
  capture: super::CaptureResult<'a>,
}

pub fn window_capture_result(result: &WindowCapture) -> WindowCaptureResult<'_> {
  WindowCaptureResult {
    window: &result.window,
    capture: super::capture_result(&result.capture),
  }
}

fn window_capture_output(result: &WindowCapture) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(&window_capture_result(result))?;
  let mut fields = window_report_fields(&result.window);
  fields.push(InvokeReportField::new("Pixel size", format!("{}x{}", result.capture.image.width(), result.capture.image.height())));
  output.report = Some(InvokeReport::new(fields, Vec::new()));
  // TODO(invoke-window-capture-backend): live testing on 2026-06-18 showed
  // ScreenCaptureKit single-window capture can time out and xcap fallback can
  // fail for Chrome/NetEase windows. Stabilize the typed window capture backend
  // before treating window.* evidence as reliably available.
  Ok(output)
}

pub async fn capture_selected_window(selector: auv_driver::WindowSelector) -> Result<WindowCapture, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let window = session.window().resolve(selector).map_err(|error| error.to_string())?;
    let capture = session.window().capture(&window).map_err(|error| error.to_string())?;
    emit_png("auv.driver.window_capture", &capture.image);
    Ok(WindowCapture { window, capture })
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = selector;
    Err("window.capture is only available on macOS".to_string())
  }
}

#[invoke_command(
  id = "window.captureAxTree",
  group = "window",
  description = "Capture an AX tree snapshot for a target macOS app window.",
  args = WINDOW_ARGS,
)]
async fn capture_ax_tree(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-window-ax-tree): promote ObservedAxTreeSnapshot into the
  // platform-neutral driver contract before sharing it across frontends.
  unimplemented!("window.captureAxTree")
}

#[invoke_command(
  id = "window.findText",
  group = "window",
  description = "Capture a resolved window and locate OCR text anchors in window pixel space.",
  args = WINDOW_TEXT_ARGS,
)]
async fn find_window_text(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let query = input.required_input("query")?.to_string();
    let result = recognize_window_text(window_selector(&input), query, false).await?;
    window_text_matches_output(&input.command_id, &result)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("window text OCR is only available on macOS".to_string())
  }
}

#[invoke_command(
  id = "window.waitForText",
  group = "window",
  description = "Poll resolved-window OCR until a text anchor appears or the timeout expires.",
  args = WINDOW_TEXT_ARGS,
)]
async fn wait_for_window_text(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let query = input.required_input("query")?.to_string();
    let result = recognize_window_text(window_selector(&input), query, true).await?;
    window_text_matches_output(&input.command_id, &result)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("window text OCR is only available on macOS".to_string())
  }
}

#[invoke_command(
  id = "window.findRows",
  group = "window",
  description = "Detect visible OCR row bands inside a resolved window.",
  args = WINDOW_ARGS,
)]
async fn find_window_rows(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-window-rows): implement after VisionApi owns typed row-band
  // detection for a resolved window.
  unimplemented!("window.findRows")
}

#[invoke_command(
  id = "window.waitForRows",
  group = "window",
  description = "Poll resolved-window row detection until enough rows appear or the timeout expires.",
  args = WINDOW_ARGS,
)]
async fn wait_for_window_rows(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-window-rows): see `find_window_rows`; waiting additionally
  // needs an owned polling and timeout policy.
  unimplemented!("window.waitForRows")
}

#[invoke_command(
  id = "window.observeRegion",
  group = "window",
  description = "Observe OCR row-like content inside a resolved macOS window region without scrolling.",
  args = WINDOW_ARGS,
)]
async fn observe_window_region(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-window-observe-region): add region arguments and a typed
  // observation result before implementing this command.
  unimplemented!("window.observeRegion")
}

#[invoke_command(
  id = "window.findIconMatch",
  group = "window",
  description = "Match a template image against a resolved macOS window screenshot using NCC and emit a RecognitionResult artifact.",
  args = WINDOW_ARGS,
)]
async fn find_icon_match(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-window-icon-match): add template artifact and threshold
  // arguments before routing through a typed VisionApi operation.
  unimplemented!("window.findIconMatch")
}

#[invoke_command(
  id = "window.scrollRegion",
  group = "window",
  description = "Scroll at the center of a resolved macOS window region and record scroll evidence.",
  args = WINDOW_ARGS,
)]
async fn scroll_window_region(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-window-scroll-region): add point and delta arguments before
  // routing through WindowApi::scroll.
  unimplemented!("window.scrollRegion")
}

#[invoke_command(
  id = "window.verifyText",
  group = "window",
  description = "Verify that a text-bearing AX node exists in the observed tree without relying on screenshot OCR.",
  args = WINDOW_VERIFY_TEXT_ARGS,
)]
async fn verify_ax_text(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-window-verify-ax-text): add an AX role argument and an
  // app/window selector contract before calling AccessibilityApi::verify_text.
  unimplemented!("window.verifyText")
}

#[invoke_command(
  id = "window.clickText",
  group = "window",
  description = "Capture a resolved window, resolve an OCR text anchor, and click its projected logical point.",
  args = WINDOW_TEXT_ARGS,
)]
async fn click_window_text(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    use auv_driver::{ClickOptions, RatioRect, ScreenPoint};

    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let query = input.required_input("query")?.to_string();
    let result = click_recognized_window_text(window_selector(&input), query).await?;

    window_text_click_output(&result)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("window.clickText is only available on macOS".to_string())
  }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct WindowTextClick {
  pub window: auv_driver::Window,
  pub matches: auv_driver::OcrMatches,
  pub point: auv_driver::geometry::WindowPoint,
  pub action: auv_driver::InputActionResult,
}

#[cfg(target_os = "macos")]
fn window_text_click_output(result: &WindowTextClick) -> InvokeCommandResult {
  let mut report = crate::commands::ocr::match_report(&result.matches.matches, Some(0));
  report.fields.extend(window_report_fields(&result.window));
  report.fields.push(InvokeReportField::new("Input path", result.action.selected_path.as_str()));
  report.fields.push(InvokeReportField::new("Window point", format!("{:.0},{:.0}", result.point.point().x, result.point.point().y)));
  Ok(InvokeCommandOutput::from_result(result)?.with_report(report))
}

pub async fn click_recognized_window_text(selector: auv_driver::WindowSelector, query: String) -> Result<WindowTextClick, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let window = session.window().resolve(selector).map_err(|error| error.to_string())?;
    let capture = session.window().capture(&window).map_err(|error| error.to_string())?;
    let matches = session
      .vision()
      .find_text_in_capture(&capture, &query, auv_driver::RatioRect::new(0.0, 0.0, 1.0, 1.0))
      .map_err(|error| error.to_string())?;
    let matched = matches.best_match().ok_or_else(|| format!("window.clickText did not find text {query:?}"))?;
    let point =
      session.window().to_window_point(&window, auv_driver::ScreenPoint::from(matched.action_point())).map_err(|error| error.to_string())?;
    let action = session.window().click(&window, point, auv_driver::ClickOptions::default()).map_err(|error| error.to_string())?;
    emit_png("auv.driver.window_ocr_source", &capture.image);
    Ok(WindowTextClick {
      window,
      matches,
      point,
      action,
    })
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (selector, query);
    Err("window.clickText is only available on macOS".to_string())
  }
}

#[invoke_command(
  id = "window.clickRow",
  group = "window",
  description = "Capture a resolved window, detect visible rows, and click a row-derived projected logical point.",
  args = WINDOW_ARGS,
)]
async fn click_window_row(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-window-rows): implement after typed row detection and
  // row-to-point policy can feed WindowApi and return InputActionResult.
  unimplemented!("window.clickRow")
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct WindowTextRecognition {
  pub window: auv_driver::Window,
  pub matches: auv_driver::OcrMatches,
}

#[cfg(target_os = "macos")]
pub async fn recognize_window_text(
  selector: auv_driver::WindowSelector,
  query: String,
  wait: bool,
) -> Result<WindowTextRecognition, String> {
  use auv_driver::{RatioRect, WaitOptions};
  use std::{thread, time::Instant};

  let session = auv_driver::open_local().map_err(|error| error.to_string())?;
  let window = session.window().resolve(selector).map_err(|error| error.to_string())?;
  let wait_options = WaitOptions::default();
  let started = Instant::now();
  loop {
    let capture = session.window().capture(&window).map_err(|error| error.to_string())?;
    let matches =
      session.vision().find_text_in_capture(&capture, &query, RatioRect::new(0.0, 0.0, 1.0, 1.0)).map_err(|error| error.to_string())?;
    if !matches.matches.is_empty() || !wait || started.elapsed() >= wait_options.timeout {
      if wait && matches.matches.is_empty() {
        return Err(format!("window.waitForText did not find text {query:?} before timeout"));
      }

      // TODO(invoke-recognition-result-artifacts): this records the window OCR
      // source screenshot and typed OCR matches, but not a structured
      // recognition-result artifact with query/bounds/confidence. Add it after
      // the artifact shape is accepted in the direct-command handoff.
      emit_png("auv.driver.window_ocr_source", &capture.image);
      return Ok(WindowTextRecognition { window, matches });
    }
    thread::sleep(wait_options.poll_interval);
  }
}

#[cfg(not(target_os = "macos"))]
pub async fn recognize_window_text(
  _selector: auv_driver::WindowSelector,
  _query: String,
  _wait: bool,
) -> Result<WindowTextRecognition, String> {
  Err("window text OCR is only available on macOS".to_string())
}

#[cfg(target_os = "macos")]
fn window_text_matches_output(_command_id: &str, result: &WindowTextRecognition) -> InvokeCommandResult {
  let mut report = crate::commands::ocr::match_report(&result.matches.matches, None);
  report.fields.extend(window_report_fields(&result.window));
  Ok(InvokeCommandOutput::from_result(result)?.with_report(report))
}

#[cfg(target_os = "macos")]
fn window_selector(input: &InvokeCommandInput) -> auv_driver::WindowSelector {
  use auv_driver::{App, TextMatcher, WindowSelector};

  let mut selector = WindowSelector {
    main_visible: true,
    ..WindowSelector::default()
  };
  if let Some(target) = input.target_or_input_target() {
    selector.app = Some(App::bundle_id(target));
  }
  if let Some(title) = input.inputs.get("title").filter(|value| !value.trim().is_empty()) {
    selector.title = Some(TextMatcher::Contains(title.clone()));
  }
  selector
}

#[cfg(target_os = "macos")]
fn window_report_fields(window: &auv_driver::Window) -> Vec<InvokeReportField> {
  let mut fields = vec![
    InvokeReportField::new("Window ID", window.reference.id.clone()),
    InvokeReportField::new("Window frame", window.frame.report_value()),
  ];
  if let Some(title) = &window.title {
    fields.push(InvokeReportField::new("Window title", title));
  }
  if let Some(app_name) = &window.app_name {
    fields.push(InvokeReportField::new("Application", app_name));
  }
  if let Some(bundle_id) = &window.app_bundle_id {
    fields.push(InvokeReportField::new("Bundle ID", bundle_id));
  }
  fields
}

fn window_list_report(windows: &[auv_driver::Window]) -> InvokeReport {
  InvokeReport {
    fields: vec![InvokeReportField::new(
      "Result",
      format!("{} window(s)", windows.len()),
    )],
    tables: vec![
      InvokeReportTable::new(
        &["REF", "APP", "TITLE", "FRAME"],
        windows
          .iter()
          .map(|window| {
            InvokeReportTableRow::new([
              window.reference.id.clone(),
              window.app_name.as_deref().report_or("unknown").to_string(),
              window.title.as_deref().report_or("untitled").to_string(),
              window.frame.report_value(),
            ])
          })
          .collect(),
      )
      .with_display_max_chars(vec![None, Some(18), Some(40), None]),
    ],
    wide_tables: vec![
      InvokeReportTable::new(
        &["REF", "APP", "TITLE", "FRAME", "BUNDLE", "PID", "FLAGS"],
        windows
          .iter()
          .map(|window| {
            let mut flags = Vec::new();
            if window.is_main {
              flags.push("main");
            }
            flags.push(if window.is_visible {
              "visible"
            } else {
              "hidden"
            });
            InvokeReportTableRow::new([
              window.reference.id.clone(),
              window.app_name.as_deref().report_or("unknown").to_string(),
              window.title.as_deref().report_or("untitled").to_string(),
              window.frame.report_value(),
              window.app_bundle_id.as_deref().report_or("unknown").to_string(),
              window.process_id.map(|pid| pid.to_string()).unwrap_or_else(|| "unknown".to_string()),
              flags.join(","),
            ])
          })
          .collect(),
      )
      .with_display_max_chars(vec![None, Some(18), Some(40), None, Some(32), None, None]),
    ],
    sections: Vec::new(),
  }
}

#[cfg(test)]
mod tests {
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

    let output =
      InvokeCommandOutput::from_result(&windows).expect("window result should serialize").with_report(window_list_report(&windows));
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

    let output =
      InvokeCommandOutput::from_result(&windows).expect("window result should serialize").with_report(window_list_report(&windows));
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
}
