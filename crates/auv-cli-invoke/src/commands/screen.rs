use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
  arg::{IMAGE_TEXT_ARGS, REGION_ARGS, SCREEN_TEXT_ARGS, TARGET_ARGS},
  invoke_command,
};

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
  summary = "Capture one display-contained region and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = REGION_ARGS,
)]
fn capture_region(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  capture_region_impl(input)
}

#[invoke_command(
  id = "screen.findText",
  group = "screen",
  summary = "Capture a screenshot and locate OCR text anchors in screenshot pixel space. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = SCREEN_TEXT_ARGS,
)]
fn find_screen_text(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  find_screen_text_impl(input, false)
}

#[invoke_command(
  id = "screen.waitForText",
  group = "screen",
  summary = "Poll live-desktop OCR until a target text anchor appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
  args = SCREEN_TEXT_ARGS,
)]
fn wait_for_screen_text(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  find_screen_text_impl(input, true)
}

#[invoke_command(
  id = "screen.findRows",
  group = "screen",
  summary = "Detect visible OCR row bands inside a constrained screen region without depending on one exact anchor string. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = TARGET_ARGS,
)]
fn find_screen_rows(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-screen-rows): row-band detection still lives in the root
  // macOS command adapter; move a typed screen-row API before enabling this
  // direct invoke command.
  Err("screen.findRows requires a typed screen row detection API".to_string())
}

#[invoke_command(
  id = "screen.waitForRows",
  group = "screen",
  summary = "Poll live-desktop OCR row detection until at least a target number of visible rows appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
  args = TARGET_ARGS,
)]
fn wait_for_screen_rows(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-screen-rows): row wait/polling still lives in the root macOS
  // command adapter; move a typed screen-row API before enabling this direct
  // invoke command.
  Err("screen.waitForRows requires a typed screen row wait API".to_string())
}

#[invoke_command(
  id = "screen.findImageText",
  group = "screen",
  summary = "Locate OCR text anchors inside an existing image artifact without touching the live desktop.",
  args = IMAGE_TEXT_ARGS,
)]
fn find_image_text(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-image-ocr): the invoke crate cannot yet decode an image path
  // into the typed VisionApi capture/image contract without adding a stable
  // image-artifact boundary; add that API before enabling this command.
  Err("screen.findImageText requires a typed image OCR API for image artifacts".to_string())
}

#[invoke_command(
  id = "screen.clickText",
  group = "screen",
  summary = "Capture a screenshot, resolve an OCR text anchor, and click its projected logical point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
  args = SCREEN_TEXT_ARGS,
)]
fn click_screen_text(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  click_screen_text_impl(input)
}

#[invoke_command(
  id = "screen.clickRow",
  group = "screen",
  summary = "Detect visible OCR row bands inside a constrained screen region and click a chosen row-derived point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
  args = TARGET_ARGS,
)]
fn click_screen_row(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-screen-rows): click-row depends on the same typed row-band
  // detector plus row-to-click-point policy; move that API before enabling
  // this direct invoke command.
  Err("screen.clickRow requires a typed screen row click API".to_string())
}

#[cfg(target_os = "macos")]
fn capture_region_impl(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  use auv_driver::{CaptureOptions, Driver, Rect};

  reject_target_activation(&input, "screen.captureRegion")?;
  if input.dry_run {
    return Ok(dry_run_output(input.command_id));
  }

  let region = Rect::new(
    required_f64(&input, "x")?,
    required_f64(&input, "y")?,
    required_f64(&input, "width")?,
    required_f64(&input, "height")?,
  );
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let capture = session
    .display()
    .capture_region(CaptureOptions {
      region: Some(region),
      ..CaptureOptions::default()
    })
    .map_err(|error| error.to_string())?;

  let mut output = InvokeCommandOutput::new("screen region captured");
  output.backend = Some(format!(
    "auv-driver-macos.display.{}",
    capture.capture.backend
  ));
  output
    .signals
    .insert("display.id".to_string(), capture.display.id);
  output.signals.insert(
    "capture.width".to_string(),
    capture.capture.image.width().to_string(),
  );
  output.signals.insert(
    "capture.height".to_string(),
    capture.capture.image.height().to_string(),
  );
  Ok(output)
}

#[cfg(not(target_os = "macos"))]
fn capture_region_impl(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  Err("screen.captureRegion is only available on macOS".to_string())
}

#[cfg(target_os = "macos")]
fn find_screen_text_impl(input: InvokeCommandInput<'_>, wait: bool) -> InvokeCommandResult {
  use auv_driver::{CaptureOptions, Driver, RatioRect, WaitOptions};
  use std::{thread, time::Instant};

  reject_target_activation(
    &input,
    if wait {
      "screen.waitForText"
    } else {
      "screen.findText"
    },
  )?;
  if input.dry_run {
    return Ok(dry_run_output(input.command_id));
  }

  let query = required_input(&input, "query")?;
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let wait_options = WaitOptions::default();
  let started = Instant::now();
  loop {
    let capture = session
      .display()
      .capture(CaptureOptions::default())
      .map_err(|error| error.to_string())?;
    let matches = session
      .vision()
      .find_text_in_capture(&capture.capture, query, RatioRect::new(0.0, 0.0, 1.0, 1.0))
      .map_err(|error| error.to_string())?;
    if !matches.matches.is_empty() || !wait || started.elapsed() >= wait_options.timeout {
      if wait && matches.matches.is_empty() {
        return Err(format!(
          "screen.waitForText did not find text {query:?} before timeout"
        ));
      }
      return Ok(text_matches_output(
        input.command_id,
        "auv-driver-macos.vision",
        matches.matches.len(),
        matches.best_match().map(|matched| matched.text.as_str()),
      ));
    }
    thread::sleep(wait_options.poll_interval);
  }
}

#[cfg(not(target_os = "macos"))]
fn find_screen_text_impl(_input: InvokeCommandInput<'_>, _wait: bool) -> InvokeCommandResult {
  Err("screen text OCR is only available on macOS".to_string())
}

#[cfg(target_os = "macos")]
fn click_screen_text_impl(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  use auv_driver::{CaptureOptions, Click, Driver, RatioRect};

  reject_target_activation(&input, "screen.clickText")?;
  if input.dry_run {
    return Ok(dry_run_output(input.command_id));
  }

  let query = required_input(&input, "query")?;
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let capture = session
    .display()
    .capture(CaptureOptions::default())
    .map_err(|error| error.to_string())?;
  let matches = session
    .vision()
    .find_text_in_capture(&capture.capture, query, RatioRect::new(0.0, 0.0, 1.0, 1.0))
    .map_err(|error| error.to_string())?;
  let matched = matches
    .best_match()
    .ok_or_else(|| format!("screen.clickText did not find text {query:?}"))?;
  let point = matched.action_point();
  session
    .input()
    .click_at(point, Click::Single)
    .map_err(|error| error.to_string())?;

  let mut output = text_matches_output(
    input.command_id,
    "auv-driver-macos.input",
    matches.matches.len(),
    Some(matched.text.as_str()),
  );
  output
    .signals
    .insert("click.x".to_string(), point.x.to_string());
  output
    .signals
    .insert("click.y".to_string(), point.y.to_string());
  Ok(output)
}

#[cfg(not(target_os = "macos"))]
fn click_screen_text_impl(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  Err("screen.clickText is only available on macOS".to_string())
}

fn required_input<'a>(input: &'a InvokeCommandInput<'_>, name: &str) -> Result<&'a str, String> {
  input
    .inputs
    .get(name)
    .map(String::as_str)
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| format!("{} requires --{name}", input.command_id))
}

fn required_f64(input: &InvokeCommandInput<'_>, name: &str) -> Result<f64, String> {
  let value = required_input(input, name)?;
  value
    .parse::<f64>()
    .map_err(|error| format!("invalid --{name} value {value:?}: {error}"))
}

fn reject_target_activation(
  input: &InvokeCommandInput<'_>,
  command_id: &str,
) -> Result<(), String> {
  if input.target_application_id.is_some() {
    // TODO(invoke-screen-activation): target activation for screen capture/OCR
    // needs a typed app activation lease before these handlers can honor
    // --target without returning to the root driver adapter.
    return Err(format!(
      "{command_id} cannot use --target until typed app activation is available"
    ));
  }
  Ok(())
}

fn dry_run_output(command_id: &str) -> InvokeCommandOutput {
  InvokeCommandOutput::new(format!("dry run: {command_id}"))
}

fn text_matches_output(
  command_id: &str,
  backend: &str,
  count: usize,
  best_text: Option<&str>,
) -> InvokeCommandOutput {
  let mut output = InvokeCommandOutput::new(format!("{command_id} matched {count} text region(s)"));
  output.backend = Some(backend.to_string());
  output
    .signals
    .insert("match.count".to_string(), count.to_string());
  if let Some(best_text) = best_text {
    output
      .signals
      .insert("match.best_text".to_string(), best_text.to_string());
  }
  output
}
