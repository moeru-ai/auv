use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandResult,
  arg::{NO_ARGS, TARGET_ARGS},
  invoke_command,
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
  description = "Move the visual AUV cursor to a target point, click, flash the click-state cursor, then hide overlay. The real cursor visibly warps to the click target and back (cursorDisturbance=warp-visible).",
  args = TARGET_ARGS,
)]
async fn overlay_click_point(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-overlay-session): implement after the overlay owns a typed
  // session/input interface shared by CLI and MCP frontends.
  unimplemented!("overlay.clickPoint")
}

#[invoke_command(
  id = "overlay.showCursor",
  group = "overlay",
  description = "Show a visual-only AUV cursor label overlay inside the current process.",
  args = NO_ARGS,
)]
async fn overlay_show_cursor(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-overlay-session): see `overlay_click_point`.
  unimplemented!("overlay.showCursor")
}

#[invoke_command(
  id = "overlay.showDualCursor",
  group = "overlay",
  description = "Show visual-only dual cursor overlays: AUV at a target point and You at the current hardware cursor.",
  args = NO_ARGS,
)]
async fn overlay_show_dual_cursor(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-overlay-session): see `overlay_click_point`.
  unimplemented!("overlay.showDualCursor")
}

#[invoke_command(
  id = "overlay.applyCursorBatch",
  group = "overlay",
  description = "Apply a JSON batch of visual-only overlay cursor operations in one process.",
  args = NO_ARGS,
)]
async fn overlay_apply_cursor_batch(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-overlay-session): see `overlay_click_point`.
  unimplemented!("overlay.applyCursorBatch")
}

#[invoke_command(
  id = "overlay.setCursor",
  group = "overlay",
  description = "Show or update one visual-only overlay cursor by cursor_id.",
  args = NO_ARGS,
)]
async fn overlay_set_cursor(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-overlay-session): see `overlay_click_point`.
  unimplemented!("overlay.setCursor")
}

#[invoke_command(
  id = "overlay.moveCursor",
  group = "overlay",
  description = "Animate the visual-only AUV cursor from the current hardware cursor toward a target point.",
  args = NO_ARGS,
)]
async fn overlay_move_cursor(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-overlay-session): see `overlay_click_point`.
  unimplemented!("overlay.moveCursor")
}

#[invoke_command(
  id = "overlay.moveCursorById",
  group = "overlay",
  description = "Animate one visual-only overlay cursor by cursor_id, reusing its previous position when available.",
  args = NO_ARGS,
)]
async fn overlay_move_cursor_by_id(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-overlay-session): see `overlay_click_point`.
  unimplemented!("overlay.moveCursorById")
}

#[invoke_command(
  id = "overlay.flashCursor",
  group = "overlay",
  description = "Flash the AUV click-state cursor sprite at a target point.",
  args = NO_ARGS,
)]
async fn overlay_flash_cursor(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-overlay-session): see `overlay_click_point`.
  unimplemented!("overlay.flashCursor")
}

#[invoke_command(
  id = "overlay.flashCursorById",
  group = "overlay",
  description = "Flash the AUV click-state cursor sprite for one overlay cursor_id.",
  args = NO_ARGS,
)]
async fn overlay_flash_cursor_by_id(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-overlay-session): see `overlay_click_point`.
  unimplemented!("overlay.flashCursorById")
}

#[invoke_command(
  id = "overlay.hideCursorId",
  group = "overlay",
  description = "Hide one visual-only overlay cursor by cursor_id.",
  args = NO_ARGS,
)]
async fn overlay_hide_cursor_id(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-overlay-session): see `overlay_click_point`.
  unimplemented!("overlay.hideCursorId")
}

#[invoke_command(
  id = "overlay.hideCursor",
  group = "overlay",
  description = "Hide the visual-only AUV cursor label overlay inside the current process.",
  args = NO_ARGS,
)]
async fn overlay_hide_cursor(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-overlay-session): see `overlay_click_point`.
  unimplemented!("overlay.hideCursor")
}

#[invoke_command(
  id = "overlay.shutdown",
  group = "overlay",
  description = "Shut down the visual-only AUV cursor overlay inside the current process.",
  args = NO_ARGS,
)]
async fn overlay_shutdown(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-overlay-session): see `overlay_click_point`.
  unimplemented!("overlay.shutdown")
}
