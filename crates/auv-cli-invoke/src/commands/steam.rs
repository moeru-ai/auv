use crate::{CommandGroup, arg::NO_ARGS, invoke_command};

pub fn group() -> CommandGroup {
  CommandGroup::new("steam", "STEAM").command(steam_library_list_invoke_command())
}

#[invoke_command(
  id = "steam.library.list.v0",
  group = "steam",
  summary = "List installed Steam library apps from local appmanifest files.",
  args = NO_ARGS,
)]
fn steam_library_list() {}
