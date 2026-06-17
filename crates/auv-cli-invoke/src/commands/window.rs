use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
  arg::{NO_ARGS, WINDOW_ARGS, WINDOW_TEXT_ARGS, WINDOW_VERIFY_TEXT_ARGS},
  invoke_command,
};
#[cfg(target_os = "macos")]
use auv_tracing_driver::{ProducedArtifact, now_millis};

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
fn list_windows(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  list_windows_impl(input)
}

#[invoke_command(
  id = "window.capture",
  group = "window",
  summary = "Capture one single-display window and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = WINDOW_ARGS,
)]
fn capture_window(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  capture_window_impl(input)
}

#[invoke_command(
  id = "window.captureAxTree",
  group = "window",
  summary = "Capture an AX tree snapshot for a target macOS app window.",
  args = WINDOW_ARGS,
)]
fn capture_ax_tree(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
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
fn find_window_text(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  find_window_text_impl(input, false)
}

#[invoke_command(
  id = "window.waitForText",
  group = "window",
  summary = "Poll resolved-window OCR until a text anchor appears or the timeout expires.",
  args = WINDOW_TEXT_ARGS,
)]
fn wait_for_window_text(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  find_window_text_impl(input, true)
}

#[invoke_command(
  id = "window.findRows",
  group = "window",
  summary = "Detect visible OCR row bands inside a resolved window.",
  args = WINDOW_ARGS,
)]
fn find_window_rows(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
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
fn wait_for_window_rows(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
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
fn observe_window_region(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
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
fn find_icon_match(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
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
fn scroll_window_region(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
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
fn verify_ax_text(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
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
fn click_window_text(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  click_window_text_impl(input)
}

#[invoke_command(
  id = "window.clickRow",
  group = "window",
  summary = "Capture a resolved window, detect visible rows, and click a row-derived projected logical point.",
  args = WINDOW_ARGS,
)]
fn click_window_row(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-window-rows): click-row depends on typed row-band detection
  // plus row-to-click-point policy; move that API before enabling this direct
  // invoke command.
  Err("window.clickRow requires a typed window row click API".to_string())
}

#[cfg(target_os = "macos")]
fn list_windows_impl(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  use auv_driver::Driver;

  if input.dry_run {
    return Ok(dry_run_output(input.command_id));
  }

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let windows = session.window().list().map_err(|error| error.to_string())?;
  let mut output = InvokeCommandOutput::new(format!("listed {} window(s)", windows.len()));
  output.backend = Some("auv-driver-macos.window".to_string());
  output
    .signals
    .insert("window.count".to_string(), windows.len().to_string());
  if let Some(front) = windows.first() {
    if let Some(title) = &front.title {
      output
        .signals
        .insert("window.first.title".to_string(), title.clone());
    }
    if let Some(app_name) = &front.app_name {
      output
        .signals
        .insert("window.first.app_name".to_string(), app_name.clone());
    }
  }
  output.verification = Some("read-only; no semantic success claim".to_string());
  output
    .known_limits
    .push("window.list records the observed visible window inventory only.".to_string());
  Ok(output)
}

#[cfg(not(target_os = "macos"))]
fn list_windows_impl(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  Err("window.list is only available on macOS".to_string())
}

#[cfg(target_os = "macos")]
fn capture_window_impl(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  use auv_driver::Driver;

  if input.dry_run {
    return Ok(dry_run_output(input.command_id));
  }

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let window = session
    .window()
    .resolve(window_selector(&input))
    .map_err(|error| error.to_string())?;
  let capture = session
    .window()
    .capture(&window)
    .map_err(|error| error.to_string())?;

  let mut output = InvokeCommandOutput::new("window captured");
  output.backend = Some(format!("auv-driver-macos.window.{}", capture.backend));
  add_window_signals(&mut output, &window);
  output.signals.insert(
    "capture.width".to_string(),
    capture.image.width().to_string(),
  );
  output.signals.insert(
    "capture.height".to_string(),
    capture.image.height().to_string(),
  );
  // TODO(invoke-window-capture-backend): live testing on 2026-06-18 showed
  // ScreenCaptureKit single-window capture can time out and xcap fallback can
  // fail for Chrome/NetEase windows. Stabilize the typed window capture backend
  // before treating window.* evidence as reliably available.
  //
  // TODO(invoke-capture-contract-artifacts): this records the window screenshot
  // and scalar capture signals, but not a standalone capture-contract artifact.
  // Add it after the direct-invoke contract JSON shape is accepted in
  // `2026-06-18-invoke-direct-command-implementations-handoff.md`.
  let source_path = invoke_artifact_path(input.command_id, "window-capture", "png");
  capture
    .image
    .save(&source_path)
    .map_err(|error| format!("failed to write window.capture artifact: {error}"))?;
  output.artifacts.push(ProducedArtifact {
    kind: "window-capture".to_string(),
    source_path,
    preferred_name: format!("{}-window-capture.png", input.command_id.replace('.', "-")),
    note: Some("Window screenshot captured by window.capture.".to_string()),
  });
  output.verification = Some("capture-only; no semantic success claim".to_string());
  output.known_limits.push(
    "window.capture records a resolved window screenshot only; it does not verify UI semantics."
      .to_string(),
  );
  Ok(output)
}

#[cfg(not(target_os = "macos"))]
fn capture_window_impl(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  Err("window.capture is only available on macOS".to_string())
}

#[cfg(target_os = "macos")]
fn find_window_text_impl(input: InvokeCommandInput<'_>, wait: bool) -> InvokeCommandResult {
  use auv_driver::{Driver, RatioRect, WaitOptions};
  use std::{thread, time::Instant};

  if input.dry_run {
    return Ok(dry_run_output(input.command_id));
  }

  let query = required_input(&input, "query")?;
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let window = session
    .window()
    .resolve(window_selector(&input))
    .map_err(|error| error.to_string())?;
  let wait_options = WaitOptions::default();
  let started = Instant::now();
  loop {
    let capture = session
      .window()
      .capture(&window)
      .map_err(|error| error.to_string())?;
    let matches = session
      .vision()
      .find_text_in_capture(&capture, query, RatioRect::new(0.0, 0.0, 1.0, 1.0))
      .map_err(|error| error.to_string())?;
    if !matches.matches.is_empty() || !wait || started.elapsed() >= wait_options.timeout {
      if wait && matches.matches.is_empty() {
        return Err(format!(
          "window.waitForText did not find text {query:?} before timeout"
        ));
      }

      let mut output = text_matches_output(
        input.command_id,
        "auv-driver-macos.window.vision",
        matches.matches.len(),
        matches.best_match().map(|matched| matched.text.as_str()),
      );
      add_window_signals(&mut output, &window);
      // TODO(invoke-recognition-result-artifacts): this records the window OCR
      // source screenshot and scalar match signals, but not a structured
      // recognition-result artifact with query/bounds/confidence. Add it after
      // the artifact shape is accepted in the direct-command handoff.
      let source_path = invoke_artifact_path(input.command_id, "ocr-screenshot", "png");
      capture
        .image
        .save(&source_path)
        .map_err(|error| format!("failed to write window OCR screenshot artifact: {error}"))?;
      output.artifacts.push(ProducedArtifact {
        kind: "window-ocr-screenshot".to_string(),
        source_path,
        preferred_name: format!("{}-ocr-screenshot.png", input.command_id.replace('.', "-")),
        note: Some("Window screenshot used for OCR matching.".to_string()),
      });
      output.verification = Some("recognition-only; no semantic success claim".to_string());
      output
        .known_limits
        .push("window OCR recognition records text matches and source screenshot only; it does not verify downstream UI state.".to_string());
      return Ok(output);
    }
    thread::sleep(wait_options.poll_interval);
  }
}

#[cfg(not(target_os = "macos"))]
fn find_window_text_impl(_input: InvokeCommandInput<'_>, _wait: bool) -> InvokeCommandResult {
  Err("window text OCR is only available on macOS".to_string())
}

#[cfg(target_os = "macos")]
fn click_window_text_impl(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  use auv_driver::{ClickOptions, Driver, RatioRect, ScreenPoint};

  if input.dry_run {
    return Ok(dry_run_output(input.command_id));
  }

  let query = required_input(&input, "query")?;
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let window = session
    .window()
    .resolve(window_selector(&input))
    .map_err(|error| error.to_string())?;
  let capture = session
    .window()
    .capture(&window)
    .map_err(|error| error.to_string())?;
  let matches = session
    .vision()
    .find_text_in_capture(&capture, query, RatioRect::new(0.0, 0.0, 1.0, 1.0))
    .map_err(|error| error.to_string())?;
  let matched = matches
    .best_match()
    .ok_or_else(|| format!("window.clickText did not find text {query:?}"))?;
  let screen_point = ScreenPoint::from(matched.action_point());
  let window_point = session
    .window()
    .to_window_point(&window, screen_point)
    .map_err(|error| error.to_string())?;
  let action = session
    .window()
    .click(&window, window_point, ClickOptions::default())
    .map_err(|error| error.to_string())?;

  let mut output = text_matches_output(
    input.command_id,
    "auv-driver-macos.window.input",
    matches.matches.len(),
    Some(matched.text.as_str()),
  );
  add_window_signals(&mut output, &window);
  // TODO(invoke-recognition-result-artifacts): clickText records the OCR source
  // screenshot used for target resolution, but not the structured
  // recognition-result artifact. Add it with window.findText once the
  // direct-invoke recognition artifact shape is accepted.
  let source_path = invoke_artifact_path(input.command_id, "ocr-screenshot", "png");
  capture
    .image
    .save(&source_path)
    .map_err(|error| format!("failed to write window click OCR screenshot artifact: {error}"))?;
  output.artifacts.push(ProducedArtifact {
    kind: "window-ocr-screenshot".to_string(),
    source_path,
    preferred_name: format!("{}-ocr-screenshot.png", input.command_id.replace('.', "-")),
    note: Some("Window screenshot used to resolve window.clickText OCR target.".to_string()),
  });
  output.signals.insert(
    "input.selected_path".to_string(),
    format!("{:?}", action.selected_path),
  );
  output.signals.insert(
    "click.window_x".to_string(),
    window_point.point().x.to_string(),
  );
  output.signals.insert(
    "click.window_y".to_string(),
    window_point.point().y.to_string(),
  );
  output.verification =
    Some("activation-only; semantic success requires a separate verification result".to_string());
  output
    .known_limits
    .push("window.clickText records OCR resolution and input delivery only; it does not verify post-click UI state.".to_string());
  Ok(output)
}

#[cfg(not(target_os = "macos"))]
fn click_window_text_impl(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  Err("window.clickText is only available on macOS".to_string())
}

#[cfg(target_os = "macos")]
fn window_selector(input: &InvokeCommandInput<'_>) -> auv_driver::WindowSelector {
  use auv_driver::{App, TextMatcher, WindowSelector};

  let mut selector = WindowSelector {
    main_visible: true,
    ..WindowSelector::default()
  };
  if let Some(target) = target(input) {
    selector.app = Some(App::bundle_id(target));
  }
  if let Some(title) = input
    .inputs
    .get("title")
    .filter(|value| !value.trim().is_empty())
  {
    selector.title = Some(TextMatcher::Contains(title.clone()));
  }
  selector
}

fn target<'a>(input: &'a InvokeCommandInput<'_>) -> Option<&'a str> {
  input
    .target_application_id
    .or_else(|| input.inputs.get("target").map(String::as_str))
    .filter(|value| !value.trim().is_empty())
}

fn required_input<'a>(input: &'a InvokeCommandInput<'_>, name: &str) -> Result<&'a str, String> {
  input
    .inputs
    .get(name)
    .map(String::as_str)
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| format!("{} requires --{name}", input.command_id))
}

fn dry_run_output(command_id: &str) -> InvokeCommandOutput {
  InvokeCommandOutput::new(format!("dry run: {command_id}"))
}

#[cfg(target_os = "macos")]
fn invoke_artifact_path(command_id: &str, label: &str, extension: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!(
    "auv-invoke-{}-{label}-{}-{}.{}",
    command_id.replace('.', "-"),
    std::process::id(),
    now_millis(),
    extension
  ))
}

#[cfg(target_os = "macos")]
fn add_window_signals(output: &mut InvokeCommandOutput, window: &auv_driver::Window) {
  output
    .signals
    .insert("window.id".to_string(), window.reference.id.clone());
  if let Some(title) = &window.title {
    output
      .signals
      .insert("window.title".to_string(), title.clone());
  }
  if let Some(app_name) = &window.app_name {
    output
      .signals
      .insert("window.app_name".to_string(), app_name.clone());
  }
  if let Some(bundle_id) = &window.app_bundle_id {
    output
      .signals
      .insert("window.app_bundle_id".to_string(), bundle_id.clone());
  }
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
