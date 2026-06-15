use auv_driver::OperationDisturbance;

use crate::{
  CommandGroup, InvokeCommand, InvokeContext, InvokeDriverDispatch,
  arg::{NO_ARGS, TARGET_ARGS},
  command::{NONE, NONE_OR_FOREGROUND},
  default_driver_dispatch, invoke_command,
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
  driver = "macos.desktop",
  operation = "capture_display",
  args = TARGET_ARGS,
  disturbance = NONE_OR_FOREGROUND,
  max_disturbance = OperationDisturbance::ForegroundApp,
  artifacts = ["display-capture", "capture-contract"],
  signals = [],
  verification = "capture-only; no semantic success claim",
)]
pub fn capture_display(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "display.list",
  group = "display",
  summary = "List connected displays using the normalized AUV coordinate contract.",
  driver = "macos.desktop",
  operation = "list_displays",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = ["display.count"],
  verification = "read-only; no semantic success claim",
)]
pub fn list_displays(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "display.projectScreenshotPoint",
  group = "display",
  summary = "Project main-display screenshot pixels back into AUV global logical coordinates.",
  driver = "macos.desktop",
  operation = "project_screenshot_point",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = [],
  verification = "projection-only; no semantic success claim",
)]
pub fn project_screenshot_point(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "display.identifyPoint",
  group = "display",
  summary = "Resolve a logical desktop point against the current macOS display layout.",
  driver = "macos.desktop",
  operation = "identify_point",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = [],
  verification = "read-only; no semantic success claim",
)]
pub fn identify_point(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "display.probeCoordinateReadiness",
  group = "display",
  summary = "Capture a screenshot and compare its pixels against the observed macOS coordinate space.",
  driver = "macos.desktop",
  operation = "probe_coordinate_readiness",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = [],
  verification = "read-only readiness probe; no semantic success claim",
)]
pub fn probe_coordinate_readiness(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}
