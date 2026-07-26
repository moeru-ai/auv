use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField,
  arg::{IMAGE_TEXT_ARGS, REGION_ARGS, SCREEN_TEXT_ARGS, TARGET_ARGS},
  artifact::emit_png,
  invoke_command,
};

/// A complete, finite capture region with a strictly positive size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region(auv_driver::Rect);

impl Region {
  pub fn parse(inputs: &std::collections::BTreeMap<String, String>, command_id: &str) -> Result<Self, String> {
    fn field(inputs: &std::collections::BTreeMap<String, String>, command_id: &str, name: &str) -> Result<f64, String> {
      let value = inputs
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{command_id} requires --{name}"))?
        .parse::<f64>()
        .map_err(|error| format!("{command_id} received invalid --{name}: {error}"))?;
      if !value.is_finite() {
        return Err(format!("{command_id} requires finite --{name}"));
      }
      Ok(value)
    }

    let x = field(inputs, command_id, "x")?;
    let y = field(inputs, command_id, "y")?;
    let width = field(inputs, command_id, "width")?;
    let height = field(inputs, command_id, "height")?;
    if width <= 0.0 {
      return Err(format!("{command_id} requires --width greater than zero"));
    }
    if height <= 0.0 {
      return Err(format!("{command_id} requires --height greater than zero"));
    }
    Ok(Self(auv_driver::Rect::new(x, y, width, height)))
  }

  pub fn into_rect(self) -> auv_driver::Rect {
    self.0
  }
}

pub fn group() -> CommandGroup {
  CommandGroup::new("screen", "SCREEN")
    .command(capture_region_invoke_command())
    .command(find_screen_text_invoke_command())
    .command(wait_for_screen_text_invoke_command())
    .command(find_screen_rows_invoke_command())
    .command(wait_for_screen_rows_invoke_command())
    .command(find_image_text_invoke_command())
    .command(click_screen_text_invoke_command())
    .command(click_screen_row_invoke_command())
}

#[invoke_command(
  id = "screen.captureRegion",
  group = "screen",
  description = "Capture one display-contained region and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = REGION_ARGS,
)]
async fn capture_region(input: InvokeCommandInput) -> InvokeCommandResult {
  reject_target_activation(&input, "screen.captureRegion")?;
  let region = Region::parse(&input.inputs, &input.command_id)?.into_rect();
  input.cancellation.check().map_err(|error| error.to_string())?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }

  #[cfg(target_os = "macos")]
  {
    let capture = capture_screen_region(region).await?;
    region_capture_output(&capture)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = region;
    Err("screen.captureRegion is only available on macOS".to_string())
  }
}

pub async fn capture_screen_region(region: auv_driver::Rect) -> Result<auv_driver::RegionCapture, String> {
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
    emit_png("auv.driver.screen_region_capture", &capture.capture.image);
    Ok(capture)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = region;
    Err("screen.captureRegion is only available on macOS".to_string())
  }
}

fn region_capture_output(capture: &auv_driver::RegionCapture) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(&super::display_capture_result(&capture.display, &capture.capture))?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Display ID", capture.display.id.clone()),
      InvokeReportField::new("Pixel size", format!("{}x{}", capture.capture.image.width(), capture.capture.image.height())),
    ],
    Vec::new(),
  ));
  Ok(output)
}

#[invoke_command(
  id = "screen.findText",
  group = "screen",
  description = "Capture a screenshot and locate OCR text anchors in screenshot pixel space. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = SCREEN_TEXT_ARGS,
)]
async fn find_screen_text(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    reject_target_activation(&input, "screen.findText")?;
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let query = input.required_input("query")?.to_string();
    let matches = recognize_screen_text(query, false).await?;
    screen_text_matches_output(&input.command_id, &matches)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("screen text OCR is only available on macOS".to_string())
  }
}

#[invoke_command(
  id = "screen.waitForText",
  group = "screen",
  description = "Poll live-desktop OCR until a target text anchor appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
  args = SCREEN_TEXT_ARGS,
)]
async fn wait_for_screen_text(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    reject_target_activation(&input, "screen.waitForText")?;
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let query = input.required_input("query")?.to_string();
    let matches = recognize_screen_text(query, true).await?;
    screen_text_matches_output(&input.command_id, &matches)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
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

#[cfg(target_os = "macos")]
fn screen_text_matches_output(_command_id: &str, matches: &auv_driver::OcrMatches) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(matches)?;
  output.report = Some(crate::commands::ocr::match_report(&matches.matches, None));
  Ok(output)
}

#[invoke_command(
  id = "screen.findRows",
  group = "screen",
  description = "Detect visible OCR row bands inside a constrained screen region without depending on one exact anchor string. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = TARGET_ARGS,
)]
async fn find_screen_rows(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-screen-rows): implement after VisionApi exposes typed row
  // detection and the command schema can express its region and thresholds.
  unimplemented!("screen.findRows")
}

#[invoke_command(
  id = "screen.waitForRows",
  group = "screen",
  description = "Poll live-desktop OCR row detection until at least a target number of visible rows appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
  args = TARGET_ARGS,
)]
async fn wait_for_screen_rows(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-screen-rows): see `find_screen_rows`; waiting additionally
  // needs an owned polling and timeout policy.
  unimplemented!("screen.waitForRows")
}

#[invoke_command(
  id = "screen.findImageText",
  group = "screen",
  description = "Locate OCR text anchors inside an existing image artifact without touching the live desktop.",
  args = IMAGE_TEXT_ARGS,
)]
async fn find_image_text(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-image-ocr): implement after VisionApi accepts a typed image
  // artifact input instead of a frontend-local path.
  unimplemented!("screen.findImageText")
}

#[invoke_command(
  id = "screen.clickText",
  group = "screen",
  description = "Capture a screenshot, resolve an OCR text anchor, and click its projected logical point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
  args = SCREEN_TEXT_ARGS,
)]
async fn click_screen_text(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    use auv_driver::{CaptureOptions, Click, RatioRect};

    reject_target_activation(&input, "screen.clickText")?;
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let query = input.required_input("query")?.to_string();
    let result = click_recognized_screen_text(query).await?;
    screen_text_click_output(&result)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("screen.clickText is only available on macOS".to_string())
  }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ScreenTextClick {
  pub matches: auv_driver::OcrMatches,
  pub point: auv_driver::geometry::Point,
  pub action: auv_driver::InputActionResult,
}

#[cfg(target_os = "macos")]
fn screen_text_click_output(result: &ScreenTextClick) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(crate::commands::ocr::match_report(&result.matches.matches, Some(0)));
  if let Some(report) = output.report.as_mut() {
    report.fields.push(InvokeReportField::new("Click point", format!("{:.0},{:.0}", result.point.x, result.point.y)));
    report.fields.push(InvokeReportField::new("Input path", result.action.selected_path.as_str()));
  }
  Ok(output)
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

#[invoke_command(
  id = "screen.clickRow",
  group = "screen",
  description = "Detect visible OCR row bands inside a constrained screen region and click a chosen row-derived point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
  args = TARGET_ARGS,
)]
async fn click_screen_row(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-screen-rows): implement after typed row detection and
  // row-to-point policy can feed InputApi and return InputActionResult.
  unimplemented!("screen.clickRow")
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
