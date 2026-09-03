use auv_cli_invoke::{InvokeNamespace, invoke_command};
use clap::Args;

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
struct ExternalArgs {
  /// Fixture value owned by this command.
  #[arg(value_name = "VALUE")]
  value: u16,
}

#[invoke_command(
  id = "external.generated",
  group = "fixture",
  description = "External generated test command.",
  input = ExternalArgs,
)]
async fn external_generated_command_handler(
  _input: auv_cli_invoke::InvokeCommandInput,
  args: ExternalArgs,
) -> auv_cli_invoke::InvokeCommandResult {
  assert_eq!(args.value, 42);
  Ok(auv_cli_invoke::InvokeCommandOutput::completed())
}

#[test]
fn invoke_command_macro_expands_for_downstream_crates() {
  let command: auv_cli_invoke::InvokeCommand = external_generated_command_handler_invoke_command();

  assert_eq!(command.id, "external.generated");
  assert_eq!(command.namespace, InvokeNamespace::Fixture);
  assert_eq!(command.description, "External generated test command.");
  assert!(command.clap_command().render_long_help().to_string().contains("<VALUE>"));
  let inputs = match command.parse_cli_args(&["42".to_string()]).expect("typed args should parse") {
    auv_cli_invoke::InvokeCommandCliParse::Invoke { inputs, .. } => inputs,
    auv_cli_invoke::InvokeCommandCliParse::Help => panic!("value parsing unexpectedly rendered help"),
  };

  futures_executor::block_on(command.invoke(auv_cli_invoke::InvokeCommandInput {
    command_id: command.id.to_string(),
    target: None,
    inputs,
    typed_args: None,
    dry_run: false,
    cancellation: auv_cli_invoke::InvokeCancellation::new(),
  }))
  .expect("handler should run");
}
