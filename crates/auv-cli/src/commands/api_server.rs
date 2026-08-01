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
    after_long_help = "Examples:\n  # Serve on the default loopback address\n  auv api-server serve\n\n  # Use a local Unix domain socket\n  auv api-server serve --unix-socket .auv/auv.sock\n\n  # Use an explicit loopback address and run store\n  auv api-server serve --host 127.0.0.1 --port 50051 --store-root .auv/runs\n\n  # Serve paired clients over mutual TLS\n  auv api-server serve --remote-listen 0.0.0.0 --port 9847 --tls-certificate server.pem --tls-private-key server-key.pem --client-ca-certificate client-ca.pem --pairing-store .auv/pairings.json --no-discovery"
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
  #[arg(long, default_value = auv_api_server::transport::DEFAULT_API_HOST)]
  pub host: String,

  /// TCP port on which the API server listens.
  #[arg(long, default_value_t = auv_api_server::transport::DEFAULT_API_PORT)]
  pub port: u16,

  /// Explicit IP interface for paired remote clients over mutual TLS.
  #[arg(long, value_name = "IP")]
  pub remote_listen: Option<String>,

  /// PEM certificate presented by a remote TLS server.
  #[arg(long, value_name = "PATH", requires = "remote_listen")]
  pub tls_certificate: Option<PathBuf>,

  /// PEM private key for the remote TLS server certificate.
  #[arg(long, value_name = "PATH", requires = "remote_listen")]
  pub tls_private_key: Option<PathBuf>,

  /// PEM CA certificate used to verify paired client certificates.
  #[arg(long, value_name = "PATH", requires = "remote_listen")]
  pub client_ca_certificate: Option<PathBuf>,

  /// Durable paired-device authority store.
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
