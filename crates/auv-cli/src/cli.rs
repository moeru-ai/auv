//! Typed root command tree and process-level routing for the core CLI.

use std::ffi::OsString;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum, error::ErrorKind};

use crate::commands::devices::DevicesArgs;
use crate::commands::doctor::DoctorArgs;
use crate::commands::invoke::InvokeArgs;
use crate::commands::mcp::McpArgs;
use crate::commands::op::OpArgs;
use crate::commands::plugin::PluginArgs;
use crate::commands::run::RunArgs;
use crate::commands::runner::RunnerArgs;
use crate::commands::serve::ServeArgs;
use crate::commands::setup::SetupArgs;

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
  /// Run the AUV daemon in the foreground.
  Serve(ServeArgs),
  /// Manage host-level support required by AUV.
  Setup(SetupArgs),
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
  /// Run compiled mode-aware operations.
  Op(OpArgs),
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
  let verbosity = verbosity(&argv[1..]);
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
  // Only the daemon is long-lived enough to need a log file; its Runners open
  // their own in `runner::run_if_internal`.
  let log_file = matches!(parsed.command, Some(RootCommand::Serve(_))).then_some("daemon");
  crate::logging::init(verbosity, log_file);
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
    Some(RootCommand::Serve(args)) => crate::commands::serve::run(args, &project_root).await,
    Some(RootCommand::Setup(args)) => {
      if selection.device_name.is_some() || selection.device_id.is_some() || selection.run_id.is_some() {
        return Err("setup cannot use --device, --device-id, or --run".to_string());
      }

      crate::commands::setup::run(args)
    }
    Some(RootCommand::Devices(args)) => crate::commands::devices::run(args, &selection, &project_root).await,
    Some(RootCommand::Runner(args)) => crate::commands::runner::run(args, &selection).await,
    Some(RootCommand::Run(args)) => crate::commands::run::run(args, &selection).await,
    Some(RootCommand::Mcp(args)) => crate::commands::mcp::run(args, &selection, &project_root).await,
    Some(RootCommand::Op(args)) => crate::commands::op::run(args).await,
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

  fn access_command(arguments: &[&str]) -> crate::commands::devices::AccessCommand {
    let parsed = RootArgs::try_parse_from(arguments).unwrap();
    let Some(RootCommand::Devices(args)) = parsed.command else {
      panic!("devices command")
    };
    let crate::commands::devices::DevicesCommand::Access(args) = args.command else {
      panic!("devices access command")
    };
    args.command
  }

  #[test]
  fn devices_access_enrollments_add_accepts_only_terminal_entered_credential() {
    let crate::commands::devices::AccessCommand::Enrollments(args) = access_command(&[
      "auv",
      "devices",
      "access",
      "enrollments",
      "--store-root",
      "state",
      "add",
      "--user",
      "neko",
    ]) else {
      panic!("enrollments command")
    };

    assert_eq!(args.store.store_root.as_deref(), Some(std::path::Path::new("state")));
    assert!(matches!(args.command, crate::commands::device_local::EnrollmentsCommand::Add { .. }));
    assert!(
      RootArgs::try_parse_from([
        "auv",
        "devices",
        "access",
        "enrollments",
        "add",
        "--user",
        "neko",
        "--credential",
        "secret"
      ])
      .is_err()
    );
  }

  #[test]
  fn devices_access_enrollments_add_takes_no_credential_kind() {
    // Every platform host accepts exactly one credential kind and derives it,
    // so a `--kind` flag would only offer a choice with one valid answer.
    assert!(
      RootArgs::try_parse_from([
        "auv",
        "devices",
        "access",
        "enrollments",
        "add",
        "--user",
        "neko",
        "--kind",
        "os-password"
      ])
      .is_err()
    );
  }

  #[test]
  fn devices_access_store_root_is_accepted_after_the_subcommand() {
    // `--store-root` must be discoverable from the leaf command's help, so it
    // is global within each target-local command group.
    let crate::commands::devices::AccessCommand::Enrollments(args) = access_command(&[
      "auv",
      "devices",
      "access",
      "enrollments",
      "list",
      "--store-root",
      "/srv/auv/store",
    ]) else {
      panic!("enrollments command")
    };

    assert_eq!(args.store.store_root.as_deref(), Some(std::path::Path::new("/srv/auv/store")));
    assert!(matches!(args.command, crate::commands::device_local::EnrollmentsCommand::List));
  }

  #[test]
  fn devices_access_groups_sessions_enrollments_policy_and_audit() {
    use crate::commands::devices::{AccessCommand, SessionsCommand};

    let sessions = |arguments: &[&str]| {
      let AccessCommand::Sessions(args) = access_command(arguments) else {
        panic!("sessions command")
      };
      args.command
    };

    assert!(matches!(sessions(&["auv", "devices", "access", "sessions", "list", "--json"]), SessionsCommand::List(_)));
    let SessionsCommand::Lock(lock) = sessions(&[
      "auv", "devices", "access", "sessions", "lock", "--user", "neko",
    ]) else {
      panic!("sessions lock")
    };
    assert_eq!(lock.user.as_deref(), Some("neko"));
    let SessionsCommand::Unlock(unlock) = sessions(&[
      "auv",
      "devices",
      "access",
      "sessions",
      "unlock",
      "--session",
      "s-1",
    ]) else {
      panic!("sessions unlock")
    };
    assert_eq!(unlock.session.as_deref(), Some("s-1"));

    let AccessCommand::Policy(policy) = access_command(&[
      "auv",
      "devices",
      "access",
      "policy",
      "disable",
      "--store-root",
      "state",
    ]) else {
      panic!("access policy")
    };
    assert!(matches!(policy.command, crate::commands::device_local::PolicyCommand::Disable));
    assert_eq!(policy.store.store_root.as_deref(), Some(std::path::Path::new("state")));

    let AccessCommand::Audit(audit) = access_command(&[
      "auv",
      "devices",
      "access",
      "audit",
      "list",
      "--limit",
      "20",
      "--page-token",
      "v1.42",
      "--json",
    ]) else {
      panic!("access audit")
    };
    assert!(audit.store.json);
    let crate::commands::device_local::AuditCommand::List { page_token, limit } = audit.command;
    assert_eq!((page_token.as_deref(), limit), (Some("v1.42"), 20));
    assert!(RootArgs::try_parse_from(["auv", "devices", "access", "audit", "list", "--cursor", "0"]).is_err());
    assert!(
      RootArgs::try_parse_from([
        "auv", "devices", "access", "audit", "list", "--limit", "101"
      ])
      .is_err()
    );

    // The former forms are gone, without aliases.
    for removed in [
      &["auv", "devices", "sessions", "list"][..],
      &["auv", "devices", "credentials", "list"][..],
      &["auv", "devices", "audit", "list"][..],
      &["auv", "devices", "lock", "--user", "neko"][..],
      &["auv", "devices", "unlock", "--user", "neko"][..],
      &[
        "auv",
        "devices",
        "access",
        "enrollments",
        "enroll",
        "--user",
        "neko",
      ][..],
    ] {
      assert!(RootArgs::try_parse_from(removed).is_err(), "{removed:?}");
    }
  }

  #[tokio::test]
  async fn devices_access_local_commands_reject_device_selection_before_connecting() {
    let selection = auv::selection::RootSelection {
      device_name: None,
      device_id: Some("7f3a".to_string()),
      run_id: None,
    };
    let parsed = RootArgs::try_parse_from(["auv", "devices", "access", "enrollments", "list"]).unwrap();
    let Some(RootCommand::Devices(args)) = parsed.command else {
      panic!("devices command")
    };
    let error = crate::commands::devices::run(args, &selection, std::path::Path::new(".")).await.unwrap_err();

    assert!(error.contains("runs only on this Device"), "{error}");
  }
  #[test]
  fn setup_parses_macos_helper_install() {
    let parsed = RootArgs::try_parse_from(["auv", "setup", "macos-helper", "install", "--json"]).unwrap();
    assert!(matches!(parsed.command, Some(RootCommand::Setup(_))));
  }

  #[test]
  fn setup_parses_macos_helper_uninstall() {
    let parsed = RootArgs::try_parse_from(["auv", "setup", "macos-helper", "uninstall", "--json"]).unwrap();
    assert!(matches!(parsed.command, Some(RootCommand::Setup(_))));
  }

  #[test]
  fn setup_parses_windows_helper_lifecycle() {
    for command in ["status", "install", "uninstall"] {
      let parsed = RootArgs::try_parse_from(["auv", "setup", "windows-helper", command, "--json"]).unwrap();
      assert!(matches!(parsed.command, Some(RootCommand::Setup(_))));
    }
  }

  #[test]
  fn pair_connect_accepts_token_stdin_without_token_argument() {
    let parsed = RootArgs::try_parse_from([
      "auv",
      "devices",
      "pair",
      "--endpoint",
      "http://127.0.0.1:9847",
      "connect",
      "--token-stdin",
      "--label",
      "MacBook",
    ]);
    assert!(parsed.is_ok());

    assert!(
      RootArgs::try_parse_from([
        "auv",
        "devices",
        "pair",
        "--endpoint",
        "http://127.0.0.1:9847",
        "connect",
        "--token",
        "secret",
        "--token-stdin",
        "--label",
        "MacBook",
      ])
      .is_err()
    );
  }
}
