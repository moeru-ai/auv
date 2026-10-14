use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportTable,
  InvokeReportValue, OptionalReportText, invoke_command,
};
use auv_cli_common::{TableRow, outputs::formats::table::TableOptions};
use auv_driver::ScreenPoint;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use auv_driver::WindowInput as _;
use auv_driver::overlay::{
  Overlay,
  components::{CaptureFrame, ClickTarget},
  layers::Outline,
  style::{Insets, OutlineStyle},
};
use auv_tracing::ArtifactMetadata;
use clap::{Args, ValueEnum};
use std::time::Duration;

use crate::artifact::{emit_capture, emit_capture_with_receipt};

pub fn group() -> CommandGroup {
  // TODO(invoke-window-stubs): incomplete window commands stay intentionally
  // unregistered until owner-approved implementations have behavioral evidence.
  CommandGroup::new("window", "WINDOW")
    .command(list_windows_invoke_command())
    .command(capture_window_invoke_command())
    .command(find_window_text_invoke_command())
    .command(wait_for_window_text_invoke_command())
    .command(click_window_text_invoke_command())
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke window.list --wide")]
struct ListWindowsArgs {}

#[invoke_command(
  id = "window.list",
  support = { macos: Supported, linux: Supported, windows: Supported },
  group = "window",
  description = "List visible window candidates using the normalized AUV window selector model.",
  input = ListWindowsArgs,
)]
async fn list_windows(input: InvokeCommandInput, _args: ListWindowsArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let windows = observe_windows().await?;
    Ok(list_windows_output(&windows)?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = input;
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.list is only available on macOS, Linux and Windows"))
  }
}

/// Builds the transport-independent direct result for `window.list`.
///
/// Local and daemon-backed frontends use this same projection so selecting a
/// Device changes placement without changing the command result schema.
pub fn list_windows_output(windows: &[auv_driver::Window]) -> InvokeCommandResult {
  Ok(InvokeCommandOutput::from_result(windows)?.with_report(window_list_report(windows)))
}

pub async fn observe_windows() -> Result<Vec<auv_driver::Window>, crate::InvokeFailure> {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
    Ok(session.window().list().map_err(|error| error.to_string())?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.list is only available on macOS, Linux and Windows"))
  }
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke window.capture --title Untitled\n  auv invoke window.capture --target com.apple.TextEdit --title Untitled\n  auv invoke window.capture --target notepad.exe\n\nOn Linux, app targets match the AT-SPI AccessibleId in window.list (BUNDLE in --wide output). Use --title when that field is absent. Linux captures a display crop and does not activate or unocclude the window.\n\nOn Windows, an app target is the executable file name shown as APP in window.list, matched exactly and ignoring case (for example notepad.exe). Windows renders the window itself with PrintWindow, without activating, raising or restoring it, and keeps the visible frame."
)]
struct CaptureWindowArgs {
  /// Window title text used to select the capture target.
  #[arg(long, value_name = "TEXT")]
  title: Option<String>,
}

#[invoke_command(
  id = "window.capture",
  support = { macos: Supported, linux: Supported, windows: Supported },
  target = OptionalApplication,
  group = "window",
  description = "Capture one single-display window without activating it and emit a coordinate contract. Linux uses an AT-SPI display crop; Windows uses PrintWindow.",
  input = CaptureWindowArgs,
)]
async fn capture_window(input: InvokeCommandInput, args: CaptureWindowArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    let selector = window_selector(&input, args.title.as_deref())?;
    input.overlay_enabled()?;
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let session = auv::local::open().map_err(|error| error.to_string())?;
    let (result, artifact) = capture_selected_window_recorded_with_session(&session, selector).await?;
    input.cancellation.check().map_err(|error| error.to_string())?;
    let capture_overlay = Overlay::new().with_layer(
      CaptureFrame::new(result.window.frame).with_label(result.window.title.clone().unwrap_or_else(|| "selected window".to_string())),
    );
    let overlay = super::overlay::show_overlay(&input, &session, capture_overlay, show_options(120, 180))?;
    let mut output = window_capture_output_with_artifact(&result.window, super::capture_result(&result.capture), artifact)?;
    output.report.as_mut().expect("window capture output always has a report").fields.push(overlay.report_field());
    // TODO(invoke-window-capture-backend): live testing on 2026-06-18 showed
    // ScreenCaptureKit single-window capture can time out and xcap fallback can
    // fail for Chrome/NetEase windows. Stabilize the typed window capture backend
    // before treating window.* evidence as reliably available.
    Ok(output)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.capture is only available on macOS, Linux and Windows"))
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

/// Records and projects a Runner-held window capture. `evidence` is its pixels
/// when this call records artifacts.
pub async fn recorded_window_capture_output(
  window: &auv_driver::Window,
  capture: super::CaptureResult<'_>,
  evidence: Option<&auv_driver::Capture>,
) -> InvokeCommandResult {
  let artifact = match evidence {
    Some(capture) => emit_capture_with_receipt("auv.driver.window_capture", capture).await,
    None => None,
  };
  window_capture_output_with_artifact(window, capture, artifact)
}

fn window_capture_output_with_artifact(
  window: &auv_driver::Window,
  capture: super::CaptureResult<'_>,
  artifact: Option<ArtifactMetadata>,
) -> InvokeCommandResult {
  let pixel_size = capture.pixel_size_report();
  let mut output = InvokeCommandOutput::from_result(&WindowCaptureResult { window, capture })?;
  let mut fields = window_report_fields(window);
  fields.push(InvokeReportField::new("Pixel size", pixel_size));
  output.report = Some(InvokeReport::new(fields, Vec::new()));
  Ok(output.with_artifacts(artifact))
}

pub async fn capture_selected_window(selector: auv_driver::WindowSelector) -> Result<WindowCapture, crate::InvokeFailure> {
  capture_selected_window_recorded(selector).await.map(|(capture, _)| capture)
}

async fn capture_selected_window_recorded(
  selector: auv_driver::WindowSelector,
) -> Result<(WindowCapture, Option<ArtifactMetadata>), crate::InvokeFailure> {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
    Ok(capture_selected_window_recorded_with_session(&session, selector).await?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = selector;
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.capture is only available on macOS, Linux and Windows"))
  }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
async fn capture_selected_window_recorded_with_session(
  session: &auv_driver::LocalDriverSession,
  selector: auv_driver::WindowSelector,
) -> Result<(WindowCapture, Option<ArtifactMetadata>), String> {
  let window = session.window().resolve(selector).map_err(|error| error.to_string())?;
  let capture = session.window().capture(&window).map_err(|error| error.to_string())?;
  // NOTICE: window is the selector snapshot; a Driver can refresh geometry
  // before capture. The captured region is authoritative in capture.bounds.
  // TODO: descriptor refresh remains deferred until an owner-approved result
  // contract defines how the resolved and captured observations agree.
  let artifact = emit_capture_with_receipt("auv.driver.window_capture", &capture).await;
  Ok((WindowCapture { window, capture }, artifact))
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke window.findText \"Settings\" --title Preferences\n\nOn Windows, an app target is the executable file name shown as APP in window.list (for example notepad.exe). Text comes from the system OCR of a PrintWindow capture of the window itself."
)]
struct FindWindowTextArgs {
  /// Text to locate in the selected window.
  #[arg(value_name = "TEXT")]
  query: String,
  /// Window title text used to select the capture target.
  #[arg(long, value_name = "TITLE")]
  title: Option<String>,
}

#[invoke_command(
  id = "window.findText",
  support = { macos: Supported, linux: Unsupported, windows: Supported },
  target = OptionalApplication,
  group = "window",
  description = "Capture a resolved window and locate OCR text anchors in logical screen coordinates.",
  input = FindWindowTextArgs,
)]
async fn find_window_text(input: InvokeCommandInput, args: FindWindowTextArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "windows"))]
  {
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let query = args.query;
    let session = auv::local::open().map_err(|error| error.to_string())?;
    let result = recognize_window_text_with_session(&session, window_selector(&input, args.title.as_deref())?, query, false).await?;
    let overlay = super::overlay::show_overlay(&input, &session, window_text_overlay(&result.matches, None), show_options(120, 420))?;
    Ok(window_text_matches_output(&input.command_id, &result, overlay)?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window text OCR is only available on macOS and Windows"))
  }
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke window.waitForText \"Ready\" --title Import\n\nOn Windows, an app target is the executable file name shown as APP in window.list (for example notepad.exe)."
)]
struct WaitForWindowTextArgs {
  /// Text to wait for in the selected window.
  #[arg(value_name = "TEXT")]
  query: String,
  /// Window title text used to select the capture target.
  #[arg(long, value_name = "TITLE")]
  title: Option<String>,
}

#[invoke_command(
  id = "window.waitForText",
  support = { macos: Supported, linux: Unsupported, windows: Supported },
  target = OptionalApplication,
  group = "window",
  description = "Poll resolved-window OCR until a text anchor appears or the timeout expires.",
  input = WaitForWindowTextArgs,
)]
async fn wait_for_window_text(input: InvokeCommandInput, args: WaitForWindowTextArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "windows"))]
  {
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let query = args.query;
    let session = auv::local::open().map_err(|error| error.to_string())?;
    let result = recognize_window_text_with_session(&session, window_selector(&input, args.title.as_deref())?, query, true).await?;
    let overlay = super::overlay::show_overlay(&input, &session, window_text_overlay(&result.matches, None), show_options(120, 420))?;
    Ok(window_text_matches_output(&input.command_id, &result, overlay)?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window text OCR is only available on macOS and Windows"))
  }
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke window.clickText \"Continue\" --title Setup\n  auv invoke window.clickText \"AGENTS.md\" --index 1"
)]
struct ClickWindowTextArgs {
  /// Text anchor to click in the selected window.
  #[arg(value_name = "TEXT")]
  query: String,
  /// Window title text used to select the capture target.
  #[arg(long, value_name = "TITLE")]
  title: Option<String>,
  /// Zero-based OCR candidate index from this invocation's capture.
  #[arg(long, value_name = "INDEX", default_value_t = 0)]
  #[serde(default)]
  index: usize,
  /// Driver policy used to deliver the click.
  #[arg(long, value_enum)]
  #[serde(rename = "input-policy")]
  input_policy: Option<WindowClickPolicyArg>,
  /// Number of clicks to deliver.
  #[arg(long, value_parser = clap::value_parser!(u8).range(1..))]
  #[serde(
    rename = "click-count",
    deserialize_with = "crate::command::deserialize_optional_nonzero_u8",
    default
  )]
  click_count: Option<u8>,
  /// Delay between repeated clicks.
  #[arg(long)]
  #[serde(rename = "click-interval-ms")]
  click_interval_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WindowClickPolicyArg {
  BackgroundOnly,
  BackgroundPreferred,
  ForegroundPreferred,
}

// Only `window.clickText` reads it.
#[cfg(any(target_os = "macos", target_os = "windows"))]
impl WindowClickPolicyArg {
  fn driver_policy(self) -> auv_driver::InputPolicy {
    match self {
      Self::BackgroundOnly => auv_driver::InputPolicy::BackgroundOnly,
      Self::BackgroundPreferred => auv_driver::InputPolicy::BackgroundPreferred,
      Self::ForegroundPreferred => auv_driver::InputPolicy::ForegroundPreferred,
    }
  }
}

#[invoke_command(
  id = "window.clickText",
  support = { macos: Supported, linux: Unsupported, windows: Partial("test-owned Win32 surface, background-only") },
  target = OptionalApplication,
  group = "window",
  description = "Capture a resolved window, resolve an OCR text anchor, and click the match point in logical screen coordinates.",
  input = ClickWindowTextArgs,
)]
async fn click_window_text(input: InvokeCommandInput, args: ClickWindowTextArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "windows"))]
  {
    let options =
      super::input::click_options(args.input_policy.map(WindowClickPolicyArg::driver_policy), args.click_count, args.click_interval_ms);
    if input.dry_run {
      return Ok(super::input::validation_only_output());
    }

    let session = auv::local::open().map_err(crate::InvokeFailure::from)?;
    let result =
      click_recognized_window_text_with_session(&session, window_selector(&input, args.title.as_deref())?, args.query, args.index, options)
        .await?;
    let overlay = super::overlay::show_overlay(
      &input,
      &session,
      window_text_overlay(&result.matches, Some(result.selected_index)),
      show_options(120, 240),
    )?;

    let mut output = window_text_click_output_base(&result)?;
    output.report.as_mut().expect("window text click output always has a report").fields.push(overlay.report_field());
    Ok(output)
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.clickText is only available on macOS and Windows"))
  }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct WindowTextClick {
  pub window: auv_driver::Window,
  pub matches: auv_driver::OcrMatches,
  pub selected_index: usize,
  pub point: auv_driver::geometry::ScreenPoint,
  /// The same point projected into the window, which is the form the driver
  /// received for delivery.
  pub window_point: auv_driver::geometry::WindowPoint,
  pub options: auv_driver::ClickOptions,
  pub action: auv_driver::InputActionResult,
}

/// Records the exact OCR source and typed delivery evidence, then builds the
/// transport-independent `window.clickText` result.
pub fn recorded_window_text_click_output(result: &WindowTextClick, ocr_source: Option<&auv_driver::Capture>) -> InvokeCommandResult {
  if let Some(capture) = ocr_source {
    emit_capture("auv.driver.window_ocr_source", capture);
  }
  super::input::emit_input_action_result(&result.action);
  window_text_click_output_base(result)
}

fn window_text_click_output_base(result: &WindowTextClick) -> InvokeCommandResult {
  let mut report = crate::commands::ocr::match_report(&result.matches.matches, Some(result.selected_index));
  report.fields.extend(window_report_fields(&result.window));
  report.fields.extend(super::input::input_action_report_fields(&result.action));
  report.fields.push(InvokeReportField::new("Input policy", result.options.policy.as_str()));
  report.fields.push(InvokeReportField::new("Click count", result.options.click.count().to_string()));
  if let Some(interval) = result.options.click.interval() {
    report.fields.push(InvokeReportField::new("Click interval", format!("{} ms", interval.as_millis())));
  }
  report.fields.push(InvokeReportField::new("Screen point", format!("{:.1},{:.1}", result.point.point().x, result.point.point().y)));
  report
    .fields
    .push(InvokeReportField::new("Window point", format!("{:.1},{:.1}", result.window_point.point().x, result.window_point.point().y)));
  Ok(InvokeCommandOutput::from_result(result)?.with_report(report))
}

pub async fn click_recognized_window_text(
  selector: auv_driver::WindowSelector,
  query: String,
) -> Result<WindowTextClick, crate::InvokeFailure> {
  click_recognized_window_text_with_options(selector, query, auv_driver::ClickOptions::default()).await
}

pub async fn click_recognized_window_text_with_options(
  selector: auv_driver::WindowSelector,
  query: String,
  options: auv_driver::ClickOptions,
) -> Result<WindowTextClick, crate::InvokeFailure> {
  click_recognized_window_text_with_index_and_options(selector, query, 0, options).await
}

pub async fn click_recognized_window_text_with_index_and_options(
  selector: auv_driver::WindowSelector,
  query: String,
  index: usize,
  options: auv_driver::ClickOptions,
) -> Result<WindowTextClick, crate::InvokeFailure> {
  #[cfg(any(target_os = "macos", target_os = "windows"))]
  {
    let session = auv::local::open().map_err(crate::InvokeFailure::from)?;
    click_recognized_window_text_with_session(&session, selector, query, index, options).await
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    let _ = (selector, query, index, options);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.clickText is only available on macOS and Windows"))
  }
}

pub fn selected_window_text_match<'a>(
  matches: &'a auv_driver::OcrMatches,
  query: &str,
  index: usize,
) -> Result<&'a auv_driver::OcrMatch, String> {
  if matches.matches.is_empty() {
    return Err(format!("window.clickText did not find text {query:?}"));
  }
  matches
    .matches
    .get(index)
    .ok_or_else(|| format!("window.clickText --index {index} is out of range for {} text match(es)", matches.matches.len()))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
async fn click_recognized_window_text_with_session(
  session: &auv_driver::LocalDriverSession,
  selector: auv_driver::WindowSelector,
  query: String,
  index: usize,
  options: auv_driver::ClickOptions,
) -> Result<WindowTextClick, crate::InvokeFailure> {
  let window = session.window().resolve(selector).map_err(crate::InvokeFailure::from)?;
  let capture = session.window().capture(&window).map_err(crate::InvokeFailure::from)?;
  let matches = session
    .vision()
    .find_text_in_capture(&capture, &query, auv_driver::RelativeRect::new(0.0, 0.0, 1.0, 1.0))
    .map_err(crate::InvokeFailure::from)?;
  if matches.matches.is_empty() {
    return Err(crate::InvokeFailure::new(crate::FailureCode::NotFound, format!("window.clickText did not find text {query:?}")));
  }
  let matched = selected_window_text_match(&matches, &query, index)
    .map_err(|error| crate::InvokeFailure::new(crate::FailureCode::InvalidInput, error))?;
  let point = auv_driver::geometry::ScreenPoint::from(matched.action_point());
  let window_point = session.window().to_window_point(&window, point).map_err(crate::InvokeFailure::from)?;
  let action = session.window().click(&window, window_point, options.clone()).map_err(crate::InvokeFailure::from)?;
  emit_capture("auv.driver.window_ocr_source", &capture);
  Ok(WindowTextClick {
    window,
    matches,
    selected_index: index,
    point,
    window_point,
    options,
    action,
  })
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct WindowTextRecognition {
  pub window: auv_driver::Window,
  pub matches: auv_driver::OcrMatches,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub async fn recognize_window_text(
  selector: auv_driver::WindowSelector,
  query: String,
  wait: bool,
) -> Result<WindowTextRecognition, crate::InvokeFailure> {
  let session = auv::local::open().map_err(|error| error.to_string())?;
  Ok(recognize_window_text_with_session(&session, selector, query, wait).await?)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
async fn recognize_window_text_with_session(
  session: &auv_driver::LocalDriverSession,
  selector: auv_driver::WindowSelector,
  query: String,
  wait: bool,
) -> Result<WindowTextRecognition, String> {
  use auv_driver::{RelativeRect, WaitOptions};
  use std::{thread, time::Instant};

  let window = session.window().resolve(selector).map_err(|error| error.to_string())?;
  let wait_options = WaitOptions::default();
  let started = Instant::now();
  loop {
    let capture = session.window().capture(&window).map_err(|error| error.to_string())?;
    let matches =
      session.vision().find_text_in_capture(&capture, &query, RelativeRect::new(0.0, 0.0, 1.0, 1.0)).map_err(|error| error.to_string())?;
    if !matches.matches.is_empty() || !wait || started.elapsed() >= wait_options.timeout {
      if wait && matches.matches.is_empty() {
        return Err(format!("window.waitForText did not find text {query:?} before timeout"));
      }

      // TODO(invoke-recognition-result-artifacts): this records the window OCR
      // source screenshot and typed OCR matches, but not a structured
      // recognition-result artifact with query/bounds/confidence. Add it after
      // the artifact shape is accepted in the direct-command handoff.
      emit_capture("auv.driver.window_ocr_source", &capture);
      return Ok(WindowTextRecognition { window, matches });
    }
    thread::sleep(wait_options.poll_interval);
  }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub async fn recognize_window_text(
  _selector: auv_driver::WindowSelector,
  _query: String,
  _wait: bool,
) -> Result<WindowTextRecognition, crate::InvokeFailure> {
  Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window text OCR is only available on macOS and Windows"))
}

fn window_text_matches_output(
  _command_id: &str,
  result: &WindowTextRecognition,
  overlay: super::overlay::OverlayStatus,
) -> InvokeCommandResult {
  let mut output = window_text_matches_output_base(result)?;
  output.report.as_mut().expect("window text output always has a report").fields.push(overlay.report_field());
  Ok(output)
}

/// Records the OCR source and builds the transport-independent
/// `window.findText` result.
pub fn recorded_window_text_matches_output(result: &WindowTextRecognition, ocr_source: Option<&auv_driver::Capture>) -> InvokeCommandResult {
  if let Some(capture) = ocr_source {
    emit_capture("auv.driver.window_ocr_source", capture);
  }
  window_text_matches_output_base(result)
}

fn window_text_matches_output_base(result: &WindowTextRecognition) -> InvokeCommandResult {
  let mut report = crate::commands::ocr::match_report(&result.matches.matches, None);
  report.fields.extend(window_report_fields(&result.window));
  Ok(InvokeCommandOutput::from_result(result)?.with_report(report))
}

fn window_text_overlay(matches: &auv_driver::OcrMatches, selected_index: Option<usize>) -> Overlay {
  let mut overlay = Overlay::new();
  for (index, matched) in matches.matches.iter().enumerate() {
    let style = if selected_index == Some(index) {
      OutlineStyle::selected()
    } else {
      OutlineStyle::default()
    };

    overlay =
      overlay.with_layer(Outline::new(matched.bounds).with_label(matched.text.clone()).with_style(style.with_padding(Insets::all(8.0))));
  }

  if let Some(matched) = selected_index.and_then(|index| matches.matches.get(index)) {
    overlay = overlay.with_layer(
      ClickTarget::new(ScreenPoint::from(matched.action_point())).with_cursor_label("auv · click").with_status("text click delivered"),
    );
  }
  overlay
}

fn show_options(motion_ms: u64, auto_removal_ms: u64) -> auv_driver::overlay::ShowOptions {
  auv_driver::overlay::ShowOptions::new()
    .with_motion_ease(Duration::from_millis(motion_ms), auv_driver::overlay::Easing::EaseInOutExpo)
    .with_auto_removal_after(Duration::from_millis(auto_removal_ms))
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn window_selector(input: &InvokeCommandInput, title: Option<&str>) -> Result<auv_driver::WindowSelector, String> {
  use auv_driver::{App, TextMatcher, WindowSelector};

  let mut selector = WindowSelector {
    main_visible: true,
    ..WindowSelector::default()
  };
  if let Some(target) = input.application_target()? {
    // NOTICE: Windows has no bundle id (`app_bundle_id` is always `None`), so a bundle selector
    // can never match there. Its application identifier is the executable file name, the
    // `app_name` of `window.list`, which the driver matches ignoring case.
    #[cfg(target_os = "windows")]
    {
      selector.app = Some(App::name(target));
    }
    // Linux reuses AccessibleId in app_bundle_id. Do not guess application
    // names when that identifier is absent; callers can select by title.
    // TODO(cross-platform-application-selector): application-name targets
    // require an owner-approved target contract, as in runner.rs.
    #[cfg(not(target_os = "windows"))]
    {
      selector.app = Some(App::bundle_id(target));
    }
  }
  if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
    selector.title = Some(TextMatcher::Contains(title.to_string()));
  }
  Ok(selector)
}

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

#[derive(TableRow)]
struct WindowRow {
  #[table(header = "REF")]
  reference: String,
  app: String,
  title: String,
  frame: String,
  #[table(wide)]
  bundle: String,
  #[table(wide, header = "PID")]
  process_id: String,
  #[table(wide)]
  flags: String,
}

fn window_list_report(windows: &[auv_driver::Window]) -> InvokeReport {
  let rows = windows
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
      WindowRow {
        reference: window.reference.id.clone(),
        app: window.app_name.as_deref().report_or("unknown").to_string(),
        title: window.title.as_deref().report_or("untitled").to_string(),
        frame: window.frame.report_value(),
        bundle: window.app_bundle_id.as_deref().report_or("unknown").to_string(),
        process_id: window.process_id.map(|pid| pid.to_string()).unwrap_or_else(|| "unknown".to_string()),
        flags: flags.join(","),
      }
    })
    .collect::<Vec<_>>();
  InvokeReport {
    fields: vec![InvokeReportField::new(
      "Result",
      format!("{} window(s)", windows.len()),
    )],
    tables: vec![InvokeReportTable::from_rows(&rows, TableOptions::default()).with_display_max_chars(vec![None, Some(18), Some(40), None])],
    wide_tables: vec![
      InvokeReportTable::from_rows(&rows, TableOptions::default().wide(true)).with_display_max_chars(vec![
        None,
        Some(18),
        Some(40),
        None,
        Some(32),
        None,
        None,
      ]),
    ],
    sections: Vec::new(),
  }
}

#[cfg(test)]
#[path = "window_test.rs"]
mod tests;
