use auv_cli_invoke::{
  InvokeCommand, InvokeContext, InvokeDriverDispatch, InvokeNamespace, default_driver_dispatch,
  invoke_command,
};

#[invoke_command(
  id = "external.generated",
  group = "fixture",
  summary = "External generated test command.",
  driver = "fixture.observe",
  operation = "observe_fixture_scene",
  args = auv_cli_invoke::arg::NO_ARGS,
  disturbance = auv_cli_invoke::command::NONE,
  max_disturbance = auv_driver::OperationDisturbance::None,
  artifacts = ["operation-result"],
  signals = ["fixture.scene"],
  verification = "read-only; no semantic success claim",
)]
fn external_generated_command_handler(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[test]
fn invoke_command_macro_expands_for_downstream_crates() {
  let command = external_generated_command_handler_invoke_command();

  assert_eq!(command.operation.id, "external.generated");
  assert_eq!(command.namespace, InvokeNamespace::Fixture);
  assert_eq!(command.handler_name, "external_generated_command_handler");
}
