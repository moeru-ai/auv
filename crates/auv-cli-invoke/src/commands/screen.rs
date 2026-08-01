use crate::{CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, invoke_command};
use auv_tracing::ArtifactMetadata;
use clap::Args;

use crate::artifact::{emit_png, emit_png_with_receipt};

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

  #[cfg(target_os = "macos")]
  {
    let (capture, artifact) = capture_screen_region_recorded(region).await?;
    region_capture_output(&capture, artifact)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = region;
    Err("screen.captureRegion is only available on macOS".to_string())
  }
}

pub async fn capture_screen_region(region: auv_driver::Rect) -> Result<auv_driver::RegionCapture, String> {
  capture_screen_region_recorded(region).await.map(|(capture, _)| capture)
}

async fn capture_screen_region_recorded(region: auv_driver::Rect) -> Result<(auv_driver::RegionCapture, Option<ArtifactMetadata>), String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let capture = session
      .display()
      .capture_region(auv_driver::CaptureOptions {
        region: Some(region),
        ..auv_driver::CaptureOptions::default()
      })
      .map_err(|error| error.to_string())?;
    let artifact = emit_png_with_receipt("auv.driver.screen_region_capture", &capture.capture.image).await;
    Ok((capture, artifact))
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = region;
    Err("screen.captureRegion is only available on macOS".to_string())
  }
}

fn region_capture_output(capture: &auv_driver::RegionCapture, artifact: Option<ArtifactMetadata>) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(&super::display_capture_result(&capture.display, &capture.capture))?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Display ID", capture.display.id.clone()),
      InvokeReportField::new("Pixel size", format!("{}x{}", capture.capture.image.width(), capture.capture.image.height())),
    ],
    Vec::new(),
  ));
  Ok(output.with_artifacts(artifact))
}

/// Records and projects a region capture returned by either a local or remote
/// Driver.
pub async fn recorded_region_capture_output(capture: &auv_driver::RegionCapture) -> InvokeCommandResult {
  let artifact = emit_png_with_receipt("auv.driver.screen_region_capture", &capture.capture.image).await;
  region_capture_output(capture, artifact)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  # Find text on the current screen\n  auv invoke screen.findText \"Settings\"\n\n  # Emit OCR matches as JSON\n  auv invoke screen.findText \"Settings\" --json"
)]
struct FindScreenTextArgs {
  /// Text to locate in the captured image.
  #[arg(value_name = "TEXT")]
  query: String,
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke screen.waitForText \"Ready\"")]
struct WaitForScreenTextArgs {
  /// Text to wait for in the captured image.
  #[arg(value_name = "TEXT")]
  query: String,
}

#[invoke_command(
  id = "screen.findText",
  group = "screen",
  description = "Capture a screenshot and locate OCR text anchors in screenshot pixel space. Target activation is not yet available for this command.",
  input = FindScreenTextArgs,
)]
async fn find_screen_text(input: InvokeCommandInput, args: FindScreenTextArgs) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    reject_target_activation(&input, "screen.findText")?;
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let matches = recognize_screen_text(args.query, false).await?;
    screen_text_matches_output(&matches)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (input, args);
    Err("screen text OCR is only available on macOS".to_string())
  }
}

#[invoke_command(
  id = "screen.waitForText",
  group = "screen",
  description = "Poll live-desktop OCR until a target text anchor appears or the timeout expires. Target activation is not yet available for this command.",
  input = WaitForScreenTextArgs,
)]
async fn wait_for_screen_text(input: InvokeCommandInput, args: WaitForScreenTextArgs) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    reject_target_activation(&input, "screen.waitForText")?;
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let matches = recognize_screen_text(args.query, true).await?;
    screen_text_matches_output(&matches)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (input, args);
    Err("screen text OCR is only available on macOS".to_string())
  }
}

#[cfg(target_os = "macos")]
pub async fn recognize_screen_text(query: String, wait: bool) -> Result<auv_driver::OcrMatches, String> {
  use auv_driver::{CaptureOptions, RatioRect, WaitOptions};
  use std::{thread, time::Instant};

  let session = auv_driver::open_local().map_err(|error| error.to_string())?;
  let wait_options = WaitOptions::default();
  let started = Instant::now();
  loop {
    let capture = session.display().capture(CaptureOptions::default()).map_err(|error| error.to_string())?;
    let matches = session
      .vision()
      .find_text_in_capture(&capture.capture, &query, RatioRect::new(0.0, 0.0, 1.0, 1.0))
      .map_err(|error| error.to_string())?;
    if !matches.matches.is_empty() || !wait || started.elapsed() >= wait_options.timeout {
      if wait && matches.matches.is_empty() {
        return Err(format!("screen.waitForText did not find text {query:?} before timeout"));
      }
      // TODO(invoke-recognition-result-artifacts): this records the OCR source
      // screenshot and typed OCR matches, but not a structured
      // recognition-result artifact with query/bounds/confidence. Add that
      // after the artifact shape is accepted in the direct-command handoff.
      emit_png("auv.driver.screen_ocr_source", &capture.capture.image);
      return Ok(matches);
    }
    thread::sleep(wait_options.poll_interval);
  }
}

#[cfg(not(target_os = "macos"))]
pub async fn recognize_screen_text(_query: String, _wait: bool) -> Result<auv_driver::OcrMatches, String> {
  Err("screen text OCR is only available on macOS".to_string())
}

fn screen_text_matches_output(matches: &auv_driver::OcrMatches) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(matches)?;
  output.report = Some(crate::commands::ocr::match_report(&matches.matches, None));
  Ok(output)
}

/// Records the OCR source and builds the transport-independent
/// `screen.findText` result.
pub fn recorded_screen_text_matches_output(matches: &auv_driver::OcrMatches, capture: &auv_driver::Capture) -> InvokeCommandResult {
  emit_png("auv.driver.screen_ocr_source", &capture.image);
  screen_text_matches_output(matches)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke screen.clickText \"Continue\"")]
struct ClickScreenTextArgs {
  /// Text anchor to click.
  #[arg(value_name = "TEXT")]
  query: String,
}

#[invoke_command(
  id = "screen.clickText",
  group = "screen",
  description = "Capture a screenshot, resolve an OCR text anchor, and click its projected logical point. Target activation is not yet available for this command.",
  input = ClickScreenTextArgs,
)]
async fn click_screen_text(input: InvokeCommandInput, args: ClickScreenTextArgs) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    reject_target_activation(&input, "screen.clickText")?;
    let query = args.query;
    if input.dry_run {
      return Ok(super::input::validation_only_output());
    }

    let result = click_recognized_screen_text(query).await?;
    screen_text_click_output(&result)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (input, args);
    Err("screen.clickText is only available on macOS".to_string())
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
pub fn recorded_screen_text_click_output(result: &ScreenTextClick, capture: &auv_driver::Capture) -> InvokeCommandResult {
  emit_png("auv.driver.screen_ocr_source", &capture.image);
  screen_text_click_output(result)
}

pub async fn click_recognized_screen_text(query: String) -> Result<ScreenTextClick, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let capture = session.display().capture(auv_driver::CaptureOptions::default()).map_err(|error| error.to_string())?;
    let matches = session
      .vision()
      .find_text_in_capture(&capture.capture, &query, auv_driver::RatioRect::new(0.0, 0.0, 1.0, 1.0))
      .map_err(|error| error.to_string())?;
    let point = matches.best_match().ok_or_else(|| format!("screen.clickText did not find text {query:?}"))?.action_point();
    let action = session.input().click_at(point, auv_driver::Click::Single).map_err(|error| error.to_string())?;
    super::input::emit_input_action_result(&action);
    emit_png("auv.driver.screen_ocr_source", &capture.capture.image);
    Ok(ScreenTextClick {
      matches,
      point,
      action,
    })
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = query;
    Err("screen.clickText is only available on macOS".to_string())
  }
}

fn reject_target_activation(input: &InvokeCommandInput, command_id: &str) -> Result<(), String> {
  if input.target_application_id.is_some() {
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
