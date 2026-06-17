use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, arg::NO_ARGS,
  invoke_command,
};

pub fn group() -> CommandGroup {
  CommandGroup::new("fixture", "FIXTURE").command(observe_invoke_command())
}

#[invoke_command(
  id = "fixture.observe",
  group = "fixture",
  summary = "Emit a deterministic observation result without touching the real UI.",
  args = NO_ARGS,
)]
fn observe(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::new("fixture observed");
  output.verification = Some("read-only; no semantic success claim".to_string());
  output
    .known_limits
    .push("fixture.observe records deterministic fixture output only.".to_string());
  Ok(output)
}
