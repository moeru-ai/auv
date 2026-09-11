//! Typed root command tree and process-level routing for the core CLI.

use std::ffi::OsString;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum, error::ErrorKind};

use crate::commands::api_server::ApiServerArgs;
use crate::commands::devices::DevicesArgs;
use crate::commands::doctor::DoctorArgs;
use crate::commands::invoke::InvokeArgs;
use crate::commands::mcp::McpArgs;
use crate::commands::plugin::PluginArgs;
use crate::commands::run::RunArgs;
use crate::commands::runner::RunnerArgs;
use crate::commands::serve::ServeArgs;

#[derive(Debug, Parser)]
#[command(
  name = "auv",
  version,
  about = "Invoke and inspect core computer-use capabilities",
  long_about = "AUV turns computer-use operations into command-like, inspectable, and recorded runs.\n\nThe root CLI owns core invoke, doctor, API-server, Device/Run/Runner control, and MCP frontends. Installed auv-* executables extend it with application-owned commands.",
  after_long_help = "Examples:\n  # Inspect available core invoke commands\n  auv invoke --help\n\n  # Diagnose local automation readiness\n  auv doctor\n\n  # Run an installed application plugin\n  auv balatro --help\n\nUse `auv plugin list` to inspect external commands visible on PATH."
)]
struct RootArgs {
  /// Increase connection and RPC diagnostics. Repeat for more detail.
  #[arg(short, long, action = clap::ArgAction::Count, global = true)]
  verbose: u8,

  /// Run a hidden repository development task.
  #[arg(long, value_enum, hide = true)]
  xtask: Option<Xtask>,

  /// Select a Device by its human-facing name for this invocation.
  #[arg(long, value_name = "NAME")]
  device: Option<String>,

  /// Select a Device by its stable ID for this invocation.
  #[arg(long, value_name = "ID")]
  device_id: Option<String>,

  /// Append this invocation to an existing Run.
  #[arg(long, value_name = "ID")]
  run: Option<String>,

  #[command(subcommand)]
  command: Option<RootCommand>,
}

#[derive(Debug, Subcommand)]
enum RootCommand {
  /// Inspect local automation permissions and environment readiness.
  Doctor(DoctorArgs),
  /// Invoke one core computer-use capability and record its run.
  Invoke(InvokeArgs),
  /// Run the AUV API server.
  #[command(hide = true)]
  ApiServer(ApiServerArgs),
  /// Run the AUV daemon in the foreground.
  Serve(ServeArgs),
  /// Inspect Devices visible through an AUV daemon.
  #[command(
    long_about = "Devices are AUV execution targets exposed by a daemon. A local daemon publishes this machine as a Device; pairing adds another daemon as a remotely selectable Device.\n\n`auv devices list` combines the local daemon with saved paired profiles and reports whether each target is online. Pairing credentials stay in the local profile store and are reused automatically by later commands."
  )]
  Devices(DevicesArgs),
  /// Create and inspect daemon-owned Runners.
  #[command(visible_alias = "runners")]
  Runner(RunnerArgs),
  /// Create and inspect Run correlation scopes.
  Run(RunArgs),
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

pub async fn run_root() -> Result<i32, String> {
  let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
  init_diagnostics(verbosity(&arguments));
  run_os(arguments).await
}

async fn run_os(arguments: Vec<OsString>) -> Result<i32, String> {
  if arguments.is_empty() {
    print!("{}", help_text());
    return Ok(0);
  }
  let mut argv = Vec::with_capacity(arguments.len() + 1);
  argv.push(OsString::from("auv"));
  argv.extend(arguments);
  let parsed = match RootArgs::try_parse_from(argv) {
    Ok(parsed) => parsed,
    Err(error) => {
      return match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
          print!("{error}");
          Ok(0)
        }
        _ => Err(error.to_string()),
      };
    }
  };
  let selection = auv::selection::RootSelection {
    device_name: parsed.device,
    device_id: parsed.device_id,
    run_id: parsed.run,
  };
  let project_root = std::env::current_dir().map_err(|error| format!("failed to resolve current directory: {error}"))?;
  if let Some(Xtask::GenerateSwiftBridge) = parsed.xtask {
    let outputs = crate::xtask::generate_swift_bridge_for_ide(&project_root)?;
    println!("generated Swift bridge files for IDE indexing");
    for output in outputs {
      println!("output: {output}");
    }
    return Ok(0);
  }
  match parsed.command {
    None => {
      print!("{}", help_text());
      Ok(0)
    }
    Some(RootCommand::Doctor(args)) => crate::commands::doctor::run(args).await,
    Some(RootCommand::Invoke(args)) => crate::commands::invoke::run(args, &selection, &project_root).await,
    Some(RootCommand::ApiServer(args)) => crate::commands::api_server::run(args, &project_root).await,
    Some(RootCommand::Serve(args)) => crate::commands::serve::run(args, &project_root).await,
    Some(RootCommand::Devices(args)) => crate::commands::devices::run(args, &selection).await,
    Some(RootCommand::Runner(args)) => crate::commands::runner::run(args, &selection).await,
    Some(RootCommand::Run(args)) => crate::commands::run::run(args, &selection).await,
    Some(RootCommand::Mcp(args)) => crate::commands::mcp::run(args, &project_root).await,
    Some(RootCommand::Plugin(args)) => crate::commands::plugin::run(args).await,
    Some(RootCommand::External(mut arguments)) => {
      let command_name = arguments.remove(0);
      crate::commands::plugin::execute(&command_name, &arguments, &selection, &project_root).await
    }
  }
}

pub fn exit_status(result: Result<i32, String>) -> i32 {
  match result {
    Ok(exit_code) => exit_code,
    Err(error) => {
      eprintln!("error: {error}");
      1
    }
  }
}

fn verbosity(arguments: &[OsString]) -> u8 {
  arguments.iter().fold(0_u8, |count, argument| {
    let Some(argument) = argument.to_str() else {
      return count;
    };
    if argument == "--verbose" {
      count.saturating_add(1)
    } else if argument.starts_with('-') && !argument.starts_with("--") && argument[1..].bytes().all(|byte| byte == b'v') {
      count.saturating_add(u8::try_from(argument.len() - 1).unwrap_or(u8::MAX))
    } else {
      count
    }
  })
}

fn init_diagnostics(verbosity: u8) {
  let level = match verbosity {
    0 => return,
    1 => "info",
    2 => "debug",
    _ => "trace",
  };
  // Keep dependency-level HTTP/gRPC tracing disabled: it may include request
  // metadata or bodies containing bootstrap and bearer credentials.
  let filter = tracing_subscriber::EnvFilter::new(format!("off,auv_cli={level}"));
  let _ = tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).with_target(false).try_init();
}

pub fn help_text() -> String {
  RootArgs::command().render_long_help().to_string()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn root_selection_remains_unresolved_after_parsing() {
    let parsed = RootArgs::try_parse_from([
      "auv",
      "--device-id",
      "abc",
      "--run",
      "def",
      "invoke",
      "display.list",
    ])
    .unwrap();
    assert_eq!(parsed.device_id.as_deref(), Some("abc"));
    assert_eq!(parsed.run.as_deref(), Some("def"));
  }

  #[test]
  fn serve_accepts_repeated_runner_provider_manifests() {
    let parsed = RootArgs::try_parse_from([
      "auv",
      "serve",
      "--runner-provider",
      "first.json",
      "--runner-provider",
      "second.json",
    ])
    .unwrap();
    let Some(RootCommand::Serve(args)) = parsed.command else {
      panic!("serve command")
    };
    assert_eq!(
      args.runner_providers,
      [
        std::path::PathBuf::from("first.json"),
        std::path::PathBuf::from("second.json")
      ]
    );
  }
}
