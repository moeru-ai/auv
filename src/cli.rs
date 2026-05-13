use std::collections::BTreeMap;

use auv_cli::model::{AuvResult, ExecutionTarget, InvokeRequest};

pub enum CliCommand {
  Help,
  ListCommands,
  ListDrivers,
  Invoke(InvokeRequest),
  Inspect { run_id: String },
}

pub fn parse_cli(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.is_empty() {
    return Ok(CliCommand::Help);
  }

  match arguments[0].as_str() {
    "help" | "--help" | "-h" => Ok(CliCommand::Help),
    "list-commands" => Ok(CliCommand::ListCommands),
    "list-drivers" => Ok(CliCommand::ListDrivers),
    "inspect" => parse_inspect(arguments),
    "invoke" => parse_invoke(arguments),
    other => Err(format!(
      "unknown subcommand {other}; use `help` to see supported commands"
    )),
  }
}

pub fn help_text() -> String {
  String::from(
    "\
auv-cli prototype

USAGE
  auv-cli list-commands
  auv-cli list-drivers
  auv-cli invoke <command-id> [--target <application-id>] [--label <text>]
  auv-cli inspect <run-id>

NOTES
  - Names are provisional and reflect the current phase-0/1 runtime skeleton.
  - The CLI is a thin frontend over the library runtime in src/lib.rs.
  - `debug.fixtureObserve` is the current runtime-validation entrypoint.
",
  )
}

fn parse_inspect(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() != 2 {
    return Err("usage: auv-cli inspect <run-id>".to_string());
  }

  Ok(CliCommand::Inspect {
    run_id: arguments[1].clone(),
  })
}

fn parse_invoke(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 2 {
    return Err(
      "usage: auv-cli invoke <command-id> [--target <application-id>] [--label <text>]".to_string(),
    );
  }

  let command_id = arguments[1].clone();
  let mut target = ExecutionTarget::default();
  let mut inputs = BTreeMap::new();
  let mut index = 2;

  while index < arguments.len() {
    let argument = &arguments[index];
    if !argument.starts_with("--") {
      return Err(format!("unexpected positional argument {argument}"));
    }
    if index + 1 >= arguments.len() {
      return Err(format!("flag {argument} requires a value"));
    }

    let value = arguments[index + 1].clone();
    match argument.as_str() {
      "--target" => {
        target.application_id = Some(value);
      }
      "--label" => {
        inputs.insert("label".to_string(), value);
      }
      other => {
        let key = other.trim_start_matches("--");
        inputs.insert(key.to_string(), value);
      }
    }

    index += 2;
  }

  Ok(CliCommand::Invoke(InvokeRequest {
    command_id,
    target,
    inputs,
  }))
}
