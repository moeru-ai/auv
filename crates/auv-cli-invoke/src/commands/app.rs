use crate::{
  CommandGroup,
  arg::{NO_ARGS, TARGET_ARGS},
  invoke_command,
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
  args = NO_ARGS,
)]
fn probe_permissions() {}

#[invoke_command(
  id = "app.activate",
  group = "app",
  summary = "Bring a target macOS app to the foreground before a foreground-dependent step.",
  args = TARGET_ARGS,
)]
fn activate_app() {}
