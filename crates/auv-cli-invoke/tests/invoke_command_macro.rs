use auv_cli_invoke::{InvokeNamespace, invoke_command};

#[invoke_command(
  id = "external.generated",
  group = "fixture",
  summary = "External generated test command.",
  args = auv_cli_invoke::arg::NO_ARGS,
)]
fn external_generated_command_handler() {}

#[test]
fn invoke_command_macro_expands_for_downstream_crates() {
  let command = external_generated_command_handler_invoke_command();

  assert_eq!(command.id, "external.generated");
  assert_eq!(command.namespace, InvokeNamespace::Fixture);
  assert_eq!(command.summary, "External generated test command.");
  assert_eq!(command.args, auv_cli_invoke::arg::NO_ARGS);
}
