use auv_cli_invoke::{InvokeNamespace, invoke_command};

#[invoke_command(
  id = "external.generated",
  group = "fixture",
  description = "External generated test command.",
  args = auv_cli_invoke::arg::NO_ARGS,
)]
async fn external_generated_command_handler(_input: auv_cli_invoke::InvokeCommandInput) -> auv_cli_invoke::InvokeCommandResult {
  Ok(auv_cli_invoke::InvokeCommandOutput::completed())
}

#[test]
fn invoke_command_macro_expands_for_downstream_crates() {
  let command: auv_cli_invoke::InvokeCommand = external_generated_command_handler_invoke_command();

  assert_eq!(command.id, "external.generated");
  assert_eq!(command.namespace, InvokeNamespace::Fixture);
  assert_eq!(command.description, "External generated test command.");
  assert_eq!(command.args, auv_cli_invoke::arg::NO_ARGS);

  futures_executor::block_on(command.invoke(auv_cli_invoke::InvokeCommandInput {
    command_id: command.id.to_string(),
    target_application_id: None,
    inputs: std::collections::BTreeMap::new(),
    dry_run: false,
    cancellation: auv_cli_invoke::InvokeCancellation::new(),
  }))
  .expect("handler should run");
}
