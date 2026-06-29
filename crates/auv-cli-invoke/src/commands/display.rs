use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
  arg::{NO_ARGS, TARGET_ARGS},
  invoke_command,
};
#[cfg(target_os = "macos")]
use auv_tracing_driver::{ProducedArtifact, now_millis};

pub fn group() -> CommandGroup {
  CommandGroup::new("display", "DISPLAY")
    .command(capture_display_invoke_command())
    .command(list_displays_invoke_command())
    .command(project_screenshot_point_invoke_command())
    .command(identify_point_invoke_command())
}

#[invoke_command(
  id = "display.capture",
  group = "display",
  summary = "Capture one display screenshot with a coordinate contract through xcap. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = TARGET_ARGS,
)]
fn capture_display(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::new(
      "dry run: display.capture would capture the primary display",
    ));
  }
  capture_display_impl()
}

#[invoke_command(
  id = "display.list",
  group = "display",
  summary = "List connected displays using the normalized AUV coordinate contract.",
  args = NO_ARGS,
)]
fn list_displays(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::new(
      "dry run: display.list would enumerate connected displays",
    ));
  }
  list_displays_impl()
}

#[invoke_command(
  id = "display.projectScreenshotPoint",
  group = "display",
  summary = "Project main-display screenshot pixels back into AUV global logical coordinates.",
  args = NO_ARGS,
)]
fn project_screenshot_point(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-display-typed-api): projectScreenshotPoint needs a typed
  // display projection API before this invoke command can replace root-driver
  // routing.
  Err(
    "display.projectScreenshotPoint requires a typed display API for screenshot point projection"
      .to_string(),
  )
}

#[invoke_command(
  id = "display.identifyPoint",
  group = "display",
  summary = "Resolve a logical desktop point against the current macOS display layout.",
  args = NO_ARGS,
)]
fn identify_point(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-display-typed-api): identifyPoint needs a typed display point
  // resolution API before this invoke command can replace root-driver routing.
  Err("display.identifyPoint requires a typed display API for point identification".to_string())
}

#[cfg(target_os = "macos")]
fn list_displays_impl() -> InvokeCommandResult {
  use auv_driver::Driver;

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let displays = session
    .display()
    .list()
    .map_err(|error| error.to_string())?;
  let primary = displays
    .displays
    .iter()
    .find(|display| display.is_primary)
    .or_else(|| displays.displays.first());
  let mut output = InvokeCommandOutput::new(match primary {
    Some(display) => format!(
      "Listed {} display(s); primary display is {} at {:.0}x{:.0} logical.",
      displays.displays.len(),
      display_label(display),
      display.frame.size.width,
      display.frame.size.height
    ),
    None => "Listed 0 display(s).".to_string(),
  });
  output.backend = Some("auv-driver-macos.display".to_string());
  output.signals.insert(
    "display.count".to_string(),
    displays.displays.len().to_string(),
  );
  if let Some(display) = primary {
    insert_display_signals(&mut output, "display.primary", display);
  }
  for (index, display) in displays.displays.iter().take(5).enumerate() {
    insert_display_signals(&mut output, &format!("display.{index}"), display);
  }
  output.verification = Some("read-only; no semantic success claim".to_string());
  output
    .known_limits
    .push("display.list records the observed display inventory only.".to_string());
  Ok(output)
}

#[cfg(not(target_os = "macos"))]
fn list_displays_impl() -> InvokeCommandResult {
  Err("display.list is only available on macOS through auv-driver-macos".to_string())
}

#[cfg(target_os = "macos")]
fn capture_display_impl() -> InvokeCommandResult {
  use auv_driver::{CaptureOptions, Driver};

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let result = session
    .display()
    .capture(CaptureOptions::default())
    .map_err(|error| error.to_string())?;
  let mut output = InvokeCommandOutput::new(format!(
    "Captured {} through {} ({}x{} pixels).",
    display_label(&result.display),
    result.capture.backend,
    result.capture.image.width(),
    result.capture.image.height()
  ));
  output.backend = Some(result.capture.backend.clone());
  insert_display_signals(&mut output, "display", &result.display);
  output.signals.insert(
    "capture.pixel_width".to_string(),
    result.capture.image.width().to_string(),
  );
  output.signals.insert(
    "capture.pixel_height".to_string(),
    result.capture.image.height().to_string(),
  );
  output.signals.insert(
    "capture.bounds".to_string(),
    format_rect(result.capture.bounds),
  );
  output.signals.insert(
    "capture.scale_factor".to_string(),
    format!("{:.3}", result.capture.scale_factor),
  );
  if let Some(reason) = result.capture.fallback_reason {
    output
      .signals
      .insert("capture.fallback_reason".to_string(), reason);
  }
  // TODO(invoke-capture-contract-artifacts): this handler records the screenshot
  // and coordinate signals, but not the old standalone capture-contract
  // artifact. Add the contract artifact after its direct-invoke JSON shape is
  // accepted in `2026-06-18-invoke-direct-command-implementations-handoff.md`.
  let source_path = invoke_artifact_path("display-capture", "png");
  result
    .capture
    .image
    .save(&source_path)
    .map_err(|error| format!("failed to write display.capture screenshot artifact: {error}"))?;
  output.artifacts.push(ProducedArtifact {
    kind: "display-screenshot".to_string(),
    source_path,
    preferred_name: "display-capture.png".to_string(),
    note: Some("Screenshot captured by display.capture.".to_string()),
  });
  output.verification = Some("capture-only; no semantic success claim".to_string());
  output
    .known_limits
    .push("display.capture records a screenshot and coordinate signals only; it does not verify UI semantics.".to_string());
  Ok(output)
}

#[cfg(not(target_os = "macos"))]
fn capture_display_impl() -> InvokeCommandResult {
  Err("display.capture is only available on macOS through auv-driver-macos".to_string())
}

#[cfg(target_os = "macos")]
fn insert_display_signals(
  output: &mut InvokeCommandOutput,
  prefix: &str,
  display: &auv_driver::Display,
) {
  output
    .signals
    .insert(format!("{prefix}.id"), display.id.clone());
  output
    .signals
    .insert(format!("{prefix}.label"), display_label(display));
  output
    .signals
    .insert(format!("{prefix}.frame"), format_rect(display.frame));
  output.signals.insert(
    format!("{prefix}.scale_factor"),
    format!("{:.3}", display.scale_factor),
  );
  output.signals.insert(
    format!("{prefix}.is_primary"),
    display.is_primary.to_string(),
  );
  if let Some(is_builtin) = display.is_builtin {
    output
      .signals
      .insert(format!("{prefix}.is_builtin"), is_builtin.to_string());
  }
}

#[cfg(target_os = "macos")]
fn display_label(display: &auv_driver::Display) -> String {
  display
    .name
    .clone()
    .unwrap_or_else(|| format!("display {}", display.id))
}

#[cfg(target_os = "macos")]
fn format_rect(rect: auv_driver::Rect) -> String {
  format!(
    "x={:.0},y={:.0},width={:.0},height={:.0}",
    rect.origin.x, rect.origin.y, rect.size.width, rect.size.height
  )
}

#[cfg(target_os = "macos")]
fn invoke_artifact_path(label: &str, extension: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!(
    "auv-invoke-{label}-{}-{}.{}",
    std::process::id(),
    now_millis(),
    extension
  ))
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use super::*;

  fn input<'a>(
    command_id: &'static str,
    inputs: &'a BTreeMap<String, String>,
  ) -> InvokeCommandInput<'a> {
    InvokeCommandInput {
      command_id,
      target_application_id: None,
      inputs,
      dry_run: false,
    }
  }

  #[test]
  fn commands_without_typed_display_api_report_explicit_gap() {
    let inputs = BTreeMap::new();

    for (command_id, invoke) in [
      (
        "display.identifyPoint",
        identify_point as fn(InvokeCommandInput<'_>) -> InvokeCommandResult,
      ),
      (
        "display.projectScreenshotPoint",
        project_screenshot_point as fn(InvokeCommandInput<'_>) -> InvokeCommandResult,
      ),
    ] {
      let error =
        invoke(input(command_id, &inputs)).expect_err("command should not route to root driver");

      assert!(
        error.contains("typed display API"),
        "{command_id} returned unclear error: {error}"
      );
    }
  }
}
