use auv_cli_invoke::{CommandRole, InvokeNamespace, Platform, RunnerSupport, SupportState, invoke_command};
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
  support = { macos: Supported, linux: Partial("a limit the test names"), windows: Unsupported("a reason the test names") },
  runner = { macos: Supported, linux: NotEvidenced("a runner gap the test names"), windows: Unsupported("a runner reason the test names") },
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
  assert_eq!(command.role, CommandRole::Desktop);
  assert_eq!(command.support.on(Platform::Macos).state, SupportState::Supported);
  assert_eq!(command.support.on(Platform::Linux).note, Some("a limit the test names"));
  assert_eq!(command.support.on(Platform::Windows).state, SupportState::Unsupported);
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

/// The `runner =` attribute is required and takes the same cell vocabulary as
/// `support =`, including `NotEvidenced` for a cell no audit has resolved.
#[test]
fn invoke_command_macro_expands_the_runner_declaration() {
  let command: auv_cli_invoke::InvokeCommand = external_generated_command_handler_invoke_command();

  let RunnerSupport::Cells(cells) = command.runner else {
    panic!("external.generated should declare explicit Runner cells");
  };
  assert_eq!(cells.on(Platform::Macos).state, SupportState::Supported);
  assert_eq!(cells.on(Platform::Linux).state, SupportState::NotEvidenced);
  assert_eq!(cells.on(Platform::Linux).note, Some("a runner gap the test names"));
  assert_eq!(cells.on(Platform::Windows).state, SupportState::Unsupported);
  assert_eq!(cells.on(Platform::Windows).note, Some("a runner reason the test names"));

  let resolved = command.runner_cells().expect("cells resolve for a routed command");
  assert_eq!(resolved, cells, "explicit cells resolve to themselves");
}
