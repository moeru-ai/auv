//! Typed root command tree for the core `auv` frontend.

use std::ffi::OsString;
use std::path::PathBuf;

use auv_cli_invoke::{ExecutionTarget, InvokeCliParse, InvokeRequest};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum, error::ErrorKind};

use crate::commands::api_server::{ApiServerArgs, ApiServerCommand};
use crate::commands::devices::{DeviceProfilesCommand, DevicesArgs, DevicesCommand};
use crate::commands::doctor::DoctorArgs;
use crate::commands::invoke::InvokeArgs;
use crate::commands::mcp::{McpArgs, McpCommand};
use crate::commands::pairing::PairingCommand;
use crate::commands::plugin::{PluginArgs, PluginCommand};
use crate::commands::run::{RunArgs, RunCommand};
use crate::commands::runner::{LifecycleArg, RunnerArgs, RunnerCommand};
use crate::commands::serve::ServeArgs;

type AuvResult<T> = Result<T, String>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TracingOptions {
  pub store_root: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParentContextOptions {
  pub device_name: Option<String>,
  pub device_id: Option<String>,
  pub run_id: Option<String>,
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
    parent_context: ParentContextOptions,
  },
  ApiServerServe {
    host: String,
    port: u16,
    remote_listen: Option<String>,
    tls_certificate: Option<PathBuf>,
    tls_private_key: Option<PathBuf>,
    client_ca_certificate: Option<PathBuf>,
    pairing_store: Option<PathBuf>,
    #[cfg(unix)]
    unix_socket: Option<PathBuf>,
    store_root: Option<PathBuf>,
    discovery_file: Option<PathBuf>,
    no_discovery: bool,
    daemon_idle_timeout: Option<std::time::Duration>,
    runner_providers: Vec<PathBuf>,
  },
  Serve {
    listeners: Vec<String>,
    tls_certificate: Option<PathBuf>,
    tls_private_key: Option<PathBuf>,
    client_ca_certificate: Option<PathBuf>,
    pairing_store: Option<PathBuf>,
    store_root: Option<PathBuf>,
    discovery_file: Option<PathBuf>,
    no_discovery: bool,
    daemon_idle_timeout: Option<std::time::Duration>,
    runner_providers: Vec<PathBuf>,
  },
  DeviceList {
    endpoint: Option<String>,
    json: bool,
    parent_context: ParentContextOptions,
  },
  DeviceGet {
    endpoint: Option<String>,
    device_id: String,
    json: bool,
    parent_context: ParentContextOptions,
  },
  DeviceProfiles {
    command: DeviceProfilesCommand,
  },
  DeviceTrust {
    store: Option<PathBuf>,
    device: String,
    action: DeviceTrustAction,
  },
  RunnerCreate {
    endpoint: Option<String>,
    runner_class: String,
    lifecycle: i32,
    json: bool,
    parent_context: ParentContextOptions,
  },
  RunnerList {
    endpoint: Option<String>,
    json: bool,
    parent_context: ParentContextOptions,
  },
  RunnerClassList {
    endpoint: Option<String>,
    json: bool,
    parent_context: ParentContextOptions,
  },
  RunnerGet {
    endpoint: Option<String>,
    runner_id: String,
    json: bool,
    parent_context: ParentContextOptions,
  },
  RunnerStop {
    endpoint: Option<String>,
    runner_id: String,
    json: bool,
    parent_context: ParentContextOptions,
  },
  RunCreate {
    endpoint: Option<String>,
    device_ids: Vec<String>,
    json: bool,
    parent_context: ParentContextOptions,
  },
  RunList {
    endpoint: Option<String>,
    json: bool,
    parent_context: ParentContextOptions,
  },
  RunGet {
    endpoint: Option<String>,
    run_id: String,
    json: bool,
    parent_context: ParentContextOptions,
  },
  RunStop {
    endpoint: Option<String>,
    run_id: String,
    outcome: i32,
    json: bool,
    parent_context: ParentContextOptions,
  },
  Pairing {
    store: Option<PathBuf>,
    command: PairingCommand,
  },
  McpServe,
  PluginList,
  External {
    command_name: OsString,
    arguments: Vec<OsString>,
    parent_context: ParentContextOptions,
  },
  XtaskGenerateSwiftBridge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceTrustAction {
  Unpair,
  Enable,
  Disable,
}

#[derive(Debug, Parser)]
#[command(
  name = "auv",
  version,
  about = "Invoke and inspect core computer-use capabilities",
  long_about = "AUV turns computer-use operations into command-like, inspectable, and recorded runs.\n\nThe root CLI owns core invoke, doctor, API-server, Device/Run/Runner control, and MCP frontends. Installed auv-* executables extend it with application-owned commands.",
  after_long_help = "Examples:\n  # Inspect available core invoke commands\n  auv invoke --help\n\n  # Diagnose local automation readiness\n  auv doctor\n\n  # Run an installed application plugin\n  auv balatro --help\n\nUse `auv plugin list` to inspect external commands visible on PATH."
)]
struct RootArgs {
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

  let parent_context = ParentContextOptions {
    device_name: parsed.device,
    device_id: parsed.device_id,
    run_id: parsed.run,
  };

  if let Some(xtask) = parsed.xtask {
    return match xtask {
      Xtask::GenerateSwiftBridge => Ok(CliCommand::XtaskGenerateSwiftBridge),
    };
  }

  match parsed.command {
    None => Ok(CliCommand::Help(help_text())),
    Some(RootCommand::Doctor(args)) => Ok(CliCommand::PermissionCheck { json: args.json }),
    Some(RootCommand::Invoke(args)) => parse_invoke(args.arguments, parent_context),
    Some(RootCommand::ApiServer(args)) => match args.command {
      ApiServerCommand::Serve(args) => {
        let remote_paths = [
          args.tls_certificate.as_ref(),
          args.tls_private_key.as_ref(),
          args.client_ca_certificate.as_ref(),
          args.pairing_store.as_ref(),
        ];
        if args.remote_listen.is_some() && remote_paths.iter().any(|value| value.is_none()) {
          return Err(
            "--remote-listen requires --tls-certificate, --tls-private-key, --client-ca-certificate, and --pairing-store".to_string(),
          );
        }
        if args.remote_listen.is_some() && !args.no_discovery {
          // TODO(paired-discovery-profile): publish remote endpoints only after
          // discovery can name a client credential profile and trust roots.
          return Err(
            "--remote-listen requires --no-discovery because the current discovery descriptor has no credential profile".to_string(),
          );
        }
        #[cfg(unix)]
        if args.remote_listen.is_some() && args.unix_socket.is_some() {
          return Err("--remote-listen conflicts with --unix-socket".to_string());
        }
        Ok(CliCommand::ApiServerServe {
          host: args.host,
          port: args.port,
          remote_listen: args.remote_listen,
          tls_certificate: args.tls_certificate,
          tls_private_key: args.tls_private_key,
          client_ca_certificate: args.client_ca_certificate,
          pairing_store: args.pairing_store,
          #[cfg(unix)]
          unix_socket: args.unix_socket,
          store_root: args.store_root,
          discovery_file: args.discovery_file,
          no_discovery: args.no_discovery,
          daemon_idle_timeout: args.daemon_idle_timeout.map(std::time::Duration::from_secs),
          runner_providers: args.runner_providers,
        })
      }
    },
    Some(RootCommand::Serve(args)) => {
      let has_remote = args.listeners.iter().any(|listener| listener.starts_with("https://"));
      let remote_options = [
        args.tls_certificate.as_ref(),
        args.tls_private_key.as_ref(),
        args.client_ca_certificate.as_ref(),
        args.pairing_store.as_ref(),
      ];
      if has_remote && remote_options.iter().any(|option| option.is_none()) {
        return Err(
          "paired https:// listeners require --tls-certificate, --tls-private-key, --client-ca-certificate, and --pairing-store".to_string(),
        );
      }
      if !has_remote && remote_options.iter().any(|option| option.is_some()) {
        return Err("TLS/pairing options require at least one https:// --listen URI".to_string());
      }
      let has_local = args.listeners.iter().any(|listener| listener.starts_with("unix://") || listener.starts_with("http://"));
      if has_remote && !has_local && !args.no_discovery {
        return Err(
          "remote-only https:// listeners require --no-discovery because daemon discovery does not publish credential-free remote endpoints"
            .to_string(),
        );
      }
      Ok(CliCommand::Serve {
        listeners: args.listeners,
        tls_certificate: args.tls_certificate,
        tls_private_key: args.tls_private_key,
        client_ca_certificate: args.client_ca_certificate,
        pairing_store: args.pairing_store,
        store_root: args.store_root,
        discovery_file: args.discovery_file,
        no_discovery: args.no_discovery,
        daemon_idle_timeout: args.daemon_idle_timeout.map(std::time::Duration::from_secs),
        runner_providers: args.runner_providers,
      })
    }
    Some(RootCommand::Devices(args)) => match args.command {
      DevicesCommand::List(args) => Ok(CliCommand::DeviceList {
        endpoint: args.endpoint,
        json: args.json,
        parent_context,
      }),
      DevicesCommand::Get(args) => Ok(CliCommand::DeviceGet {
        endpoint: args.endpoint,
        device_id: args.device_id,
        json: args.json,
        parent_context,
      }),
      DevicesCommand::Pair(args) => Ok(CliCommand::Pairing {
        store: args.store,
        command: args.command,
      }),
      DevicesCommand::Unpair(args) => Ok(CliCommand::DeviceTrust {
        store: args.store,
        device: args.device,
        action: DeviceTrustAction::Unpair,
      }),
      DevicesCommand::Enable(args) => Ok(CliCommand::DeviceTrust {
        store: args.store,
        device: args.device,
        action: DeviceTrustAction::Enable,
      }),
      DevicesCommand::Disable(args) => Ok(CliCommand::DeviceTrust {
        store: args.store,
        device: args.device,
        action: DeviceTrustAction::Disable,
      }),
      DevicesCommand::Profiles(args) => Ok(CliCommand::DeviceProfiles {
        command: args.command,
      }),
    },
    Some(RootCommand::Runner(args)) => match args.command {
      RunnerCommand::Create(args) => Ok(CliCommand::RunnerCreate {
        endpoint: args.endpoint,
        runner_class: args.runner_class,
        lifecycle: match args.lifecycle {
          LifecycleArg::Ephemeral => auv_api_proto::auv::api::core::v1::RunnerLifecycle::Ephemeral as i32,
          LifecycleArg::UnlessIdle => auv_api_proto::auv::api::core::v1::RunnerLifecycle::UnlessIdle as i32,
          LifecycleArg::UnlessShutdown => auv_api_proto::auv::api::core::v1::RunnerLifecycle::UnlessShutdown as i32,
        },
        json: args.json,
        parent_context,
      }),
      RunnerCommand::List(args) => Ok(CliCommand::RunnerList {
        endpoint: args.endpoint,
        json: args.json,
        parent_context,
      }),
      RunnerCommand::Classes(args) => Ok(CliCommand::RunnerClassList {
        endpoint: args.endpoint,
        json: args.json,
        parent_context,
      }),
      RunnerCommand::Get(args) => Ok(CliCommand::RunnerGet {
        endpoint: args.endpoint,
        runner_id: args.runner_id,
        json: args.json,
        parent_context,
      }),
      RunnerCommand::Stop(args) => Ok(CliCommand::RunnerStop {
        endpoint: args.endpoint,
        runner_id: args.runner_id,
        json: args.json,
        parent_context,
      }),
    },
    Some(RootCommand::Run(args)) => match args.command {
      RunCommand::Create(args) => Ok(CliCommand::RunCreate {
        endpoint: args.endpoint,
        device_ids: args.device_ids,
        json: args.json,
        parent_context,
      }),
      RunCommand::List(args) => Ok(CliCommand::RunList {
        endpoint: args.endpoint,
        json: args.json,
        parent_context,
      }),
      RunCommand::Get(args) => Ok(CliCommand::RunGet {
        endpoint: args.endpoint,
        run_id: args.run_id,
        json: args.json,
        parent_context,
      }),
      RunCommand::Stop(args) => Ok(CliCommand::RunStop {
        endpoint: args.endpoint,
        run_id: args.run_id,
        outcome: match args.outcome {
          crate::commands::run::RunOutcomeArg::Succeeded => auv_api_proto::auv::api::core::v1::RunOutcome::Succeeded as i32,
          crate::commands::run::RunOutcomeArg::Failed => auv_api_proto::auv::api::core::v1::RunOutcome::Failed as i32,
          crate::commands::run::RunOutcomeArg::Canceled => auv_api_proto::auv::api::core::v1::RunOutcome::Canceled as i32,
        },
        json: args.json,
        parent_context,
      }),
    },
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
        parent_context,
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

fn parse_invoke(arguments: Vec<String>, parent_context: ParentContextOptions) -> AuvResult<CliCommand> {
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
      parent_context,
    }),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn invoke_preserves_root_device_and_run_selection() {
    let command = parse_cli(&[
      "--device-id".to_string(),
      "device_local".to_string(),
      "--run".to_string(),
      "run_parent".to_string(),
      "invoke".to_string(),
      "display.list".to_string(),
      "--json".to_string(),
    ])
    .expect("parse selected invoke command");

    let CliCommand::Invoke { parent_context, .. } = command else {
      panic!("expected invoke command");
    };
    assert_eq!(parent_context.device_id.as_deref(), Some("device_local"));
    assert_eq!(parent_context.run_id.as_deref(), Some("run_parent"));
  }

  #[test]
  fn serve_accepts_repeated_trusted_runner_provider_manifests() {
    let command = parse_cli(&[
      "serve".to_string(),
      "--runner-provider".to_string(),
      "first.json".to_string(),
      "--runner-provider".to_string(),
      "second.json".to_string(),
    ])
    .expect("parse Runner provider manifests");

    match command {
      CliCommand::Serve {
        runner_providers, ..
      } => {
        assert_eq!(runner_providers, vec![PathBuf::from("first.json"), PathBuf::from("second.json")]);
      }
      command => panic!("unexpected command: {command:?}"),
    }
  }

  #[test]
  fn serve_accepts_repeated_local_and_paired_tls_listeners() {
    let command = parse_cli(&[
      "serve".to_string(),
      "--listen".to_string(),
      "unix:///tmp/auv.sock".to_string(),
      "--listen".to_string(),
      "https://127.0.0.1:9847".to_string(),
      "--tls-certificate".to_string(),
      "server.pem".to_string(),
      "--tls-private-key".to_string(),
      "server-key.pem".to_string(),
      "--client-ca-certificate".to_string(),
      "client-ca.pem".to_string(),
      "--pairing-store".to_string(),
      "pairings.json".to_string(),
    ])
    .expect("parse local and paired listeners");

    let CliCommand::Serve {
      listeners,
      tls_certificate,
      pairing_store,
      ..
    } = command
    else {
      panic!("expected serve command");
    };
    assert_eq!(listeners, ["unix:///tmp/auv.sock", "https://127.0.0.1:9847"]);
    assert_eq!(tls_certificate, Some(PathBuf::from("server.pem")));
    assert_eq!(pairing_store, Some(PathBuf::from("pairings.json")));
  }

  #[test]
  fn serve_rejects_https_listener_without_complete_tls_authority() {
    let error = parse_cli(&[
      "serve".to_string(),
      "--listen".to_string(),
      "https://127.0.0.1:9847".to_string(),
      "--no-discovery".to_string(),
    ])
    .expect_err("HTTPS can never fall through to plaintext or local authority");
    assert!(error.contains("require --tls-certificate"));
  }
}
