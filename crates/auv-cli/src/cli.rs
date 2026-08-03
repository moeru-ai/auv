//! Typed root command tree for the core `auv` frontend.

use std::ffi::OsString;
use std::path::PathBuf;

use auv_cli_invoke::{ExecutionTarget, InvokeCliParse, InvokeRequest};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum, error::ErrorKind};

use crate::commands::doctor::DoctorArgs;
use crate::commands::invoke::InvokeArgs;
use crate::commands::mcp::{McpArgs, McpCommand};
use crate::commands::plugin::{PluginArgs, PluginCommand};

type AuvResult<T> = Result<T, String>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TracingOptions {
  pub store_root: Option<PathBuf>,
}

#[derive(Debug)]
pub enum CliCommand {
  Help(String),
  Version,
  PermissionCheck {
    json: bool,
  },
  InvokeHelp {
    command_id: Option<String>,
  },
  Invoke {
    request: InvokeRequest,
    typed_args: auv_cli_invoke::TypedInvokeArgs,
    tracing: TracingOptions,
    output: auv_cli_invoke::InvokeOutputOptions,
  },
  McpServe,
  PluginList,
  External {
    command_name: OsString,
    arguments: Vec<OsString>,
  },
  XtaskGenerateSwiftBridge,
}

#[derive(Debug, Parser)]
#[command(
  name = "auv",
  version,
  about = "Invoke and inspect core computer-use capabilities",
  long_about = "AUV turns computer-use operations into command-like, inspectable, and recorded runs.\n\nThe root CLI owns core invoke, doctor, and MCP frontends. Installed auv-* executables extend it with application-owned commands.",
  after_long_help = "Examples:\n  # Inspect available core invoke commands\n  auv invoke --help\n\n  # Diagnose local automation readiness\n  auv doctor\n\n  # Run an installed application plugin\n  auv balatro --help\n\nUse `auv plugin list` to inspect external commands visible on PATH."
)]
struct RootArgs {
  /// Run a hidden repository development task.
  #[arg(long, value_enum, hide = true)]
  xtask: Option<Xtask>,

  #[command(subcommand)]
  command: Option<RootCommand>,
}

#[derive(Debug, Subcommand)]
enum RootCommand {
  /// Inspect local automation permissions and environment readiness.
  Doctor(DoctorArgs),

  /// Invoke one core computer-use capability and record its run.
  Invoke(InvokeArgs),

  /// Expose core AUV capabilities through MCP.
  Mcp(McpArgs),

  /// Inspect external auv-* command plugins visible on PATH.
  Plugin(PluginArgs),

  #[command(external_subcommand)]
  External(Vec<OsString>),
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Xtask {
  GenerateSwiftBridge,
}

pub fn parse_cli(arguments: &[String]) -> AuvResult<CliCommand> {
  parse_cli_os(arguments.iter().map(OsString::from))
}

pub fn parse_cli_os(arguments: impl IntoIterator<Item = OsString>) -> AuvResult<CliCommand> {
  let arguments = arguments.into_iter().collect::<Vec<_>>();
  if arguments.is_empty() {
    return Ok(CliCommand::Help(help_text()));
  }

  let mut argv = Vec::with_capacity(arguments.len() + 1);
  argv.push(OsString::from("auv"));
  argv.extend(arguments);
  let parsed = match RootArgs::try_parse_from(argv) {
    Ok(parsed) => parsed,
    Err(error) => {
      return match error.kind() {
        ErrorKind::DisplayHelp => Ok(CliCommand::Help(error.to_string())),
        ErrorKind::DisplayVersion => Ok(CliCommand::Version),
        _ => Err(error.to_string()),
      };
    }
  };

  if let Some(xtask) = parsed.xtask {
    return match xtask {
      Xtask::GenerateSwiftBridge => Ok(CliCommand::XtaskGenerateSwiftBridge),
    };
  }

  match parsed.command {
    None => Ok(CliCommand::Help(help_text())),
    Some(RootCommand::Doctor(args)) => Ok(CliCommand::PermissionCheck { json: args.json }),
    Some(RootCommand::Invoke(args)) => parse_invoke(args.arguments),
    Some(RootCommand::Mcp(args)) => match args.command {
      McpCommand::Serve => Ok(CliCommand::McpServe),
    },
    Some(RootCommand::Plugin(args)) => match args.command {
      PluginCommand::List => Ok(CliCommand::PluginList),
    },
    Some(RootCommand::External(mut arguments)) => {
      let command_name = arguments.remove(0);
      Ok(CliCommand::External {
        command_name,
        arguments,
      })
    }
  }
}

pub fn help_text() -> String {
  RootArgs::command().render_long_help().to_string()
}

pub fn version_text() -> String {
  format!("auv {}\n", env!("CARGO_PKG_VERSION"))
}

fn parse_invoke(arguments: Vec<String>) -> AuvResult<CliCommand> {
  let mut invoke_arguments = vec!["invoke".to_string()];
  invoke_arguments.extend(arguments);

  match auv_cli_invoke::parse_invoke_args(&invoke_arguments)? {
    InvokeCliParse::Help { command_id } => Ok(CliCommand::InvokeHelp { command_id }),
    InvokeCliParse::Invoke {
      command_id,
      target_application_id,
      inputs,
      typed_args,
      store_root,
      dry_run,
      output,
    } => Ok(CliCommand::Invoke {
      request: InvokeRequest {
        command_id,
        target: ExecutionTarget {
          application_id: target_application_id,
        },
        inputs,
        dry_run,
      },
      typed_args,
      tracing: TracingOptions { store_root },
      output,
    }),
  }
}
