use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
  arg::{
    KEY_ARGS, QUERY_ARGS, QUERY_OR_CANDIDATE_ARGS, QUERY_OR_CANDIDATE_OVERLAY_ARGS, QUERY_OVERLAY_ARGS, TARGET_ARGS, TEXT_ARGS, WINDOW_ARGS,
    WINDOW_CLICK_POINT_ARGS, WINDOW_QUERY_OVERLAY_ARGS,
  },
  artifact::emit_prepared,
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
async fn focus_text_input(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let app = input.target_or_input_target().ok_or_else(|| "input.focusText requires --target".to_string())?.to_string();
  let query = input.inputs.get("query").cloned().unwrap_or_default();
  let candidate = input.inputs.get("candidate").cloned().unwrap_or_default();
  InvokeCommandOutput::from_result(&focus_text(app, query, candidate).await?)
}

pub async fn focus_text(app: String, query: String, candidate: String) -> Result<auv_driver::AxFocusResult, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    session.accessibility().focus_text_by_query(&app, &query, None, &candidate).map_err(|error| error.to_string())
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (app, query, candidate);
    Err("input.focusText is only available on macOS".to_string())
  }
}

#[invoke_command(
  id = "input.pressButton",
  group = "input",
  description = "Press a known macOS button-like control by query through AX.",
  args = QUERY_ARGS,
)]
async fn press_button(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-input-ax-press): implement after AccessibilityApi owns a
  // query-to-press operation returning InputActionResult.
  unimplemented!("input.pressButton")
}

#[invoke_command(
  id = "input.axPressButton",
  group = "input",
  description = "Press a control by query via AXUIElementPerformAction without moving the real cursor. Pass --overlay true to draw a visual AUV cursor over the target. Falls back with an error when the AX target has no matching action; use input.pressButton for non-AX-pressable targets.",
  args = QUERY_OVERLAY_ARGS,
)]
async fn ax_press_button(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-input-ax-press): see `press_button`.
  unimplemented!("input.axPressButton")
}

#[invoke_command(
  id = "input.axFocusText",
  group = "input",
  description = "Focus a text input by query or promoted --candidate JSON via AXUIElementSetAttributeValue(kAXFocusedAttribute) without moving the real cursor. Pass --overlay true for the dual-cursor visual. Errors when the target does not accept programmatic focus; use input.focusText if pointer movement is acceptable.",
  args = QUERY_OR_CANDIDATE_OVERLAY_ARGS,
)]
async fn ax_focus_text_input(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let app = input.target_or_input_target().ok_or_else(|| "input.axFocusText requires --target".to_string())?.to_string();
  let query = input.inputs.get("query").cloned().unwrap_or_default();
  let candidate = input.inputs.get("candidate").cloned().unwrap_or_default();
  InvokeCommandOutput::from_result(&focus_text(app, query, candidate).await?)
}

#[invoke_command(
  id = "input.axClickWindowText",
  group = "input",
  description = "Find visible text in a window via Vision OCR, resolve the AX node at that point, then press it via AXUIElementPerformAction without moving the real cursor. Pass --overlay true for the dual-cursor visual. Errors with a hint to window.clickText when the OCR anchor maps to a canvas-rendered or non-AX-pressable region.",
  args = WINDOW_QUERY_OVERLAY_ARGS,
)]
async fn ax_click_window_text(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-input-ax-click-window-text): implement after the driver owns
  // OCR-to-AX resolution and returns InputActionResult evidence.
  unimplemented!("input.axClickWindowText")
}

#[invoke_command(
  id = "input.smartPress",
  group = "input",
  description = "ActionResolver v0 diagnostic press: try OCR-to-AX press first; if it fails and pointer fallback is allowed, fall back to pointer click.",
  args = WINDOW_QUERY_OVERLAY_ARGS,
)]
async fn smart_press(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-input-smart-press): implement only after an owner-approved
  // resolver consumes current typed recognition and input results.
  unimplemented!("input.smartPress")
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
      return Ok(InvokeCommandOutput::completed());
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
      return Ok(InvokeCommandOutput::completed());
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
      return Ok(InvokeCommandOutput::completed());
    }

    let key = input.required_input("key")?.to_string();
    let result = press_key_in_active_app(key.clone()).await?;
    let mut fields = input_action_report_fields(&result);
    fields.insert(1, InvokeReportField::new("Key", key));
    fields.insert(2, InvokeReportField::new("Target", "active app"));
    fields.push(InvokeReportField::new("Backend", "auv-driver-macos.input"));
    Ok(InvokeCommandOutput::from_result(&result)?.with_report(InvokeReport::new(fields, Vec::new())))
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
  // TODO(invoke-input-click-point): add global x/y arguments before calling
  // InputApi::click_at; the current command schema cannot express a point.
  unimplemented!("input.clickPoint")
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
      WindowPointKind::Relative(relative) => {
        auv_driver::geometry::WindowPoint::new(window.frame.size.width * relative.x, window.frame.size.height * relative.y)
      }
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
      let mut fields = input_action_report_fields(action);
      fields.push(InvokeReportField::new("Window ID", result.window.reference.id.clone()));
      if let Some(title) = &result.window.title {
        fields.push(InvokeReportField::new("Window title", title.clone()));
      }
      if let Some(app_name) = &result.window.app_name {
        fields.push(InvokeReportField::new("Application", app_name.clone()));
      }
      if let Some(bundle_id) = &result.window.app_bundle_id {
        fields.push(InvokeReportField::new("Bundle ID", bundle_id.clone()));
      }
      fields.push(InvokeReportField::new("Window point", format!("{:.0},{:.0}", result.point.point().x, result.point.point().y)));
      Ok(InvokeCommandOutput::from_result(&result)?.with_report(InvokeReport::new(fields, Vec::new())))
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
  // TODO(invoke-input-teach-click): implement after an owning module exposes
  // a typed interactive workflow result shared by frontends.
  unimplemented!("input.teachClick")
}

#[invoke_command(
  id = "input.scrollPoint",
  group = "input",
  description = "Scroll at a macOS global logical point through Quartz.",
  args = TARGET_ARGS,
)]
async fn scroll_point(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-input-scroll-point): add point and delta arguments before
  // calling InputApi::scroll_global_hid.
  unimplemented!("input.scrollPoint")
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

fn reject_target_activation(input: &InvokeCommandInput, command_id: &str) -> Result<(), String> {
  if input.target_application_id.is_some() {
    // TODO(invoke-input-target-activation): foreground input APIs currently
    // act on the active control; add a typed app/window input lease before
    // honoring --target here.
    return Err(format!("{command_id} cannot use --target until typed input target activation is available"));
  }
  Ok(())
}

fn input_action_output(result: &auv_driver::InputActionResult) -> InvokeCommandResult {
  Ok(InvokeCommandOutput::from_result(result)?.with_report(InvokeReport::new(input_action_report_fields(result), Vec::new())))
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
  if !auv_tracing::Context::current().can_publish_artifacts() {
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

#[cfg(test)]
#[path = "input_test.rs"]
mod tests;
