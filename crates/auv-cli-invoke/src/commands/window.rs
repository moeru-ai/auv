use crate::{
  CommandGroup,
  arg::{NO_ARGS, WINDOW_ARGS, WINDOW_TEXT_ARGS, WINDOW_VERIFY_TEXT_ARGS},
  invoke_command,
};

pub fn group() -> CommandGroup {
  CommandGroup::new("window", "WINDOW")
    .command(list_windows_invoke_command())
    .command(capture_window_invoke_command())
    .command(capture_ax_tree_invoke_command())
    .command(find_window_text_invoke_command())
    .command(wait_for_window_text_invoke_command())
    .command(find_window_rows_invoke_command())
    .command(wait_for_window_rows_invoke_command())
    .command(observe_window_region_invoke_command())
    .command(find_icon_match_invoke_command())
    .command(scroll_window_region_invoke_command())
    .command(verify_ax_text_invoke_command())
    .command(click_window_text_invoke_command())
    .command(click_window_row_invoke_command())
}

#[invoke_command(
  id = "window.list",
  group = "window",
  summary = "List visible macOS window candidates using the normalized AUV window selector model.",
  args = NO_ARGS,
)]
fn list_windows() {}

#[invoke_command(
  id = "window.capture",
  group = "window",
  summary = "Capture one single-display window and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = WINDOW_ARGS,
)]
fn capture_window() {}

#[invoke_command(
  id = "window.captureAxTree",
  group = "window",
  summary = "Capture an AX tree snapshot for a target macOS app window.",
  args = WINDOW_ARGS,
)]
fn capture_ax_tree() {}

#[invoke_command(
  id = "window.findText",
  group = "window",
  summary = "Capture a resolved window and locate OCR text anchors in window pixel space.",
  args = WINDOW_TEXT_ARGS,
)]
fn find_window_text() {}

#[invoke_command(
  id = "window.waitForText",
  group = "window",
  summary = "Poll resolved-window OCR until a text anchor appears or the timeout expires.",
  args = WINDOW_TEXT_ARGS,
)]
fn wait_for_window_text() {}

#[invoke_command(
  id = "window.findRows",
  group = "window",
  summary = "Detect visible OCR row bands inside a resolved window.",
  args = WINDOW_ARGS,
)]
fn find_window_rows() {}

#[invoke_command(
  id = "window.waitForRows",
  group = "window",
  summary = "Poll resolved-window row detection until enough rows appear or the timeout expires.",
  args = WINDOW_ARGS,
)]
fn wait_for_window_rows() {}

#[invoke_command(
  id = "window.observeRegion",
  group = "window",
  summary = "Observe OCR row-like content inside a resolved macOS window region without scrolling.",
  args = WINDOW_ARGS,
)]
fn observe_window_region() {}

#[invoke_command(
  id = "window.findIconMatch",
  group = "window",
  summary = "Match a template image against a resolved macOS window screenshot using NCC and emit a RecognitionResult artifact.",
  args = WINDOW_ARGS,
)]
fn find_icon_match() {}

#[invoke_command(
  id = "window.scrollRegion",
  group = "window",
  summary = "Scroll at the center of a resolved macOS window region and record scroll evidence.",
  args = WINDOW_ARGS,
)]
fn scroll_window_region() {}

#[invoke_command(
  id = "window.verifyText",
  group = "window",
  summary = "Verify that a text-bearing AX node exists in the observed tree without relying on screenshot OCR.",
  args = WINDOW_VERIFY_TEXT_ARGS,
)]
fn verify_ax_text() {}

#[invoke_command(
  id = "window.clickText",
  group = "window",
  summary = "Capture a resolved window, resolve an OCR text anchor, and click its projected logical point.",
  args = WINDOW_TEXT_ARGS,
)]
fn click_window_text() {}

#[invoke_command(
  id = "window.clickRow",
  group = "window",
  summary = "Capture a resolved window, detect visible rows, and click a row-derived projected logical point.",
  args = WINDOW_ARGS,
)]
fn click_window_row() {}
