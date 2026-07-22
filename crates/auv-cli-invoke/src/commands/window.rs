use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportLabels,
  InvokeReportTable, InvokeReportTableRow, InvokeReportValue, OptionalReportText,
  arg::{NO_ARGS, WINDOW_ARGS, WINDOW_TEXT_ARGS, WINDOW_VERIFY_TEXT_ARGS},
  artifact::{emission_enabled, png_artifact},
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
  summary = "List visible macOS window candidates using the normalized AUV window selector model.",
  args = NO_ARGS,
)]
async fn list_windows(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let windows = observe_windows().await?;
    let mut output = window_list_output(&windows);
    output.backend = Some("auv-driver-macos.window".to_string());
    output.signals.insert("window.count".to_string(), windows.len().to_string());
    if let Some(front) = windows.first() {
      if let Some(title) = &front.title {
        output.signals.insert("window.first.title".to_string(), title.clone());
      }
      if let Some(app_name) = &front.app_name {
        output.signals.insert("window.first.app_name".to_string(), app_name.clone());
      }
    }
    output.verification = Some("read-only; no semantic success claim".to_string());
    output.known_limits.push("window.list records the observed visible window inventory only.".to_string());
    Ok(output)
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
  summary = "Capture one single-display window and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = WINDOW_ARGS,
)]
async fn capture_window(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let result = capture_selected_window(window_selector(&input)).await?;

    let mut output = InvokeCommandOutput::new("window captured");
    output.backend = Some(format!("auv-driver-macos.window.{}", result.capture.backend));
    add_window_signals(&mut output, &result.window);
    output.signals.insert("capture.width".to_string(), result.capture.image.width().to_string());
    output.signals.insert("capture.height".to_string(), result.capture.image.height().to_string());
    // TODO(invoke-window-capture-backend): live testing on 2026-06-18 showed
    // ScreenCaptureKit single-window capture can time out and xcap fallback can
    // fail for Chrome/NetEase windows. Stabilize the typed window capture backend
    // before treating window.* evidence as reliably available.
    //
    // TODO(invoke-capture-contract-artifacts): this records the window screenshot
    // and scalar capture signals, but not a standalone capture-contract artifact.
    // Add it after the direct-invoke contract JSON shape is accepted in
    // `2026-06-18-invoke-direct-command-implementations-handoff.md`.
    output.verification = Some("capture-only; no semantic success claim".to_string());
    output.known_limits.push("window.capture records a resolved window screenshot only; it does not verify UI semantics.".to_string());
    Ok(output)
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

pub async fn capture_selected_window(selector: auv_driver::WindowSelector) -> Result<WindowCapture, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let window = session.window().resolve(selector).map_err(|error| error.to_string())?;
    let capture = session.window().capture(&window).map_err(|error| error.to_string())?;
    if emission_enabled()
      && let Ok(artifact) = png_artifact("auv.driver.window_capture", &capture.image, auv_tracing::Attributes::empty())
    {
      let _ = auv_tracing::emit_artifact!(artifact).await;
    }
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
  summary = "Capture an AX tree snapshot for a target macOS app window.",
  args = WINDOW_ARGS,
)]
async fn capture_ax_tree(_input: InvokeCommandInput) -> InvokeCommandResult {
  capture_ax_tree_snapshot().await?;
  Ok(InvokeCommandOutput::new("captured AX tree"))
}

pub async fn capture_ax_tree_snapshot() -> Result<(), String> {
  // TODO(invoke-window-ax-tree): AX tree capture still lives in the root
  // macOS command adapter; move a typed AX snapshot API before enabling this
  // direct invoke command.
  Err("window.captureAxTree requires a typed AX tree capture API".to_string())
}

#[invoke_command(
  id = "window.findText",
  group = "window",
  summary = "Capture a resolved window and locate OCR text anchors in window pixel space.",
  args = WINDOW_TEXT_ARGS,
)]
async fn find_window_text(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let query = input.required_input("query")?.to_string();
    let result = recognize_window_text(window_selector(&input), query, false).await?;
    Ok(window_text_matches_output(&input.command_id, &result))
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
  summary = "Poll resolved-window OCR until a text anchor appears or the timeout expires.",
  args = WINDOW_TEXT_ARGS,
)]
async fn wait_for_window_text(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let query = input.required_input("query")?.to_string();
    let result = recognize_window_text(window_selector(&input), query, true).await?;
    Ok(window_text_matches_output(&input.command_id, &result))
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
  summary = "Detect visible OCR row bands inside a resolved window.",
  args = WINDOW_ARGS,
)]
async fn find_window_rows(_input: InvokeCommandInput) -> InvokeCommandResult {
  find_window_rows_domain().await?;
  Ok(InvokeCommandOutput::new("found window rows"))
}

pub async fn find_window_rows_domain() -> Result<(), String> {
  // TODO(invoke-window-rows): row-band detection still lives in the root
  // macOS command adapter; move a typed window-row API before enabling this
  // direct invoke command.
  Err("window.findRows requires a typed window row detection API".to_string())
}

#[invoke_command(
  id = "window.waitForRows",
  group = "window",
  summary = "Poll resolved-window row detection until enough rows appear or the timeout expires.",
  args = WINDOW_ARGS,
)]
async fn wait_for_window_rows(_input: InvokeCommandInput) -> InvokeCommandResult {
  wait_for_window_rows_domain().await?;
  Ok(InvokeCommandOutput::new("found window rows after waiting"))
}

pub async fn wait_for_window_rows_domain() -> Result<(), String> {
  // TODO(invoke-window-rows): row wait/polling still lives in the root macOS
  // command adapter; move a typed window-row API before enabling this direct
  // invoke command.
  Err("window.waitForRows requires a typed window row wait API".to_string())
}

#[invoke_command(
  id = "window.observeRegion",
  group = "window",
  summary = "Observe OCR row-like content inside a resolved macOS window region without scrolling.",
  args = WINDOW_ARGS,
)]
async fn observe_window_region(_input: InvokeCommandInput) -> InvokeCommandResult {
  observe_window_region_domain().await?;
  Ok(InvokeCommandOutput::new("observed window region"))
}

pub async fn observe_window_region_domain() -> Result<(), String> {
  // TODO(invoke-window-observe-region): region observation still lives in the
  // root macOS command adapter and needs a typed region/OCR result API before
  // this direct invoke command can run.
  Err("window.observeRegion requires a typed window region observation API".to_string())
}

#[invoke_command(
  id = "window.findIconMatch",
  group = "window",
  summary = "Match a template image against a resolved macOS window screenshot using NCC and emit a RecognitionResult artifact.",
  args = WINDOW_ARGS,
)]
async fn find_icon_match(_input: InvokeCommandInput) -> InvokeCommandResult {
  find_window_icon_match().await?;
  Ok(InvokeCommandOutput::new("found window icon match"))
}

pub async fn find_window_icon_match() -> Result<(), String> {
  // TODO(invoke-window-icon-match): icon/template matching has no stable typed
  // invoke input contract for the template image and threshold yet; add that
  // API before enabling this command.
  Err("window.findIconMatch requires a typed icon-match API and invoke args".to_string())
}

#[invoke_command(
  id = "window.scrollRegion",
  group = "window",
  summary = "Scroll at the center of a resolved macOS window region and record scroll evidence.",
  args = WINDOW_ARGS,
)]
async fn scroll_window_region(_input: InvokeCommandInput) -> InvokeCommandResult {
  scroll_window_region_domain().await?;
  Ok(InvokeCommandOutput::new("scrolled window region"))
}

pub async fn scroll_window_region_domain() -> Result<(), String> {
  // TODO(invoke-window-scroll-region): WindowApi::scroll exists, but this
  // invoke command exposes only window selection args; add typed region point
  // and delta inputs before enabling direct scroll delivery.
  Err("window.scrollRegion requires direct region point and scroll delta inputs".to_string())
}

#[invoke_command(
  id = "window.verifyText",
  group = "window",
  summary = "Verify that a text-bearing AX node exists in the observed tree without relying on screenshot OCR.",
  args = WINDOW_VERIFY_TEXT_ARGS,
)]
async fn verify_ax_text(_input: InvokeCommandInput) -> InvokeCommandResult {
  verify_window_ax_text().await?;
  Ok(InvokeCommandOutput::new("verified window AX text"))
}

pub async fn verify_window_ax_text() -> Result<(), String> {
  // TODO(invoke-window-verify-ax-text): AX text verification still lives in
  // the root macOS command adapter; move a typed AX text query API before
  // enabling this direct invoke command.
  Err("window.verifyText requires a typed AX text verification API".to_string())
}

#[invoke_command(
  id = "window.clickText",
  group = "window",
  summary = "Capture a resolved window, resolve an OCR text anchor, and click its projected logical point.",
  args = WINDOW_TEXT_ARGS,
)]
async fn click_window_text(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    use auv_driver::{ClickOptions, RatioRect, ScreenPoint};

    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let query = input.required_input("query")?.to_string();
    let result = click_recognized_window_text(window_selector(&input), query).await?;

    let mut output = text_matches_output(&input.command_id, "auv-driver-macos.window.input", &result.matches.matches, Some(0));
    add_window_signals(&mut output, &result.window);
    // TODO(invoke-recognition-result-artifacts): clickText records the OCR source
    // screenshot used for target resolution, but not the structured
    // recognition-result artifact. Add it with window.findText once the
    // direct-invoke recognition artifact shape is accepted.
    output.signals.insert("input.selected_path".to_string(), format!("{:?}", result.action.selected_path));
    output.signals.insert("click.window_x".to_string(), result.point.point().x.to_string());
    output.signals.insert("click.window_y".to_string(), result.point.point().y.to_string());
    output.verification = Some("activation-only; semantic success requires a separate verification result".to_string());
    output
      .known_limits
      .push("window.clickText records OCR resolution and input delivery only; it does not verify post-click UI state.".to_string());
    Ok(output)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("window.clickText is only available on macOS".to_string())
  }
}

#[derive(Clone, Debug)]
pub struct WindowTextClick {
  pub window: auv_driver::Window,
  pub matches: auv_driver::OcrMatches,
  pub point: auv_driver::geometry::WindowPoint,
  pub action: auv_driver::InputActionResult,
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
    if emission_enabled()
      && let Ok(artifact) = png_artifact("auv.driver.window_ocr_source", &capture.image, auv_tracing::Attributes::empty())
    {
      let _ = auv_tracing::emit_artifact!(artifact).await;
    }
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
  summary = "Capture a resolved window, detect visible rows, and click a row-derived projected logical point.",
  args = WINDOW_ARGS,
)]
async fn click_window_row(_input: InvokeCommandInput) -> InvokeCommandResult {
  click_window_row_domain().await?;
  Ok(InvokeCommandOutput::new("clicked window row"))
}

pub async fn click_window_row_domain() -> Result<(), String> {
  // TODO(invoke-window-rows): click-row depends on typed row-band detection
  // plus row-to-click-point policy; move that API before enabling this direct
  // invoke command.
  Err("window.clickRow requires a typed window row click API".to_string())
}

#[derive(Clone, Debug)]
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
      // source screenshot and scalar match signals, but not a structured
      // recognition-result artifact with query/bounds/confidence. Add it after
      // the artifact shape is accepted in the direct-command handoff.
      if emission_enabled()
        && let Ok(artifact) = png_artifact("auv.driver.window_ocr_source", &capture.image, auv_tracing::Attributes::empty())
      {
        let _ = auv_tracing::emit_artifact!(artifact).await;
      }
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
fn window_text_matches_output(command_id: &str, result: &WindowTextRecognition) -> InvokeCommandOutput {
  let mut output = text_matches_output(command_id, "auv-driver-macos.window.vision", &result.matches.matches, None);
  add_window_signals(&mut output, &result.window);
  output.verification = Some("recognition-only; no semantic success claim".to_string());
  output
    .known_limits
    .push("window OCR recognition records text matches and source screenshot only; it does not verify downstream UI state.".to_string());
  output
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

fn dry_run_output(command_id: &str) -> InvokeCommandOutput {
  InvokeCommandOutput::new(format!("dry run: {command_id}"))
}

#[cfg(target_os = "macos")]
fn add_window_signals(output: &mut InvokeCommandOutput, window: &auv_driver::Window) {
  output.signals.insert("window.id".to_string(), window.reference.id.clone());
  if let Some(title) = &window.title {
    output.signals.insert("window.title".to_string(), title.clone());
  }
  if let Some(app_name) = &window.app_name {
    output.signals.insert("window.app_name".to_string(), app_name.clone());
  }
  if let Some(bundle_id) = &window.app_bundle_id {
    output.signals.insert("window.app_bundle_id".to_string(), bundle_id.clone());
  }
}

fn window_list_output(windows: &[auv_driver::Window]) -> InvokeCommandOutput {
  let mut output = InvokeCommandOutput::new(format!("listed {} window(s)", windows.len()));
  output.report = Some(window_list_report(windows));
  output
}

fn window_list_report(windows: &[auv_driver::Window]) -> InvokeReport {
  InvokeReport {
    fields: vec![InvokeReportField::new(
      "Result",
      format!("{} window(s)", windows.len()),
    )],
    tables: vec![InvokeReportTable::from_columns_with_display_max_chars(
      &["REF", "APP", "TITLE", "FRAME"],
      windows
        .iter()
        .map(|window| {
          InvokeReportTableRow::from_cells([
            window.reference.id.clone(),
            window.app_name.as_deref().report_or("unknown").to_string(),
            window.title.as_deref().report_or("untitled").to_string(),
            window.frame.report_value(),
          ])
        })
        .collect(),
      vec![None, Some(18), Some(40), None],
    )],
    wide_tables: vec![InvokeReportTable::from_columns_with_display_max_chars(
      &["REF", "APP", "TITLE", "FRAME", "BUNDLE", "PID", "FLAGS"],
      windows
        .iter()
        .map(|window| {
          InvokeReportTableRow::from_cells([
            window.reference.id.clone(),
            window.app_name.as_deref().report_or("unknown").to_string(),
            window.title.as_deref().report_or("untitled").to_string(),
            window.frame.report_value(),
            window.app_bundle_id.as_deref().report_or("unknown").to_string(),
            window.process_id.map(|pid| pid.to_string()).unwrap_or_else(|| "unknown".to_string()),
            window_flags(window),
          ])
        })
        .collect(),
      vec![None, Some(18), Some(40), None, Some(32), None, None],
    )],
    sections: Vec::new(),
  }
}

fn window_flags(window: &auv_driver::Window) -> String {
  let mut flags = Vec::new();
  if window.is_main {
    flags.push("main");
  }
  if window.is_visible {
    flags.push("visible");
  } else {
    flags.push("hidden");
  }
  flags.report_labels()
}

#[cfg(target_os = "macos")]
fn text_matches_output(
  command_id: &str,
  backend: &str,
  matches: &[auv_driver::OcrMatch],
  selected_index: Option<usize>,
) -> InvokeCommandOutput {
  let count = matches.len();
  let mut output = InvokeCommandOutput::new(format!("{command_id} matched {count} text region(s)"));
  output.backend = Some(backend.to_string());
  output.signals.insert("match.count".to_string(), count.to_string());
  if let Some(best_text) = matches.first() {
    output.signals.insert("match.best_text".to_string(), best_text.text.clone());
  }
  output.report = Some(crate::commands::ocr::match_report(matches, selected_index));
  output
}

#[cfg(test)]
mod tests {
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

    let output = window_list_output(&windows);
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
  }

  #[test]
  fn window_list_report_preserves_full_cell_values_for_machine_output() {
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

    let output = window_list_output(&windows);
    let report = output.report.as_ref().expect("window.list should expose a report");

    assert_eq!(report.tables[0].rows[0].cells[1], long_app_name);
    assert_eq!(report.tables[0].rows[0].cells[2], long_title);
    assert_eq!(report.wide_tables[0].rows[0].cells[4], long_bundle_id);
  }
}
