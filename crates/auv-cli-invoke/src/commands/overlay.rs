use crate::{
  CommandGroup,
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
  summary = "Move the visual AUV cursor to a target point, click, flash the click-state cursor, then hide overlay. The real cursor visibly warps to the click target and back (cursorDisturbance=warp-visible).",
  args = TARGET_ARGS,
)]
fn overlay_click_point() {}

#[invoke_command(
  id = "overlay.showCursor",
  group = "overlay",
  summary = "Show a visual-only AUV cursor label overlay inside the current process.",
  args = NO_ARGS,
)]
fn overlay_show_cursor() {}

#[invoke_command(
  id = "overlay.showDualCursor",
  group = "overlay",
  summary = "Show visual-only dual cursor overlays: AUV at a target point and You at the current hardware cursor.",
  args = NO_ARGS,
)]
fn overlay_show_dual_cursor() {}

#[invoke_command(
  id = "overlay.applyCursorBatch",
  group = "overlay",
  summary = "Apply a JSON batch of visual-only overlay cursor operations in one process.",
  args = NO_ARGS,
)]
fn overlay_apply_cursor_batch() {}

#[invoke_command(
  id = "overlay.setCursor",
  group = "overlay",
  summary = "Show or update one visual-only overlay cursor by cursor_id.",
  args = NO_ARGS,
)]
fn overlay_set_cursor() {}

#[invoke_command(
  id = "overlay.moveCursor",
  group = "overlay",
  summary = "Animate the visual-only AUV cursor from the current hardware cursor toward a target point.",
  args = NO_ARGS,
)]
fn overlay_move_cursor() {}

#[invoke_command(
  id = "overlay.moveCursorById",
  group = "overlay",
  summary = "Animate one visual-only overlay cursor by cursor_id, reusing its previous position when available.",
  args = NO_ARGS,
)]
fn overlay_move_cursor_by_id() {}

#[invoke_command(
  id = "overlay.flashCursor",
  group = "overlay",
  summary = "Flash the AUV click-state cursor sprite at a target point.",
  args = NO_ARGS,
)]
fn overlay_flash_cursor() {}

#[invoke_command(
  id = "overlay.flashCursorById",
  group = "overlay",
  summary = "Flash the AUV click-state cursor sprite for one overlay cursor_id.",
  args = NO_ARGS,
)]
fn overlay_flash_cursor_by_id() {}

#[invoke_command(
  id = "overlay.hideCursorId",
  group = "overlay",
  summary = "Hide one visual-only overlay cursor by cursor_id.",
  args = NO_ARGS,
)]
fn overlay_hide_cursor_id() {}

#[invoke_command(
  id = "overlay.hideCursor",
  group = "overlay",
  summary = "Hide the visual-only AUV cursor label overlay inside the current process.",
  args = NO_ARGS,
)]
fn overlay_hide_cursor() {}

#[invoke_command(
  id = "overlay.shutdown",
  group = "overlay",
  summary = "Shut down the visual-only AUV cursor overlay inside the current process.",
  args = NO_ARGS,
)]
fn overlay_shutdown() {}
