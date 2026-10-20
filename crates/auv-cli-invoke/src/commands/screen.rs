use crate::{CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, invoke_command};
use auv_tracing::ArtifactMetadata;
use clap::Args;

use crate::artifact::{emit_capture, emit_capture_with_receipt};

pub fn group() -> CommandGroup {
  // TODO(invoke-screen-stubs): row and image commands stay intentionally
  // unregistered until owner-approved implementations have behavioral evidence.
  CommandGroup::new("screen", "SCREEN")
    .command(capture_region_invoke_command())
    .command(find_screen_text_invoke_command())
    .command(wait_for_screen_text_invoke_command())
    .command(click_screen_text_invoke_command())
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke screen.captureRegion --x 0 --y 0 --width 800 --height 600")]
struct CaptureRegionArgs {
  /// Region left coordinate in logical display space.
  #[arg(long)]
  x: f64,
  /// Region top coordinate in logical display space.
  #[arg(long)]
  y: f64,
  /// Region width; must be greater than zero.
  #[arg(long)]
  width: f64,
  /// Region height; must be greater than zero.
  #[arg(long)]
  height: f64,
  /// Human-readable capture label.
  #[arg(long)]
  label: Option<String>,
}

#[invoke_command(
  id = "screen.captureRegion",
  support = { macos: Supported, linux: Supported, windows: Supported },
  group = "screen",
  description = "Capture one display-contained region and emit its coordinate contract.",
  input = CaptureRegionArgs,
)]
async fn capture_region(input: InvokeCommandInput, args: CaptureRegionArgs) -> InvokeCommandResult {
  reject_target_activation(&input, "screen.captureRegion")?;
  if !args.x.is_finite() || !args.y.is_finite() {
    return Err("screen.captureRegion requires finite --x and --y".to_string());
  }
  if !args.width.is_finite() || !args.height.is_finite() || args.width <= 0.0 || args.height <= 0.0 {
    return Err("screen.captureRegion requires --width and --height greater than zero".to_string());
  }
  let region = auv_driver::Rect::new(args.x, args.y, args.width, args.height);
  input.cancellation.check().map_err(|error| error.to_string())?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }

  #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
  {
    let (capture, artifact) = capture_screen_region_recorded(region).await?;
    region_capture_output(&capture.display, super::capture_result(&capture.capture), artifact)
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
  {
    let _ = region;
    Err("screen.captureRegion is unavailable on this platform".to_string())
  }
}

pub async fn capture_screen_region(region: auv_driver::Rect) -> Result<auv_driver::RegionCapture, String> {
  capture_screen_region_recorded(region).await.map(|(capture, _)| capture)
}

async fn capture_screen_region_recorded(region: auv_driver::Rect) -> Result<(auv_driver::RegionCapture, Option<ArtifactMetadata>), String> {
  #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
    let capture = session
      .display()
      .capture_region(auv_driver::CaptureOptions {
        region: Some(region),
        ..auv_driver::CaptureOptions::default()
      })
      .map_err(|error| error.to_string())?;
    let artifact = emit_capture_with_receipt("auv.driver.screen_region_capture", &capture.capture).await;
    Ok((capture, artifact))
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
  {
    let _ = region;
    Err("screen.captureRegion is unavailable on this platform".to_string())
  }
}

fn region_capture_output(
  display: &auv_driver::Display,
  capture: super::CaptureResult<'_>,
  artifact: Option<ArtifactMetadata>,
) -> InvokeCommandResult {
  let pixel_size = capture.pixel_size_report();
  let mut output = InvokeCommandOutput::from_result(&super::display_capture_result(display, capture))?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Display ID", display.id.clone()),
      InvokeReportField::new("Pixel size", pixel_size),
    ],
    Vec::new(),
  ));
  Ok(output.with_artifacts(artifact))
}

/// Records and projects a Runner-held region capture. `evidence` is its pixels
/// when this call records artifacts.
pub async fn recorded_region_capture_output(
  display: &auv_driver::Display,
  capture: super::CaptureResult<'_>,
  evidence: Option<&auv_driver::Capture>,
) -> InvokeCommandResult {
  let artifact = match evidence {
    Some(capture) => emit_capture_with_receipt("auv.driver.screen_region_capture", capture).await,
    None => None,
  };
  region_capture_output(display, capture, artifact)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  # Find text on the current screen\n  auv invoke screen.findText \"Settings\"\n\n  # Emit OCR matches as JSON\n  auv invoke screen.findText \"Settings\" --json\n\nWindows reads the primary display only. Text on another display is not found."
)]
struct FindScreenTextArgs {
  /// Text to locate in the captured image.
  #[arg(value_name = "TEXT")]
  query: String,
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:
  auv invoke screen.waitForText \"Ready\"

  # Wait up to 30 seconds for a slow page
  auv invoke screen.waitForText \"Order confirmed\" --timeout-ms 30000

Windows reads the primary display only. Text on another display is not found.")]
pub(crate) struct WaitForScreenTextArgs {
  /// Text to wait for in the captured image.
  #[arg(value_name = "TEXT")]
  query: String,
  /// Milliseconds to keep polling before failing (default 10000, 100..=120000).
  #[arg(long = "timeout-ms", value_name = "MS")]
  #[serde(rename = "timeout-ms", default)]
  timeout_ms: Option<u64>,
  /// Milliseconds between OCR attempts (default 250, 50..=10000).
  #[arg(long = "interval-ms", value_name = "MS")]
  #[serde(rename = "interval-ms", default)]
  interval_ms: Option<u64>,
}

/// Validates a text wait budget shared by screen and window waits.
///
/// NOTICE: The shared 5 s `WaitOptions` default was too short for page
/// loads: OSWorld agents saw text appear 10-85 s after a failed wait, and 49
/// calls passed a `timeout` key that did not exist and was ignored.
pub(crate) fn text_wait_options(timeout_ms: Option<u64>, interval_ms: Option<u64>) -> Result<auv_driver::WaitOptions, String> {
  let timeout_ms = timeout_ms.unwrap_or(10_000);
  let interval_ms = interval_ms.unwrap_or(250);
  if !(100..=120_000).contains(&timeout_ms) {
    return Err(format!("timeout-ms must be within 100..=120000, got {timeout_ms}"));
  }
  if !(50..=10_000).contains(&interval_ms) {
    return Err(format!("interval-ms must be within 50..=10000, got {interval_ms}"));
  }
  Ok(auv_driver::WaitOptions {
    timeout: std::time::Duration::from_millis(timeout_ms),
    poll_interval: std::time::Duration::from_millis(interval_ms),
  })
}

/// The invoke text wait defaults: 10 s, polling every 250 ms.
pub(crate) fn default_text_wait_options() -> auv_driver::WaitOptions {
  text_wait_options(None, None).expect("the default text wait budget is in range")
}

impl WaitForScreenTextArgs {
  fn options(&self) -> Result<auv_driver::WaitOptions, String> {
    text_wait_options(self.timeout_ms, self.interval_ms)
  }

  /// Decodes the query and wait budget from protocol inputs (Runner path).
  pub(crate) fn decode(input: &InvokeCommandInput) -> Result<(String, auv_driver::WaitOptions), String> {
    let args = crate::command::decode_args::<Self>(input)?;
    let options = args.options()?;
    Ok((args.query, options))
  }
}

#[invoke_command(
  id = "screen.findText",
  support = { macos: Supported, linux: Supported, windows: Partial("primary display only") },
  group = "screen",
  description = "Capture a screenshot and locate OCR text anchors in logical screen coordinates. Target activation is not yet available for this command.",
  input = FindScreenTextArgs,
)]
async fn find_screen_text(input: InvokeCommandInput, args: FindScreenTextArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    reject_target_activation(&input, "screen.findText")?;
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let matches = recognize_screen_text_with_cancellation(args.query, None, &input.cancellation).await?;
    Ok(screen_text_matches_output(&matches)?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "screen text OCR is only available on macOS, Linux and Windows"))
  }
}

#[invoke_command(
  id = "screen.waitForText",
  support = { macos: Supported, linux: Supported, windows: Partial("primary display only") },
  group = "screen",
  description = "Poll live-desktop OCR until a text anchor appears or the timeout expires. Target activation is not yet available for this command.",
  input = WaitForScreenTextArgs,
)]
async fn wait_for_screen_text(input: InvokeCommandInput, args: WaitForScreenTextArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    reject_target_activation(&input, "screen.waitForText")?;
    let options = args.options().map_err(|message| crate::InvokeFailure::new(crate::FailureCode::InvalidInput, message))?;
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let matches = recognize_screen_text_with_cancellation(args.query.clone(), Some(options), &input.cancellation).await?;
    Ok(screen_text_matches_output(&matches)?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "screen text OCR is only available on macOS, Linux and Windows"))
  }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub async fn recognize_screen_text(query: String, wait: bool) -> Result<auv_driver::OcrMatches, crate::InvokeFailure> {
  let wait = wait.then(default_text_wait_options);
  Ok(recognize_screen_text_with_cancellation(query, wait, &Default::default()).await?)
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
async fn recognize_screen_text_with_cancellation(
  query: String,
  wait: Option<auv_driver::WaitOptions>,
  cancellation: &crate::InvokeCancellation,
) -> Result<auv_driver::OcrMatches, String> {
  use auv_driver::{CaptureOptions, RelativeRect};

  cancellation.check().map_err(|error| error.to_string())?;
  let session = auv::local::open().map_err(|error| error.to_string())?;
  let observe = || async {
    // NOTICE: capture and OCR are synchronous Driver calls. Cancellation is
    // observed between calls; an active call retains its Driver timeout.
    cancellation.check().map_err(|error| error.to_string())?;
    let capture = session.display().capture(CaptureOptions::default()).map_err(|error| error.to_string())?;
    cancellation.check().map_err(|error| error.to_string())?;
    let matches = session
      .vision()
      .find_text_in_capture(&capture.capture, &query, RelativeRect::new(0.0, 0.0, 1.0, 1.0))
      .map_err(|error| error.to_string())?;
    cancellation.check().map_err(|error| error.to_string())?;
    Ok((matches, capture))
  };
  let (matches, capture) = if let Some(options) = wait {
    crate::runner::wait_for_selected_text("screen.waitForText", &query, options, cancellation, observe, |(matches, _)| {
      !matches.matches.is_empty()
    })
    .await?
  } else {
    observe().await?
  };
  // TODO(invoke-recognition-result-artifacts): this records the OCR source
  // screenshot and typed OCR matches, but not a structured
  // recognition-result artifact with query/bounds/confidence. Add that
  // after the artifact shape is accepted in the direct-command handoff.
  emit_capture("auv.driver.screen_ocr_source", &capture.capture);
  Ok(matches)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub async fn recognize_screen_text(_query: String, _wait: bool) -> Result<auv_driver::OcrMatches, crate::InvokeFailure> {
  Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "screen text OCR is only available on macOS, Linux and Windows"))
}

fn screen_text_matches_output(matches: &auv_driver::OcrMatches) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(matches)?;
  output.report = Some(crate::commands::ocr::match_report(&matches.matches, None));
  Ok(output)
}

/// Records the OCR source and builds the transport-independent
/// `screen.findText` result.
pub fn recorded_screen_text_matches_output(
  matches: &auv_driver::OcrMatches,
  ocr_source: Option<&auv_driver::Capture>,
) -> InvokeCommandResult {
  if let Some(capture) = ocr_source {
    emit_capture("auv.driver.screen_ocr_source", capture);
  }
  screen_text_matches_output(matches)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke screen.clickText \"Continue\"\n\nWindows reads the primary display only. This command moves the real pointer and clicks in the foreground."
)]
struct ClickScreenTextArgs {
  /// Text anchor to click.
  #[arg(value_name = "TEXT")]
  query: String,
}

#[invoke_command(
  id = "screen.clickText",
  support = { macos: Supported, linux: Unsupported, windows: Partial("primary display only; foreground pointer input") },
  group = "screen",
  description = "Capture a screenshot, resolve an OCR text anchor, and click its projected logical point. Target activation is not yet available for this command.",
  input = ClickScreenTextArgs,
)]
async fn click_screen_text(input: InvokeCommandInput, args: ClickScreenTextArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "windows"))]
  {
    reject_target_activation(&input, "screen.clickText")?;
    input.cancellation.check().map_err(|error| error.to_string())?;
    let query = args.query;
    if input.dry_run {
      return Ok(super::input::validation_only_output());
    }

    let result = click_recognized_screen_text_with_cancellation(query, &input.cancellation).await?;
    Ok(screen_text_click_output(&result)?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "screen.clickText is only available on macOS and Windows"))
  }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ScreenTextClick {
  pub matches: auv_driver::OcrMatches,
  pub point: auv_driver::geometry::Point,
  pub action: auv_driver::InputActionResult,
}

fn screen_text_click_output(result: &ScreenTextClick) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(crate::commands::ocr::match_report(&result.matches.matches, Some(0)));
  if let Some(report) = output.report.as_mut() {
    report.fields.extend(super::input::input_action_report_fields(&result.action));
    report.fields.push(InvokeReportField::new("Click point", format!("{:.0},{:.0}", result.point.x, result.point.y)));
  }
  Ok(output)
}

/// Builds the transport-independent `screen.clickText` result and records the
/// OCR source capture through the shared tracing artifact path.
pub fn recorded_screen_text_click_output(result: &ScreenTextClick, ocr_source: Option<&auv_driver::Capture>) -> InvokeCommandResult {
  if let Some(capture) = ocr_source {
    emit_capture("auv.driver.screen_ocr_source", capture);
  }
  screen_text_click_output(result)
}

pub async fn click_recognized_screen_text(query: String) -> Result<ScreenTextClick, crate::InvokeFailure> {
  click_recognized_screen_text_with_cancellation(query, &Default::default()).await
}

async fn click_recognized_screen_text_with_cancellation(
  query: String,
  cancellation: &crate::InvokeCancellation,
) -> Result<ScreenTextClick, crate::InvokeFailure> {
  #[cfg(any(target_os = "macos", target_os = "windows"))]
  {
    cancellation.check().map_err(|error| error.to_string())?;
    let session = auv::local::open().map_err(crate::InvokeFailure::from)?;
    let capture = session.display().capture(auv_driver::CaptureOptions::default()).map_err(crate::InvokeFailure::from)?;
    cancellation.check().map_err(|error| error.to_string())?;
    let matches = session
      .vision()
      .find_text_in_capture(&capture.capture, &query, auv_driver::RelativeRect::new(0.0, 0.0, 1.0, 1.0))
      .map_err(crate::InvokeFailure::from)?;
    cancellation.check().map_err(|error| error.to_string())?;
    let point = matches
      .best_match()
      .ok_or_else(|| crate::InvokeFailure::new(crate::FailureCode::NotFound, format!("screen.clickText did not find text {query:?}")))?
      .action_point();
    let action = session
      .input()
      .click_at(point, auv_driver::MouseButton::Left, auv_driver::Click::Single, Default::default())
      .map_err(crate::InvokeFailure::from)?;
    super::input::emit_input_action_result(&action);
    emit_capture("auv.driver.screen_ocr_source", &capture.capture);
    Ok(ScreenTextClick {
      matches,
      point,
      action,
    })
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    let _ = (query, cancellation);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "screen.clickText is only available on macOS and Windows"))
  }
}

fn reject_target_activation(input: &InvokeCommandInput, command_id: &str) -> Result<(), String> {
  if input.target.is_some() {
    // TODO(invoke-screen-activation): target activation for screen capture/OCR
    // needs a typed app activation lease before these handlers can honor
    // --target without returning to the root driver adapter.
    return Err(format!("{command_id} cannot use --target until typed app activation is available"));
  }
  Ok(())
}

#[cfg(test)]
#[path = "screen_test.rs"]
mod tests;
