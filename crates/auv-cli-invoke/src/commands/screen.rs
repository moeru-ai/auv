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
    return Ok(dry_run_output(&input.command_id));
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
      return Ok(dry_run_output(&input.command_id));
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
      return Ok(dry_run_output(&input.command_id));
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
  find_screen_rows_domain().await?;
  Ok(InvokeCommandOutput::completed())
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
  description = "Poll live-desktop OCR row detection until at least a target number of visible rows appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
  args = TARGET_ARGS,
)]
async fn wait_for_screen_rows(_input: InvokeCommandInput) -> InvokeCommandResult {
  wait_for_screen_rows_domain().await?;
  Ok(InvokeCommandOutput::completed())
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
  description = "Locate OCR text anchors inside an existing image artifact without touching the live desktop.",
  args = IMAGE_TEXT_ARGS,
)]
async fn find_image_text(_input: InvokeCommandInput) -> InvokeCommandResult {
  recognize_image_text().await?;
  Ok(InvokeCommandOutput::completed())
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
  description = "Capture a screenshot, resolve an OCR text anchor, and click its projected logical point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
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
  click_screen_row_domain().await?;
  Ok(InvokeCommandOutput::completed())
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

fn dry_run_output(_command_id: &str) -> InvokeCommandOutput {
  InvokeCommandOutput::completed()
}

#[cfg(test)]
mod region_tests {
  use std::collections::BTreeMap;

  use image::RgbaImage;

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

  #[cfg(target_os = "macos")]
  #[test]
  fn screen_text_output_returns_typed_ocr_matches() {
    let matches = auv_driver::OcrMatches {
      matches: vec![auv_driver::OcrMatch {
        text: "Pause".to_string(),
        confidence: 0.95,
        bounds: auv_driver::Rect::new(10.0, 20.0, 80.0, 24.0),
      }],
    };

    let output = screen_text_matches_output("screen.findText", &matches).expect("OCR result should serialize");

    assert_eq!(output.result(), Some(&serde_json::to_value(&matches).expect("fixture should serialize")));
  }

  #[test]
  fn region_capture_result_keeps_pixels_out_of_json() {
    let capture = auv_driver::RegionCapture {
      display: auv_driver::Display {
        id: "display_1".to_string(),
        name: None,
        frame: auv_driver::Rect::new(0.0, 0.0, 1920.0, 1080.0),
        coordinate_space: auv_driver::CoordinateSpace::Screen,
        scale_factor: 1.0,
        is_primary: false,
        is_builtin: Some(false),
      },
      capture: auv_driver::Capture {
        image: RgbaImage::new(320, 180),
        bounds: auv_driver::Rect::new(100.0, 120.0, 320.0, 180.0),
        scale_factor: 1.0,
        backend: "fixture-region".to_string(),
        fallback_reason: Some("fixture fallback".to_string()),
      },
    };

    let output = region_capture_output(&capture).expect("region result should serialize");
    let result = output.result().expect("capture should have a result");

    assert_eq!(result["display"]["id"], "display_1");
    assert_eq!(result["capture"]["bounds"]["origin"]["x"], 100.0);
    assert_eq!(result["capture"]["pixel_dimensions"]["width"], 320);
    assert_eq!(result["capture"]["backend"], "fixture-region");
    assert_eq!(result["capture"]["fallback_reason"], "fixture fallback");
    assert!(result.get("image").is_none());
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn screen_text_click_result_keeps_resolution_and_delivery_together() {
    let click = ScreenTextClick {
      matches: auv_driver::OcrMatches {
        matches: vec![auv_driver::OcrMatch {
          text: "Pause".to_string(),
          confidence: 0.97,
          bounds: auv_driver::Rect::new(40.0, 50.0, 70.0, 20.0),
        }],
      },
      point: auv_driver::Point::new(75.0, 60.0),
      action: auv_driver::InputActionResult::single_success(auv_driver::InputDeliveryPath::ForegroundSystemEvents),
    };

    let output = screen_text_click_output(&click).expect("screen click result should serialize");
    let result = output.result().expect("click should have a result");

    assert_eq!(result["matches"]["matches"][0]["text"], "Pause");
    assert_eq!(result["point"]["x"], 75.0);
    assert_eq!(result["action"]["selected_path"], "foreground_system_events");
  }
}
