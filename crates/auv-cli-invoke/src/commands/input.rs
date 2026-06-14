use auv_driver::OperationDisturbance;

use crate::{
  CommandGroup, InvokeCommand, InvokeContext, InvokeDriverDispatch,
  arg::{
    KEY_ARGS, QUERY_ARGS, QUERY_OR_CANDIDATE_ARGS, QUERY_OR_CANDIDATE_OVERLAY_ARGS,
    QUERY_OVERLAY_ARGS, TARGET_ARGS, TEXT_ARGS, WINDOW_ARGS, WINDOW_QUERY_OVERLAY_ARGS,
  },
  command::{
    CAPTURE_AX_TREE_DISTURBANCE, FOCUS_POINTER_ENTRY, FOREGROUND_KEYBOARD,
    FOREGROUND_KEYBOARD_CLIPBOARD, POINTER_WITH_FOREGROUND, PRESS_BUTTON_DISTURBANCE,
  },
  default_driver_dispatch, invoke_command,
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
  summary = "Focus a target macOS text input through AX, either by --query text or by a promoted --candidate JSON payload carrying the typed search-entry contract candidate.",
  driver = "macos.desktop",
  operation = "focus_text_input",
  args = QUERY_OR_CANDIDATE_ARGS,
  disturbance = FOCUS_POINTER_ENTRY,
  max_disturbance = OperationDisturbance::Pointer,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn focus_text_input(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "input.pressButton",
  group = "input",
  summary = "Press a known macOS button-like control by query through AX.",
  driver = "macos.desktop",
  operation = "press_button",
  args = QUERY_ARGS,
  disturbance = PRESS_BUTTON_DISTURBANCE,
  max_disturbance = OperationDisturbance::Pointer,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn press_button(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "input.axPressButton",
  group = "input",
  summary = "Press a control by query via AXUIElementPerformAction; does not warp the real cursor (cursorDisturbance=none). Pass --overlay true to draw a visual AUV cursor over the target during the press for the dual-cursor effect. Falls back with an error when the AX target has no matching action; use input.pressButton for non-AX-pressable targets.",
  driver = "macos.desktop",
  operation = "ax_press_button",
  args = QUERY_OVERLAY_ARGS,
  disturbance = CAPTURE_AX_TREE_DISTURBANCE,
  max_disturbance = OperationDisturbance::Keyboard,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn ax_press_button(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "input.axFocusText",
  group = "input",
  summary = "Focus a text input by query or promoted --candidate JSON via AXUIElementSetAttributeValue(kAXFocusedAttribute); does not warp the real cursor (cursorDisturbance=none, focusMechanism=ax-attribute). Pass --overlay true for the dual-cursor visual (auv replay cursor animates to the target while the real cursor stays put). Errors when the target does not accept programmatic focus; use input.focusText if pointer warp is acceptable.",
  driver = "macos.desktop",
  operation = "ax_focus_text_input",
  args = QUERY_OR_CANDIDATE_OVERLAY_ARGS,
  disturbance = CAPTURE_AX_TREE_DISTURBANCE,
  max_disturbance = OperationDisturbance::Keyboard,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn ax_focus_text_input(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "input.axClickWindowText",
  group = "input",
  summary = "Find visible text in a window via Vision OCR, resolve the AX node at that point, then press it via AXUIElementPerformAction (cursorDisturbance=none). Pass --overlay true for the dual-cursor visual. Errors with a hint to window.clickText when the OCR anchor maps to a canvas-rendered or non-AX-pressable region.",
  driver = "macos.desktop",
  operation = "ax_click_window_text",
  args = WINDOW_QUERY_OVERLAY_ARGS,
  disturbance = CAPTURE_AX_TREE_DISTURBANCE,
  max_disturbance = OperationDisturbance::Keyboard,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn ax_click_window_text(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "input.smartPress",
  group = "input",
  summary = "ActionResolver v0 diagnostic press: try OCR-to-AX press first; if it fails and --allow_pointer_fallback is not false, fall back to pointer click. Records actionResolver.* signals plus the selected method, fallback reason, and disturbance metadata.",
  driver = "macos.desktop",
  operation = "smart_press",
  args = WINDOW_QUERY_OVERLAY_ARGS,
  disturbance = POINTER_WITH_FOREGROUND,
  max_disturbance = OperationDisturbance::Pointer,
  artifacts = ["input-action-result"],
  signals = ["actionResolver.method", "actionResolver.fallbackReason"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn smart_press(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "input.typeText",
  group = "input",
  summary = "Type text into the active macOS control through System Events.",
  driver = "macos.desktop",
  operation = "type_text",
  args = TEXT_ARGS,
  disturbance = FOREGROUND_KEYBOARD,
  max_disturbance = OperationDisturbance::Keyboard,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn type_text(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "input.pasteText",
  group = "input",
  summary = "Paste text into the active macOS control through the clipboard, then restore the prior clipboard snapshot.",
  driver = "macos.desktop",
  operation = "paste_text_preserve_clipboard",
  args = TEXT_ARGS,
  disturbance = FOREGROUND_KEYBOARD_CLIPBOARD,
  max_disturbance = OperationDisturbance::Clipboard,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn paste_text_preserve_clipboard(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "input.key",
  group = "input",
  summary = "Press a keyboard key or shortcut in the active macOS app through System Events.",
  driver = "macos.desktop",
  operation = "press_key",
  args = KEY_ARGS,
  disturbance = FOREGROUND_KEYBOARD,
  max_disturbance = OperationDisturbance::Keyboard,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn press_key(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "input.clickPoint",
  group = "input",
  summary = "Click a macOS global logical point through Quartz and record its display contract.",
  driver = "macos.desktop",
  operation = "click_point",
  args = TARGET_ARGS,
  disturbance = POINTER_WITH_FOREGROUND,
  max_disturbance = OperationDisturbance::Pointer,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn click_point(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "input.clickWindowPoint",
  group = "input",
  summary = "Click a point relative to a target macOS window and record the resolved global point, either from --relative_x/--relative_y inputs or from a promoted --candidate JSON payload carrying the typed window-action contract candidate.",
  driver = "macos.desktop",
  operation = "click_window_point",
  args = WINDOW_ARGS,
  disturbance = POINTER_WITH_FOREGROUND,
  max_disturbance = OperationDisturbance::Pointer,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn click_window_point(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "input.teachClick",
  group = "input",
  summary = "Capture a target window before and after a human-taught click, recording global and window-local click coordinates for automation debugging.",
  driver = "macos.desktop",
  operation = "teach_click",
  args = WINDOW_ARGS,
  disturbance = POINTER_WITH_FOREGROUND,
  max_disturbance = OperationDisturbance::Pointer,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn teach_click(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "input.scrollPoint",
  group = "input",
  summary = "Scroll at a macOS global logical point through Quartz and record its display contract.",
  driver = "macos.desktop",
  operation = "scroll_point",
  args = TARGET_ARGS,
  disturbance = POINTER_WITH_FOREGROUND,
  max_disturbance = OperationDisturbance::Pointer,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn scroll_point(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}
