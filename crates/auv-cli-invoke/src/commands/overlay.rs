use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
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
async fn overlay_click_point(_input: InvokeCommandInput) -> InvokeCommandResult {
  click_point().await?;
  Ok(InvokeCommandOutput::new("clicked overlay point"))
}

pub async fn click_point() -> Result<(), String> {
  // TODO(invoke-overlay-session): overlay click still lives behind the root
  // macOS command adapter; expose a stable overlay session/input API before
  // enabling this direct invoke command.
  Err("overlay.clickPoint requires a typed overlay session API".to_string())
}

#[invoke_command(
  id = "overlay.showCursor",
  group = "overlay",
  summary = "Show a visual-only AUV cursor label overlay inside the current process.",
  args = NO_ARGS,
)]
async fn overlay_show_cursor(_input: InvokeCommandInput) -> InvokeCommandResult {
  show_cursor().await?;
  Ok(InvokeCommandOutput::new("showed overlay cursor"))
}

pub async fn show_cursor() -> Result<(), String> {
  // TODO(invoke-overlay-session): visual cursor state still lives behind the
  // root overlay adapter; expose a stable overlay session API before enabling
  // this direct invoke command.
  Err("overlay.showCursor requires a typed overlay session API".to_string())
}

#[invoke_command(
  id = "overlay.showDualCursor",
  group = "overlay",
  summary = "Show visual-only dual cursor overlays: AUV at a target point and You at the current hardware cursor.",
  args = NO_ARGS,
)]
async fn overlay_show_dual_cursor(_input: InvokeCommandInput) -> InvokeCommandResult {
  show_dual_cursor().await?;
  Ok(InvokeCommandOutput::new("showed dual overlay cursors"))
}

pub async fn show_dual_cursor() -> Result<(), String> {
  // TODO(invoke-overlay-session): dual-cursor state still lives behind the
  // root overlay adapter; expose a stable overlay session API before enabling
  // this direct invoke command.
  Err("overlay.showDualCursor requires a typed overlay session API".to_string())
}

#[invoke_command(
  id = "overlay.applyCursorBatch",
  group = "overlay",
  summary = "Apply a JSON batch of visual-only overlay cursor operations in one process.",
  args = NO_ARGS,
)]
async fn overlay_apply_cursor_batch(_input: InvokeCommandInput) -> InvokeCommandResult {
  apply_cursor_batch().await?;
  Ok(InvokeCommandOutput::new("applied overlay cursor batch"))
}

pub async fn apply_cursor_batch() -> Result<(), String> {
  // TODO(invoke-overlay-session): batch overlay operations need a stable typed
  // cursor-operation contract before this direct invoke command can run.
  Err("overlay.applyCursorBatch requires a typed overlay batch API".to_string())
}

#[invoke_command(
  id = "overlay.setCursor",
  group = "overlay",
  summary = "Show or update one visual-only overlay cursor by cursor_id.",
  args = NO_ARGS,
)]
async fn overlay_set_cursor(_input: InvokeCommandInput) -> InvokeCommandResult {
  set_cursor().await?;
  Ok(InvokeCommandOutput::new("set overlay cursor"))
}

pub async fn set_cursor() -> Result<(), String> {
  // TODO(invoke-overlay-session): cursor mutation still lives behind the root
  // overlay adapter; expose a stable overlay session API before enabling this
  // direct invoke command.
  Err("overlay.setCursor requires a typed overlay session API".to_string())
}

#[invoke_command(
  id = "overlay.moveCursor",
  group = "overlay",
  summary = "Animate the visual-only AUV cursor from the current hardware cursor toward a target point.",
  args = NO_ARGS,
)]
async fn overlay_move_cursor(_input: InvokeCommandInput) -> InvokeCommandResult {
  move_cursor().await?;
  Ok(InvokeCommandOutput::new("moved overlay cursor"))
}

pub async fn move_cursor() -> Result<(), String> {
  // TODO(invoke-overlay-session): cursor animation still lives behind the root
  // overlay adapter; expose a stable overlay session API before enabling this
  // direct invoke command.
  Err("overlay.moveCursor requires a typed overlay session API".to_string())
}

#[invoke_command(
  id = "overlay.moveCursorById",
  group = "overlay",
  summary = "Animate one visual-only overlay cursor by cursor_id, reusing its previous position when available.",
  args = NO_ARGS,
)]
async fn overlay_move_cursor_by_id(_input: InvokeCommandInput) -> InvokeCommandResult {
  move_cursor_by_id().await?;
  Ok(InvokeCommandOutput::new("moved overlay cursor by id"))
}

pub async fn move_cursor_by_id() -> Result<(), String> {
  // TODO(invoke-overlay-session): cursor-id animation still lives behind the
  // root overlay adapter; expose a stable overlay session API before enabling
  // this direct invoke command.
  Err("overlay.moveCursorById requires a typed overlay session API".to_string())
}

#[invoke_command(
  id = "overlay.flashCursor",
  group = "overlay",
  summary = "Flash the AUV click-state cursor sprite at a target point.",
  args = NO_ARGS,
)]
async fn overlay_flash_cursor(_input: InvokeCommandInput) -> InvokeCommandResult {
  flash_cursor().await?;
  Ok(InvokeCommandOutput::new("flashed overlay cursor"))
}

pub async fn flash_cursor() -> Result<(), String> {
  // TODO(invoke-overlay-session): cursor flashing still lives behind the root
  // overlay adapter; expose a stable overlay session API before enabling this
  // direct invoke command.
  Err("overlay.flashCursor requires a typed overlay session API".to_string())
}

#[invoke_command(
  id = "overlay.flashCursorById",
  group = "overlay",
  summary = "Flash the AUV click-state cursor sprite for one overlay cursor_id.",
  args = NO_ARGS,
)]
async fn overlay_flash_cursor_by_id(_input: InvokeCommandInput) -> InvokeCommandResult {
  flash_cursor_by_id().await?;
  Ok(InvokeCommandOutput::new("flashed overlay cursor by id"))
}

pub async fn flash_cursor_by_id() -> Result<(), String> {
  // TODO(invoke-overlay-session): cursor-id flashing still lives behind the
  // root overlay adapter; expose a stable overlay session API before enabling
  // this direct invoke command.
  Err("overlay.flashCursorById requires a typed overlay session API".to_string())
}

#[invoke_command(
  id = "overlay.hideCursorId",
  group = "overlay",
  summary = "Hide one visual-only overlay cursor by cursor_id.",
  args = NO_ARGS,
)]
async fn overlay_hide_cursor_id(_input: InvokeCommandInput) -> InvokeCommandResult {
  hide_cursor_by_id().await?;
  Ok(InvokeCommandOutput::new("hid overlay cursor by id"))
}

pub async fn hide_cursor_by_id() -> Result<(), String> {
  // TODO(invoke-overlay-session): cursor-id hide still lives behind the root
  // overlay adapter; expose a stable overlay session API before enabling this
  // direct invoke command.
  Err("overlay.hideCursorId requires a typed overlay session API".to_string())
}

#[invoke_command(
  id = "overlay.hideCursor",
  group = "overlay",
  summary = "Hide the visual-only AUV cursor label overlay inside the current process.",
  args = NO_ARGS,
)]
async fn overlay_hide_cursor(_input: InvokeCommandInput) -> InvokeCommandResult {
  hide_cursor().await?;
  Ok(InvokeCommandOutput::new("hid overlay cursor"))
}

pub async fn hide_cursor() -> Result<(), String> {
  // TODO(invoke-overlay-session): cursor hide still lives behind the root
  // overlay adapter; expose a stable overlay session API before enabling this
  // direct invoke command.
  Err("overlay.hideCursor requires a typed overlay session API".to_string())
}

#[invoke_command(
  id = "overlay.shutdown",
  group = "overlay",
  summary = "Shut down the visual-only AUV cursor overlay inside the current process.",
  args = NO_ARGS,
)]
async fn overlay_shutdown(_input: InvokeCommandInput) -> InvokeCommandResult {
  shutdown().await?;
  Ok(InvokeCommandOutput::new("shut down overlay"))
}

pub async fn shutdown() -> Result<(), String> {
  // TODO(invoke-overlay-session): overlay lifecycle shutdown still lives
  // behind the root overlay adapter; expose a stable overlay session API
  // before enabling this direct invoke command.
  Err("overlay.shutdown requires a typed overlay session API".to_string())
}
