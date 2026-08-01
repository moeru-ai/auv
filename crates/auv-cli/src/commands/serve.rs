use std::path::PathBuf;

use clap::Args;

/// Run the AUV daemon API in the foreground.
#[derive(Clone, Debug, Args)]
pub struct ServeArgs {
  /// Listener URI. May be repeated with unix://, loopback http://, or paired
  /// https://IP:PORT. HTTPS listeners require the four TLS/pairing options.
  #[arg(long = "listen", value_name = "URI")]
  pub listeners: Vec<String>,

  /// PEM certificate presented by every paired HTTPS listener.
  #[arg(long, value_name = "PATH")]
  pub tls_certificate: Option<PathBuf>,

  /// PEM private key for the HTTPS server certificate.
  #[arg(long, value_name = "PATH")]
  pub tls_private_key: Option<PathBuf>,

  /// PEM CA certificate used to verify paired client certificates.
  #[arg(long, value_name = "PATH")]
  pub client_ca_certificate: Option<PathBuf>,

  /// Durable paired-device authority store used by HTTPS listeners.
  #[arg(long, value_name = "PATH")]
  pub pairing_store: Option<PathBuf>,

  /// Root directory used for daemon control state and recorded runs.
  #[arg(long, value_name = "PATH")]
  pub store_root: Option<PathBuf>,

  /// Publish daemon discovery metadata at this path.
  #[arg(long, value_name = "PATH", conflicts_with = "no_discovery")]
  pub discovery_file: Option<PathBuf>,

  /// Do not publish this foreground daemon for implicit client discovery.
  #[arg(long)]
  pub no_discovery: bool,

  /// Stop the daemon after this many seconds without live Runners.
  #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..))]
  pub daemon_idle_timeout: Option<u64>,

  /// Load an operator-trusted custom Runner provider manifest. May be repeated.
  #[arg(long = "runner-provider", value_name = "PATH")]
  pub runner_providers: Vec<PathBuf>,
}
