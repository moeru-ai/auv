use std::path::PathBuf;

use clap::{Args, Subcommand};

/// Run the AUV API server in the foreground.
#[derive(Clone, Debug, Args)]
pub struct ApiServerArgs {
  #[command(subcommand)]
  pub command: ApiServerCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum ApiServerCommand {
  /// Serve the AUV API until interrupted.
  #[command(
    after_long_help = "Examples:\n  # Serve on the default loopback address\n  auv api-server serve\n\n  # Use a local Unix domain socket\n  auv api-server serve --unix-socket .auv/auv.sock\n\n  # Expose a bearer-authenticated network listener\n  auv api-server serve --remote-listen 0.0.0.0 --port 9847 --pairing-store .auv/pairings.json"
  )]
  Serve(ApiServerServeArgs),
}

#[derive(Clone, Debug, Args)]
pub struct ApiServerServeArgs {
  /// Serve through this local Unix domain socket instead of loopback TCP.
  #[cfg(unix)]
  #[arg(long, value_name = "PATH", conflicts_with_all = ["host", "port"])]
  pub unix_socket: Option<PathBuf>,

  /// Host interface on which the API server listens.
  #[arg(long, default_value = auv_api_server::server::DEFAULT_API_HOST)]
  pub host: String,

  /// TCP port on which the API server listens.
  #[arg(long, default_value_t = auv_api_server::server::DEFAULT_API_PORT)]
  pub port: u16,

  /// IP interface for the bearer-authenticated network listener.
  #[arg(long, value_name = "IP")]
  pub remote_listen: Option<String>,

  /// Durable paired-device authentication store.
  #[arg(long, value_name = "PATH", requires = "remote_listen")]
  pub pairing_store: Option<PathBuf>,

  /// Root directory used for recorded run data.
  #[arg(long, value_name = "PATH")]
  pub store_root: Option<PathBuf>,

  /// Publish daemon discovery metadata at this path.
  #[arg(long, value_name = "PATH", conflicts_with = "no_discovery")]
  pub discovery_file: Option<PathBuf>,

  /// Do not publish this foreground server for implicit client discovery.
  #[arg(long)]
  pub no_discovery: bool,

  /// Stop the daemon after this many seconds with no live Runners.
  #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..))]
  pub daemon_idle_timeout: Option<u64>,

  /// Load an operator-trusted custom Runner provider manifest. May be repeated.
  #[arg(long = "runner-provider", value_name = "PATH")]
  pub runner_providers: Vec<PathBuf>,
}

pub async fn run(args: ApiServerArgs, project_root: &std::path::Path) -> Result<i32, String> {
  match args.command {
    ApiServerCommand::Serve(args) => serve(args, project_root).await,
  }
}

async fn serve(args: ApiServerServeArgs, project_root: &std::path::Path) -> Result<i32, String> {
  if args.remote_listen.is_some() && args.pairing_store.is_none() {
    return Err("--remote-listen requires --pairing-store".to_string());
  }
  #[cfg(unix)]
  if args.remote_listen.is_some() && args.unix_socket.is_some() {
    return Err("--remote-listen conflicts with --unix-socket".to_string());
  }
  let listener = if let Some(host) = args.remote_listen {
    auv_daemon::ListenEndpoint::Remote {
      host,
      port: args.port,
    }
  } else {
    #[cfg(unix)]
    if let Some(path) = args.unix_socket {
      auv_daemon::ListenEndpoint::Unix {
        path: if path.is_absolute() {
          path
        } else {
          project_root.join(path)
        },
      }
    } else {
      auv_daemon::ListenEndpoint::Tcp {
        host: args.host,
        port: args.port,
      }
    }
    #[cfg(not(unix))]
    auv_daemon::ListenEndpoint::Tcp {
      host: args.host,
      port: args.port,
    }
  };
  super::serve::run_listeners(
    super::serve::HostOptions {
      listeners: vec![listener],
      pairing_store: args.pairing_store,
      store_root: args.store_root,
      discovery_file: args.discovery_file,
      publish_discovery: !args.no_discovery,
      daemon_idle_timeout: args.daemon_idle_timeout,
      runner_providers: args.runner_providers,
    },
    project_root,
  )
  .await
}
