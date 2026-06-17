use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
  arg::{
    KEY_ARGS, QUERY_ARGS, QUERY_OR_CANDIDATE_ARGS, QUERY_OR_CANDIDATE_OVERLAY_ARGS,
    QUERY_OVERLAY_ARGS, TARGET_ARGS, TEXT_ARGS, WINDOW_ARGS, WINDOW_QUERY_OVERLAY_ARGS,
  },
  invoke_command,
};

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
  args = WINDOW_ARGS,
)]
fn click_window_point(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-input-click-window-point): WindowApi::click exists, but this
  // invoke command exposes only window selection args and no relative point or
  // candidate parser; add that typed input contract before enabling it.
  Err("input.clickWindowPoint requires direct window-relative point inputs".to_string())
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
  let result = session
    .input()
    .type_text(text, TypeTextOptions::default())
    .map_err(|error| error.to_string())?;
  Ok(input_action_output(
    "typed text into active control",
    "auv-driver-macos.input",
    &result,
  ))
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
  output
    .signals
    .insert("clipboard_disturbance".to_string(), "temporary".to_string());
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
  Ok(input_action_output(
    "pressed key in active app",
    "auv-driver-macos.input",
    &result,
  ))
}

#[cfg(not(target_os = "macos"))]
fn press_key_impl(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  Err("input.key is only available on macOS".to_string())
}

fn required_input<'a>(input: &'a InvokeCommandInput<'_>, name: &str) -> Result<&'a str, String> {
  input
    .inputs
    .get(name)
    .map(String::as_str)
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| format!("{} requires --{name}", input.command_id))
}

fn reject_target_activation(
  input: &InvokeCommandInput<'_>,
  command_id: &str,
) -> Result<(), String> {
  if input.target_application_id.is_some() {
    // TODO(invoke-input-target-activation): foreground input APIs currently
    // act on the active control; add a typed app/window input lease before
    // honoring --target here.
    return Err(format!(
      "{command_id} cannot use --target until typed input target activation is available"
    ));
  }
  Ok(())
}

fn dry_run_output(command_id: &str) -> InvokeCommandOutput {
  InvokeCommandOutput::new(format!("dry run: {command_id}"))
}

#[cfg(target_os = "macos")]
fn input_action_output(
  summary: &str,
  backend: &str,
  result: &auv_driver::InputActionResult,
) -> InvokeCommandOutput {
  let mut output = InvokeCommandOutput::new(summary);
  output.backend = Some(backend.to_string());
  output.signals.insert(
    "input.selected_path".to_string(),
    format!("{:?}", result.selected_path),
  );
  output.signals.insert(
    "input.attempt_count".to_string(),
    result.attempts.len().to_string(),
  );
  output.signals.insert(
    "input.mouse_disturbance".to_string(),
    format!("{:?}", result.mouse_disturbance),
  );
  output.signals.insert(
    "input.focus_disturbance".to_string(),
    format!("{:?}", result.focus_disturbance),
  );
  output.signals.insert(
    "input.clipboard_disturbance".to_string(),
    format!("{:?}", result.clipboard_disturbance),
  );
  if let Some(reason) = &result.fallback_reason {
    output
      .signals
      .insert("input.fallback_reason".to_string(), reason.clone());
  }
  output
}
