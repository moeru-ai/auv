use crate::{CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, artifact::emit_prepared, invoke_command};
use crate::{InvokeReport, InvokeReportField};
use auv_tracing::{Attributes, ByteLength, NewArtifact};
use clap::{Args, ValueEnum};
use futures_util::io::Cursor as AsyncCursor;

use auv_driver::{INPUT_ACTION_RESULT_PURPOSE, ScreenPoint, WindowInput as _};
const ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;

pub fn group() -> CommandGroup {
  // TODO(invoke-input-stubs): incomplete input commands stay intentionally
  // unregistered until owner-approved implementations have behavioral evidence.
  // TODO(invoke-keyboard-hold): cross-call holds need a persistent invoke owner
  // and release route; expose them here after that frontend lifecycle is defined.
  // TODO(invoke-mouse-hold): cross-call mouseDown/mouseUp have the same owner and
  // release requirement. input.drag exposes only the complete one-admission gesture.
  CommandGroup::new("input", "INPUT")
    .command(focus_text_input_invoke_command())
    .command(ax_focus_text_input_invoke_command())
    .command(type_text_invoke_command())
    .command(paste_text_preserve_clipboard_invoke_command())
    .command(press_key_invoke_command())
    .command(press_keys_invoke_command())
    .command(press_keys_named_invoke_command())
    .command(hold_keys_invoke_command())
    .command(input_keyboard_invoke_command())
    .command(move_mouse_invoke_command())
    .command(click_point_invoke_command())
    .command(drag_invoke_command())
    .command(scroll_invoke_command())
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke input.moveMouse 1032.5 1212")]
struct MoveMouseArgs {
  /// Logical screen X coordinate.
  x: f64,
  /// Logical screen Y coordinate.
  y: f64,
}

#[invoke_command(
  id = "input.moveMouse",
  group = "input",
  description = "Move the pointer to a logical screen coordinate without activating it.",
  input = MoveMouseArgs,
)]
async fn move_mouse(input: InvokeCommandInput, args: MoveMouseArgs) -> InvokeCommandResult {
  if !args.x.is_finite() || !args.y.is_finite() {
    return Err("input.moveMouse requires finite coordinates".to_string());
  }
  let point = ScreenPoint::new(args.x, args.y);
  if input.dry_run {
    return mouse_move_output(MouseMoveResult {
      point,
      action: None,
    });
  }
  #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
    let action = session.input().move_to(point.point()).map_err(|error| error.to_string())?;
    emit_input_action_result(&action);
    mouse_move_output(MouseMoveResult {
      point,
      action: Some(action),
    })
  }
  #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
  {
    let _ = input;
    Err("input.moveMouse is unavailable on this platform".to_string())
  }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct MouseMoveResult {
  pub point: ScreenPoint,
  pub action: Option<auv_driver::InputActionResult>,
}

pub fn mouse_move_output(result: MouseMoveResult) -> InvokeCommandResult {
  let mut fields = match result.action.as_ref() {
    Some(action) => input_action_report_fields(action),
    None => vec![
      InvokeReportField::new("Delivery", "not_performed"),
      InvokeReportField::new("Verification", "validation_only"),
    ],
  };
  fields.push(InvokeReportField::new("Screen point", format!("{:.1},{:.1}", result.point.point().x, result.point.point().y)));
  Ok(InvokeCommandOutput::from_result(&result)?.with_report(InvokeReport::new(fields, Vec::new())))
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
  target = RequiredApplication,
  group = "input",
  description = "Focus a target macOS text input through AX using its visible text.",
  input = FocusTextArgs,
)]
async fn focus_text_input(input: InvokeCommandInput, _args: FocusTextArgs) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let app = input.application_target()?.ok_or_else(|| "input.focusText requires --target app:".to_string())?.to_string();
  let query = input.inputs.get("query").cloned().unwrap_or_default();
  let candidate = input.inputs.get("candidate").cloned().unwrap_or_default();
  let result = focus_text(app, query, candidate.clone()).await?;
  focus_text_output(&result, &candidate)
}

pub async fn focus_text(app: String, query: String, candidate: String) -> Result<auv_driver::AxFocusResult, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
    let selector = if candidate.trim().is_empty() {
      auv_driver::AxTextSelector::Query(query)
    } else {
      auv_driver::AxTextSelector::Path(candidate)
    };
    session
      .accessibility()
      .focus_text(auv_driver::FocusTextOptions {
        app,
        selector,
        expected_role: None,
      })
      .map_err(|error| error.to_string())
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

// NOTICE(input-ax-focus-text-alias): this command is a compatibility alias for
// input.focusText. Both use the same typed macOS AX focus operation; it does
// not provide an overlay, pointer fallback, or post-delivery focus readback.
// Remove the alias only after an owner-approved CLI compatibility boundary.
#[invoke_command(
  id = "input.axFocusText",
  target = RequiredApplication,
  group = "input",
  description = "Compatibility alias for input.focusText; focuses a text input through the same macOS AX focus operation.",
  input = AxFocusTextArgs,
)]
async fn ax_focus_text_input(input: InvokeCommandInput, _args: AxFocusTextArgs) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let app = input.application_target()?.ok_or_else(|| "input.axFocusText requires --target app:".to_string())?.to_string();
  let query = input.inputs.get("query").cloned().unwrap_or_default();
  let candidate = input.inputs.get("candidate").cloned().unwrap_or_default();
  let result = focus_text(app, query, candidate.clone()).await?;
  focus_text_output(&result, &candidate)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke input.typeText \"hello from AUV\"")]
struct TypeTextArgs {
  /// Target delivery policy. Foreground prepares focus; background modes do not activate or automatically fall back.
  #[arg(long, value_enum)]
  #[serde(rename = "input-policy")]
  input_policy: Option<InputPolicyArg>,
  /// Text to type into the active control.
  #[arg(value_name = "TEXT")]
  text: String,
}

#[invoke_command(
  id = "input.typeText",
  target = OptionalKeyboard,
  group = "input",
  description = "Type text into the active control through the platform input driver.",
  input = TypeTextArgs,
)]
async fn type_text(input: InvokeCommandInput, args: TypeTextArgs) -> crate::InvokeExecutionResult {
  execute_keyboard(&input, vec![args.into()])
}

pub async fn type_text_into_active_control(text: String) -> Result<auv_driver::InputActionResult, String> {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
    let result = session.input().type_text(&text, auv_driver::TypeTextOptions::default()).map_err(|error| error.to_string())?;
    emit_input_action_result(&result);
    Ok(result)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = text;
    Err("input.typeText is unavailable on this platform".to_string())
  }
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke input.pasteText \"hello from AUV\"")]
struct PasteTextArgs {
  /// Target delivery policy. Foreground prepares focus; background modes do not activate or automatically fall back.
  #[arg(long, value_enum)]
  #[serde(rename = "input-policy")]
  input_policy: Option<InputPolicyArg>,
  /// Text to paste into the active control.
  #[arg(value_name = "TEXT")]
  text: String,
}

#[invoke_command(
  id = "input.pasteText",
  target = OptionalKeyboard,
  group = "input",
  description = "Paste text into the active control through the clipboard, then restore the prior clipboard snapshot.",
  input = PasteTextArgs,
)]
async fn paste_text_preserve_clipboard(input: InvokeCommandInput, args: PasteTextArgs) -> crate::InvokeExecutionResult {
  execute_keyboard(&input, vec![args.into()])
}

pub async fn paste_text_into_active_control(text: String) -> Result<auv_driver::InputActionResult, String> {
  #[cfg(any(target_os = "macos", target_os = "linux"))]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
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
  #[cfg(not(any(target_os = "macos", target_os = "linux")))]
  {
    let _ = text;
    Err("input.pasteText is available only on macOS and Linux".to_string())
  }
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke input.key cmd+f")]
struct PressKeyArgs {
  /// Physical key or legacy shortcut (for example cmd+a). Use input.typeText for literal Unicode text.
  #[arg(value_name = "KEY")]
  key: String,
  /// Foreground prepares focus; background modes do not activate or automatically fall back.
  #[arg(long, value_enum)]
  #[serde(rename = "input-policy")]
  input_policy: Option<InputPolicyArg>,
  /// Number of complete presses, 1..255. Repetition requires --interval-ms.
  #[arg(long)]
  count: Option<u32>,
  /// Interval between complete presses in milliseconds; no delay after the last.
  #[arg(long)]
  #[serde(rename = "interval-ms")]
  interval_ms: Option<u64>,
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke input.keys cmd shift p\n  auv invoke input.keys return --count 2 --interval-ms 100")]
struct PressKeysArgs {
  /// One key combination: key names or ANSI characters. Modifiers precede ordinary keys. MCP inputs encode this list as a JSON array string.
  #[arg(value_name = "KEY", num_args = 1..)]
  keys: Vec<String>,
  /// Foreground prepares focus; background modes do not activate or automatically fall back.
  #[arg(long, value_enum)]
  #[serde(rename = "input-policy")]
  input_policy: Option<InputPolicyArg>,
  /// Number of complete presses, 1..255. Repetition requires --interval-ms.
  #[arg(long)]
  count: Option<u32>,
  /// Interval between complete presses in milliseconds; no delay after the last.
  #[arg(long)]
  #[serde(rename = "interval-ms")]
  interval_ms: Option<u64>,
}

#[invoke_command(id = "input.keys", target = OptionalKeyboard, group = "input",
  description = "Press and release a key combination, optionally repeated. Keys are released in reverse order; effects remain unverified.", input = PressKeysArgs)]
async fn press_keys(input: InvokeCommandInput, args: PressKeysArgs) -> crate::InvokeExecutionResult {
  execute_keyboard(&input, vec![args.into()])
}

// NOTICE: input.keys is the existing complete-press spelling. Keep both
// commands on the same typed path while callers adopt the explicit verb.
#[invoke_command(id = "input.pressKeys", target = OptionalKeyboard, group = "input",
  description = "Press and release a key combination, optionally repeated. Equivalent to input.keys.", input = PressKeysArgs)]
async fn press_keys_named(input: InvokeCommandInput, args: PressKeysArgs) -> crate::InvokeExecutionResult {
  press_keys(input, args).await
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Example:\n  auv invoke input.holdKeys shift --duration-ms 800")]
struct HoldKeysArgs {
  /// One key combination. Modifiers precede ordinary keys.
  #[arg(value_name = "KEY", num_args = 1..)]
  keys: Vec<String>,

  /// Time between key down and release, in milliseconds (1..=30000).
  #[arg(long)]
  #[serde(rename = "duration-ms")]
  duration_ms: u64,

  /// Foreground prepares focus; background modes require an application or window target.
  #[arg(long, value_enum)]
  #[serde(rename = "input-policy")]
  input_policy: Option<InputPolicyArg>,
}

impl HoldKeysArgs {
  fn validated(self, has_target: bool) -> Result<(Vec<String>, auv_driver::InputPolicy, std::time::Duration), crate::InvokeFailure> {
    if self.keys.is_empty() {
      return Err(crate::InvokeFailure::new(crate::FailureCode::InvalidInput, "hold keys must not be empty"));
    }

    if !(1..=30_000).contains(&self.duration_ms) {
      return Err(crate::InvokeFailure::new(crate::FailureCode::InvalidInput, "hold duration must be in (0, 30s]"));
    }

    let policy = keyboard_policy(self.input_policy);
    if !has_target && policy != auv_driver::InputPolicy::ForegroundPreferred {
      return Err(crate::InvokeFailure::new(crate::FailureCode::InvalidInput, "background keyboard input requires --target"));
    }

    Ok((self.keys, policy, std::time::Duration::from_millis(self.duration_ms)))
  }
}

#[invoke_command(id = "input.holdKeys", target = OptionalKeyboard, group = "input",
  description = "Hold one key combination for a bounded duration, then release it. Delivery does not verify application effects.", input = HoldKeysArgs)]
async fn hold_keys(input: InvokeCommandInput, args: HoldKeysArgs) -> crate::InvokeExecutionResult {
  let (keys, policy, duration) = args.validated(input.target.is_some())?;
  execute_hold_keys(&input, keys, policy, duration).await
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
async fn execute_hold_keys(
  input: &InvokeCommandInput,
  keys: Vec<String>,
  policy: auv_driver::InputPolicy,
  duration: std::time::Duration,
) -> crate::InvokeExecutionResult {
  let session = auv::local::open()?;
  let target = local_keyboard_target(input, &session)?;
  input.cancellation.check().map_err(|error| error.to_string())?;

  if input.dry_run {
    let options = auv_driver::PressKeysOptions {
      keys,
      ..Default::default()
    };
    session.input().input_keyboard(&target, vec![auv_driver::KeyboardInput::PressKeys { options, policy }], true)?;
    return targeted_keyboard_output(None).map_err(Into::into);
  }

  // Dropping the invoke future wakes the held-key wait, including RPC cancellation.
  let action = run_cancellable_input(&input.cancellation, move || session.input().hold_keys(&target, keys, policy, duration)).await?;
  targeted_keyboard_output(Some(&action)).map_err(Into::into)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
async fn execute_hold_keys(
  _input: &InvokeCommandInput,
  _keys: Vec<String>,
  _policy: auv_driver::InputPolicy,
  _duration: std::time::Duration,
) -> crate::InvokeExecutionResult {
  Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "keyboard invoke is unavailable on this platform"))
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke input.keyboard --actions '[{\"kind\":\"press\",\"keys\":[\"cmd\",\"a\"]},{\"kind\":\"type_text\",\"text\":\"hello\"}]' --target app:com.netease.163music\nActions: press (keys, optional count and interval_ms), type_text (text), paste_text (text). Entire list is validated first; a delivery failure stops execution and retains progress."
)]
struct InputKeyboardArgs {
  /// JSON array: press {keys, count?, interval_ms?}, type_text {text}, or paste_text {text}; each object requires kind.
  #[arg(long, value_name = "JSON")]
  actions: String,
  /// Apply this policy to every action; foreground preparation is the default.
  #[arg(long, value_enum)]
  #[serde(rename = "input-policy")]
  input_policy: Option<InputPolicyArg>,
}

// CLI JSON uses millisecond durations; domain options retain typed Duration.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum KeyboardActionArg {
  Press {
    keys: Vec<String>,
    count: Option<u32>,
    interval_ms: Option<u64>,
  },
  TypeText {
    text: String,
  },
  PasteText {
    text: String,
  },
}

#[invoke_command(id = "input.keyboard", target = OptionalKeyboard, group = "input",
  description = "Execute ordered keyboard actions. Stops on failure with partial progress; delivery does not verify control effects.", input = InputKeyboardArgs)]
async fn input_keyboard(input: InvokeCommandInput, args: InputKeyboardArgs) -> crate::InvokeExecutionResult {
  let actions = args.into_keyboard_inputs().map_err(|message| crate::InvokeFailure::new(crate::FailureCode::InvalidInput, message))?;
  execute_keyboard(&input, actions)
}

#[invoke_command(
  id = "input.key",
  target = OptionalKeyboard,
  group = "input",
  description = "Press a keyboard key or shortcut through the platform input driver.",
  input = PressKeyArgs,
)]
async fn press_key(input: InvokeCommandInput, args: PressKeyArgs) -> crate::InvokeExecutionResult {
  execute_keyboard(&input, vec![args.into()])
}

pub async fn press_key_in_active_app(key: String) -> Result<auv_driver::InputActionResult, String> {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
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
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = key;
    Err("input.key is unavailable on this platform".to_string())
  }
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke input.clickPoint 1032.5 1212\n  auv invoke input.clickPoint 0.5 0.5 --target app:com.apple.TextEdit --relative-to window --normalized\n  auv invoke input.clickPoint 100 80 --target display:1 --relative-to display"
)]
struct ClickPointArgs {
  /// Mouse button: left, right, or middle. Defaults to left.
  #[arg(long, default_value = "left", value_parser = ["left", "right", "middle"])]
  #[serde(default = "default_click_button")]
  button: String,
  /// X coordinate in the selected coordinate basis.
  x: f64,
  /// Y coordinate in the selected coordinate basis.
  y: f64,
  /// Coordinate basis. Defaults from --target: screen, window, or display.
  #[arg(long, value_enum)]
  #[serde(rename = "relative-to", default)]
  relative_to: Option<RelativeToArg>,
  /// Interpret X and Y as normalized values in 0..=1.
  #[arg(long)]
  #[serde(default)]
  normalized: bool,
  /// Window title text used with an app target.
  #[arg(long, value_name = "TEXT")]
  title: Option<String>,
  /// Window input delivery policy. Valid only with --relative-to window.
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
  #[serde(rename = "click-interval-ms", default)]
  click_interval_ms: Option<u64>,
  /// Mouse modifiers: shift, control, alt/option, meta/cmd. Repeat this option or separate names with commas.
  #[arg(long, value_name = "KEYS", value_delimiter = ',')]
  #[serde(
    default,
    serialize_with = "serialize_click_modifiers",
    deserialize_with = "deserialize_click_modifiers"
  )]
  modifiers: Vec<String>,
}

fn default_click_button() -> String {
  "left".to_string()
}

pub(crate) fn parse_click_button(value: Option<&str>) -> Result<auv_driver::MouseButton, String> {
  match value.unwrap_or("left") {
    "left" => Ok(auv_driver::MouseButton::Left),
    "right" => Ok(auv_driver::MouseButton::Right),
    "middle" => Ok(auv_driver::MouseButton::Middle),
    value => Err(format!("unknown mouse button {value:?}; expected left, right, or middle")),
  }
}

// Keep invoke protocol and recorded arguments as one comma-separated scalar,
// while Clap collects repeated flags into a list.
fn serialize_click_modifiers<S: serde::Serializer>(values: &[String], serializer: S) -> Result<S::Ok, S::Error> {
  if values.is_empty() {
    serializer.serialize_none()
  } else {
    serializer.serialize_str(&values.join(","))
  }
}

fn deserialize_click_modifiers<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
  let value = <Option<String> as serde::Deserialize>::deserialize(deserializer)?;
  Ok(value.map(|value| value.split(',').map(str::to_owned).collect()).unwrap_or_default())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RelativeToArg {
  Screen,
  Window,
  Display,
}

impl RelativeToArg {
  pub(crate) fn as_str(self) -> &'static str {
    match self {
      Self::Screen => "screen",
      Self::Window => "window",
      Self::Display => "display",
    }
  }
}

impl ClickPointArgs {
  fn basis(&self, target: Option<&crate::ExecutionTarget>) -> Result<RelativeToArg, String> {
    let basis = point_basis(
      "input.clickPoint",
      target,
      self.relative_to.map(RelativeToArg::as_str),
      self.normalized,
      self.input_policy.is_some(),
      self.title.is_some(),
    )?;
    if !self.x.is_finite() || !self.y.is_finite() {
      return Err("input.clickPoint requires finite coordinates".to_string());
    }
    if self.normalized && (!(0.0..=1.0).contains(&self.x) || !(0.0..=1.0).contains(&self.y)) {
      return Err("input.clickPoint --normalized coordinates must be within 0..=1".to_string());
    }
    if self.click_count.unwrap_or(1) > 1 && self.click_interval_ms == Some(0) {
      return Err("input.clickPoint requires a positive --click-interval-ms for repeated clicks".to_string());
    }
    Ok(basis)
  }

  fn click_options(&self) -> Result<auv_driver::ClickOptions, String> {
    let mut options = click_options(self.input_policy.map(InputPolicyArg::driver_policy), self.click_count, self.click_interval_ms);
    options.button = parse_click_button(Some(&self.button))?;
    let modifiers = self.modifiers.join(",");
    options.modifiers = parse_click_modifiers((!self.modifiers.is_empty()).then_some(modifiers.as_str()))?;
    Ok(options)
  }
}

/// Resolves and validates the coordinate basis shared by point-based input commands.
pub(crate) fn point_basis(
  command_id: &str,
  target: Option<&crate::ExecutionTarget>,
  relative_to: Option<&str>,
  normalized: bool,
  has_input_policy: bool,
  has_title: bool,
) -> Result<RelativeToArg, String> {
  let basis = match relative_to {
    Some("screen") => RelativeToArg::Screen,
    Some("window") => RelativeToArg::Window,
    Some("display") => RelativeToArg::Display,
    Some(value) => return Err(format!("{command_id} has unknown --relative-to {value:?}")),
    None => match target {
      None => RelativeToArg::Screen,
      Some(crate::ExecutionTarget::Application { .. } | crate::ExecutionTarget::Window { .. }) => RelativeToArg::Window,
      Some(crate::ExecutionTarget::Display { .. }) => RelativeToArg::Display,
    },
  };
  match (target, basis) {
    (None, RelativeToArg::Screen)
    | (Some(crate::ExecutionTarget::Application { .. } | crate::ExecutionTarget::Window { .. }), RelativeToArg::Window)
    | (Some(crate::ExecutionTarget::Display { .. }), RelativeToArg::Display) => {}
    (None, RelativeToArg::Window) => return Err(format!("{command_id} --relative-to window requires --target app: or window:")),
    (None, RelativeToArg::Display) => return Err(format!("{command_id} --relative-to display requires --target display:")),
    (Some(_), _) => return Err(format!("{command_id} --target kind is incompatible with --relative-to {}", basis.as_str())),
  }
  if normalized && basis == RelativeToArg::Screen {
    return Err(format!("{command_id} --normalized is valid only relative to a window or display"));
  }
  if has_input_policy && basis != RelativeToArg::Window {
    return Err(format!("{command_id} --input-policy is valid only with --relative-to window"));
  }
  if has_title && !matches!(target, Some(crate::ExecutionTarget::Application { .. })) {
    return Err(format!("{command_id} --title requires --target app:"));
  }
  Ok(basis)
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ClickPointResult {
  pub relative_to: String,
  pub requested_point: auv_driver::Point,
  pub normalized: bool,
  pub screen_point: ScreenPoint,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub window: Option<auv_driver::Window>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub display: Option<auv_driver::Display>,
  pub action: Option<auv_driver::InputActionResult>,
}

#[invoke_command(
  id = "input.clickPoint",
  target = OptionalPoint,
  group = "input",
  description = "Click a point relative to the screen, a target window, or a target display.",
  input = ClickPointArgs,
)]
async fn click_point(input: InvokeCommandInput, args: ClickPointArgs) -> InvokeCommandResult {
  let basis = args.basis(input.target.as_ref())?;
  let click = args.click_options()?;
  #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
  {
    match basis {
      RelativeToArg::Screen => {
        let screen_point = ScreenPoint::new(args.x, args.y);
        let action = if input.dry_run {
          None
        } else {
          input.cancellation.check().map_err(|error| error.to_string())?;
          let session = auv::local::open().map_err(|error| error.to_string())?;
          let action =
            session.input().click_at(screen_point.point(), click.button, click.click, click.modifiers).map_err(|error| error.to_string())?;
          emit_input_action_result(&action);
          Some(action)
        };
        click_point_output(ClickPointResult {
          relative_to: basis.as_str().to_string(),
          requested_point: auv_driver::Point::new(args.x, args.y),
          normalized: args.normalized,
          screen_point,
          window: None,
          display: None,
          action,
        })
      }
      RelativeToArg::Window => {
        let session = auv::local::open().map_err(|error| error.to_string())?;
        let target = input.target.as_ref().expect("window-relative target validated");
        let window = match target {
          crate::ExecutionTarget::Application { id } => {
            session.window().resolve(click_window_selector(id, args.title.as_deref())).map_err(|error| error.to_string())?
          }
          crate::ExecutionTarget::Window { id } => session
            .window()
            .list()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|window| window.reference.id == *id)
            .ok_or_else(|| format!("input.clickPoint could not find window target {id:?}"))?,
          crate::ExecutionTarget::Display { .. } => unreachable!("target/basis validated"),
        };
        let point = resolve_local_point("input.clickPoint", args.x, args.y, args.normalized, window.frame.size, "window")?;
        let window_point = auv_driver::WindowPoint::new(point.x, point.y);
        let screen_point = ScreenPoint::new(window.frame.origin.x + point.x, window.frame.origin.y + point.y);
        let action = if input.dry_run {
          None
        } else {
          input.cancellation.check().map_err(|error| error.to_string())?;
          let action = session.window().click(&window, window_point, click).map_err(|error| error.to_string())?;
          emit_input_action_result(&action);
          Some(action)
        };
        click_point_output(ClickPointResult {
          relative_to: basis.as_str().to_string(),
          requested_point: auv_driver::Point::new(args.x, args.y),
          normalized: args.normalized,
          screen_point,
          window: Some(window),
          display: None,
          action,
        })
      }
      RelativeToArg::Display => {
        let session = auv::local::open().map_err(|error| error.to_string())?;
        let crate::ExecutionTarget::Display { id } = input.target.as_ref().expect("display-relative target validated") else {
          unreachable!("target/basis validated")
        };
        let display = session
          .display()
          .list()
          .map_err(|error| error.to_string())?
          .displays
          .into_iter()
          .find(|display| display.id == *id)
          .ok_or_else(|| format!("input.clickPoint could not find display target {id:?}"))?;
        let point = resolve_local_point("input.clickPoint", args.x, args.y, args.normalized, display.frame.size, "display")?;
        let screen_point = ScreenPoint::new(display.frame.origin.x + point.x, display.frame.origin.y + point.y);
        let action = if input.dry_run {
          None
        } else {
          input.cancellation.check().map_err(|error| error.to_string())?;
          let action =
            session.input().click_at(screen_point.point(), click.button, click.click, click.modifiers).map_err(|error| error.to_string())?;
          emit_input_action_result(&action);
          Some(action)
        };
        click_point_output(ClickPointResult {
          relative_to: basis.as_str().to_string(),
          requested_point: auv_driver::Point::new(args.x, args.y),
          normalized: args.normalized,
          screen_point,
          window: None,
          display: Some(display),
          action,
        })
      }
    }
  }
  #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
  {
    let _ = (input, args, basis, click);
    Err("input.clickPoint is unavailable on this platform".to_string())
  }
}

pub(crate) fn resolve_local_point(
  command_id: &str,
  x: f64,
  y: f64,
  normalized: bool,
  size: auv_driver::Size,
  basis: &str,
) -> Result<auv_driver::Point, String> {
  let point = if normalized {
    auv_driver::Point::new(size.width * x, size.height * y)
  } else {
    auv_driver::Point::new(x, y)
  };
  if !(0.0..=size.width).contains(&point.x) || !(0.0..=size.height).contains(&point.y) {
    return Err(format!(
      "{command_id} point {},{} is outside target {basis} bounds 0..={},0..={}",
      point.x, point.y, size.width, size.height
    ));
  }
  Ok(point)
}

pub fn click_point_output(result: ClickPointResult) -> InvokeCommandResult {
  let mut fields = match result.action.as_ref() {
    Some(action) => input_action_report_fields(action),
    None => vec![
      InvokeReportField::new("Delivery", "not_performed"),
      InvokeReportField::new("Verification", "validation_only"),
    ],
  };
  fields.push(InvokeReportField::new("Relative to", result.relative_to.clone()));
  fields.push(InvokeReportField::new("Screen point", format!("{:.1},{:.1}", result.screen_point.point().x, result.screen_point.point().y)));
  if let Some(window) = &result.window {
    fields.push(InvokeReportField::new("Window ID", window.reference.id.clone()));
  }
  if let Some(display) = &result.display {
    fields.push(InvokeReportField::new("Display ID", display.id.clone()));
  }
  Ok(InvokeCommandOutput::from_result(&result)?.with_report(InvokeReport::new(fields, Vec::new())))
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke input.drag 400 300 700 300\n  auv invoke input.drag 0.2 0.5 0.8 0.5 --target app:com.apple.TextEdit --relative-to window --normalized\n  auv invoke input.drag 20 38 380 38 --target app:com.apple.TextEdit --input-policy foreground-preferred\n  auv invoke input.drag 100 80 300 80 --target display:1 --button right --duration-ms 600\nThe button goes down at the start point, the pointer moves in a straight line, and the button goes up at the end point under one input admission. Screen and display drags move the real pointer; activate the target app first, because an inactive window can treat the first press as activation only. Window-relative drags default to background-preferred: window-targeted delivery that does not move the pointer, which an inactive application can ignore. Use --input-policy foreground-preferred to foreground the window before a desktop drag. Delivery does not verify the application drop result."
)]
struct DragArgs {
  /// Start X coordinate in the selected coordinate basis.
  start_x: f64,
  /// Start Y coordinate in the selected coordinate basis.
  start_y: f64,
  /// End X coordinate in the selected coordinate basis.
  end_x: f64,
  /// End Y coordinate in the selected coordinate basis.
  end_y: f64,
  /// Mouse button held during the drag: left, right, or middle. Defaults to left.
  #[arg(long, default_value = "left", value_parser = ["left", "right", "middle"])]
  #[serde(default = "default_click_button")]
  button: String,
  /// Coordinate basis. Defaults from --target: screen, window, or display.
  #[arg(long, value_enum)]
  #[serde(rename = "relative-to", default)]
  relative_to: Option<RelativeToArg>,
  /// Interpret all coordinates as normalized values in 0..=1.
  #[arg(long)]
  #[serde(default)]
  normalized: bool,
  /// Window title text used with an app target.
  #[arg(long, value_name = "TEXT")]
  title: Option<String>,
  /// Window input delivery policy. Valid only with --relative-to window.
  #[arg(long, value_enum)]
  #[serde(rename = "input-policy")]
  input_policy: Option<InputPolicyArg>,
  /// Pointer travel time from start to end in milliseconds (0..=30000).
  #[arg(long, default_value_t = DEFAULT_DRAG_DURATION_MS)]
  #[serde(rename = "duration-ms", default = "default_drag_duration_ms")]
  duration_ms: u64,
}

// NOTICE: Many toolkits start a drag only after pointer motion passes a
// threshold, so the default keeps a visible travel time instead of a jump.
const DEFAULT_DRAG_DURATION_MS: u64 = 300;
const MAX_DRAG_DURATION_MS: u64 = 30_000;

fn default_drag_duration_ms() -> u64 {
  DEFAULT_DRAG_DURATION_MS
}

/// A validated drag request before its coordinates are resolved against a
/// window or display. Local and Runner execution share this plan.
#[derive(Clone, Debug)]
pub(crate) struct DragPlan {
  pub(crate) basis: RelativeToArg,
  pub(crate) start: auv_driver::Point,
  pub(crate) end: auv_driver::Point,
  pub(crate) normalized: bool,
  pub(crate) title: Option<String>,
  pub(crate) button: auv_driver::MouseButton,
  pub(crate) duration: std::time::Duration,
  /// Window delivery policy; `None` selects the driver default.
  pub(crate) policy: Option<auv_driver::InputPolicy>,
}

impl DragArgs {
  fn plan(&self, target: Option<&crate::ExecutionTarget>) -> Result<DragPlan, String> {
    let basis = point_basis(
      "input.drag",
      target,
      self.relative_to.map(RelativeToArg::as_str),
      self.normalized,
      self.input_policy.is_some(),
      self.title.is_some(),
    )?;
    let coordinates = [self.start_x, self.start_y, self.end_x, self.end_y];
    if coordinates.iter().any(|value| !value.is_finite()) {
      return Err("input.drag requires finite coordinates".to_string());
    }
    if self.normalized && coordinates.iter().any(|value| !(0.0..=1.0).contains(value)) {
      return Err("input.drag --normalized coordinates must be within 0..=1".to_string());
    }
    if self.duration_ms > MAX_DRAG_DURATION_MS {
      return Err(format!("input.drag --duration-ms must be within 0..={MAX_DRAG_DURATION_MS}"));
    }
    Ok(DragPlan {
      basis,
      start: auv_driver::Point::new(self.start_x, self.start_y),
      end: auv_driver::Point::new(self.end_x, self.end_y),
      normalized: self.normalized,
      title: self.title.clone(),
      button: parse_click_button(Some(&self.button))?,
      duration: std::time::Duration::from_millis(self.duration_ms),
      policy: self.input_policy.map(InputPolicyArg::driver_policy),
    })
  }
}

impl DragPlan {
  /// Projects both endpoints from a window or display frame into screen space.
  pub(crate) fn screen_points(&self, frame: auv_driver::Rect, basis: &str) -> Result<(ScreenPoint, ScreenPoint), crate::InvokeFailure> {
    let local = |point: auv_driver::Point| {
      resolve_local_point("input.drag", point.x, point.y, self.normalized, frame.size, basis)
        .map_err(|message| crate::InvokeFailure::new(crate::FailureCode::InvalidInput, message))
    };
    let (start, end) = (local(self.start)?, local(self.end)?);
    Ok((
      ScreenPoint::new(frame.origin.x + start.x, frame.origin.y + start.y),
      ScreenPoint::new(frame.origin.x + end.x, frame.origin.y + end.y),
    ))
  }

  /// Builds one straight drag path in screen coordinates. Window-targeted
  /// delivery also takes screen points; the driver stamps window-local positions.
  pub(crate) fn movement(&self, target: auv_driver::InputTarget, start: ScreenPoint, end: ScreenPoint) -> auv_driver::MoveMouseRequest {
    // A cubic segment with controls at 1/3 and 2/3 of the chord is the straight
    // line. Curve points are offsets from the curve start, mapped at scale 1.
    let (dx, dy) = (end.point().x - start.point().x, end.point().y - start.point().y);
    let mut request = auv_driver::MoveMouseRequest::direct(start.point());
    request.target = Some(target);
    request.curve.segments.push(auv_driver::MouseCubicBezierSegment {
      control_1: auv_driver::Point::new(dx / 3.0, dy / 3.0),
      control_2: auv_driver::Point::new(dx * 2.0 / 3.0, dy * 2.0 / 3.0),
      end: auv_driver::Point::new(dx, dy),
    });
    // NOTICE: 60 Hz sampling gives toolkits regular motion events during the
    // drag. A 0.5 point tolerance only affects curves; this path is straight.
    request.options = auv_driver::MouseMotionOptions {
      duration: self.duration,
      sample_rate_hz: 60,
      curve_tolerance: 0.5,
    };
    request
  }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct DragResult {
  pub relative_to: String,
  pub requested_start: auv_driver::Point,
  pub requested_end: auv_driver::Point,
  pub normalized: bool,
  pub screen_start: ScreenPoint,
  pub screen_end: ScreenPoint,
  pub button: auv_driver::MouseButton,
  pub duration_ms: u64,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub window: Option<auv_driver::Window>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub display: Option<auv_driver::Display>,
  /// Pointer position the driver reports after release.
  pub final_point: Option<ScreenPoint>,
  pub action: Option<auv_driver::InputActionResult>,
}

impl DragResult {
  pub(crate) fn planned(plan: &DragPlan, screen_start: ScreenPoint, screen_end: ScreenPoint) -> Self {
    Self {
      relative_to: plan.basis.as_str().to_string(),
      requested_start: plan.start,
      requested_end: plan.end,
      normalized: plan.normalized,
      screen_start,
      screen_end,
      button: plan.button,
      duration_ms: u64::try_from(plan.duration.as_millis()).unwrap_or(u64::MAX),
      window: None,
      display: None,
      final_point: None,
      action: None,
    }
  }
}

#[invoke_command(
  id = "input.drag",
  target = OptionalPoint,
  group = "input",
  description = "Press a mouse button, move in a straight line, and release it at the end point.",
  input = DragArgs,
)]
async fn drag(input: InvokeCommandInput, args: DragArgs) -> crate::InvokeExecutionResult {
  let plan = args.plan(input.target.as_ref()).map_err(|message| crate::InvokeFailure::new(crate::FailureCode::InvalidInput, message))?;
  execute_drag(&input, plan).await
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
async fn execute_drag(input: &InvokeCommandInput, plan: DragPlan) -> crate::InvokeExecutionResult {
  // Screen-relative validation needs no target lookup, so its dry run stays hermetic.
  let session = match (plan.basis, input.dry_run) {
    (RelativeToArg::Screen, true) => None,
    _ => Some(auv::local::open()?),
  };
  let mut result = match plan.basis {
    RelativeToArg::Screen => {
      DragResult::planned(&plan, ScreenPoint::new(plan.start.x, plan.start.y), ScreenPoint::new(plan.end.x, plan.end.y))
    }
    RelativeToArg::Window => {
      let session = session.as_ref().expect("window resolution opens a session");
      let window = match input.target.as_ref().expect("window-relative target validated") {
        crate::ExecutionTarget::Application { id } => session.window().resolve(click_window_selector(id, plan.title.as_deref()))?,
        crate::ExecutionTarget::Window { id } => {
          session.window().list()?.into_iter().find(|window| window.reference.id == *id).ok_or_else(|| {
            crate::InvokeFailure::new(crate::FailureCode::NotFound, format!("input.drag could not find window target {id:?}"))
          })?
        }
        crate::ExecutionTarget::Display { .. } => unreachable!("target/basis validated"),
      };
      let (start, end) = plan.screen_points(window.frame, "window")?;
      let mut result = DragResult::planned(&plan, start, end);
      result.window = Some(window);
      result
    }
    RelativeToArg::Display => {
      let crate::ExecutionTarget::Display { id } = input.target.as_ref().expect("display-relative target validated") else {
        unreachable!("target/basis validated")
      };
      let session = session.as_ref().expect("display resolution opens a session");
      let display = session.display().list()?.displays.into_iter().find(|display| display.id == *id).ok_or_else(|| {
        crate::InvokeFailure::new(crate::FailureCode::NotFound, format!("input.drag could not find display target {id:?}"))
      })?;
      let (start, end) = plan.screen_points(display.frame, "display")?;
      let mut result = DragResult::planned(&plan, start, end);
      result.display = Some(display);
      result
    }
  };
  if input.dry_run {
    return drag_output(result).map_err(Into::into);
  }

  input.cancellation.check().map_err(|error| error.to_string())?;
  let session = session.expect("delivery opens a session");
  let movement = plan.movement(auv_driver::InputTarget::Foreground, result.screen_start, result.screen_end);
  let button = plan.button;
  // Window points use the driver's window drag policy, like a window click.
  // Screen and display points use the desktop drag directly.
  let (point, action) = match result.window.clone() {
    Some(window) => {
      let policy = plan.policy.unwrap_or_default();
      run_cancellable_input(&input.cancellation, move || session.window().drag(&window, movement, button, policy)).await?
    }
    None => run_cancellable_input(&input.cancellation, move || session.input().drag_mouse(movement, button)).await?,
  };
  emit_input_action_result(&action);
  result.final_point = Some(ScreenPoint::new(point.x, point.y));
  result.action = Some(action);
  drag_output(result).map_err(Into::into)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
async fn execute_drag(_input: &InvokeCommandInput, _plan: DragPlan) -> crate::InvokeExecutionResult {
  Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "input.drag is unavailable on this platform"))
}

pub fn drag_output(result: DragResult) -> InvokeCommandResult {
  let mut fields = match result.action.as_ref() {
    Some(action) => input_action_report_fields(action),
    None => vec![
      InvokeReportField::new("Delivery", "not_performed"),
      InvokeReportField::new("Verification", "validation_only"),
    ],
  };
  fields.push(InvokeReportField::new("Relative to", result.relative_to.clone()));
  fields.push(InvokeReportField::new("Screen start", format!("{:.1},{:.1}", result.screen_start.point().x, result.screen_start.point().y)));
  fields.push(InvokeReportField::new("Screen end", format!("{:.1},{:.1}", result.screen_end.point().x, result.screen_end.point().y)));
  fields.push(InvokeReportField::new("Duration", format!("{} ms", result.duration_ms)));
  if let Some(window) = &result.window {
    fields.push(InvokeReportField::new("Window ID", window.reference.id.clone()));
  }
  if let Some(display) = &result.display {
    fields.push(InvokeReportField::new("Display ID", display.id.clone()));
  }
  Ok(InvokeCommandOutput::from_result(&result)?.with_report(InvokeReport::new(fields, Vec::new())))
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke input.scroll 200 300 --dy 300 --target app:com.google.Chrome\n  auv invoke input.scroll 0.5 0.5 --normalized --dy -600 --target window:12345 --input-policy background-only\nDeltas are logical pixels. Positive --dy scrolls toward later content (down) and positive --dx scrolls right, matching DOM WheelEvent; the natural-scrolling preference does not apply. The default background-preferred policy tries window-targeted delivery before a foreground wheel. Delivery does not prove the viewport moved: a covered Chromium window can accept a wheel event without scrolling, so verify with a capture."
)]
struct ScrollArgs {
  /// X coordinate inside the target window.
  x: f64,
  /// Y coordinate inside the target window.
  y: f64,
  /// Horizontal delta in logical pixels; positive scrolls right.
  #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
  #[serde(default)]
  dx: f64,
  /// Vertical delta in logical pixels; positive scrolls down.
  #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
  #[serde(default)]
  dy: f64,
  /// Interpret X and Y as normalized values in 0..=1.
  #[arg(long)]
  #[serde(default)]
  normalized: bool,
  /// Window title text used with an app target.
  #[arg(long, value_name = "TEXT")]
  title: Option<String>,
  /// Window input delivery policy. Defaults to background-preferred.
  #[arg(long, value_enum)]
  #[serde(rename = "input-policy")]
  input_policy: Option<InputPolicyArg>,
  /// Delay after delivery in milliseconds (0..=30000).
  #[arg(long, default_value_t = 0)]
  #[serde(rename = "settle-ms", default)]
  settle_ms: u64,
}

const MAX_SCROLL_SETTLE_MS: u64 = 30_000;

/// A validated window scroll before its point is resolved against the target
/// window. Local and Runner execution share this plan.
// TODO(screen-point-scroll): screen/display-relative scroll is deferred; the
// Runner exposes only ScrollWindowPoint until a caller needs global scroll.
// TODO(scroll-delivery-candidates-cli): the CLI keeps the Driver default
// candidate ladder; expose an ordered candidate flag when a caller needs it.
#[derive(Clone, Debug)]
pub(crate) struct ScrollPlan {
  pub(crate) point: auv_driver::Point,
  pub(crate) normalized: bool,
  pub(crate) title: Option<String>,
  pub(crate) scroll: auv_driver::Scroll,
  pub(crate) options: auv_driver::ScrollOptions,
}

impl ScrollArgs {
  fn plan(&self, target: Option<&crate::ExecutionTarget>) -> Result<ScrollPlan, String> {
    if !matches!(target, Some(crate::ExecutionTarget::Application { .. } | crate::ExecutionTarget::Window { .. })) {
      return Err("input.scroll requires --target app: or window:".to_string());
    }
    if self.title.is_some() && !matches!(target, Some(crate::ExecutionTarget::Application { .. })) {
      return Err("input.scroll --title requires --target app:".to_string());
    }
    if !self.x.is_finite() || !self.y.is_finite() {
      return Err("input.scroll requires finite coordinates".to_string());
    }
    if self.normalized && (!(0.0..=1.0).contains(&self.x) || !(0.0..=1.0).contains(&self.y)) {
      return Err("input.scroll --normalized coordinates must be within 0..=1".to_string());
    }
    if !self.dx.is_finite() || !self.dy.is_finite() {
      return Err("input.scroll requires finite --dx and --dy".to_string());
    }
    if self.dx == 0.0 && self.dy == 0.0 {
      return Err("input.scroll requires a non-zero --dx or --dy".to_string());
    }
    if self.settle_ms > MAX_SCROLL_SETTLE_MS {
      return Err(format!("input.scroll --settle-ms must be within 0..={MAX_SCROLL_SETTLE_MS}"));
    }
    Ok(ScrollPlan {
      point: auv_driver::Point::new(self.x, self.y),
      normalized: self.normalized,
      title: self.title.clone(),
      scroll: auv_driver::Scroll::new(self.dx, self.dy),
      options: auv_driver::ScrollOptions {
        policy: self.input_policy.map(InputPolicyArg::driver_policy).unwrap_or_default(),
        settle: std::time::Duration::from_millis(self.settle_ms),
        ..auv_driver::ScrollOptions::default()
      },
    })
  }
}

impl ScrollPlan {
  /// Resolves the requested point against the target window's current size.
  pub(crate) fn window_point(&self, window: &auv_driver::Window) -> Result<auv_driver::WindowPoint, crate::InvokeFailure> {
    let point = resolve_local_point("input.scroll", self.point.x, self.point.y, self.normalized, window.frame.size, "window")
      .map_err(|message| crate::InvokeFailure::new(crate::FailureCode::InvalidInput, message))?;
    Ok(auv_driver::WindowPoint::new(point.x, point.y))
  }

  pub(crate) fn result(&self, window: auv_driver::Window, point: auv_driver::WindowPoint) -> ScrollResult {
    let local = point.point();
    ScrollResult {
      requested_point: self.point,
      normalized: self.normalized,
      screen_point: ScreenPoint::new(window.frame.origin.x + local.x, window.frame.origin.y + local.y),
      window_point: local,
      scroll: self.scroll,
      policy: self.options.policy,
      window,
      action: None,
    }
  }
}

/// Runner dispatch decodes transport arguments once. Local handlers already
/// receive typed arguments; both validate through `ScrollArgs::plan`.
pub(crate) fn decode_scroll(input: &InvokeCommandInput) -> Result<ScrollPlan, crate::InvokeFailure> {
  crate::command::decode_args::<ScrollArgs>(input)
    .and_then(|args| args.plan(input.target.as_ref()))
    .map_err(|message| crate::InvokeFailure::new(crate::FailureCode::InvalidInput, message))
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ScrollResult {
  pub requested_point: auv_driver::Point,
  pub normalized: bool,
  pub window_point: auv_driver::Point,
  pub screen_point: ScreenPoint,
  /// Logical pixels, positive toward later content (down/right).
  pub scroll: auv_driver::Scroll,
  pub policy: auv_driver::InputPolicy,
  pub window: auv_driver::Window,
  pub action: Option<auv_driver::InputActionResult>,
}

#[invoke_command(
  id = "input.scroll",
  target = RequiredWindow,
  group = "input",
  description = "Scroll at a point in a target window by logical-pixel deltas (positive dy scrolls down).",
  input = ScrollArgs,
)]
async fn scroll(input: InvokeCommandInput, args: ScrollArgs) -> crate::InvokeExecutionResult {
  let plan = args.plan(input.target.as_ref()).map_err(|message| crate::InvokeFailure::new(crate::FailureCode::InvalidInput, message))?;
  execute_scroll(&input, plan).await
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
async fn execute_scroll(input: &InvokeCommandInput, plan: ScrollPlan) -> crate::InvokeExecutionResult {
  let session = auv::local::open()?;
  let window = match input.target.as_ref().expect("window target validated") {
    crate::ExecutionTarget::Application { id } => session.window().resolve(click_window_selector(id, plan.title.as_deref()))?,
    crate::ExecutionTarget::Window { id } => {
      session.window().list()?.into_iter().find(|window| window.reference.id == *id).ok_or_else(|| {
        crate::InvokeFailure::new(crate::FailureCode::NotFound, format!("input.scroll could not find window target {id:?}"))
      })?
    }
    crate::ExecutionTarget::Display { .. } => unreachable!("target validated"),
  };
  let point = plan.window_point(&window)?;
  let mut result = plan.result(window.clone(), point);
  if input.dry_run {
    return scroll_output(result).map_err(Into::into);
  }

  input.cancellation.check().map_err(|error| error.to_string())?;
  let (scroll, options) = (plan.scroll, plan.options.clone());
  let action = run_cancellable_input(&input.cancellation, move || session.window().scroll(&window, point, scroll, options)).await?;
  emit_input_action_result(&action);
  result.action = Some(action);
  scroll_output(result).map_err(Into::into)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
async fn execute_scroll(_input: &InvokeCommandInput, _plan: ScrollPlan) -> crate::InvokeExecutionResult {
  Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "input.scroll is unavailable on this platform"))
}

pub fn scroll_output(result: ScrollResult) -> InvokeCommandResult {
  let mut fields = match result.action.as_ref() {
    Some(action) => input_action_report_fields(action),
    None => vec![
      InvokeReportField::new("Delivery", "not_performed"),
      InvokeReportField::new("Verification", "validation_only"),
    ],
  };
  fields.push(InvokeReportField::new("Scroll", format!("dx={} dy={} (logical px, +dy down)", result.scroll.delta_x, result.scroll.delta_y)));
  fields.push(InvokeReportField::new("Window point", format!("{:.1},{:.1}", result.window_point.x, result.window_point.y)));
  fields.push(InvokeReportField::new("Window ID", result.window.reference.id.clone()));
  Ok(InvokeCommandOutput::from_result(&result)?.with_report(InvokeReport::new(fields, Vec::new())))
}

/// Runs one blocking driver input on the blocking pool. Invoke cancellation,
/// or dropping the invoke future, wakes waits inside the driver input.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
async fn run_cancellable_input<T: Send + 'static>(
  cancellation: &crate::InvokeCancellation,
  operation: impl FnOnce() -> auv_driver::DriverResult<T> + Send + 'static,
) -> Result<T, crate::InvokeFailure> {
  struct CancelOnDrop(std::sync::Arc<auv_driver::input_cancellation::InputCancellation>);
  impl Drop for CancelOnDrop {
    fn drop(&mut self) {
      self.0.cancel();
    }
  }

  let signal = std::sync::Arc::new(auv_driver::input_cancellation::InputCancellation::default());
  let _guard = CancelOnDrop(signal.clone());
  tokio::select! {
    _ = cancellation.cancelled() => Err("invoke cancelled".to_string().into()),
    result = tokio::task::spawn_blocking(move || auv_driver::input_cancellation::with_input_cancellation(signal, operation)) => {
      Ok(result.map_err(|error| format!("input task failed: {error}"))??)
    }
  }
}

#[derive(Clone, Copy, Debug, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum InputPolicyArg {
  BackgroundOnly,
  BackgroundPreferred,
  ForegroundPreferred,
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

/// Parse click state for both local invoke and Runner dispatch before delivery.
pub(crate) fn parse_click_modifiers(value: Option<&str>) -> Result<auv_driver::ClickModifiers, String> {
  let mut modifiers = auv_driver::ClickModifiers::default();
  let Some(value) = value else {
    return Ok(modifiers);
  };
  for name in value.split(',') {
    let normalized = name.trim().to_ascii_lowercase();
    // NOTICE: Keyboard aliases win/super are not part of the existing click CLI contract.
    let modifier = normalized
      .parse::<auv_driver::Modifier>()
      .ok()
      .filter(|_| !matches!(normalized.as_str(), "win" | "super"))
      .ok_or_else(|| format!("unknown click modifier {name:?}; expected shift, control, alt/option or meta/cmd"))?;
    let slot = match modifier {
      auv_driver::Modifier::Shift => &mut modifiers.shift,
      auv_driver::Modifier::Control => &mut modifiers.control,
      auv_driver::Modifier::Alt => &mut modifiers.alt,
      auv_driver::Modifier::Meta => &mut modifiers.meta,
    };
    if *slot {
      return Err(format!("duplicate click modifier {name:?}"));
    }
    *slot = true;
  }
  Ok(modifiers)
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

fn click_window_selector(application_id: &str, title: Option<&str>) -> auv_driver::WindowSelector {
  use auv_driver::{App, TextMatcher, WindowSelector};

  let title = title.filter(|value| !value.trim().is_empty()).map(|title| TextMatcher::Contains(title.to_string()));
  WindowSelector {
    app: Some(App::bundle_id(application_id)),
    main_visible: title.is_none(),
    title,
  }
}

impl From<PressKeysArgs> for auv_driver::KeyboardInput {
  fn from(args: PressKeysArgs) -> Self {
    Self::PressKeys {
      policy: keyboard_policy(args.input_policy),
      options: auv_driver::PressKeysOptions {
        keys: args.keys,
        count: args.count.unwrap_or(1),
        interval: std::time::Duration::from_millis(args.interval_ms.unwrap_or(0)),
        ..Default::default()
      },
    }
  }
}

impl From<PressKeyArgs> for auv_driver::KeyboardInput {
  fn from(args: PressKeyArgs) -> Self {
    // Preserve the released shortcut spelling through the shared conversion.
    let options: auv_driver::PressKeysOptions = auv_driver::KeyPressOptions {
      key: args.key,
      ..Default::default()
    }
    .into();
    PressKeysArgs {
      keys: options.keys,
      count: args.count,
      interval_ms: args.interval_ms,
      input_policy: args.input_policy,
    }
    .into()
  }
}

impl From<TypeTextArgs> for auv_driver::KeyboardInput {
  fn from(args: TypeTextArgs) -> Self {
    Self::TypeText {
      text: args.text,
      options: auv_driver::TypeTextOptions {
        policy: keyboard_policy(args.input_policy),
        ..Default::default()
      },
    }
  }
}

impl From<PasteTextArgs> for auv_driver::KeyboardInput {
  fn from(args: PasteTextArgs) -> Self {
    Self::PasteText {
      policy: keyboard_policy(args.input_policy),
      options: auv_driver::PasteTextOptions {
        text: args.text,
        ..Default::default()
      },
    }
  }
}

impl InputKeyboardArgs {
  fn into_keyboard_inputs(self) -> Result<Vec<auv_driver::KeyboardInput>, String> {
    let actions: Vec<KeyboardActionArg> =
      serde_json::from_str(&self.actions).map_err(|error| format!("invalid keyboard actions: {error}"))?;
    Ok(
      actions
        .into_iter()
        .map(|action| match action {
          KeyboardActionArg::Press {
            keys,
            count,
            interval_ms,
          } => PressKeysArgs {
            keys,
            count,
            interval_ms,
            input_policy: self.input_policy,
          }
          .into(),
          KeyboardActionArg::TypeText { text } => TypeTextArgs {
            text,
            input_policy: self.input_policy,
          }
          .into(),
          KeyboardActionArg::PasteText { text } => PasteTextArgs {
            text,
            input_policy: self.input_policy,
          }
          .into(),
        })
        .collect(),
    )
  }
}

fn keyboard_policy(policy: Option<InputPolicyArg>) -> auv_driver::InputPolicy {
  policy.map(InputPolicyArg::driver_policy).unwrap_or(auv_driver::InputPolicy::ForegroundPreferred)
}

/// Runner dispatch decodes transport arguments once. Local handlers already
/// receive typed arguments; both use the same argument-to-driver conversions.
pub(crate) fn decode_keyboard_input(input: &InvokeCommandInput) -> Result<Vec<auv_driver::KeyboardInput>, String> {
  use crate::command::decode_args;
  match input.command_id.as_str() {
    "input.key" => decode_args::<PressKeyArgs>(input).map(|args| vec![args.into()]),
    "input.keys" | "input.pressKeys" => decode_args::<PressKeysArgs>(input).map(|args| vec![args.into()]),
    "input.typeText" => decode_args::<TypeTextArgs>(input).map(|args| vec![args.into()]),
    "input.pasteText" => decode_args::<PasteTextArgs>(input).map(|args| vec![args.into()]),
    "input.keyboard" => decode_args::<InputKeyboardArgs>(input)?.into_keyboard_inputs(),
    _ => Err(format!("{} is not a keyboard input command", input.command_id)),
  }
}

/// Runner dispatch decodes transport arguments once. Local handlers already
/// receive typed arguments; both validate through `DragArgs::plan`.
pub(crate) fn decode_drag(input: &InvokeCommandInput) -> Result<DragPlan, crate::InvokeFailure> {
  crate::command::decode_args::<DragArgs>(input)
    .and_then(|args| args.plan(input.target.as_ref()))
    .map_err(|message| crate::InvokeFailure::new(crate::FailureCode::InvalidInput, message))
}

pub(crate) fn decode_hold_keys(
  input: &InvokeCommandInput,
) -> Result<(Vec<String>, auv_driver::InputPolicy, std::time::Duration), crate::InvokeFailure> {
  crate::command::decode_args::<HoldKeysArgs>(input)
    .map_err(|message| crate::InvokeFailure::new(crate::FailureCode::InvalidInput, message))?
    .validated(input.target.is_some())
}

/// Reject an impossible foreground policy before opening either driver route.
pub(crate) fn validate_keyboard_policy(
  input: &InvokeCommandInput,
  actions: &[auv_driver::KeyboardInput],
) -> Result<(), crate::InvokeFailure> {
  if input.target.is_none() && actions.iter().any(|action| action.policy() != auv_driver::InputPolicy::ForegroundPreferred) {
    return Err(crate::InvokeFailure::new(crate::FailureCode::InvalidInput, "background keyboard input requires --target"));
  }
  Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn execute_keyboard(input: &InvokeCommandInput, keyboard: Vec<auv_driver::KeyboardInput>) -> crate::InvokeExecutionResult {
  validate_keyboard_policy(input, &keyboard)?;
  let session = auv::local::open()?;
  let target = local_keyboard_target(input, &session)?;

  input.cancellation.check().map_err(|error| error.to_string())?;
  let result = session.input().input_keyboard(&target, keyboard, input.dry_run).map_err(Into::into);
  keyboard_output(input, result)
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn local_keyboard_target(
  input: &InvokeCommandInput,
  session: &auv_driver::LocalDriverSession,
) -> Result<auv_driver::InputTarget, crate::InvokeFailure> {
  let target = match input.target.as_ref() {
    None => auv_driver::InputTarget::Foreground,
    Some(crate::ExecutionTarget::Application { id }) => auv_driver::InputTarget::Application {
      bundle_id: id.clone(),
    },
    Some(crate::ExecutionTarget::Window { id }) => {
      auv_driver::InputTarget::Window(session.window().list()?.into_iter().find(|window| window.reference.id == *id).ok_or_else(|| {
        auv_driver::DriverError::NotFound {
          target: format!("window:{id}"),
        }
      })?)
    }
    Some(crate::ExecutionTarget::Display { .. }) => {
      return Err(crate::InvokeFailure::new(crate::FailureCode::InvalidTarget, "display target is unsupported for keyboard input"));
    }
  };
  Ok(target)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn execute_keyboard(_input: &InvokeCommandInput, _keyboard: Vec<auv_driver::KeyboardInput>) -> crate::InvokeExecutionResult {
  Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "keyboard input is unavailable on this platform"))
}

/// Both frontends preserve completed action artifacts even when delivery stops.
pub(crate) fn keyboard_output(
  input: &InvokeCommandInput,
  result: Result<Option<Vec<auv_driver::InputActionResult>>, crate::InvokeFailure>,
) -> crate::InvokeExecutionResult {
  let actions = match result {
    Ok(actions) => actions,
    Err(error) => {
      if let Some(progress) = &error.keyboard_progress {
        for action in &progress.completed {
          emit_input_action_result(action);
        }
      }
      return Err(error);
    }
  };
  if input.command_id != "input.keyboard" {
    return targeted_keyboard_output(actions.as_ref().and_then(|actions| actions.first())).map_err(Into::into);
  }
  let Some(actions) = actions else {
    return Ok(validation_only_output());
  };
  for action in &actions {
    emit_input_action_result(action);
  }
  Ok(InvokeCommandOutput::from_result(&serde_json::json!({"actions": actions}))?.with_report(InvokeReport::new(
    vec![
      InvokeReportField::new("Delivery", "events_submitted"),
      InvokeReportField::new("Verification", "unverified"),
    ],
    Vec::new(),
  )))
}

/// Keep the driver action as the direct result and tracing artifact.
pub(crate) fn targeted_keyboard_output(action: Option<&auv_driver::InputActionResult>) -> InvokeCommandResult {
  let mut output = match action {
    Some(action) => {
      emit_input_action_result(action);
      input_action_output(action)?
    }
    None => validation_only_output(),
  };
  output
    .report
    .as_mut()
    .expect("input report")
    .fields
    .push(InvokeReportField::new("Control focus", "application-owned; no control selection or semantic verification"));
  Ok(output)
}

/// Builds the transport-independent delivery result used by local and
/// daemon-backed input frontends.
pub fn input_action_output(result: &auv_driver::InputActionResult) -> InvokeCommandResult {
  Ok(InvokeCommandOutput::from_result(result)?.with_report(InvokeReport::new(input_action_report_fields(result), Vec::new())))
}

/// Builds the shared `input.key` result while keeping transport selection out
/// of the command's public output contract.
pub fn press_key_output(result: &auv_driver::InputActionResult, key: &str) -> InvokeCommandResult {
  let mut fields = input_action_report_fields(result);
  fields.insert(1, InvokeReportField::new("Key", key));
  fields.insert(2, InvokeReportField::new("Target", "active app"));
  fields.push(InvokeReportField::new("Backend", "auv-driver-macos.input"));
  Ok(InvokeCommandOutput::from_result(result)?.with_report(InvokeReport::new(fields, Vec::new())))
}

pub fn focus_text_output(result: &auv_driver::AxFocusResult, candidate: &str) -> InvokeCommandResult {
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
