use auv_driver::OperationDisturbance;

use crate::{
  CommandGroup, InvokeCommand, InvokeContext, InvokeDriverDispatch, arg::NO_ARGS, command::NONE,
  default_driver_dispatch, invoke_command,
};

pub fn group() -> CommandGroup {
  CommandGroup::new("fixture", "FIXTURE").command(observe_invoke_command())
}

#[invoke_command(
  id = "fixture.observe",
  group = "fixture",
  summary = "Emit a deterministic observation result without touching the real UI.",
  driver = "fixture.observe",
  operation = "observe_fixture_scene",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = ["operation-result"],
  signals = ["fixture.scene"],
  verification = "read-only; no semantic success claim",
)]
pub fn observe(context: InvokeContext<'_>, command: &InvokeCommand) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}
