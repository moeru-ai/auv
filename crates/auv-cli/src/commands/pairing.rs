use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

/// Provision paired remote-client certificates while the daemon is stopped.
#[derive(Clone, Debug, Args)]
pub struct PairingArgs {
  /// Durable pairing-store path used by the remote API server.
  #[arg(long, global = true, value_name = "PATH")]
  pub store: Option<PathBuf>,

  #[command(subcommand)]
  pub command: PairingCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum PairingCommand {
  /// List paired devices, scopes, and certificate fingerprints.
  #[command(alias = "ls")]
  List {
    /// Render the store as JSON.
    #[arg(long)]
    json: bool,
  },
  /// Enroll a new stable paired-device identity.
  Add {
    /// Stable pair ID. A UUIDv7 is generated when omitted.
    #[arg(long)]
    pair_id: Option<String>,
    /// Human-readable operator label; never used as identity.
    #[arg(long)]
    label: String,
    /// PEM leaf certificate to enroll.
    #[arg(long, value_name = "PATH")]
    certificate: PathBuf,
    /// Granted API scope. Repeat to grant multiple scopes.
    #[arg(long, value_enum, required = true)]
    scope: Vec<PairingScope>,
  },
  /// Add a replacement certificate to an existing stable pair.
  Rotate {
    pair_id: String,
    /// PEM leaf certificate to add without revoking older credentials.
    #[arg(long, value_name = "PATH")]
    certificate: PathBuf,
  },
  /// Replace a pair's authorization scopes.
  SetScopes {
    pair_id: String,
    /// Granted API scope. Repeat to grant multiple scopes.
    #[arg(long, value_enum, required = true)]
    scope: Vec<PairingScope>,
  },
  /// Enable a paired device.
  // NOTICE(pairing-cli-compat): Keep the pair-ID form for existing scripts;
  // resource-oriented callers should use `auv devices enable <device>`.
  Enable { pair_id: String },
  /// Disable a paired device without deleting its history.
  // NOTICE(pairing-cli-compat): Keep the pair-ID form for existing scripts;
  // resource-oriented callers should use `auv devices disable <device>`.
  Disable { pair_id: String },
  /// Revoke one enrolled leaf certificate.
  Revoke {
    /// PEM leaf certificate whose fingerprint will be revoked.
    #[arg(long, value_name = "PATH")]
    certificate: PathBuf,
  },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PairingScope {
  ControlInspect,
  ControlManage,
  OperationsExecute,
}

impl From<PairingScope> for auv_api_server::authority::ApiScope {
  fn from(value: PairingScope) -> Self {
    match value {
      PairingScope::ControlInspect => Self::ControlInspect,
      PairingScope::ControlManage => Self::ControlManage,
      PairingScope::OperationsExecute => Self::OperationsExecute,
    }
  }
}
