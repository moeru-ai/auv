use crate::{CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, artifact::emit_prepared, invoke_command};
use crate::{InvokeReport, InvokeReportField};
use auv_tracing::{Attributes, ByteLength, NewArtifact};
use clap::{Args, ValueEnum};
use futures_util::io::Cursor as AsyncCursor;

use auv_driver::overlay::{Overlay, components::ClickTarget};
use auv_driver::{INPUT_ACTION_RESULT_PURPOSE, ScreenPoint, WindowInput as _};
const ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;

pub fn group() -> CommandGroup {
  // TODO(invoke-input-stubs): incomplete input commands stay intentionally
  // unregistered until owner-approved implementations have behavioral evidence.
  CommandGroup::new("input", "INPUT")
    .command(focus_text_input_invoke_command())
    .command(ax_focus_text_input_invoke_command())
    .command(type_text_invoke_command())
    .command(paste_text_preserve_clipboard_invoke_command())
    .command(press_key_invoke_command())
    .command(click_window_point_invoke_command())
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke input.focusText \"Search\" --target com.apple.TextEdit")]
struct FocusTextArgs {
  /// Text identifying the input to focus.
  #[arg(value_name = "TEXT")]
  query: String,
}

#[invoke_command(
  id = "input.focusText",
  group = "input",
  description = "Focus a target macOS text input through AX using its visible text.",
  input = FocusTextArgs,
)]
async fn focus_text_input(input: InvokeCommandInput, _args: FocusTextArgs) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let app = input.target_or_input_target().ok_or_else(|| "input.focusText requires --target".to_string())?.to_string();
  let query = input.inputs.get("query").cloned().unwrap_or_default();
  let candidate = input.inputs.get("candidate").cloned().unwrap_or_default();
  let result = focus_text(app, query, candidate.clone()).await?;
  focus_text_output(&result, &candidate)
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

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke input.axFocusText \"Search\" --target com.apple.TextEdit")]
struct AxFocusTextArgs {
  /// Text identifying the input to focus.
  #[arg(value_name = "TEXT")]
  query: String,
}

#[invoke_command(
  id = "input.axFocusText",
  group = "input",
  description = "Focus a text input by visible text via AXUIElementSetAttributeValue(kAXFocusedAttribute) without moving the real cursor. Errors when the target does not accept programmatic focus; use input.focusText if pointer movement is acceptable.",
  input = AxFocusTextArgs,
)]
async fn ax_focus_text_input(input: InvokeCommandInput, _args: AxFocusTextArgs) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let app = input.target_or_input_target().ok_or_else(|| "input.axFocusText requires --target".to_string())?.to_string();
  let query = input.inputs.get("query").cloned().unwrap_or_default();
  let candidate = input.inputs.get("candidate").cloned().unwrap_or_default();
  let result = focus_text(app, query, candidate.clone()).await?;
  focus_text_output(&result, &candidate)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke input.typeText \"hello from AUV\"")]
struct TypeTextArgs {
  /// Text to type into the active control.
  #[arg(value_name = "TEXT")]
  text: String,
}

#[invoke_command(
  id = "input.typeText",
  group = "input",
  description = "Type text into the active macOS control through System Events.",
  input = TypeTextArgs,
)]
async fn type_text(input: InvokeCommandInput, args: TypeTextArgs) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    reject_target_activation(&input, "input.typeText")?;
    let text = args.text;
    if input.dry_run {
      return Ok(validation_only_output());
    }

    let result = type_text_into_active_control(text).await?;
    input_action_output(&result)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (input, args);
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

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke input.pasteText \"hello from AUV\"")]
struct PasteTextArgs {
  /// Text to paste into the active control.
  #[arg(value_name = "TEXT")]
  text: String,
}

#[invoke_command(
  id = "input.pasteText",
  group = "input",
  description = "Paste text into the active macOS control through the clipboard, then restore the prior clipboard snapshot.",
  input = PasteTextArgs,
)]
async fn paste_text_preserve_clipboard(input: InvokeCommandInput, args: PasteTextArgs) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    reject_target_activation(&input, "input.pasteText")?;
    let text = args.text;
    if input.dry_run {
      return Ok(validation_only_output());
    }

    let result = paste_text_into_active_control(text).await?;
    input_action_output(&result)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (input, args);
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

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke input.key cmd+f")]
struct PressKeyArgs {
  /// Keyboard key or shortcut to press.
  #[arg(value_name = "KEY")]
  key: String,
}

#[invoke_command(
  id = "input.key",
  group = "input",
  description = "Press a keyboard key or shortcut in the active macOS app through System Events.",
  input = PressKeyArgs,
)]
async fn press_key(input: InvokeCommandInput, args: PressKeyArgs) -> InvokeCommandResult {
  #[cfg(target_os = "macos")]
  {
    reject_target_activation(&input, "input.key")?;
    let key = args.key;
    if input.dry_run {
      return Ok(validation_only_output());
    }

    let result = press_key_in_active_app(key.clone()).await?;
    let mut fields = input_action_report_fields(&result);
    fields.insert(1, InvokeReportField::new("Key", key));
    fields.insert(2, InvokeReportField::new("Target", "active app"));
    fields.push(InvokeReportField::new("Backend", "auv-driver-macos.input"));
    Ok(InvokeCommandOutput::from_result(&result)?.with_report(InvokeReport::new(fields, Vec::new())))
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (input, args);
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

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke input.clickWindowPoint --target com.apple.TextEdit --relative-x 0.5 --relative-y 0.5")]
struct ClickWindowPointArgs {
  /// Window title text used to select the target.
  #[arg(long, value_name = "TEXT")]
  title: Option<String>,
  /// Absolute window-pixel X coordinate.
  #[arg(long, requires = "offset_y", conflicts_with = "relative_x")]
  #[serde(rename = "offset-x", default)]
  offset_x: Option<f64>,
  /// Absolute window-pixel Y coordinate.
  #[arg(long, requires = "offset_x", conflicts_with = "relative_y")]
  #[serde(rename = "offset-y", default)]
  offset_y: Option<f64>,
  /// Relative window X coordinate in 0..1.
  #[arg(long, requires = "relative_y")]
  #[serde(rename = "relative-x", default)]
  relative_x: Option<f64>,
  /// Relative window Y coordinate in 0..1.
  #[arg(long, requires = "relative_x")]
  #[serde(rename = "relative-y", default)]
  relative_y: Option<f64>,
  /// Window input delivery policy.
  #[arg(long, value_enum)]
  #[serde(rename = "input-policy")]
  input_policy: Option<InputPolicyArg>,
  /// Number of consecutive clicks.
  #[arg(long, value_parser = clap::value_parser!(u8).range(1..))]
  #[serde(
    rename = "click-count",
    deserialize_with = "crate::command::deserialize_optional_nonzero_u8",
    default
  )]
  click_count: Option<u8>,
  /// Delay between clicks in milliseconds.
  #[arg(long)]
  #[serde(rename = "click-interval-ms")]
  click_interval_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum InputPolicyArg {
  BackgroundOnly,
  BackgroundPreferred,
  ForegroundPreferred,
}

impl ClickWindowPointArgs {
  fn point(&self, command_id: &str) -> Result<WindowPointInput, String> {
    match (self.offset_x, self.offset_y, self.relative_x, self.relative_y) {
      (Some(x), Some(y), None, None) if x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0 => {
        Ok(WindowPointInput(WindowPointKind::Offset(auv_driver::geometry::WindowPoint::new(x, y))))
      }
      (None, None, Some(x), Some(y)) if x.is_finite() && y.is_finite() && (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y) => {
        Ok(WindowPointInput(WindowPointKind::Relative(RelativeWindowPoint { x, y })))
      }
      (Some(_), Some(_), None, None) => Err(format!("{command_id} requires finite non-negative window offsets")),
      (None, None, Some(_), Some(_)) => Err(format!("{command_id} requires relative coordinates within 0..=1")),
      _ => Err(format!("{command_id} requires --offset-x/--offset-y or --relative-x/--relative-y")),
    }
  }

  fn click_options(&self) -> auv_driver::ClickOptions {
    click_options(self.input_policy.map(InputPolicyArg::driver_policy), self.click_count, self.click_interval_ms)
  }
}

impl InputPolicyArg {
  fn driver_policy(self) -> auv_driver::InputPolicy {
    match self {
      Self::BackgroundOnly => auv_driver::InputPolicy::BackgroundOnly,
      Self::BackgroundPreferred => auv_driver::InputPolicy::BackgroundPreferred,
      Self::ForegroundPreferred => auv_driver::InputPolicy::ForegroundPreferred,
    }
  }
}

#[invoke_command(
  id = "input.clickWindowPoint",
  group = "input",
  description = "Click a point relative to a target macOS window using either --offset-x/--offset-y or --relative-x/--relative-y coordinates.",
  input = ClickWindowPointArgs,
)]
async fn click_window_point(input: InvokeCommandInput, args: ClickWindowPointArgs) -> InvokeCommandResult {
  let point = args.point(&input.command_id)?;
  let options = args.click_options();
  #[cfg(target_os = "macos")]
  {
    let presentation_input = input.clone();
    let capability = LocalWindowPointCapability::open()?;
    let outcome = click_resolved_point_with_capability(input, args.title.as_deref(), point, options, &capability).await?;
    let result = outcome.into_result();
    let point = result.point.point();
    let screen_point = ScreenPoint::new(result.window.frame.origin.x + point.x, result.window.frame.origin.y + point.y);
    let click_overlay =
      Overlay::new().with_layer(ClickTarget::new(screen_point).with_cursor_label("auv · click").with_status("click delivered"));
    let overlay = super::overlay::show_overlay(
      &presentation_input,
      &capability.session,
      click_overlay,
      auv_driver::overlay::ShowOptions::new()
        .with_motion_ease(std::time::Duration::from_millis(420), auv_driver::overlay::Easing::EaseInOutExpo)
        .with_auto_removal_after(std::time::Duration::from_millis(140)),
    )?;
    window_point_click_output(result, overlay)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (input, point, options);
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
    options: auv_driver::ClickOptions,
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
    options: auv_driver::ClickOptions,
  ) -> auv_driver::DriverResult<auv_driver::InputActionResult> {
    self.session.window().click(window, point, options)
  }
}

async fn click_resolved_point_with_capability<C>(
  input: InvokeCommandInput,
  title: Option<&str>,
  point: WindowPointInput,
  options: auv_driver::ClickOptions,
  capability: &C,
) -> Result<WindowPointClickOutcome, String>
where
  C: WindowPointCapability + Sync + ?Sized,
{
  // TODO(invoke-input-click-window-point-candidate): candidate promotion is
  // intentionally deferred until an owner-approved typed candidate input exists.
  let window = capability.resolve(click_window_selector(&input, title)).map_err(|error| error.to_string())?;
  let point = point.resolve(&window, &input.command_id)?;
  input.cancellation.check().map_err(|error| error.to_string())?;
  if input.dry_run {
    return Ok(WindowPointClickOutcome::Validated { window, point });
  }

  let click = click_resolved_window_point(capability, window, point, options).await?;
  Ok(WindowPointClickOutcome::Delivered { click })
}

#[derive(Clone, Debug)]
struct WindowPointInput(WindowPointKind);

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

fn window_point_click_output(result: WindowPointClickResult, overlay: super::overlay::OverlayStatus) -> InvokeCommandResult {
  match &result.action {
    None => {
      let mut output = InvokeCommandOutput::from_result(&result)?;
      output.report = Some(InvokeReport::new(
        vec![
          InvokeReportField::new("Delivery", "not_performed"),
          InvokeReportField::new("Verification", "validation_only"),
          InvokeReportField::new("Window ID", result.window.reference.id.clone()),
          InvokeReportField::new("Window point", format!("{:.0},{:.0}", result.point.point().x, result.point.point().y)),
          overlay.report_field(),
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
      fields.push(overlay.report_field());
      Ok(InvokeCommandOutput::from_result(&result)?.with_report(InvokeReport::new(fields, Vec::new())))
    }
  }
}

async fn click_resolved_window_point<C>(
  capability: &C,
  window: auv_driver::Window,
  point: auv_driver::geometry::WindowPoint,
  options: auv_driver::ClickOptions,
) -> Result<WindowPointClick, String>
where
  C: WindowPointCapability + Sync + ?Sized,
{
  let action = capability.click(&window, point, options).map_err(|error| error.to_string())?;
  emit_input_action_result(&action);
  Ok(WindowPointClick {
    window,
    point,
    action,
  })
}

pub(crate) fn click_options(
  policy: Option<auv_driver::InputPolicy>,
  count: Option<u8>,
  interval_ms: Option<u64>,
) -> auv_driver::ClickOptions {
  let count = count.unwrap_or(1);
  let interval_ms = interval_ms.unwrap_or(75);
  auv_driver::ClickOptions {
    policy: policy.unwrap_or_default(),
    click: match count {
      1 => auv_driver::Click::Single,
      2 => auv_driver::Click::Double {
        interval: std::time::Duration::from_millis(interval_ms),
      },
      count => auv_driver::Click::Repeated {
        count,
        interval: std::time::Duration::from_millis(interval_ms),
      },
    },
    ..Default::default()
  }
}

fn click_window_selector(input: &InvokeCommandInput, title: Option<&str>) -> auv_driver::WindowSelector {
  use auv_driver::{App, TextMatcher, WindowSelector};

  let mut selector = WindowSelector {
    main_visible: true,
    ..WindowSelector::default()
  };
  if let Some(target) = input.target_or_input_target() {
    selector.app = Some(App::bundle_id(target));
  }
  if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
    selector.title = Some(TextMatcher::Contains(title.to_string()));
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

fn focus_text_output(result: &auv_driver::AxFocusResult, candidate: &str) -> InvokeCommandResult {
  let mut fields = vec![
    InvokeReportField::new("Delivery", "delivered"),
    InvokeReportField::new("Target", result.app.clone()),
  ];
  if candidate.trim().is_empty() {
    fields.push(InvokeReportField::new("Query", result.query.clone()));
  } else {
    fields.push(InvokeReportField::new("Candidate", candidate));
  }
  fields.extend([
    InvokeReportField::new("Resolved AX path", result.path.clone()),
    InvokeReportField::new("Role", result.role.clone()),
    InvokeReportField::new("Focus method", result.input_action_result.selected_path.as_str()),
    InvokeReportField::new("Verification", "delivery_only; focused element was not read back after AX delivery"),
  ]);
  Ok(InvokeCommandOutput::from_result(result)?.with_report(InvokeReport::new(fields, Vec::new())))
}

pub(super) fn input_action_report_fields(result: &auv_driver::InputActionResult) -> Vec<InvokeReportField> {
  let mut fields = vec![
    InvokeReportField::new("Delivery", "delivered"),
    InvokeReportField::new(
      "Verification",
      if result.verified {
        "verified"
      } else {
        "delivery_only"
      },
    ),
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

pub(super) fn validation_only_output() -> InvokeCommandOutput {
  InvokeCommandOutput::completed().with_report(InvokeReport::new(
    vec![
      InvokeReportField::new("Delivery", "not_performed"),
      InvokeReportField::new("Verification", "validation_only"),
    ],
    Vec::new(),
  ))
}

/// Emits validated input-delivery evidence into the active tracing context.
pub fn emit_input_action_result(result: &auv_driver::InputActionResult) {
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return;
  }
  emit_prepared(INPUT_ACTION_RESULT_PURPOSE, input_action_result_artifact(result));
}

fn input_action_result_artifact(result: &auv_driver::InputActionResult) -> Result<NewArtifact<AsyncCursor<Vec<u8>>>, String> {
  result.validate().map_err(|error| format!("{INPUT_ACTION_RESULT_PURPOSE} failed domain validation: {error}"))?;
  NewArtifact::from_json(
    INPUT_ACTION_RESULT_PURPOSE,
    Attributes::empty(),
    ByteLength::new(ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static input-action JSON limit is valid"),
    result,
  )
  .map_err(|error| format!("failed to construct {INPUT_ACTION_RESULT_PURPOSE} artifact: {error}"))
}

#[cfg(test)]
#[path = "input_test.rs"]
mod tests;
