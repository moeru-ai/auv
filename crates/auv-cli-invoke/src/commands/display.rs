use crate::{
  CommandGroup,
  arg::{NO_ARGS, TARGET_ARGS},
  invoke_command,
};

pub fn group() -> CommandGroup {
  CommandGroup::new("display", "DISPLAY")
    .command(capture_display_invoke_command())
    .command(list_displays_invoke_command())
    .command(project_screenshot_point_invoke_command())
    .command(identify_point_invoke_command())
    .command(probe_coordinate_readiness_invoke_command())
}

#[invoke_command(
  id = "display.capture",
  group = "display",
  summary = "Capture one display screenshot with a coordinate contract through xcap. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = TARGET_ARGS,
)]
fn capture_display() {}

#[invoke_command(
  id = "display.list",
  group = "display",
  summary = "List connected displays using the normalized AUV coordinate contract.",
  args = NO_ARGS,
)]
fn list_displays() {}

#[invoke_command(
  id = "display.projectScreenshotPoint",
  group = "display",
  summary = "Project main-display screenshot pixels back into AUV global logical coordinates.",
  args = NO_ARGS,
)]
fn project_screenshot_point() {}

#[invoke_command(
  id = "display.identifyPoint",
  group = "display",
  summary = "Resolve a logical desktop point against the current macOS display layout.",
  args = NO_ARGS,
)]
fn identify_point() {}

#[invoke_command(
  id = "display.probeCoordinateReadiness",
  group = "display",
  summary = "Capture a screenshot and compare its pixels against the observed macOS coordinate space.",
  args = NO_ARGS,
)]
fn probe_coordinate_readiness() {}
