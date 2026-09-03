//! Registry-backed CLI invoke metadata and help rendering.
//!
//! This crate owns how invoke-visible commands are described, grouped, and
//! parsed for `auv invoke ...`.

use std::collections::BTreeMap;
use std::path::PathBuf;

extern crate self as auv_cli_invoke;

pub mod artifact;
pub mod command;
pub mod commands;
pub mod help;
pub mod models;
pub mod registry;
pub mod render;
pub mod runner;

pub use auv_cli_invoke_macros::invoke_command;
pub use command::{
  CommandGroup, CommandNode, InvokeCancellation, InvokeCancelled, InvokeCommand, InvokeCommandCliParse, InvokeCommandFuture,
  InvokeCommandHandler, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeNamespace, TypedInvokeArgs,
};
pub use commands::input::emit_input_action_result;
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
    target: Option<ExecutionTarget>,
    inputs: BTreeMap<String, String>,
    typed_args: TypedInvokeArgs,
    store_root: Option<PathBuf>,
    dry_run: bool,
    output: InvokeOutputOptions,
  },
}

pub fn parse_invoke_args(arguments: &[String]) -> Result<InvokeCliParse, String> {
  let tokens = match arguments.first().map(String::as_str) {
    Some("invoke") => &arguments[1..],
    _ => arguments,
  };
  if tokens.is_empty() || tokens.first().is_some_and(|token| matches!(token.as_str(), "help" | "--help" | "-h")) {
    return Ok(InvokeCliParse::Help { command_id: None });
  }

  let command_id = tokens.first().expect("non-empty invoke tokens");
  let registry = default_registry();
  let command = registry.resolve(command_id).ok_or_else(|| format!("unknown invoke command {command_id}; use `auv invoke --help`"))?;
  match command.parse_cli_args(&tokens[1..])? {
    InvokeCommandCliParse::Help => Ok(InvokeCliParse::Help {
      command_id: Some(command.id.to_string()),
    }),
    InvokeCommandCliParse::Invoke {
      target,
      mut inputs,
      typed_args,
      store_root,
      dry_run,
      json,
      detail,
      wide,
      overlay_enabled,
    } => {
      if !overlay_enabled {
        inputs.insert("overlay".to_string(), "false".to_string());
      }
      Ok(InvokeCliParse::Invoke {
        command_id: command.id.to_string(),
        target,
        inputs,
        typed_args,
        store_root,
        dry_run,
        output: InvokeOutputOptions { json, detail, wide },
      })
    }
  }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
