use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
  arg::{
    KEY_ARGS, QUERY_ARGS, QUERY_OR_CANDIDATE_ARGS, QUERY_OR_CANDIDATE_OVERLAY_ARGS, QUERY_OVERLAY_ARGS, TARGET_ARGS, TEXT_ARGS, WINDOW_ARGS,
    WINDOW_CLICK_POINT_ARGS, WINDOW_QUERY_OVERLAY_ARGS,
  },
  artifact::{emission_enabled, emit_prepared},
  invoke_command,
};
use crate::{InvokeReport, InvokeReportField};
use auv_tracing::{ArtifactPurpose, Attributes, ByteLength, NewArtifact};
use futures_util::io::Cursor as AsyncCursor;

use auv_driver::INPUT_ACTION_RESULT_PURPOSE;
const ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;

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
  description = "Focus a target macOS text input through AX, either by --query text or by a promoted --candidate JSON payload.",
  args = QUERY_OR_CANDIDATE_ARGS,
)]
async fn focus_text_input(_input: InvokeCommandInput) -> InvokeCommandResult {
  focus_text().await?;
  Ok(InvokeCommandOutput::completed())
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
  description = "Press a known macOS button-like control by query through AX.",
  args = QUERY_ARGS,
)]
async fn press_button(_input: InvokeCommandInput) -> InvokeCommandResult {
  press_button_by_query().await?;
  Ok(InvokeCommandOutput::completed())
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
  description = "Press a control by query via AXUIElementPerformAction without moving the real cursor. Pass --overlay true to draw a visual AUV cursor over the target. Falls back with an error when the AX target has no matching action; use input.pressButton for non-AX-pressable targets.",
  args = QUERY_OVERLAY_ARGS,
)]
async fn ax_press_button(_input: InvokeCommandInput) -> InvokeCommandResult {
  press_button_with_ax().await?;
  Ok(InvokeCommandOutput::completed())
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
  description = "Focus a text input by query or promoted --candidate JSON via AXUIElementSetAttributeValue(kAXFocusedAttribute) without moving the real cursor. Pass --overlay true for the dual-cursor visual. Errors when the target does not accept programmatic focus; use input.focusText if pointer movement is acceptable.",
  args = QUERY_OR_CANDIDATE_OVERLAY_ARGS,
)]
async fn ax_focus_text_input(_input: InvokeCommandInput) -> InvokeCommandResult {
  focus_text_with_ax().await?;
  Ok(InvokeCommandOutput::completed())
}

pub async fn focus_text_with_ax() -> Result<(), String> {
  // TODO(invoke-input-ax-focus): AX focus-by-query/candidate has not moved
  // into a stable typed driver API yet; enable this after that boundary exists.
  Err("input.axFocusText requires a typed AX focus API".to_string())
}

#[invoke_command(
  id = "input.axClickWindowText",
  group = "input",
  description = "Find visible text in a window via Vision OCR, resolve the AX node at that point, then press it via AXUIElementPerformAction without moving the real cursor. Pass --overlay true for the dual-cursor visual. Errors with a hint to window.clickText when the OCR anchor maps to a canvas-rendered or non-AX-pressable region.",
  args = WINDOW_QUERY_OVERLAY_ARGS,
)]
async fn ax_click_window_text(_input: InvokeCommandInput) -> InvokeCommandResult {
  click_window_text_with_ax().await?;
  Ok(InvokeCommandOutput::completed())
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
  description = "ActionResolver v0 diagnostic press: try OCR-to-AX press first; if it fails and pointer fallback is allowed, fall back to pointer click.",
  args = WINDOW_QUERY_OVERLAY_ARGS,
)]
async fn smart_press(_input: InvokeCommandInput) -> InvokeCommandResult {
  resolve_and_press().await?;
  Ok(InvokeCommandOutput::completed())
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
  description = "Type text into the active macOS control through System Events.",
  args = TEXT_ARGS,
)]
async fn type_text(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    reject_target_activation(&input, "input.typeText")?;
    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let text = input.required_input("text")?.to_string();
    let result = type_text_into_active_control(text).await?;
    input_action_output(&result)
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
    emit_input_action_result(&result);
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
  description = "Paste text into the active macOS control through the clipboard, then restore the prior clipboard snapshot.",
  args = TEXT_ARGS,
)]
async fn paste_text_preserve_clipboard(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    reject_target_activation(&input, "input.pasteText")?;
    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let text = input.required_input("text")?.to_string();
    let result = paste_text_into_active_control(text).await?;
    input_action_output(&result)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("input.pasteText is only available on macOS".to_string())
  }
}

pub async fn paste_text_into_active_control(text: String) -> Result<auv_driver::InputActionResult, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let result = session
      .input()
      .paste_text(auv_driver::PasteTextOptions {
        text,
        ..auv_driver::PasteTextOptions::default()
      })
      .map_err(|error| error.to_string())?;
    emit_input_action_result(&result);
    Ok(result)
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
  description = "Press a keyboard key or shortcut in the active macOS app through System Events.",
  args = KEY_ARGS,
)]
async fn press_key(input: InvokeCommandInput) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    reject_target_activation(&input, "input.key")?;
    if input.dry_run {
      return Ok(dry_run_output(&input.command_id));
    }

    let key = input.required_input("key")?.to_string();
    let result = press_key_in_active_app(key.clone()).await?;
    let mut output = input_action_output(&result)?;
    attach_input_key_report(&mut output, &key, Some("active app"), Some("auv-driver-macos.input"), &result);
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
    emit_input_action_result(&result);
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
  description = "Click a macOS global logical point through Quartz.",
  args = TARGET_ARGS,
)]
async fn click_point(_input: InvokeCommandInput) -> InvokeCommandResult {
  click_global_point().await?;
  Ok(InvokeCommandOutput::completed())
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
  description = "Click a point relative to a target macOS window, either from --relative_x/--relative_y inputs or from a promoted --candidate JSON payload.",
  args = WINDOW_CLICK_POINT_ARGS,
)]
async fn click_window_point(input: InvokeCommandInput) -> InvokeCommandResult {
  let outcome = click_window_point_domain(input).await?;
  window_point_click_output(outcome)
}

/// Resolves and optionally delivers `input.clickWindowPoint`, returning the
/// typed domain value used independently by CLI and MCP adapters.
pub async fn click_window_point_domain(input: InvokeCommandInput) -> Result<WindowPointClickOutcome, String> {
  #[cfg(target_os = "macos")]
  {
    let capability = LocalWindowPointCapability::open()?;
    click_window_point_with_capability(input, &capability).await
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = input;
    Err("input.clickWindowPoint is only available on macOS".to_string())
  }
}

// Resolves target-window geometry and optionally delivers its validated click.
trait WindowPointCapability {
  fn resolve(&self, selector: auv_driver::WindowSelector) -> auv_driver::DriverResult<auv_driver::Window>;

  fn click(
    &self,
    window: &auv_driver::Window,
    point: auv_driver::geometry::WindowPoint,
  ) -> auv_driver::DriverResult<auv_driver::InputActionResult>;
}

#[cfg(target_os = "macos")]
struct LocalWindowPointCapability {
  session: auv_driver::LocalDriverSession,
}

#[cfg(target_os = "macos")]
impl LocalWindowPointCapability {
  fn open() -> Result<Self, String> {
    auv_driver::open_local().map(|session| Self { session }).map_err(|error| error.to_string())
  }
}

#[cfg(target_os = "macos")]
impl WindowPointCapability for LocalWindowPointCapability {
  fn resolve(&self, selector: auv_driver::WindowSelector) -> auv_driver::DriverResult<auv_driver::Window> {
    self.session.window().resolve(selector)
  }

  fn click(
    &self,
    window: &auv_driver::Window,
    point: auv_driver::geometry::WindowPoint,
  ) -> auv_driver::DriverResult<auv_driver::InputActionResult> {
    self.session.window().click(window, point, auv_driver::ClickOptions::default())
  }
}

async fn click_window_point_with_capability<C>(input: InvokeCommandInput, capability: &C) -> Result<WindowPointClickOutcome, String>
where
  C: WindowPointCapability + Sync + ?Sized,
{
  // TODO(invoke-input-click-window-point-candidate): --candidate JSON promotion
  // path is documented on the command summary but intentionally deferred; MC-19
  // D4 uses direct offset/relative point inputs only.
  let point = WindowPointInput::parse(&input.inputs, &input.command_id)?;
  let window = capability.resolve(click_window_selector(&input)).map_err(|error| error.to_string())?;
  let point = point.resolve(&window, &input.command_id)?;
  input.cancellation.check().map_err(|error| error.to_string())?;
  if input.dry_run {
    return Ok(WindowPointClickOutcome::Validated { window, point });
  }

  let click = click_resolved_window_point(capability, window, point).await?;
  Ok(WindowPointClickOutcome::Delivered { click })
}

#[derive(Clone, Debug)]
pub struct WindowPointInput(WindowPointKind);

#[derive(Clone, Debug)]
enum WindowPointKind {
  Offset(auv_driver::geometry::WindowPoint),
  Relative(RelativeWindowPoint),
}

#[derive(Clone, Copy, Debug)]
struct RelativeWindowPoint {
  x: f64,
  y: f64,
}

impl WindowPointInput {
  pub fn parse(inputs: &std::collections::BTreeMap<String, String>, command_id: &str) -> Result<Self, String> {
    let has_offset_x = inputs.contains_key("offset_x");
    let has_offset_y = inputs.contains_key("offset_y");
    let has_relative_x = inputs.contains_key("relative_x");
    let has_relative_y = inputs.contains_key("relative_y");

    if (has_offset_x || has_offset_y) && (has_relative_x || has_relative_y) {
      return Err(format!("{command_id} accepts either --offset_x/--offset_y or --relative_x/--relative_y, not both"));
    }
    if has_offset_x || has_offset_y {
      if !has_offset_x || !has_offset_y {
        return Err(format!("{command_id} requires both --offset_x and --offset_y when using absolute window points"));
      }
      let x = required_offset_number(inputs, "offset_x", command_id)?;
      let y = required_offset_number(inputs, "offset_y", command_id)?;
      return Ok(Self(WindowPointKind::Offset(auv_driver::geometry::WindowPoint::new(x, y))));
    }
    if has_relative_x || has_relative_y {
      if !has_relative_x || !has_relative_y {
        return Err(format!("{command_id} requires both --relative_x and --relative_y when using relative window points"));
      }
      let x = required_relative_number(inputs, "relative_x", command_id)?;
      let y = required_relative_number(inputs, "relative_y", command_id)?;
      return Ok(Self(WindowPointKind::Relative(RelativeWindowPoint { x, y })));
    }

    Err(format!("{command_id} requires --offset_x/--offset_y or --relative_x/--relative_y"))
  }

  fn resolve(&self, window: &auv_driver::Window, command_id: &str) -> Result<auv_driver::geometry::WindowPoint, String> {
    let point = match self.0 {
      WindowPointKind::Offset(point) => point,
      WindowPointKind::Relative(relative) => window_relative_window_point(window, relative.x, relative.y),
    };
    let coordinates = point.point();
    if !(0.0..=window.frame.size.width).contains(&coordinates.x) || !(0.0..=window.frame.size.height).contains(&coordinates.y) {
      return Err(format!(
        "{command_id} point {},{} is outside target window bounds 0..={},0..={}",
        coordinates.x, coordinates.y, window.frame.size.width, window.frame.size.height
      ));
    }
    Ok(point)
  }
}

#[derive(Clone, Debug)]
pub struct WindowPointClick {
  pub window: auv_driver::Window,
  pub point: auv_driver::geometry::WindowPoint,
  pub action: auv_driver::InputActionResult,
}

#[derive(Clone, Debug)]
pub enum WindowPointClickOutcome {
  Validated {
    window: auv_driver::Window,
    point: auv_driver::geometry::WindowPoint,
  },
  Delivered {
    click: WindowPointClick,
  },
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct WindowPointClickResult {
  pub window: auv_driver::Window,
  pub point: auv_driver::geometry::WindowPoint,
  pub action: Option<auv_driver::InputActionResult>,
}

impl WindowPointClickOutcome {
  pub fn into_result(self) -> WindowPointClickResult {
    match self {
      Self::Validated { window, point } => WindowPointClickResult {
        window,
        point,
        action: None,
      },
      Self::Delivered { click } => WindowPointClickResult {
        window: click.window,
        point: click.point,
        action: Some(click.action),
      },
    }
  }
}

fn window_point_click_output(outcome: WindowPointClickOutcome) -> InvokeCommandResult {
  let result = outcome.into_result();
  match &result.action {
    None => {
      let mut output = InvokeCommandOutput::from_result(&result)?;
      output.report = Some(InvokeReport::new(
        vec![
          InvokeReportField::new("Window ID", result.window.reference.id.clone()),
          InvokeReportField::new("Window point", format!("{:.0},{:.0}", result.point.point().x, result.point.point().y)),
        ],
        Vec::new(),
      ));
      Ok(output)
    }
    Some(action) => {
      let mut output = InvokeCommandOutput::from_result(&result)?;
      output.report = Some(InvokeReport::new(input_action_report_fields(action), Vec::new()));
      append_click_window_report(&mut output, &result.window, result.point);
      Ok(output)
    }
  }
}

pub async fn click_point_in_window(selector: auv_driver::WindowSelector, point: WindowPointInput) -> Result<WindowPointClick, String> {
  #[cfg(target_os = "macos")]
  {
    let capability = LocalWindowPointCapability::open()?;
    let window = capability.resolve(selector).map_err(|error| error.to_string())?;
    let point = point.resolve(&window, "input.clickWindowPoint")?;
    click_resolved_window_point(&capability, window, point).await
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (selector, point);
    Err("input.clickWindowPoint is only available on macOS".to_string())
  }
}

async fn click_resolved_window_point<C>(
  capability: &C,
  window: auv_driver::Window,
  point: auv_driver::geometry::WindowPoint,
) -> Result<WindowPointClick, String>
where
  C: WindowPointCapability + Sync + ?Sized,
{
  let action = capability.click(&window, point).map_err(|error| error.to_string())?;
  emit_input_action_result(&action);
  Ok(WindowPointClick {
    window,
    point,
    action,
  })
}

#[invoke_command(
  id = "input.teachClick",
  group = "input",
  description = "Capture a target window before and after a human-taught click, recording global and window-local click coordinates for automation debugging.",
  args = WINDOW_ARGS,
)]
async fn teach_click(_input: InvokeCommandInput) -> InvokeCommandResult {
  teach_click_workflow().await?;
  Ok(InvokeCommandOutput::completed())
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
  description = "Scroll at a macOS global logical point through Quartz.",
  args = TARGET_ARGS,
)]
async fn scroll_point(_input: InvokeCommandInput) -> InvokeCommandResult {
  scroll_global_point().await?;
  Ok(InvokeCommandOutput::completed())
}

pub async fn scroll_global_point() -> Result<(), String> {
  // TODO(invoke-input-scroll-point): InputApi::scroll_global_hid exists, but
  // this invoke command exposes no x/y point or delta args; add direct scroll
  // inputs before enabling this command.
  Err("input.scrollPoint requires direct x/y point and scroll delta inputs".to_string())
}

fn required_number(inputs: &std::collections::BTreeMap<String, String>, name: &str, command_id: &str) -> Result<f64, String> {
  let raw = inputs.get(name).ok_or_else(|| format!("{command_id} requires --{name}"))?;
  let value = raw.parse::<f64>().map_err(|error| format!("{command_id} received invalid --{name}: {error}"))?;
  if !value.is_finite() {
    return Err(format!("{command_id} requires --{name} to be finite"));
  }
  Ok(value)
}

fn required_offset_number(inputs: &std::collections::BTreeMap<String, String>, name: &str, command_id: &str) -> Result<f64, String> {
  let value = required_number(inputs, name, command_id)?;
  if value < 0.0 {
    return Err(format!("{command_id} requires --{name} to be non-negative"));
  }
  Ok(value)
}

fn required_relative_number(inputs: &std::collections::BTreeMap<String, String>, name: &str, command_id: &str) -> Result<f64, String> {
  let value = required_number(inputs, name, command_id)?;
  if !(0.0..=1.0).contains(&value) {
    return Err(format!("{command_id} requires --{name} to be within 0..=1"));
  }
  Ok(value)
}

fn window_relative_window_point(window: &auv_driver::Window, relative_x: f64, relative_y: f64) -> auv_driver::geometry::WindowPoint {
  auv_driver::geometry::WindowPoint::new(window.frame.size.width * relative_x, window.frame.size.height * relative_y)
}

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

fn append_click_window_report(
  output: &mut InvokeCommandOutput,
  window: &auv_driver::Window,
  window_point: auv_driver::geometry::WindowPoint,
) {
  let Some(report) = output.report.as_mut() else {
    return;
  };
  report.fields.push(InvokeReportField::new("Window ID", window.reference.id.clone()));
  if let Some(title) = &window.title {
    report.fields.push(InvokeReportField::new("Window title", title.clone()));
  }
  if let Some(app_name) = &window.app_name {
    report.fields.push(InvokeReportField::new("Application", app_name.clone()));
  }
  if let Some(bundle_id) = &window.app_bundle_id {
    report.fields.push(InvokeReportField::new("Bundle ID", bundle_id.clone()));
  }
  report.fields.push(InvokeReportField::new("Window point", format!("{:.0},{:.0}", window_point.point().x, window_point.point().y)));
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

fn dry_run_output(_command_id: &str) -> InvokeCommandOutput {
  InvokeCommandOutput::completed()
}

fn input_action_output(result: &auv_driver::InputActionResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(InvokeReport::new(input_action_report_fields(result), Vec::new()));
  Ok(output)
}

fn input_action_report_fields(result: &auv_driver::InputActionResult) -> Vec<InvokeReportField> {
  let mut fields = vec![
    InvokeReportField::new("Result", "delivered"),
    InvokeReportField::new("Path", result.selected_path.as_str()),
    InvokeReportField::new("Attempts", result.attempts.len().to_string()),
    InvokeReportField::new("Mouse disturbance", result.mouse_disturbance.as_str()),
    InvokeReportField::new("Focus disturbance", result.focus_disturbance.as_str()),
    InvokeReportField::new("Clipboard disturbance", result.clipboard_disturbance.as_str()),
  ];
  if let Some(reason) = result.fallback_reason() {
    fields.push(InvokeReportField::new("Fallback reason", reason));
  }
  fields
}

pub(super) fn emit_input_action_result(result: &auv_driver::InputActionResult) {
  if !emission_enabled() {
    return;
  }
  emit_prepared(INPUT_ACTION_RESULT_PURPOSE, input_action_result_artifact(result));
}

fn input_action_result_artifact(result: &auv_driver::InputActionResult) -> Result<NewArtifact<AsyncCursor<Vec<u8>>>, String> {
  if result.attempts.iter().any(|attempt| attempt.succeeded && attempt.path != result.selected_path) {
    return Err(format!("{INPUT_ACTION_RESULT_PURPOSE} failed domain validation: successful input attempt must match selected_path"));
  }
  NewArtifact::from_json(
    ArtifactPurpose::parse(INPUT_ACTION_RESULT_PURPOSE)
      .map_err(|error| format!("invalid {INPUT_ACTION_RESULT_PURPOSE} purpose: {error}"))?,
    Attributes::empty(),
    ByteLength::new(ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static input-action JSON limit is valid"),
    result,
  )
  .map_err(|error| format!("failed to construct {INPUT_ACTION_RESULT_PURPOSE} artifact: {error}"))
}

fn input_key_report(key: &str, target: Option<&str>, backend: Option<&str>, result: &auv_driver::InputActionResult) -> InvokeReport {
  let mut fields = input_action_report_fields(result);
  fields.insert(1, report_field("Key", key));
  fields.insert(2, report_field("Target", target.unwrap_or("active app")));
  if let Some(backend) = backend {
    fields.push(report_field("Backend", backend));
  }
  InvokeReport::new(fields, Vec::new())
}

fn attach_input_key_report(
  output: &mut InvokeCommandOutput,
  key: &str,
  target: Option<&str>,
  backend: Option<&str>,
  result: &auv_driver::InputActionResult,
) {
  output.report = Some(input_key_report(key, target, backend, result));
}

fn report_field(label: &str, value: impl Into<String>) -> InvokeReportField {
  InvokeReportField::new(label, value)
}

#[cfg(test)]
mod click_window_point_tests {
  use super::*;
  use auv_driver::{InputActionResult, InputDeliveryPath};
  use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure, dispatcher};
  use futures_util::StreamExt;
  use std::collections::BTreeMap;
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};

  #[derive(Clone)]
  struct ControlledWindowCapability {
    window: auv_driver::Window,
    action: InputActionResult,
    resolve_calls: Arc<AtomicUsize>,
    click_calls: Arc<AtomicUsize>,
  }

  impl ControlledWindowCapability {
    fn new() -> Self {
      Self {
        window: test_window(),
        action: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
        resolve_calls: Arc::new(AtomicUsize::new(0)),
        click_calls: Arc::new(AtomicUsize::new(0)),
      }
    }

    fn resolve_count(&self) -> usize {
      self.resolve_calls.load(Ordering::SeqCst)
    }

    fn click_count(&self) -> usize {
      self.click_calls.load(Ordering::SeqCst)
    }

    fn with_action(mut self, action: InputActionResult) -> Self {
      self.action = action;
      self
    }
  }

  impl WindowPointCapability for ControlledWindowCapability {
    fn resolve(&self, _selector: auv_driver::WindowSelector) -> auv_driver::DriverResult<auv_driver::Window> {
      self.resolve_calls.fetch_add(1, Ordering::SeqCst);
      Ok(self.window.clone())
    }

    fn click(
      &self,
      _window: &auv_driver::Window,
      _point: auv_driver::geometry::WindowPoint,
    ) -> auv_driver::DriverResult<auv_driver::InputActionResult> {
      self.click_calls.fetch_add(1, Ordering::SeqCst);
      Ok(self.action.clone())
    }
  }

  #[test]
  fn click_window_point_missing_point_args_returns_error() {
    let inputs = BTreeMap::new();
    let input = InvokeCommandInput {
      command_id: "input.clickWindowPoint".to_string(),
      target_application_id: Some("com.example.App".to_string()),
      inputs,
      dry_run: false,
      cancellation: crate::InvokeCancellation::new(),
    };
    let error = futures_executor::block_on(click_window_point(input)).expect_err("missing point args should fail");
    assert!(error.contains("requires --offset_x/--offset_y or --relative_x/--relative_y"));
  }

  #[test]
  fn click_window_point_valid_dry_run_resolves_window_without_clicking() {
    let capability = ControlledWindowCapability::new();
    let mut inputs = BTreeMap::new();
    inputs.insert("offset_x".to_string(), "640".to_string());
    inputs.insert("offset_y".to_string(), "360".to_string());
    let input = InvokeCommandInput {
      command_id: "input.clickWindowPoint".to_string(),
      target_application_id: Some("com.example.App".to_string()),
      inputs,
      dry_run: true,
      cancellation: crate::InvokeCancellation::new(),
    };
    let outcome = futures_executor::block_on(click_window_point_with_capability(input, &capability)).expect("dry run should succeed");

    assert!(matches!(outcome, WindowPointClickOutcome::Validated { .. }));
    assert_eq!(capability.resolve_count(), 1);
    assert_eq!(capability.click_count(), 0);
  }

  #[test]
  fn click_window_point_out_of_bounds_dry_run_fails_without_clicking() {
    let capability = ControlledWindowCapability::new();
    let input = InvokeCommandInput {
      command_id: "input.clickWindowPoint".to_string(),
      target_application_id: Some("com.example.App".to_string()),
      inputs: BTreeMap::from([
        ("offset_x".to_string(), "1280.01".to_string()),
        ("offset_y".to_string(), "360".to_string()),
      ]),
      dry_run: true,
      cancellation: crate::InvokeCancellation::new(),
    };

    // ROOT CAUSE:
    //
    // If a syntactically valid positive offset exceeded the resolved window,
    // dry-run completed because the handler returned before window resolution.
    //
    // Before the fix, this input completed without consulting window geometry.
    // The fix resolves containment before dry-run returns and never clicks.
    let error = futures_executor::block_on(click_window_point_with_capability(input, &capability))
      .expect_err("out-of-bounds dry-run offset must fail");

    assert!(error.contains("outside target window bounds"), "{error}");
    assert_eq!(capability.resolve_count(), 1);
    assert_eq!(capability.click_count(), 0);
  }

  #[test]
  fn click_window_point_live_resolves_once_before_clicking() {
    let capability = ControlledWindowCapability::new();
    let input = InvokeCommandInput {
      command_id: "input.clickWindowPoint".to_string(),
      target_application_id: Some("com.example.App".to_string()),
      inputs: BTreeMap::from([
        ("offset_x".to_string(), "640".to_string()),
        ("offset_y".to_string(), "360".to_string()),
      ]),
      dry_run: false,
      cancellation: crate::InvokeCancellation::new(),
    };

    let outcome = futures_executor::block_on(click_window_point_with_capability(input, &capability)).expect("valid live point");

    assert!(matches!(outcome, WindowPointClickOutcome::Delivered { .. }));
    assert_eq!(capability.resolve_count(), 1);
    assert_eq!(capability.click_count(), 1);
  }

  #[tokio::test]
  async fn resolved_window_click_returns_direct_action_and_publishes_through_typed_root_contract() {
    let capability = ControlledWindowCapability::new();
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let expected = InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse);
    let future =
      root.in_scope(|| click_resolved_window_point(&capability, test_window(), auv_driver::geometry::WindowPoint::new(640.0, 360.0)));

    let delivered = root.instrument(future).await.expect("direct window click result");
    dispatch.flush().await.expect("flush input action telemetry");
    let snapshot = store.load_snapshot(run_id).await.expect("snapshot read").expect("input run");

    assert_eq!(delivered.action, expected);
    let publication = snapshot.artifacts().values().next().expect("input action artifact");
    assert_eq!(snapshot.artifacts().len(), 1);
    assert_eq!(publication.metadata().purpose().as_str(), INPUT_ACTION_RESULT_PURPOSE);
    assert_eq!(publication.metadata().content_type().to_string(), "application/json");
    let mut reader = store.open_artifact(publication.metadata().uri().clone()).await.expect("open input action artifact");
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.next().await {
      bytes.extend_from_slice(&chunk.expect("input action artifact chunk"));
    }
    let recorded: InputActionResult = serde_json::from_slice(&bytes).expect("typed input action payload");
    assert_eq!(recorded, expected);
  }

  #[tokio::test]
  async fn invalid_input_artifact_does_not_change_the_typed_call_or_reexecute_driver_input() {
    let invalid = InputActionResult {
      selected_path: InputDeliveryPath::WindowTargetedMouse,
      attempts: vec![auv_driver::InputAttempt::success(
        InputDeliveryPath::AxPress,
      )],
      mouse_disturbance: auv_driver::DisturbanceLevel::None,
      focus_disturbance: auv_driver::DisturbanceLevel::None,
      clipboard_disturbance: auv_driver::DisturbanceLevel::None,
    };
    let capability = ControlledWindowCapability::new().with_action(invalid.clone());
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let future =
      root.in_scope(|| click_resolved_window_point(&capability, test_window(), auv_driver::geometry::WindowPoint::new(640.0, 360.0)));

    let delivered = root.instrument(future).await.expect("artifact preparation must not replace the direct input result");
    dispatch.flush().await.expect("typed preparation diagnostic should flush");
    let snapshot = store.load_snapshot(run_id).await.expect("snapshot read").expect("diagnostic run");

    assert_eq!(delivered.action, invalid);
    assert_eq!(capability.click_count(), 1, "artifact preparation must not reexecute direct input");
    assert!(snapshot.artifacts().is_empty(), "invalid evidence must not commit an artifact");
  }

  #[tokio::test]
  async fn input_action_emission_short_circuits_without_run_context() {
    let invalid = InputActionResult {
      selected_path: InputDeliveryPath::WindowTargetedMouse,
      attempts: vec![auv_driver::InputAttempt::success(
        InputDeliveryPath::AxPress,
      )],
      mouse_disturbance: auv_driver::DisturbanceLevel::None,
      focus_disturbance: auv_driver::DisturbanceLevel::None,
      clipboard_disturbance: auv_driver::DisturbanceLevel::None,
    };

    emit_input_action_result(&invalid);
  }

  #[test]
  fn input_action_artifact_enforces_domain_and_four_mibibyte_bounds() {
    let invalid = InputActionResult {
      selected_path: InputDeliveryPath::WindowTargetedMouse,
      attempts: vec![auv_driver::InputAttempt::success(
        InputDeliveryPath::AxPress,
      )],
      mouse_disturbance: auv_driver::DisturbanceLevel::None,
      focus_disturbance: auv_driver::DisturbanceLevel::None,
      clipboard_disturbance: auv_driver::DisturbanceLevel::None,
    };
    let domain_error = input_action_result_artifact(&invalid).err().expect("mismatched successful attempt must fail");
    assert!(domain_error.contains("successful input attempt must match selected_path"));

    let oversized = InputActionResult {
      selected_path: InputDeliveryPath::WindowTargetedMouse,
      attempts: vec![
        auv_driver::InputAttempt::failure(InputDeliveryPath::AxPress, "x".repeat(ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT as usize)),
        auv_driver::InputAttempt::success(InputDeliveryPath::WindowTargetedMouse),
      ],
      mouse_disturbance: auv_driver::DisturbanceLevel::None,
      focus_disturbance: auv_driver::DisturbanceLevel::None,
      clipboard_disturbance: auv_driver::DisturbanceLevel::None,
    };
    let size_error = input_action_result_artifact(&oversized).err().expect("oversized input action must fail");
    assert!(size_error.contains("4194304-byte limit"));
  }

  #[test]
  fn click_window_point_negative_offset_dry_run_fails_without_driver() {
    let input = InvokeCommandInput {
      command_id: "input.clickWindowPoint".to_string(),
      target_application_id: Some("com.example.App".to_string()),
      inputs: BTreeMap::from([
        ("offset_x".to_string(), "-0.01".to_string()),
        ("offset_y".to_string(), "20".to_string()),
      ]),
      dry_run: true,
      cancellation: crate::InvokeCancellation::new(),
    };

    let error = futures_executor::block_on(click_window_point(input)).expect_err("negative dry-run offset must fail before driver work");

    assert!(error.contains("offset_x") && error.contains("non-negative"), "{error}");
  }

  #[test]
  fn resolve_click_window_point_accepts_inclusive_offset_boundaries() {
    let window = test_window();
    for (x, y) in [(0.0, 0.0), (1280.0, 720.0)] {
      let inputs = BTreeMap::from([
        ("offset_x".to_string(), x.to_string()),
        ("offset_y".to_string(), y.to_string()),
      ]);
      let point = WindowPointInput::parse(&inputs, "input.clickWindowPoint")
        .and_then(|point| point.resolve(&window, "input.clickWindowPoint"))
        .expect("inclusive window boundary");
      assert_eq!(point, auv_driver::geometry::WindowPoint::new(x, y));
    }
  }

  #[test]
  fn resolve_click_window_point_rejects_offsets_outside_window_bounds() {
    let window = test_window();
    for (name, x, y, expected_error) in [
      ("negative x", -0.01, 20.0, "non-negative"),
      ("negative y", 10.0, -0.01, "non-negative"),
      ("oversized x", 1280.01, 20.0, "outside target window"),
      ("oversized y", 10.0, 720.01, "outside target window"),
    ] {
      let inputs = BTreeMap::from([
        ("offset_x".to_string(), x.to_string()),
        ("offset_y".to_string(), y.to_string()),
      ]);
      let error = WindowPointInput::parse(&inputs, "input.clickWindowPoint")
        .and_then(|point| point.resolve(&window, "input.clickWindowPoint"))
        .expect_err("out-of-window offset must fail");
      assert!(error.contains(expected_error), "{name}: {error}");
    }
  }

  #[test]
  fn window_point_input_rejects_mixed_coordinate_modes() {
    let inputs = BTreeMap::from([
      ("offset_x".to_string(), "10".to_string()),
      ("offset_y".to_string(), "20".to_string()),
      ("relative_x".to_string(), "0.5".to_string()),
      ("relative_y".to_string(), "0.5".to_string()),
    ]);

    let error = WindowPointInput::parse(&inputs, "input.clickWindowPoint").expect_err("mixed modes must fail");

    assert!(error.contains("not both"));
  }

  #[test]
  fn window_point_input_rejects_incomplete_pairs() {
    for inputs in [
      BTreeMap::from([("offset_x".to_string(), "10".to_string())]),
      BTreeMap::from([("relative_y".to_string(), "0.5".to_string())]),
    ] {
      let error = WindowPointInput::parse(&inputs, "input.clickWindowPoint").expect_err("incomplete pair must fail");
      assert!(error.contains("requires both"));
    }
  }

  #[test]
  fn window_point_input_rejects_non_finite_values() {
    for (x_name, y_name) in [("offset_x", "offset_y"), ("relative_x", "relative_y")] {
      for value in ["NaN", "inf", "-inf"] {
        let inputs = BTreeMap::from([
          (x_name.to_string(), value.to_string()),
          (y_name.to_string(), "0.5".to_string()),
        ]);
        let error = WindowPointInput::parse(&inputs, "input.clickWindowPoint").expect_err("non-finite coordinate must fail");
        assert!(error.contains("finite"), "{x_name}={value}: {error}");
      }
    }
  }

  #[test]
  fn window_point_input_rejects_relative_values_outside_unit_interval() {
    for value in ["-0.01", "1.01"] {
      let inputs = BTreeMap::from([
        ("relative_x".to_string(), value.to_string()),
        ("relative_y".to_string(), "0.5".to_string()),
      ]);
      let error = WindowPointInput::parse(&inputs, "input.clickWindowPoint").expect_err("out-of-range relative coordinate must fail");
      assert!(error.contains("0..=1"));
    }
  }

  #[test]
  fn resolve_click_window_point_converts_relative_pair() {
    let mut inputs = BTreeMap::new();
    inputs.insert("relative_x".to_string(), "0.5".to_string());
    inputs.insert("relative_y".to_string(), "0.5".to_string());
    let window = test_window();
    let point = WindowPointInput::parse(&inputs, "input.clickWindowPoint")
      .and_then(|point| point.resolve(&window, "input.clickWindowPoint"))
      .expect("relative pair");
    assert_eq!(point, auv_driver::geometry::WindowPoint::new(640.0, 360.0));
  }

  #[test]
  fn resolve_click_window_point_accepts_inclusive_relative_boundaries() {
    let window = test_window();
    for (relative_x, relative_y, expected_x, expected_y) in [(0.0, 0.0, 0.0, 0.0), (1.0, 1.0, 1280.0, 720.0)] {
      let inputs = BTreeMap::from([
        ("relative_x".to_string(), relative_x.to_string()),
        ("relative_y".to_string(), relative_y.to_string()),
      ]);
      let point = WindowPointInput::parse(&inputs, "input.clickWindowPoint")
        .and_then(|point| point.resolve(&window, "input.clickWindowPoint"))
        .expect("inclusive relative boundary");
      assert_eq!(point, auv_driver::geometry::WindowPoint::new(expected_x, expected_y));
    }
  }

  fn test_window() -> auv_driver::Window {
    use auv_driver::geometry::{CoordinateSpace, Point, Rect, Size};
    use auv_driver::window::{Window, WindowRef};

    Window {
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
    }
  }

  #[test]
  fn input_key_report_includes_delivered_key_target_and_backend() {
    let result = InputActionResult::single_success(InputDeliveryPath::ForegroundSystemEvents);

    let mut output = InvokeCommandOutput::completed();
    attach_input_key_report(&mut output, "Cmd+L", Some("active app"), Some("auv-driver-macos.input"), &result);
    assert!(
      output.report.is_some(),
      "input.key live path calls this helper after driver delivery, so this stable helper test verifies report population without sending a real key"
    );
    let report = output.report.as_ref().expect("report should be set");

    assert_eq!(field_value(report, "Result"), "delivered");
    assert_eq!(field_value(report, "Key"), "Cmd+L");
    assert_eq!(field_value(report, "Target"), "active app");
    assert_eq!(field_value(report, "Backend"), "auv-driver-macos.input");
    assert_eq!(field_value(report, "Path"), "foreground_system_events");
  }

  #[test]
  fn input_action_output_reports_explicit_domain_values() {
    let result = InputActionResult {
      selected_path: InputDeliveryPath::WindowTargetedKeyboardScroll,
      attempts: vec![],
      mouse_disturbance: auv_driver::DisturbanceLevel::None,
      focus_disturbance: auv_driver::DisturbanceLevel::Foreground,
      clipboard_disturbance: auv_driver::DisturbanceLevel::Temporary,
    };

    let output = input_action_output(&result).expect("input result should serialize");

    let report = output.report.as_ref().expect("input action report");
    assert_eq!(field_value(report, "Path"), "window_targeted_keyboard_scroll");
    assert_eq!(field_value(report, "Mouse disturbance"), "none");
    assert_eq!(field_value(report, "Focus disturbance"), "foreground");
    assert_eq!(field_value(report, "Clipboard disturbance"), "temporary");
    assert_eq!(output.result(), Some(&serde_json::to_value(&result).expect("fixture should serialize")));
  }

  #[test]
  fn window_point_click_result_keeps_resolved_target_and_delivery_together() {
    let click = WindowPointClick {
      window: test_window(),
      point: auv_driver::geometry::WindowPoint::new(640.0, 360.0),
      action: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
    };

    let output = window_point_click_output(WindowPointClickOutcome::Delivered { click }).expect("click result should serialize");
    let result = output.result().expect("click should have a result");

    assert_eq!(result["window"]["reference"]["id"], "window-1");
    assert_eq!(result["point"]["x"], 640.0);
    assert_eq!(result["point"]["y"], 360.0);
    assert_eq!(result["action"]["selected_path"], "window_targeted_mouse");
  }

  fn field_value<'a>(report: &'a InvokeReport, label: &str) -> &'a str {
    report.fields.iter().find(|field| field.label == label).map(|field| field.value.as_str()).expect("field should exist")
  }
}
