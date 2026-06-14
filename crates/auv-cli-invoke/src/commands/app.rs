use auv_driver::OperationDisturbance;

use crate::{
  CommandGroup, InvokeCommand, InvokeContext, InvokeDriverDispatch,
  arg::{NO_ARGS, TARGET_ARGS},
  command::{FOREGROUND_ONLY, NONE},
  default_driver_dispatch, invoke_command,
};

pub fn group() -> CommandGroup {
  CommandGroup::new("app", "APP")
    .command(probe_permissions_invoke_command())
    .command(activate_app_invoke_command())
}

#[invoke_command(
  id = "app.probePermissions",
  group = "app",
  summary = "Probe macOS screen recording, accessibility, and automation permissions.",
  driver = "macos.desktop",
  operation = "probe_permissions",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = [],
  signals = ["permission.screenRecording", "permission.accessibility"],
  verification = "read-only; no semantic success claim",
)]
pub fn probe_permissions(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "app.activate",
  group = "app",
  summary = "Bring a target macOS app to the foreground before a foreground-dependent step.",
  driver = "macos.desktop",
  operation = "activate_app",
  args = TARGET_ARGS,
  disturbance = FOREGROUND_ONLY,
  max_disturbance = OperationDisturbance::ForegroundApp,
  artifacts = ["input-action-result"],
  signals = ["app.activated"],
  verification = "activation-only; semantic success requires a separate verification result",
)]
pub fn activate_app(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}
