use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::commands::pairing::PairingArgs;

#[derive(Clone, Debug, Args)]
pub struct DevicesArgs {
  #[command(subcommand)]
  pub command: DevicesCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum DevicesCommand {
  /// List Devices visible through the selected daemon.
  #[command(alias = "ls")]
  List(DeviceListArgs),
  /// Get one Device by stable ID.
  Get(DeviceGetArgs),
  /// Establish or administer paired Device trust.
  Pair(PairingArgs),
  /// Remove a paired Device trust relationship.
  Unpair(DeviceTrustArgs),
  /// Enable a paired Device trust relationship.
  Enable(DeviceTrustArgs),
  /// Disable a paired Device trust relationship without deleting its history.
  Disable(DeviceTrustArgs),
  /// Manage configured paired Device profiles (provisional).
  Profiles(DeviceProfilesArgs),
}

#[derive(Clone, Debug, Args)]
pub struct DeviceTrustArgs {
  /// Stable Device ID or human-facing Device name.
  pub device: String,
  /// Durable pairing-store path used by the remote API server.
  #[arg(long, value_name = "PATH")]
  pub store: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
pub struct DeviceProfilesArgs {
  #[command(subcommand)]
  pub command: DeviceProfilesCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum DeviceProfilesCommand {
  List(ProfileOutputArgs),
  Get(ProfileGetArgs),
  Create(ProfileWriteArgs),
  Update(ProfileWriteArgs),
  Delete(ProfileDeleteArgs),
}

#[derive(Clone, Debug, Args)]
pub struct ProfileOutputArgs {
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct ProfileGetArgs {
  pub name: String,
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct ProfileDeleteArgs {
  pub name: String,
}

#[derive(Clone, Debug, Args)]
pub struct ProfileWriteArgs {
  pub name: String,
  #[arg(long)]
  pub device_id: String,
  #[arg(long)]
  pub device_name: String,
  #[arg(long)]
  pub endpoint: String,
  #[arg(long)]
  pub server_name: String,
  #[arg(long)]
  pub credential_profile: String,
  #[arg(long, value_name = "ABSOLUTE_PATH", requires_all = ["client_certificate", "client_private_key"])]
  pub server_ca_certificate: Option<PathBuf>,
  #[arg(long, value_name = "ABSOLUTE_PATH", requires_all = ["server_ca_certificate", "client_private_key"])]
  pub client_certificate: Option<PathBuf>,
  #[arg(long, value_name = "ABSOLUTE_PATH", requires_all = ["server_ca_certificate", "client_certificate"])]
  pub client_private_key: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
pub struct DeviceListArgs {
  /// Override daemon discovery with an explicit endpoint.
  #[arg(long, value_name = "URI")]
  pub endpoint: Option<String>,
  /// Render machine-readable JSON.
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct DeviceGetArgs {
  /// Stable Device ID.
  pub device_id: String,
  /// Override daemon discovery with an explicit endpoint.
  #[arg(long, value_name = "URI")]
  pub endpoint: Option<String>,
  /// Render machine-readable JSON.
  #[arg(long)]
  pub json: bool,
}
