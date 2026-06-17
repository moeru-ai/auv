use crate::{CommandGroup, arg::NO_ARGS, invoke_command};

pub fn group() -> CommandGroup {
  CommandGroup::new("fixture", "FIXTURE").command(observe_invoke_command())
}

#[invoke_command(
  id = "fixture.observe",
  group = "fixture",
  summary = "Emit a deterministic observation result without touching the real UI.",
  args = NO_ARGS,
)]
fn observe() {}
