use clap::{Args, Subcommand, ValueEnum};

#[derive(Clone, Debug, Args)]
pub struct RunArgs {
  #[command(subcommand)]
  pub command: RunCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum RunCommand {
  /// Create an explicit Run correlation and control scope.
  Create(CreateRunArgs),
  /// List Runs visible to the current principal.
  #[command(visible_alias = "ls")]
  List(ListRunsArgs),
  /// Get one Run.
  Get(GetRunArgs),
  /// Finish a Run and release all Runner leases it owns.
  Stop(StopRunArgs),
}

#[derive(Clone, Debug, Args)]
pub struct CreateRunArgs {
  /// Place the Run on this Device ID; repeat to select several Devices.
  #[arg(long = "device-id")]
  pub device_ids: Vec<String>,
  #[arg(long)]
  pub endpoint: Option<String>,
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct ListRunsArgs {
  #[arg(long)]
  pub endpoint: Option<String>,
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct GetRunArgs {
  pub run_id: String,
  #[arg(long)]
  pub endpoint: Option<String>,
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum RunOutcomeArg {
  Succeeded,
  Failed,
  Canceled,
}

#[derive(Clone, Debug, Args)]
pub struct StopRunArgs {
  pub run_id: String,
  #[arg(long, value_enum, default_value = "canceled")]
  pub outcome: RunOutcomeArg,
  #[arg(long)]
  pub endpoint: Option<String>,
  #[arg(long)]
  pub json: bool,
}
