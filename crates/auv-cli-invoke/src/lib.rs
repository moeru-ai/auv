//! Registry-backed CLI invoke metadata and help rendering.
//!
//! This crate owns how invoke-visible commands are described, grouped, and
//! parsed for `auv invoke ...`.

use std::collections::BTreeMap;

use clap::{Arg, ArgAction, Command};

extern crate self as auv_cli_invoke;

pub mod arg;
pub mod artifact;
pub mod command;
pub mod commands;
pub mod help;
pub mod models;
pub mod registry;
pub mod render;

pub use arg::ArgSpec;
pub use auv_cli_invoke_macros::invoke_command;
pub use command::{
  CommandGroup, CommandNode, InvokeCancellation, InvokeCancelled, InvokeCommand, InvokeCommandFuture, InvokeCommandHandler,
  InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeNamespace,
};
pub use help::{render_command_help, render_help_index};
pub use models::{
  ExecutionTarget, InvokeOutputOptions, InvokeReport, InvokeReportField, InvokeReportSection, InvokeReportTable, InvokeReportTableRow,
  InvokeRequest, InvokeResult, InvokeStatus,
};
pub(crate) use models::{InvokeReportValue, OptionalReportText};
pub use registry::{InvokeRegistry, default_registry};
pub use render::{InvokeCliOutcome, render_invoke_result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvokeCliParse {
  Help {
    command_id: Option<String>,
  },
  Invoke {
    command_id: String,
    target_application_id: Option<String>,
    inputs: BTreeMap<String, String>,
    dry_run: bool,
    output: InvokeOutputOptions,
  },
}

pub fn parse_invoke_args(arguments: &[String]) -> Result<InvokeCliParse, String> {
  let tokens = match arguments.first().map(String::as_str) {
    Some("invoke") => &arguments[1..],
    _ => arguments,
  };
  if tokens.is_empty() || tokens.first().is_some_and(|token| token == "help") {
    return Ok(InvokeCliParse::Help { command_id: None });
  }

  let normalized = normalize_for_clap(tokens)?;
  if let Some(help) = normalized.help {
    return Ok(help);
  }

  let matches = invoke_cli_command().try_get_matches_from(normalized.clap_arguments).map_err(|error| error.to_string())?;
  let command_id = matches.get_one::<String>("command_id").cloned().ok_or_else(|| "missing invoke command id".to_string())?;
  let mut inputs = normalized.inputs;
  if let Some(label) = matches.get_one::<String>("label") {
    inputs.insert("label".to_string(), label.clone());
  }

  Ok(InvokeCliParse::Invoke {
    command_id,
    target_application_id: matches.get_one::<String>("target").cloned(),
    inputs,
    dry_run: matches.get_flag("dry_run"),
    output: InvokeOutputOptions {
      json: matches.get_flag("json") || matches.get_flag("format"),
      detail: matches.get_flag("detail"),
      wide: matches.get_flag("wide"),
    },
  })
}

pub fn invoke_argument_consumes_value(argument: &str) -> bool {
  match argument {
    "--dry-run" | "--detail" | "--wide" | "--json" | "--format" | "--help" | "-h" => false,
    other => other.starts_with("--"),
  }
}

struct NormalizedInvokeArguments {
  clap_arguments: Vec<String>,
  inputs: BTreeMap<String, String>,
  help: Option<InvokeCliParse>,
}

fn invoke_cli_command() -> Command {
  Command::new("invoke")
    .disable_help_flag(true)
    .arg(Arg::new("command_id").index(1).value_name("command-id"))
    .arg(Arg::new("dry_run").long("dry-run").action(ArgAction::SetTrue))
    .arg(Arg::new("target").long("target").value_name("bundle-id").num_args(1))
    .arg(Arg::new("label").long("label").value_name("value").num_args(1))
    .arg(Arg::new("detail").long("detail").action(ArgAction::SetTrue))
    .arg(Arg::new("wide").long("wide").action(ArgAction::SetTrue))
    .arg(Arg::new("json").long("json").action(ArgAction::SetTrue))
    .arg(Arg::new("format").long("format").action(ArgAction::SetTrue).hide(true))
}

fn normalize_for_clap(tokens: &[String]) -> Result<NormalizedInvokeArguments, String> {
  let mut clap_arguments = vec!["invoke".to_string()];
  let mut inputs = BTreeMap::new();
  let mut command_id = None;
  let mut index = 0;

  while index < tokens.len() {
    let token = &tokens[index];
    match token.as_str() {
      "--help" | "-h" => {
        return Ok(NormalizedInvokeArguments {
          clap_arguments,
          inputs,
          help: Some(InvokeCliParse::Help { command_id }),
        });
      }
      "--dry-run" | "--detail" | "--wide" | "--json" | "--format" => {
        clap_arguments.push(token.clone());
        index += 1;
      }
      "--target" | "--label" => {
        clap_arguments.push(token.clone());
        if let Some(value) = tokens.get(index + 1) {
          clap_arguments.push(value.clone());
          index += 2;
        } else {
          index += 1;
        }
      }
      flag if flag.starts_with("--") => {
        let Some(value) = tokens.get(index + 1) else {
          return Err(format!("flag {flag} requires a value"));
        };
        let key = flag.trim_start_matches("--");
        inputs.insert(key.to_string(), value.clone());
        index += 2;
      }
      positional => {
        if command_id.is_none() {
          command_id = Some(positional.to_string());
          clap_arguments.push(positional.to_string());
          index += 1;
        } else {
          return Err(format!("unexpected positional argument {positional}"));
        }
      }
    }
  }

  Ok(NormalizedInvokeArguments {
    clap_arguments,
    inputs,
    help: None,
  })
}
