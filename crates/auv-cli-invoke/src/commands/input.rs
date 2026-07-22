use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
  arg::{
    KEY_ARGS, QUERY_ARGS, QUERY_OR_CANDIDATE_ARGS, QUERY_OR_CANDIDATE_OVERLAY_ARGS, QUERY_OVERLAY_ARGS, TARGET_ARGS, TEXT_ARGS, WINDOW_ARGS,
    WINDOW_CLICK_POINT_ARGS, WINDOW_QUERY_OVERLAY_ARGS,
  },
  artifact::{emission_enabled, json_artifact},
  invoke_command,
};
use crate::{InvokeReport, InvokeReportField};

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
async fn focus_text_input(_input: InvokeCommandInput) -> InvokeCommandResult {
  focus_text().await?;
  Ok(InvokeCommandOutput::new("focused text input"))
}

pub async fn focus_text() -> Result<(), String> {
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
async fn press_button(_input: InvokeCommandInput) -> InvokeCommandResult {
  press_button_by_query().await?;
  Ok(InvokeCommandOutput::new("pressed button"))
}

pub async fn press_button_by_query() -> Result<(), String> {
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
async fn ax_press_button(_input: InvokeCommandInput) -> InvokeCommandResult {
  press_button_with_ax().await?;
  Ok(InvokeCommandOutput::new("pressed button through AX"))
}

pub async fn press_button_with_ax() -> Result<(), String> {
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
async fn ax_focus_text_input(_input: InvokeCommandInput) -> InvokeCommandResult {
  focus_text_with_ax().await?;
  Ok(InvokeCommandOutput::new("focused text input through AX"))
}

pub async fn focus_text_with_ax() -> Result<(), String> {
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
async fn ax_click_window_text(_input: InvokeCommandInput) -> InvokeCommandResult {
  click_window_text_with_ax().await?;
  Ok(InvokeCommandOutput::new("clicked window text through AX"))
}

pub async fn click_window_text_with_ax() -> Result<(), String> {
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
async fn smart_press(_input: InvokeCommandInput) -> InvokeCommandResult {
  resolve_and_press().await?;
  Ok(InvokeCommandOutput::new("resolved and pressed target"))
}

pub async fn resolve_and_press() -> Result<(), String> {
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
async fn type_text(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    use auv_driver::TypeTextOptions;

    reject_target_activation(&input, "input.typeText")?;
    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let text = input.required_input("text")?.to_string();
    let result = type_text_into_active_control(text).await?;
    Ok(input_action_output("typed text into active control", "auv-driver-macos.input", &result))
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("input.typeText is only available on macOS".to_string())
  }
}

pub async fn type_text_into_active_control(text: String) -> Result<auv_driver::InputActionResult, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let result = session.input().type_text(&text, auv_driver::TypeTextOptions::default()).map_err(|error| error.to_string())?;
    emit_input_action_result(&result).await;
    Ok(result)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = text;
    Err("input.typeText is only available on macOS".to_string())
  }
}

#[invoke_command(
  id = "input.pasteText",
  group = "input",
  summary = "Paste text into the active macOS control through the clipboard, then restore the prior clipboard snapshot.",
  args = TEXT_ARGS,
)]
async fn paste_text_preserve_clipboard(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    use auv_driver::PasteTextOptions;

    reject_target_activation(&input, "input.pasteText")?;
    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let text = input.required_input("text")?.to_string();
    paste_text_into_active_control(text).await?;

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
  {
    let _ = input;
    Err("input.pasteText is only available on macOS".to_string())
  }
}

pub async fn paste_text_into_active_control(text: String) -> Result<(), String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    session
      .input()
      .paste_text(auv_driver::PasteTextOptions {
        text,
        ..auv_driver::PasteTextOptions::default()
      })
      .map_err(|error| error.to_string())
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = text;
    Err("input.pasteText is only available on macOS".to_string())
  }
}

#[invoke_command(
  id = "input.key",
  group = "input",
  summary = "Press a keyboard key or shortcut in the active macOS app through System Events.",
  args = KEY_ARGS,
)]
async fn press_key(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    use auv_driver::KeyPressOptions;

    reject_target_activation(&input, "input.key")?;
    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let key = input.required_input("key")?.to_string();
    let result = press_key_in_active_app(key.clone()).await?;
    let mut output = input_action_output("pressed key in active app", "auv-driver-macos.input", &result);
    attach_input_key_report(&mut output, &key, Some("active app"), &result);
    Ok(output)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("input.key is only available on macOS".to_string())
  }
}

pub async fn press_key_in_active_app(key: String) -> Result<auv_driver::InputActionResult, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let result = session
      .input()
      .press_key(auv_driver::KeyPressOptions {
        key,
        ..auv_driver::KeyPressOptions::default()
      })
      .map_err(|error| error.to_string())?;
    emit_input_action_result(&result).await;
    Ok(result)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = key;
    Err("input.key is only available on macOS".to_string())
  }
}

#[invoke_command(
  id = "input.clickPoint",
  group = "input",
  summary = "Click a macOS global logical point through Quartz.",
  args = TARGET_ARGS,
)]
async fn click_point(_input: InvokeCommandInput) -> InvokeCommandResult {
  click_global_point().await?;
  Ok(InvokeCommandOutput::new("clicked global point"))
}

pub async fn click_global_point() -> Result<(), String> {
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
async fn click_window_point(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    use auv_driver::ClickOptions;

    // TODO(invoke-input-click-window-point-candidate): --candidate JSON promotion
    // path is documented on the command summary but intentionally deferred; MC-19
    // D4 uses direct offset/relative point inputs only.
    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let point = if input.inputs.contains_key("relative_x") || input.inputs.contains_key("relative_y") {
      WindowPointInput::Relative {
        x: required_number(&input.inputs, "relative_x", &input.command_id)?,
        y: required_number(&input.inputs, "relative_y", &input.command_id)?,
      }
    } else {
      WindowPointInput::Offset(resolve_click_window_point(&input.inputs, &input.command_id, None)?)
    };
    let result = click_point_in_window(click_window_selector(&input), point).await?;
    let mut output = input_action_output("clicked window point", "auv-driver-macos.window.input", &result.action);
    add_click_window_signals(&mut output, &result.window, result.point);
    Ok(output)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("input.clickWindowPoint is only available on macOS".to_string())
  }
}

#[derive(Clone, Debug)]
pub enum WindowPointInput {
  Offset(auv_driver::geometry::WindowPoint),
  Relative { x: f64, y: f64 },
}

#[derive(Clone, Debug)]
pub struct WindowPointClick {
  pub window: auv_driver::Window,
  pub point: auv_driver::geometry::WindowPoint,
  pub action: auv_driver::InputActionResult,
}

pub async fn click_point_in_window(selector: auv_driver::WindowSelector, point: WindowPointInput) -> Result<WindowPointClick, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let window = session.window().resolve(selector).map_err(|error| error.to_string())?;
    let point = match point {
      WindowPointInput::Offset(point) => point,
      WindowPointInput::Relative { x, y } => window_relative_window_point(&window, x, y),
    };
    let action = session.window().click(&window, point, auv_driver::ClickOptions::default()).map_err(|error| error.to_string())?;
    emit_input_action_result(&action).await;
    Ok(WindowPointClick {
      window,
      point,
      action,
    })
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (selector, point);
    Err("input.clickWindowPoint is only available on macOS".to_string())
  }
}

#[invoke_command(
  id = "input.teachClick",
  group = "input",
  summary = "Capture a target window before and after a human-taught click, recording global and window-local click coordinates for automation debugging.",
  args = WINDOW_ARGS,
)]
async fn teach_click(_input: InvokeCommandInput) -> InvokeCommandResult {
  teach_click_workflow().await?;
  Ok(InvokeCommandOutput::new("recorded taught click"))
}

pub async fn teach_click_workflow() -> Result<(), String> {
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
async fn scroll_point(_input: InvokeCommandInput) -> InvokeCommandResult {
  scroll_global_point().await?;
  Ok(InvokeCommandOutput::new("scrolled global point"))
}

pub async fn scroll_global_point() -> Result<(), String> {
  // TODO(invoke-input-scroll-point): InputApi::scroll_global_hid exists, but
  // this invoke command exposes no x/y point or delta args; add direct scroll
  // inputs before enabling this command.
  Err("input.scrollPoint requires direct x/y point and scroll delta inputs".to_string())
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
    let offset_x = required_number(inputs, "offset_x", command_id)?;
    let offset_y = required_number(inputs, "offset_y", command_id)?;
    return Ok(WindowPoint::new(offset_x, offset_y));
  }

  if has_relative_x || has_relative_y {
    if !has_relative_x || !has_relative_y {
      return Err(format!("{command_id} requires both --relative_x and --relative_y when using relative window points"));
    }
    let window = window.ok_or_else(|| format!("{command_id} requires a resolved window for --relative_x/--relative_y"))?;
    let relative_x = required_number(inputs, "relative_x", command_id)?;
    let relative_y = required_number(inputs, "relative_y", command_id)?;
    return Ok(window_relative_window_point(window, relative_x, relative_y));
  }

  Err(format!("{command_id} requires --offset_x/--offset_y or --relative_x/--relative_y"))
}

fn required_number(inputs: &std::collections::BTreeMap<String, String>, name: &str, command_id: &str) -> Result<f64, String> {
  let raw = inputs.get(name).ok_or_else(|| format!("{command_id} requires --{name}"))?;
  raw.parse::<f64>().map_err(|error| format!("{command_id} received invalid --{name}: {error}"))
}

fn window_relative_window_point(window: &auv_driver::Window, relative_x: f64, relative_y: f64) -> auv_driver::geometry::WindowPoint {
  auv_driver::geometry::WindowPoint::new(window.frame.size.width * relative_x, window.frame.size.height * relative_y)
}

#[cfg(target_os = "macos")]
fn click_window_selector(input: &InvokeCommandInput) -> auv_driver::WindowSelector {
  use auv_driver::{App, TextMatcher, WindowSelector};

  let mut selector = WindowSelector {
    main_visible: true,
    ..WindowSelector::default()
  };
  if let Some(target) = input.target_or_input_target() {
    selector.app = Some(App::bundle_id(target));
  }
  if let Some(title) = input.inputs.get("title").filter(|value| !value.trim().is_empty()) {
    selector.title = Some(TextMatcher::Contains(title.clone()));
  }
  selector
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

fn reject_target_activation(input: &InvokeCommandInput, command_id: &str) -> Result<(), String> {
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
fn input_action_output(summary: &str, backend: &str, result: &auv_driver::InputActionResult) -> InvokeCommandOutput {
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
  output.verification = Some("activation-only; semantic success requires a separate verification result".to_string());
  output
    .known_limits
    .push("input delivery records the selected input path and attempts only; it does not verify target UI state.".to_string());
  output
}

async fn emit_input_action_result(result: &auv_driver::InputActionResult) {
  if emission_enabled()
    && let Ok(artifact) = json_artifact("auv.driver.input_action_result", result, auv_tracing::Attributes::empty())
  {
    let _ = auv_tracing::emit_artifact!(artifact).await;
  }
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
  InvokeReportField::new(label, value)
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
      command_id: "input.clickWindowPoint".to_string(),
      target_application_id: Some("com.example.App".to_string()),
      inputs,
      dry_run: false,
    };
    let error = futures_executor::block_on(click_window_point(input)).expect_err("missing point args should fail");
    assert!(error.contains("requires --offset_x/--offset_y or --relative_x/--relative_y"));
  }

  #[test]
  fn click_window_point_dry_run_succeeds_without_driver() {
    let mut inputs = BTreeMap::new();
    inputs.insert("offset_x".to_string(), "640".to_string());
    inputs.insert("offset_y".to_string(), "360".to_string());
    let input = InvokeCommandInput {
      command_id: "input.clickWindowPoint".to_string(),
      target_application_id: Some("com.example.App".to_string()),
      inputs,
      dry_run: true,
    };
    let output = futures_executor::block_on(click_window_point(input)).expect("dry run should succeed");
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
