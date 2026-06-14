use auv_driver::OperationDisturbance;

use crate::{
  CommandGroup, InvokeCommand, InvokeContext, InvokeDriverDispatch,
  arg::{IMAGE_TEXT_ARGS, REGION_ARGS, SCREEN_TEXT_ARGS, TARGET_ARGS},
  command::{NONE, NONE_OR_FOREGROUND, POINTER_WITH_FOREGROUND},
  default_driver_dispatch, invoke_command,
};

pub fn group() -> CommandGroup {
  CommandGroup::new("screen", "SCREEN")
    .command(capture_region_invoke_command())
    .command(find_screen_text_invoke_command())
    .command(wait_for_screen_text_invoke_command())
    .command(find_screen_rows_invoke_command())
    .command(wait_for_screen_rows_invoke_command())
    .command(find_image_text_invoke_command())
    .command(click_screen_text_invoke_command())
    .command(click_screen_row_invoke_command())
}

#[invoke_command(
  id = "screen.captureRegion",
  group = "screen",
  summary = "Capture one display-contained region and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
  driver = "macos.desktop",
  operation = "capture_region",
  args = REGION_ARGS,
  disturbance = NONE_OR_FOREGROUND,
  max_disturbance = OperationDisturbance::ForegroundApp,
  artifacts = ["region-capture", "capture-contract"],
  signals = [],
  verification = "capture-only; no semantic success claim",
)]
pub fn capture_region(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "screen.findText",
  group = "screen",
  summary = "Capture a screenshot and locate OCR text anchors in screenshot pixel space. If activate_target_before_capture is true, the target app is foregrounded first.",
  driver = "macos.desktop",
  operation = "find_screen_text",
  args = SCREEN_TEXT_ARGS,
  disturbance = NONE_OR_FOREGROUND,
  max_disturbance = OperationDisturbance::ForegroundApp,
  artifacts = ["recognition-result"],
  signals = ["candidate.count"],
  verification = "recognition-only; no semantic success claim",
)]
pub fn find_screen_text(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "screen.waitForText",
  group = "screen",
  summary = "Poll live-desktop OCR until a target text anchor appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
  driver = "macos.desktop",
  operation = "wait_for_screen_text",
  args = SCREEN_TEXT_ARGS,
  disturbance = NONE_OR_FOREGROUND,
  max_disturbance = OperationDisturbance::ForegroundApp,
  artifacts = ["recognition-result"],
  signals = ["candidate.count"],
  verification = "recognition-only; no semantic success claim",
)]
pub fn wait_for_screen_text(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "screen.findRows",
  group = "screen",
  summary = "Detect visible OCR row bands inside a constrained screen region without depending on one exact anchor string. If activate_target_before_capture is true, the target app is foregrounded first.",
  driver = "macos.desktop",
  operation = "find_screen_rows",
  args = TARGET_ARGS,
  disturbance = NONE_OR_FOREGROUND,
  max_disturbance = OperationDisturbance::ForegroundApp,
  artifacts = ["recognition-result"],
  signals = ["candidate.count"],
  verification = "recognition-only; no semantic success claim",
)]
pub fn find_screen_rows(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "screen.waitForRows",
  group = "screen",
  summary = "Poll live-desktop OCR row detection until at least a target number of visible rows appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
  driver = "macos.desktop",
  operation = "wait_for_screen_rows",
  args = TARGET_ARGS,
  disturbance = NONE_OR_FOREGROUND,
  max_disturbance = OperationDisturbance::ForegroundApp,
  artifacts = ["recognition-result"],
  signals = ["candidate.count"],
  verification = "recognition-only; no semantic success claim",
)]
pub fn wait_for_screen_rows(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "screen.findImageText",
  group = "screen",
  summary = "Locate OCR text anchors inside an existing image artifact without touching the live desktop.",
  driver = "macos.desktop",
  operation = "find_image_text",
  args = IMAGE_TEXT_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = ["recognition-result"],
  signals = ["candidate.count"],
  verification = "recognition-only; no semantic success claim",
)]
pub fn find_image_text(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "screen.clickText",
  group = "screen",
  summary = "Capture a screenshot, resolve an OCR text anchor, and click its projected logical point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
  driver = "macos.desktop",
  operation = "click_screen_text",
  args = SCREEN_TEXT_ARGS,
  disturbance = POINTER_WITH_FOREGROUND,
  max_disturbance = OperationDisturbance::Pointer,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn click_screen_text(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "screen.clickRow",
  group = "screen",
  summary = "Detect visible OCR row bands inside a constrained screen region and click a chosen row-derived point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
  driver = "macos.desktop",
  operation = "click_screen_row",
  args = TARGET_ARGS,
  disturbance = POINTER_WITH_FOREGROUND,
  max_disturbance = OperationDisturbance::Pointer,
  artifacts = ["input-action-result"],
  signals = ["input.method"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn click_screen_row(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}
