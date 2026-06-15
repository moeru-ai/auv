use auv_driver::{OperationDisturbance, OperationNamespace};

use crate::{
  CommandGroup, InvokeCommand, InvokeContext, InvokeDriverDispatch,
  arg::{NO_ARGS, WINDOW_ARGS, WINDOW_TEXT_ARGS, WINDOW_VERIFY_TEXT_ARGS},
  command::{CAPTURE_AX_TREE_DISTURBANCE, NONE, NONE_OR_FOREGROUND, POINTER_WITH_FOREGROUND},
  default_driver_dispatch, invoke_command,
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
  driver = "macos.desktop",
  operation = "list_windows",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = ["window.count"],
  verification = "read-only; no semantic success claim",
)]
pub fn list_windows(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "window.capture",
  group = "window",
  summary = "Capture one single-display window and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
  driver = "macos.desktop",
  operation = "capture_window",
  args = WINDOW_ARGS,
  disturbance = NONE_OR_FOREGROUND,
  max_disturbance = OperationDisturbance::ForegroundApp,
  artifacts = ["window-capture", "capture-contract"],
  signals = [],
  verification = "capture-only; no semantic success claim",
)]
pub fn capture_window(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "window.captureAxTree",
  group = "window",
  summary = "Capture an AX tree snapshot for a target macOS app window.",
  driver = "macos.desktop",
  operation = "capture_ax_tree",
  args = WINDOW_ARGS,
  disturbance = CAPTURE_AX_TREE_DISTURBANCE,
  max_disturbance = OperationDisturbance::Keyboard,
  artifacts = ["ax-tree"],
  signals = [],
  verification = "capture-only; no semantic success claim",
)]
pub fn capture_ax_tree(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "window.findText",
  group = "window",
  summary = "Capture a resolved window and locate OCR text anchors in window pixel space.",
  driver = "macos.desktop",
  operation = "find_window_text",
  args = WINDOW_TEXT_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = ["recognition-result"],
  signals = ["candidate.count"],
  verification = "recognition-only; no semantic success claim",
)]
pub fn find_window_text(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "window.waitForText",
  group = "window",
  summary = "Poll resolved-window OCR until a text anchor appears or the timeout expires.",
  driver = "macos.desktop",
  operation = "wait_for_window_text",
  args = WINDOW_TEXT_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = ["recognition-result"],
  signals = ["candidate.count"],
  verification = "recognition-only; no semantic success claim",
)]
pub fn wait_for_window_text(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "window.findRows",
  group = "window",
  summary = "Detect visible OCR row bands inside a resolved window.",
  driver = "macos.desktop",
  operation = "find_window_rows",
  args = WINDOW_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = ["recognition-result"],
  signals = ["candidate.count"],
  verification = "recognition-only; no semantic success claim",
)]
pub fn find_window_rows(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "window.waitForRows",
  group = "window",
  summary = "Poll resolved-window row detection until enough rows appear or the timeout expires.",
  driver = "macos.desktop",
  operation = "wait_for_window_rows",
  args = WINDOW_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = ["recognition-result"],
  signals = ["candidate.count"],
  verification = "recognition-only; no semantic success claim",
)]
pub fn wait_for_window_rows(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "window.observeRegion",
  group = "window",
  summary = "Observe OCR row-like content inside a resolved macOS window region without scrolling.",
  driver = "macos.desktop",
  operation = "observe_window_region",
  args = WINDOW_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = ["recognition-result"],
  signals = ["candidate.count"],
  verification = "recognition-only; no semantic success claim",
)]
pub fn observe_window_region(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "window.findIconMatch",
  group = "window",
  summary = "Match a template image against a resolved macOS window screenshot using NCC and emit a RecognitionResult artifact.",
  driver = "macos.desktop",
  operation = "find_icon_match",
  args = WINDOW_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = ["recognition-result"],
  signals = ["candidate.count"],
  verification = "recognition-only; no semantic success claim",
)]
pub fn find_icon_match(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "window.scrollRegion",
  group = "window",
  summary = "Scroll at the center of a resolved macOS window region and record scroll evidence.",
  driver = "macos.desktop",
  operation = "scroll_window_region",
  args = WINDOW_ARGS,
  disturbance = POINTER_WITH_FOREGROUND,
  max_disturbance = OperationDisturbance::Pointer,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn scroll_window_region(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "window.verifyText",
  group = "window",
  operation_namespace = OperationNamespace::Verify,
  summary = "Verify that a text-bearing AX node exists in the observed tree without relying on screenshot OCR.",
  driver = "macos.desktop",
  operation = "verify_ax_text",
  args = WINDOW_VERIFY_TEXT_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = ["verification-result"],
  signals = ["verification.matched"],
  verification = "AX/window text verification; success requires a typed VerificationResult match",
)]
pub fn verify_ax_text(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "window.clickText",
  group = "window",
  summary = "Capture a resolved window, resolve an OCR text anchor, and click its projected logical point.",
  driver = "macos.desktop",
  operation = "click_window_text",
  args = WINDOW_TEXT_ARGS,
  disturbance = POINTER_WITH_FOREGROUND,
  max_disturbance = OperationDisturbance::Pointer,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn click_window_text(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "window.clickRow",
  group = "window",
  summary = "Capture a resolved window, detect visible rows, and click a row-derived projected logical point.",
  driver = "macos.desktop",
  operation = "click_window_row",
  args = WINDOW_ARGS,
  disturbance = POINTER_WITH_FOREGROUND,
  max_disturbance = OperationDisturbance::Pointer,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn click_window_row(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}
