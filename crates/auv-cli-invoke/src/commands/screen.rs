use crate::{
  CommandGroup,
  arg::{IMAGE_TEXT_ARGS, REGION_ARGS, SCREEN_TEXT_ARGS, TARGET_ARGS},
  invoke_command,
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
  args = REGION_ARGS,
)]
fn capture_region() {}

#[invoke_command(
  id = "screen.findText",
  group = "screen",
  summary = "Capture a screenshot and locate OCR text anchors in screenshot pixel space. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = SCREEN_TEXT_ARGS,
)]
fn find_screen_text() {}

#[invoke_command(
  id = "screen.waitForText",
  group = "screen",
  summary = "Poll live-desktop OCR until a target text anchor appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
  args = SCREEN_TEXT_ARGS,
)]
fn wait_for_screen_text() {}

#[invoke_command(
  id = "screen.findRows",
  group = "screen",
  summary = "Detect visible OCR row bands inside a constrained screen region without depending on one exact anchor string. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = TARGET_ARGS,
)]
fn find_screen_rows() {}

#[invoke_command(
  id = "screen.waitForRows",
  group = "screen",
  summary = "Poll live-desktop OCR row detection until at least a target number of visible rows appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
  args = TARGET_ARGS,
)]
fn wait_for_screen_rows() {}

#[invoke_command(
  id = "screen.findImageText",
  group = "screen",
  summary = "Locate OCR text anchors inside an existing image artifact without touching the live desktop.",
  args = IMAGE_TEXT_ARGS,
)]
fn find_image_text() {}

#[invoke_command(
  id = "screen.clickText",
  group = "screen",
  summary = "Capture a screenshot, resolve an OCR text anchor, and click its projected logical point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
  args = SCREEN_TEXT_ARGS,
)]
fn click_screen_text() {}

#[invoke_command(
  id = "screen.clickRow",
  group = "screen",
  summary = "Detect visible OCR row bands inside a constrained screen region and click a chosen row-derived point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
  args = TARGET_ARGS,
)]
fn click_screen_row() {}
