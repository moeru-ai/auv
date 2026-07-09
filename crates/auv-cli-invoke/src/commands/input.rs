use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
  arg::{
    KEY_ARGS, QUERY_ARGS, QUERY_OR_CANDIDATE_ARGS, QUERY_OR_CANDIDATE_OVERLAY_ARGS, QUERY_OVERLAY_ARGS, TARGET_ARGS, TEXT_ARGS, WINDOW_ARGS,
    WINDOW_CLICK_POINT_ARGS, WINDOW_QUERY_OVERLAY_ARGS,
  },
  invoke_command,
};
use crate::{InvokeReport, InvokeReportField};
#[cfg(target_os = "macos")]
use auv_tracing_driver::{ProducedArtifact, now_millis};

pub fn group() -> CommandGroup {
  CommandGroup::new("input", "INPUT")
    .command(focus_text_input_invoke_command())
    .command(press_button_invoke_command())
    .command(ax_press_button_invoke_command())
    .command(ax_focus_text_input_invoke_command())
    .command(ax_click_window_text_invoke_command())
    .command(smart_press_invoke_command())
    .command(type_text_invoke_command())
    .command(paste_text_preserve_clipboard_invoke_command())
    .command(press_key_invoke_command())
    .command(click_point_invoke_command())
    .command(click_window_point_invoke_command())
    .command(teach_click_invoke_command())
    .command(scroll_point_invoke_command())
}

#[invoke_command(
  id = "input.focusText",
  group = "input",
  summary = "Focus a target macOS text input through AX, either by --query text or by a promoted --candidate JSON payload.",
  args = QUERY_OR_CANDIDATE_ARGS,
)]
fn focus_text_input(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-input-ax-focus): focusText still depends on the archived
  // root AX candidate/action adapter; move a typed focus API before enabling
  // this direct invoke command.
  Err("input.focusText requires a typed AX/text focus API".to_string())
}

#[invoke_command(
  id = "input.pressButton",
  group = "input",
  summary = "Press a known macOS button-like control by query through AX.",
  args = QUERY_ARGS,
)]
fn press_button(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-input-ax-press): pressButton still depends on root AX query
  // resolution; move a typed button press API before enabling this direct
  // invoke command.
  Err("input.pressButton requires a typed AX/button press API".to_string())
}

#[invoke_command(
  id = "input.axPressButton",
  group = "input",
  summary = "Press a control by query via AXUIElementPerformAction without moving the real cursor. Pass --overlay true to draw a visual AUV cursor over the target. Falls back with an error when the AX target has no matching action; use input.pressButton for non-AX-pressable targets.",
  args = QUERY_OVERLAY_ARGS,
)]
fn ax_press_button(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-input-ax-press): AXUIElementPerformAction routing has not
  // moved into a stable typed driver API yet; enable this after that boundary
  // exists.
  Err("input.axPressButton requires a typed AX press API".to_string())
}

#[invoke_command(
  id = "input.axFocusText",
  group = "input",
  summary = "Focus a text input by query or promoted --candidate JSON via AXUIElementSetAttributeValue(kAXFocusedAttribute) without moving the real cursor. Pass --overlay true for the dual-cursor visual. Errors when the target does not accept programmatic focus; use input.focusText if pointer movement is acceptable.",
  args = QUERY_OR_CANDIDATE_OVERLAY_ARGS,
)]
fn ax_focus_text_input(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-input-ax-focus): AX focus-by-query/candidate has not moved
  // into a stable typed driver API yet; enable this after that boundary exists.
  Err("input.axFocusText requires a typed AX focus API".to_string())
}

#[invoke_command(
  id = "input.axClickWindowText",
  group = "input",
  summary = "Find visible text in a window via Vision OCR, resolve the AX node at that point, then press it via AXUIElementPerformAction without moving the real cursor. Pass --overlay true for the dual-cursor visual. Errors with a hint to window.clickText when the OCR anchor maps to a canvas-rendered or non-AX-pressable region.",
  args = WINDOW_QUERY_OVERLAY_ARGS,
)]
fn ax_click_window_text(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-input-ax-click-window-text): OCR-to-AX click resolution still
  // lives in the root macOS command adapter; move a typed resolver API before
  // enabling this direct invoke command.
  Err("input.axClickWindowText requires a typed OCR-to-AX click API".to_string())
}

#[invoke_command(
  id = "input.smartPress",
  group = "input",
  summary = "ActionResolver v0 diagnostic press: try OCR-to-AX press first; if it fails and pointer fallback is allowed, fall back to pointer click.",
  args = WINDOW_QUERY_OVERLAY_ARGS,
)]
fn smart_press(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-input-smart-press): ActionResolver execution still lives in
  // the root macOS command adapter; move the typed resolver boundary before
  // enabling this direct invoke command.
  Err("input.smartPress requires a typed ActionResolver invoke API".to_string())
}

#[invoke_command(
  id = "input.typeText",
  group = "input",
  summary = "Type text into the active macOS control through System Events.",
  args = TEXT_ARGS,
)]
fn type_text(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  type_text_impl(input)
}

#[invoke_command(
  id = "input.pasteText",
  group = "input",
  summary = "Paste text into the active macOS control through the clipboard, then restore the prior clipboard snapshot.",
  args = TEXT_ARGS,
)]
fn paste_text_preserve_clipboard(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  paste_text_preserve_clipboard_impl(input)
}

#[invoke_command(
  id = "input.key",
  group = "input",
  summary = "Press a keyboard key or shortcut in the active macOS app through System Events.",
  args = KEY_ARGS,
)]
fn press_key(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  press_key_impl(input)
}

#[invoke_command(
  id = "input.clickPoint",
  group = "input",
  summary = "Click a macOS global logical point through Quartz.",
  args = TARGET_ARGS,
)]
fn click_point(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-input-click-point): InputApi::click_at exists, but this
  // invoke command exposes no x/y point args; add direct point inputs before
  // enabling this command.
  Err("input.clickPoint requires direct x/y point inputs".to_string())
}

#[invoke_command(
  id = "input.clickWindowPoint",
  group = "input",
  summary = "Click a point relative to a target macOS window, either from --relative_x/--relative_y inputs or from a promoted --candidate JSON payload.",
  args = WINDOW_CLICK_POINT_ARGS,
)]
fn click_window_point(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  click_window_point_impl(input)
}

#[invoke_command(
  id = "input.teachClick",
  group = "input",
  summary = "Capture a target window before and after a human-taught click, recording global and window-local click coordinates for automation debugging.",
  args = WINDOW_ARGS,
)]
fn teach_click(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-input-teach-click): teach-click is an interactive root
  // adapter workflow, not a stable typed input API; move the workflow boundary
  // before enabling this direct invoke command.
  Err("input.teachClick requires a typed teach-click workflow API".to_string())
}

#[invoke_command(
  id = "input.scrollPoint",
  group = "input",
  summary = "Scroll at a macOS global logical point through Quartz.",
  args = TARGET_ARGS,
)]
fn scroll_point(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-input-scroll-point): InputApi::scroll_global_hid exists, but
  // this invoke command exposes no x/y point or delta args; add direct scroll
  // inputs before enabling this command.
  Err("input.scrollPoint requires direct x/y point and scroll delta inputs".to_string())
}

#[cfg(target_os = "macos")]
fn type_text_impl(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  use auv_driver::{Driver, TypeTextOptions};

  reject_target_activation(&input, "input.typeText")?;
  if input.dry_run {
    return Ok(dry_run_output(input.command_id));
  }

  let text = required_input(&input, "text")?;
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let result = session.input().type_text(text, TypeTextOptions::default()).map_err(|error| error.to_string())?;
  input_action_output("typed text into active control", "auv-driver-macos.input", &result)
}

#[cfg(not(target_os = "macos"))]
fn type_text_impl(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  Err("input.typeText is only available on macOS".to_string())
}

#[cfg(target_os = "macos")]
fn paste_text_preserve_clipboard_impl(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  use auv_driver::{Driver, PasteTextOptions};

  reject_target_activation(&input, "input.pasteText")?;
  if input.dry_run {
    return Ok(dry_run_output(input.command_id));
  }

  let text = required_input(&input, "text")?;
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  session
    .input()
    .paste_text(PasteTextOptions {
      text: text.to_string(),
      ..PasteTextOptions::default()
    })
    .map_err(|error| error.to_string())?;

  let mut output = InvokeCommandOutput::new("pasted text into active control");
  output.backend = Some("auv-driver-macos.input".to_string());
  output.signals.insert("clipboard_disturbance".to_string(), "temporary".to_string());
  // TODO(invoke-paste-input-action-result): paste_text currently returns only
  // success/failure, so this handler cannot persist a typed InputActionResult
  // artifact like input.typeText/input.key. Extend the typed paste API to
  // return delivery evidence before claiming full input artifact coverage.
  output.verification = Some("activation-only; semantic success requires a separate verification result".to_string());
  output.known_limits.push("input.pasteText records clipboard-based input delivery only; it does not verify target UI state.".to_string());
  Ok(output)
}

#[cfg(not(target_os = "macos"))]
fn paste_text_preserve_clipboard_impl(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  Err("input.pasteText is only available on macOS".to_string())
}

#[cfg(target_os = "macos")]
fn press_key_impl(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  use auv_driver::{Driver, KeyPressOptions};

  reject_target_activation(&input, "input.key")?;
  if input.dry_run {
    return Ok(dry_run_output(input.command_id));
  }

  let key = required_input(&input, "key")?;
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let result = session
    .input()
    .press_key(KeyPressOptions {
      key: key.to_string(),
      ..KeyPressOptions::default()
    })
    .map_err(|error| error.to_string())?;
  input_action_output("pressed key in active app", "auv-driver-macos.input", &result).map(|mut output| {
    attach_input_key_report(&mut output, key, Some("active app"), &result);
    output
  })
}

#[cfg(not(target_os = "macos"))]
fn press_key_impl(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  Err("input.key is only available on macOS".to_string())
}

#[cfg(target_os = "macos")]
fn click_window_point_impl(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  use auv_driver::{ClickOptions, Driver};

  // TODO(invoke-input-click-window-point-candidate): --candidate JSON promotion
  // path is documented on the command summary but intentionally deferred; MC-19
  // D4 uses direct offset/relative point inputs only.
  if input.dry_run {
    return Ok(dry_run_output(input.command_id));
  }

  let uses_relative_window_point = input.inputs.contains_key("relative_x") || input.inputs.contains_key("relative_y");
  let absolute_window_point = if uses_relative_window_point {
    None
  } else {
    Some(resolve_click_window_point(input.inputs, input.command_id, None)?)
  };

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let window = session.window().resolve(click_window_selector(&input)).map_err(|error| error.to_string())?;
  let window_point = if uses_relative_window_point {
    resolve_click_window_point(input.inputs, input.command_id, Some(&window))?
  } else {
    absolute_window_point.expect("absolute window point must be present when relative inputs are absent")
  };
  let action = session.window().click(&window, window_point, ClickOptions::default()).map_err(|error| error.to_string())?;

  let mut output = input_action_output("clicked window point", "auv-driver-macos.window.input", &action)?;
  add_click_window_signals(&mut output, &window, window_point);
  Ok(output)
}

#[cfg(not(target_os = "macos"))]
fn click_window_point_impl(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  Err("input.clickWindowPoint is only available on macOS".to_string())
}

fn resolve_click_window_point(
  inputs: &std::collections::BTreeMap<String, String>,
  command_id: &str,
  window: Option<&auv_driver::Window>,
) -> Result<auv_driver::geometry::WindowPoint, String> {
  use auv_driver::geometry::WindowPoint;

  let has_offset_x = inputs.contains_key("offset_x");
  let has_offset_y = inputs.contains_key("offset_y");
  let has_relative_x = inputs.contains_key("relative_x");
  let has_relative_y = inputs.contains_key("relative_y");

  if has_offset_x || has_offset_y {
    if !has_offset_x || !has_offset_y {
      return Err(format!("{command_id} requires both --offset_x and --offset_y when using absolute window points"));
    }
    if has_relative_x || has_relative_y {
      return Err(format!("{command_id} accepts either --offset_x/--offset_y or --relative_x/--relative_y, not both"));
    }
    let offset_x = parse_required_number(inputs, "offset_x", command_id)?;
    let offset_y = parse_required_number(inputs, "offset_y", command_id)?;
    return Ok(WindowPoint::new(offset_x, offset_y));
  }

  if has_relative_x || has_relative_y {
    if !has_relative_x || !has_relative_y {
      return Err(format!("{command_id} requires both --relative_x and --relative_y when using relative window points"));
    }
    let window = window.ok_or_else(|| format!("{command_id} requires a resolved window for --relative_x/--relative_y"))?;
    let relative_x = parse_required_number(inputs, "relative_x", command_id)?;
    let relative_y = parse_required_number(inputs, "relative_y", command_id)?;
    return Ok(window_relative_window_point(window, relative_x, relative_y));
  }

  Err(format!("{command_id} requires --offset_x/--offset_y or --relative_x/--relative_y"))
}

fn parse_required_number(inputs: &std::collections::BTreeMap<String, String>, name: &str, command_id: &str) -> Result<f64, String> {
  let raw = inputs.get(name).ok_or_else(|| format!("{command_id} requires --{name}"))?;
  raw.parse::<f64>().map_err(|error| format!("{command_id} received invalid --{name}: {error}"))
}

fn window_relative_window_point(window: &auv_driver::Window, relative_x: f64, relative_y: f64) -> auv_driver::geometry::WindowPoint {
  auv_driver::geometry::WindowPoint::new(window.frame.size.width * relative_x, window.frame.size.height * relative_y)
}

#[cfg(target_os = "macos")]
fn click_window_selector(input: &InvokeCommandInput<'_>) -> auv_driver::WindowSelector {
  use auv_driver::{App, TextMatcher, WindowSelector};

  let mut selector = WindowSelector {
    main_visible: true,
    ..WindowSelector::default()
  };
  if let Some(target) = click_window_target(input) {
    selector.app = Some(App::bundle_id(target));
  }
  if let Some(title) = input.inputs.get("title").filter(|value| !value.trim().is_empty()) {
    selector.title = Some(TextMatcher::Contains(title.clone()));
  }
  selector
}

fn click_window_target<'a>(input: &'a InvokeCommandInput<'_>) -> Option<&'a str> {
  input.target_application_id.or_else(|| input.inputs.get("target").map(String::as_str)).filter(|value| !value.trim().is_empty())
}

#[cfg(target_os = "macos")]
fn add_click_window_signals(output: &mut InvokeCommandOutput, window: &auv_driver::Window, window_point: auv_driver::geometry::WindowPoint) {
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
  output.signals.insert("click.window_x".to_string(), window_point.point().x.to_string());
  output.signals.insert("click.window_y".to_string(), window_point.point().y.to_string());
}

fn required_input<'a>(input: &'a InvokeCommandInput<'_>, name: &str) -> Result<&'a str, String> {
  input
    .inputs
    .get(name)
    .map(String::as_str)
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| format!("{} requires --{name}", input.command_id))
}

fn reject_target_activation(input: &InvokeCommandInput<'_>, command_id: &str) -> Result<(), String> {
  if input.target_application_id.is_some() {
    // TODO(invoke-input-target-activation): foreground input APIs currently
    // act on the active control; add a typed app/window input lease before
    // honoring --target here.
    return Err(format!("{command_id} cannot use --target until typed input target activation is available"));
  }
  Ok(())
}

fn dry_run_output(command_id: &str) -> InvokeCommandOutput {
  InvokeCommandOutput::new(format!("dry run: {command_id}"))
}

#[cfg(target_os = "macos")]
fn input_action_output(summary: &str, backend: &str, result: &auv_driver::InputActionResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::new(summary);
  output.backend = Some(backend.to_string());
  output.signals.insert("input.selected_path".to_string(), format!("{:?}", result.selected_path));
  output.signals.insert("input.attempt_count".to_string(), result.attempts.len().to_string());
  output.signals.insert("input.mouse_disturbance".to_string(), format!("{:?}", result.mouse_disturbance));
  output.signals.insert("input.focus_disturbance".to_string(), format!("{:?}", result.focus_disturbance));
  output.signals.insert("input.clipboard_disturbance".to_string(), format!("{:?}", result.clipboard_disturbance));
  if let Some(reason) = &result.fallback_reason {
    output.signals.insert("input.fallback_reason".to_string(), reason.clone());
  }
  output.artifacts.push(input_action_artifact(result, "input-action-result")?);
  output.verification = Some("activation-only; semantic success requires a separate verification result".to_string());
  output
    .known_limits
    .push("input delivery records the selected input path and attempts only; it does not verify target UI state.".to_string());
  Ok(output)
}

fn input_key_report(key: &str, target: Option<&str>, backend: Option<&str>, result: &auv_driver::InputActionResult) -> InvokeReport {
  let mut fields = vec![
    report_field("Result", "delivered"),
    report_field("Key", key),
    report_field("Target", target.unwrap_or("active app")),
    report_field("Path", format!("{:?}", result.selected_path)),
  ];
  if let Some(backend) = backend {
    fields.push(report_field("Backend", backend));
  }
  InvokeReport::new(fields, Vec::new())
}

fn attach_input_key_report(output: &mut InvokeCommandOutput, key: &str, target: Option<&str>, result: &auv_driver::InputActionResult) {
  output.report = Some(input_key_report(key, target, output.backend.as_deref(), result));
}

fn report_field(label: &str, value: impl Into<String>) -> InvokeReportField {
  InvokeReportField {
    label: label.to_string(),
    value: value.into(),
  }
}

#[cfg(target_os = "macos")]
fn input_action_artifact(result: &auv_driver::InputActionResult, label: &str) -> Result<ProducedArtifact, String> {
  let source_path = std::env::temp_dir().join(format!("auv-invoke-{label}-{}-{}.json", std::process::id(), now_millis()));
  let body = serde_json::to_vec_pretty(result).map_err(|error| format!("failed to serialize input action artifact: {error}"))?;
  std::fs::write(&source_path, body).map_err(|error| format!("failed to write input action artifact: {error}"))?;
  Ok(ProducedArtifact {
    kind: "input-action-result".to_string(),
    source_path,
    preferred_name: format!("{label}.json"),
    note: Some("Typed InputActionResult recorded by the invoke handler.".to_string()),
  })
}

#[cfg(test)]
mod click_window_point_tests {
  use super::*;
  use auv_driver::{InputActionResult, InputDeliveryPath};
  use std::collections::BTreeMap;

  #[test]
  fn click_window_point_missing_point_args_returns_error() {
    let inputs = BTreeMap::new();
    let input = InvokeCommandInput {
      command_id: "input.clickWindowPoint",
      target_application_id: Some("com.example.App"),
      inputs: &inputs,
      dry_run: false,
    };
    let error = click_window_point_impl(input).expect_err("missing point args should fail");
    assert!(error.contains("requires --offset_x/--offset_y or --relative_x/--relative_y"));
  }

  #[test]
  fn click_window_point_dry_run_succeeds_without_driver() {
    let mut inputs = BTreeMap::new();
    inputs.insert("offset_x".to_string(), "640".to_string());
    inputs.insert("offset_y".to_string(), "360".to_string());
    let input = InvokeCommandInput {
      command_id: "input.clickWindowPoint",
      target_application_id: Some("com.example.App"),
      inputs: &inputs,
      dry_run: true,
    };
    let output = click_window_point_impl(input).expect("dry run should succeed");
    assert!(output.summary.contains("dry run: input.clickWindowPoint"));
  }

  #[test]
  fn resolve_click_window_point_accepts_offset_pair() {
    let mut inputs = BTreeMap::new();
    inputs.insert("offset_x".to_string(), "640".to_string());
    inputs.insert("offset_y".to_string(), "360".to_string());
    let point = resolve_click_window_point(&inputs, "input.clickWindowPoint", None).expect("offset pair");
    assert_eq!(point, auv_driver::geometry::WindowPoint::new(640.0, 360.0));
  }

  #[test]
  fn resolve_click_window_point_converts_relative_pair() {
    use auv_driver::geometry::{CoordinateSpace, Point, Rect, Size};
    use auv_driver::window::{Window, WindowRef};

    let mut inputs = BTreeMap::new();
    inputs.insert("relative_x".to_string(), "0.5".to_string());
    inputs.insert("relative_y".to_string(), "0.5".to_string());
    let window = Window {
      reference: WindowRef {
        id: "window-1".to_string(),
      },
      title: Some("Example".to_string()),
      app_name: Some("Example".to_string()),
      app_bundle_id: Some("com.example.App".to_string()),
      process_id: Some(1),
      frame: Rect {
        origin: Point::new(0.0, 0.0),
        size: Size::new(1280.0, 720.0),
      },
      coordinate_space: CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    };
    let point = resolve_click_window_point(&inputs, "input.clickWindowPoint", Some(&window)).expect("relative pair");
    assert_eq!(point, auv_driver::geometry::WindowPoint::new(640.0, 360.0));
  }

  #[test]
  fn input_key_report_includes_delivered_key_target_and_backend() {
    let result = InputActionResult::single_success(InputDeliveryPath::ForegroundSystemEvents);

    let mut output = InvokeCommandOutput::new("pressed key in active app");
    output.backend = Some("auv-driver-macos.input".to_string());
    attach_input_key_report(&mut output, "Cmd+L", Some("active app"), &result);
    assert!(
      output.report.is_some(),
      "input.key live path calls this helper after driver delivery, so this stable helper test verifies report population without sending a real key"
    );
    let report = output.report.as_ref().expect("report should be set");

    assert_eq!(field_value(&report, "Result"), "delivered");
    assert_eq!(field_value(&report, "Key"), "Cmd+L");
    assert_eq!(field_value(&report, "Target"), "active app");
    assert_eq!(field_value(&report, "Backend"), "auv-driver-macos.input");
  }

  fn field_value<'a>(report: &'a InvokeReport, label: &str) -> &'a str {
    report.fields.iter().find(|field| field.label == label).map(|field| field.value.as_str()).expect("field should exist")
  }
}
