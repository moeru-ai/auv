use crate::{
  CommandGroup,
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
fn focus_text_input() {}

#[invoke_command(
  id = "input.pressButton",
  group = "input",
  summary = "Press a known macOS button-like control by query through AX.",
  args = QUERY_ARGS,
)]
fn press_button() {}

#[invoke_command(
  id = "input.axPressButton",
  group = "input",
  summary = "Press a control by query via AXUIElementPerformAction without moving the real cursor. Pass --overlay true to draw a visual AUV cursor over the target. Falls back with an error when the AX target has no matching action; use input.pressButton for non-AX-pressable targets.",
  args = QUERY_OVERLAY_ARGS,
)]
fn ax_press_button() {}

#[invoke_command(
  id = "input.axFocusText",
  group = "input",
  summary = "Focus a text input by query or promoted --candidate JSON via AXUIElementSetAttributeValue(kAXFocusedAttribute) without moving the real cursor. Pass --overlay true for the dual-cursor visual. Errors when the target does not accept programmatic focus; use input.focusText if pointer movement is acceptable.",
  args = QUERY_OR_CANDIDATE_OVERLAY_ARGS,
)]
fn ax_focus_text_input() {}

#[invoke_command(
  id = "input.axClickWindowText",
  group = "input",
  summary = "Find visible text in a window via Vision OCR, resolve the AX node at that point, then press it via AXUIElementPerformAction without moving the real cursor. Pass --overlay true for the dual-cursor visual. Errors with a hint to window.clickText when the OCR anchor maps to a canvas-rendered or non-AX-pressable region.",
  args = WINDOW_QUERY_OVERLAY_ARGS,
)]
fn ax_click_window_text() {}

#[invoke_command(
  id = "input.smartPress",
  group = "input",
  summary = "ActionResolver v0 diagnostic press: try OCR-to-AX press first; if it fails and pointer fallback is allowed, fall back to pointer click.",
  args = WINDOW_QUERY_OVERLAY_ARGS,
)]
fn smart_press() {}

#[invoke_command(
  id = "input.typeText",
  group = "input",
  summary = "Type text into the active macOS control through System Events.",
  args = TEXT_ARGS,
)]
fn type_text() {}

#[invoke_command(
  id = "input.pasteText",
  group = "input",
  summary = "Paste text into the active macOS control through the clipboard, then restore the prior clipboard snapshot.",
  args = TEXT_ARGS,
)]
fn paste_text_preserve_clipboard() {}

#[invoke_command(
  id = "input.key",
  group = "input",
  summary = "Press a keyboard key or shortcut in the active macOS app through System Events.",
  args = KEY_ARGS,
)]
fn press_key() {}

#[invoke_command(
  id = "input.clickPoint",
  group = "input",
  summary = "Click a macOS global logical point through Quartz.",
  args = TARGET_ARGS,
)]
fn click_point() {}

#[invoke_command(
  id = "input.clickWindowPoint",
  group = "input",
  summary = "Click a point relative to a target macOS window, either from --relative_x/--relative_y inputs or from a promoted --candidate JSON payload.",
  args = WINDOW_ARGS,
)]
fn click_window_point() {}

#[invoke_command(
  id = "input.teachClick",
  group = "input",
  summary = "Capture a target window before and after a human-taught click, recording global and window-local click coordinates for automation debugging.",
  args = WINDOW_ARGS,
)]
fn teach_click() {}

#[invoke_command(
  id = "input.scrollPoint",
  group = "input",
  summary = "Scroll at a macOS global logical point through Quartz.",
  args = TARGET_ARGS,
)]
fn scroll_point() {}
