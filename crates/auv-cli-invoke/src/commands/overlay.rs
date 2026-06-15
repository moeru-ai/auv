use auv_driver::OperationDisturbance;

use crate::{
  CommandGroup, InvokeCommand, InvokeContext, InvokeDriverDispatch,
  arg::{NO_ARGS, TARGET_ARGS},
  command::{NONE, POINTER_WITH_FOREGROUND},
  default_driver_dispatch, invoke_command,
};

pub fn group() -> CommandGroup {
  CommandGroup::new("overlay", "OVERLAY")
    .command(overlay_click_point_invoke_command())
    .command(overlay_show_cursor_invoke_command())
    .command(overlay_show_dual_cursor_invoke_command())
    .command(overlay_apply_cursor_batch_invoke_command())
    .command(overlay_set_cursor_invoke_command())
    .command(overlay_move_cursor_invoke_command())
    .command(overlay_move_cursor_by_id_invoke_command())
    .command(overlay_flash_cursor_invoke_command())
    .command(overlay_flash_cursor_by_id_invoke_command())
    .command(overlay_hide_cursor_id_invoke_command())
    .command(overlay_hide_cursor_invoke_command())
    .command(overlay_shutdown_invoke_command())
}

#[invoke_command(
  id = "overlay.clickPoint",
  group = "overlay",
  summary = "Move the visual AUV cursor to a target point, click, flash the click-state cursor, then hide overlay. The real cursor visibly warps to the click target and back (cursorDisturbance=warp-visible).",
  driver = "macos.desktop",
  operation = "overlay_click_point",
  args = TARGET_ARGS,
  disturbance = POINTER_WITH_FOREGROUND,
  max_disturbance = OperationDisturbance::Pointer,
  artifacts = ["input-action-result"],
  signals = ["overlay.cursor"],
  verification = "visualization-only; semantic success requires a separate verification result",
)]
pub fn overlay_click_point(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "overlay.showCursor",
  group = "overlay",
  summary = "Show a visual-only AUV cursor label overlay inside the current process.",
  driver = "macos.desktop",
  operation = "overlay_show_cursor",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = ["overlay.cursor"],
  verification = "visualization-only; no semantic success claim",
)]
pub fn overlay_show_cursor(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "overlay.showDualCursor",
  group = "overlay",
  summary = "Show visual-only dual cursor overlays: AUV at a target point and You at the current hardware cursor.",
  driver = "macos.desktop",
  operation = "overlay_show_dual_cursor",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = ["overlay.cursor"],
  verification = "visualization-only; no semantic success claim",
)]
pub fn overlay_show_dual_cursor(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "overlay.applyCursorBatch",
  group = "overlay",
  summary = "Apply a JSON batch of visual-only overlay cursor operations in one process.",
  driver = "macos.desktop",
  operation = "overlay_apply_cursor_batch",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = ["overlay.cursor"],
  verification = "visualization-only; no semantic success claim",
)]
pub fn overlay_apply_cursor_batch(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "overlay.setCursor",
  group = "overlay",
  summary = "Show or update one visual-only overlay cursor by cursor_id.",
  driver = "macos.desktop",
  operation = "overlay_set_cursor",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = ["overlay.cursor"],
  verification = "visualization-only; no semantic success claim",
)]
pub fn overlay_set_cursor(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "overlay.moveCursor",
  group = "overlay",
  summary = "Animate the visual-only AUV cursor from the current hardware cursor toward a target point.",
  driver = "macos.desktop",
  operation = "overlay_move_cursor",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = ["overlay.cursor"],
  verification = "visualization-only; no semantic success claim",
)]
pub fn overlay_move_cursor(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "overlay.moveCursorById",
  group = "overlay",
  summary = "Animate one visual-only overlay cursor by cursor_id, reusing its previous position when available.",
  driver = "macos.desktop",
  operation = "overlay_move_cursor_by_id",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = ["overlay.cursor"],
  verification = "visualization-only; no semantic success claim",
)]
pub fn overlay_move_cursor_by_id(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "overlay.flashCursor",
  group = "overlay",
  summary = "Flash the AUV click-state cursor sprite at a target point.",
  driver = "macos.desktop",
  operation = "overlay_flash_cursor",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = ["overlay.cursor"],
  verification = "visualization-only; no semantic success claim",
)]
pub fn overlay_flash_cursor(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "overlay.flashCursorById",
  group = "overlay",
  summary = "Flash the AUV click-state cursor sprite for one overlay cursor_id.",
  driver = "macos.desktop",
  operation = "overlay_flash_cursor_by_id",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = ["overlay.cursor"],
  verification = "visualization-only; no semantic success claim",
)]
pub fn overlay_flash_cursor_by_id(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "overlay.hideCursorId",
  group = "overlay",
  summary = "Hide one visual-only overlay cursor by cursor_id.",
  driver = "macos.desktop",
  operation = "overlay_hide_cursor_id",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = ["overlay.cursor"],
  verification = "visualization-only; no semantic success claim",
)]
pub fn overlay_hide_cursor_id(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "overlay.hideCursor",
  group = "overlay",
  summary = "Hide the visual-only AUV cursor label overlay inside the current process.",
  driver = "macos.desktop",
  operation = "overlay_hide_cursor",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = ["overlay.cursor"],
  verification = "visualization-only; no semantic success claim",
)]
pub fn overlay_hide_cursor(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "overlay.shutdown",
  group = "overlay",
  summary = "Shut down the visual-only AUV cursor overlay inside the current process.",
  driver = "macos.desktop",
  operation = "overlay_shutdown",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = ["overlay.shutdown"],
  verification = "visualization-only; no semantic success claim",
)]
pub fn overlay_shutdown(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}
