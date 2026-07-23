use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
  arg::{IMAGE_TEXT_ARGS, REGION_ARGS, SCREEN_TEXT_ARGS, TARGET_ARGS},
  artifact::{ArtifactInstrumentationReceipt, ArtifactPublication},
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
  summary = "Capture one display-contained region and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = REGION_ARGS,
)]
async fn capture_region(input: InvokeCommandInput) -> InvokeCommandResult {
  reject_target_activation(&input, "screen.captureRegion")?;
  let region = Region::parse(&input.inputs, &input.command_id)?.into_rect();
  input.cancellation.check().map_err(|error| error.to_string())?;
  if input.dry_run {
    return Ok(dry_run_output(&input.command_id));
  }

  #[cfg(target_os = "macos")]
  {
    let (capture, instrumentation) = capture_screen_region(region).await?.into_parts();

    let mut output = InvokeCommandOutput::new("screen region captured");
    output.backend = Some(format!("auv-driver-macos.display.{}", capture.capture.backend));
    output.signals.insert("display.id".to_string(), capture.display.id);
    output.signals.insert("capture.width".to_string(), capture.capture.image.width().to_string());
    output.signals.insert("capture.height".to_string(), capture.capture.image.height().to_string());
    // TODO(invoke-capture-contract-artifacts): this records the captured pixels
    // and basic dimensions, but not the standalone capture-contract artifact.
    // Add it after the direct-invoke contract JSON shape is accepted in
    // `2026-06-18-invoke-direct-command-implementations-handoff.md`.
    output.verification = Some("capture-only; no semantic success claim".to_string());
    output.known_limits.push("screen.captureRegion records a region screenshot only; it does not verify UI semantics.".to_string());
    output.apply_artifact_instrumentation(instrumentation);
    Ok(output)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = region;
    Err("screen.captureRegion is only available on macOS".to_string())
  }
}

pub async fn capture_screen_region(region: auv_driver::Rect) -> Result<ArtifactPublication<auv_driver::RegionCapture>, String> {
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
    let mut instrumentation = ArtifactInstrumentationReceipt::default();
    instrumentation.publish_png("auv.driver.screen_region_capture", &capture.capture.image).await;
    Ok(ArtifactPublication::new(capture, instrumentation))
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = region;
    Err("screen.captureRegion is only available on macOS".to_string())
  }
}

#[invoke_command(
  id = "screen.findText",
  group = "screen",
  summary = "Capture a screenshot and locate OCR text anchors in screenshot pixel space. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = SCREEN_TEXT_ARGS,
)]
async fn find_screen_text(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    reject_target_activation(&input, "screen.findText")?;
    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let query = input.required_input("query")?.to_string();
    let (matches, instrumentation) = recognize_screen_text(query, false).await?.into_parts();
    let mut output = screen_text_matches_output(&input.command_id, &matches);
    output.apply_artifact_instrumentation(instrumentation);
    Ok(output)
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
  summary = "Poll live-desktop OCR until a target text anchor appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
  args = SCREEN_TEXT_ARGS,
)]
async fn wait_for_screen_text(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    reject_target_activation(&input, "screen.waitForText")?;
    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let query = input.required_input("query")?.to_string();
    let (matches, instrumentation) = recognize_screen_text(query, true).await?.into_parts();
    let mut output = screen_text_matches_output(&input.command_id, &matches);
    output.apply_artifact_instrumentation(instrumentation);
    Ok(output)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("screen text OCR is only available on macOS".to_string())
  }
}

#[cfg(target_os = "macos")]
pub async fn recognize_screen_text(query: String, wait: bool) -> Result<ArtifactPublication<auv_driver::OcrMatches>, String> {
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
      // screenshot and scalar match signals, but not a structured
      // recognition-result artifact with query/bounds/confidence. Add that
      // after the artifact shape is accepted in the direct-command handoff.
      let mut instrumentation = ArtifactInstrumentationReceipt::default();
      instrumentation.publish_png("auv.driver.screen_ocr_source", &capture.capture.image).await;
      return Ok(ArtifactPublication::new(matches, instrumentation));
    }
    thread::sleep(wait_options.poll_interval);
  }
}

#[cfg(not(target_os = "macos"))]
pub async fn recognize_screen_text(_query: String, _wait: bool) -> Result<ArtifactPublication<auv_driver::OcrMatches>, String> {
  Err("screen text OCR is only available on macOS".to_string())
}

#[cfg(target_os = "macos")]
fn screen_text_matches_output(command_id: &str, matches: &auv_driver::OcrMatches) -> InvokeCommandOutput {
  let mut output = text_matches_output(command_id, "auv-driver-macos.vision", &matches.matches, None);
  output.verification = Some("recognition-only; no semantic success claim".to_string());
  output
    .known_limits
    .push("screen OCR recognition records text matches and source screenshot only; it does not verify downstream UI state.".to_string());
  output
}

#[invoke_command(
  id = "screen.findRows",
  group = "screen",
  summary = "Detect visible OCR row bands inside a constrained screen region without depending on one exact anchor string. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = TARGET_ARGS,
)]
async fn find_screen_rows(_input: InvokeCommandInput) -> InvokeCommandResult {
  find_screen_rows_domain().await?;
  Ok(InvokeCommandOutput::new("found screen rows"))
}

pub async fn find_screen_rows_domain() -> Result<(), String> {
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
async fn wait_for_screen_rows(_input: InvokeCommandInput) -> InvokeCommandResult {
  wait_for_screen_rows_domain().await?;
  Ok(InvokeCommandOutput::new("found screen rows after waiting"))
}

pub async fn wait_for_screen_rows_domain() -> Result<(), String> {
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
async fn find_image_text(_input: InvokeCommandInput) -> InvokeCommandResult {
  recognize_image_text().await?;
  Ok(InvokeCommandOutput::new("recognized image text"))
}

pub async fn recognize_image_text() -> Result<(), String> {
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
async fn click_screen_text(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    use auv_driver::{CaptureOptions, Click, RatioRect};

    reject_target_activation(&input, "screen.clickText")?;
    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let query = input.required_input("query")?.to_string();
    let (result, instrumentation) = click_recognized_screen_text(query).await?.into_parts();

    let mut output = text_matches_output(&input.command_id, "auv-driver-macos.input", &result.matches.matches, Some(0));
    // TODO(invoke-recognition-result-artifacts): clickText records the OCR
    // source screenshot used for target resolution, but not the structured
    // recognition-result artifact. Add it with screen.findText once the
    // direct-invoke recognition artifact shape is accepted.
    output.signals.insert("click.x".to_string(), result.point.x.to_string());
    output.signals.insert("click.y".to_string(), result.point.y.to_string());
    output.verification = Some("activation-only; semantic success requires a separate verification result".to_string());
    output
      .known_limits
      .push("screen.clickText records OCR resolution and input delivery only; it does not verify post-click UI state.".to_string());
    output.apply_artifact_instrumentation(instrumentation);
    Ok(output)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("screen.clickText is only available on macOS".to_string())
  }
}

#[derive(Clone, Debug)]
pub struct ScreenTextClick {
  pub matches: auv_driver::OcrMatches,
  pub point: auv_driver::geometry::Point,
}

pub async fn click_recognized_screen_text(query: String) -> Result<ArtifactPublication<ScreenTextClick>, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let capture = session.display().capture(auv_driver::CaptureOptions::default()).map_err(|error| error.to_string())?;
    let matches = session
      .vision()
      .find_text_in_capture(&capture.capture, &query, auv_driver::RatioRect::new(0.0, 0.0, 1.0, 1.0))
      .map_err(|error| error.to_string())?;
    let point = matches.best_match().ok_or_else(|| format!("screen.clickText did not find text {query:?}"))?.action_point();
    session.input().click_at(point, auv_driver::Click::Single).map_err(|error| error.to_string())?;
    let mut instrumentation = ArtifactInstrumentationReceipt::default();
    instrumentation.publish_png("auv.driver.screen_ocr_source", &capture.capture.image).await;
    Ok(ArtifactPublication::new(ScreenTextClick { matches, point }, instrumentation))
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
  summary = "Detect visible OCR row bands inside a constrained screen region and click a chosen row-derived point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
  args = TARGET_ARGS,
)]
async fn click_screen_row(_input: InvokeCommandInput) -> InvokeCommandResult {
  click_screen_row_domain().await?;
  Ok(InvokeCommandOutput::new("clicked screen row"))
}

pub async fn click_screen_row_domain() -> Result<(), String> {
  // TODO(invoke-screen-rows): click-row depends on the same typed row-band
  // detector plus row-to-click-point policy; move that API before enabling
  // this direct invoke command.
  Err("screen.clickRow requires a typed screen row click API".to_string())
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

fn dry_run_output(command_id: &str) -> InvokeCommandOutput {
  InvokeCommandOutput::new(format!("dry run: {command_id}"))
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
mod region_tests {
  use std::collections::BTreeMap;

  use super::*;
  use crate::InvokeCancellation;

  fn inputs(values: [(&str, &str); 4]) -> BTreeMap<String, String> {
    values.into_iter().map(|(name, value)| (name.to_string(), value.to_string())).collect()
  }

  #[test]
  fn region_rejects_incomplete_non_finite_and_non_positive_fields() {
    let invalid = [
      (BTreeMap::new(), "screen.captureRegion requires --x"),
      (BTreeMap::from([("x".to_string(), "1".to_string())]), "screen.captureRegion requires --y"),
      (inputs([("x", "NaN"), ("y", "2"), ("width", "3"), ("height", "4")]), "screen.captureRegion requires finite --x"),
      (inputs([("x", "1"), ("y", "inf"), ("width", "3"), ("height", "4")]), "screen.captureRegion requires finite --y"),
      (inputs([("x", "1"), ("y", "2"), ("width", "0"), ("height", "4")]), "screen.captureRegion requires --width greater than zero"),
      (inputs([("x", "1"), ("y", "2"), ("width", "-3"), ("height", "4")]), "screen.captureRegion requires --width greater than zero"),
      (inputs([("x", "1"), ("y", "2"), ("width", "3"), ("height", "0")]), "screen.captureRegion requires --height greater than zero"),
      (inputs([("x", "1"), ("y", "2"), ("width", "3"), ("height", "-4")]), "screen.captureRegion requires --height greater than zero"),
      (inputs([("x", "1"), ("y", "2"), ("width", "3"), ("height", "-inf")]), "screen.captureRegion requires finite --height"),
    ];

    for (fields, expected) in invalid {
      assert_eq!(Region::parse(&fields, "screen.captureRegion").expect_err("invalid region must fail"), expected);
    }
  }

  #[test]
  fn region_accepts_finite_origin_and_positive_size() {
    let region = Region::parse(
      &inputs([
        ("x", "-12.5"),
        ("y", "0"),
        ("width", "640.25"),
        ("height", "480"),
      ]),
      "screen.captureRegion",
    )
    .expect("valid region")
    .into_rect();

    assert_eq!(region, auv_driver::Rect::new(-12.5, 0.0, 640.25, 480.0));
  }

  #[test]
  fn capture_region_validates_the_same_region_before_dry_and_live_branches() {
    let valid_dry_run = InvokeCommandInput {
      command_id: "screen.captureRegion".to_string(),
      target_application_id: None,
      inputs: inputs([("x", "1"), ("y", "2"), ("width", "3"), ("height", "4")]),
      dry_run: true,
      cancellation: InvokeCancellation::new(),
    };
    assert!(futures_executor::block_on(capture_region(valid_dry_run)).is_ok());

    let invalid_live = InvokeCommandInput {
      command_id: "screen.captureRegion".to_string(),
      target_application_id: None,
      inputs: inputs([("x", "1"), ("y", "2"), ("width", "0"), ("height", "4")]),
      dry_run: false,
      cancellation: InvokeCancellation::new(),
    };
    let error = futures_executor::block_on(capture_region(invalid_live)).expect_err("invalid live region must fail before capture");
    assert!(error.contains("width") && error.contains("greater than zero"));
  }
}
