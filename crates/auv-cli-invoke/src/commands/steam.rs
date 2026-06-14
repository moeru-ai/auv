use auv_driver::OperationDisturbance;

use crate::{
  CommandGroup, InvokeCommand, InvokeContext, InvokeDriverDispatch, arg::NO_ARGS, command::NONE,
  default_driver_dispatch, invoke_command,
};

pub fn group() -> CommandGroup {
  CommandGroup::new("steam", "STEAM").command(steam_library_list_invoke_command())
}

#[invoke_command(
  id = "steam.library.list.v0",
  group = "steam",
  summary = "List installed Steam library apps from local appmanifest files.",
  driver = "steam.local",
  operation = "steam_library_list",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = ["steam-library-list"],
  signals = ["steam.library.source", "steam.library.status", "steam.library.app_count"],
  verification = "read-only; verified means local Steam appmanifest data was read",
)]
pub fn steam_library_list(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}
