use clap::{Args, Subcommand, ValueEnum};
use std::str::FromStr;

use auv_cli_common::{
  TableRow,
  outputs::formats::table::{self, TableOptions},
};

#[derive(Clone, Debug, Args)]
pub struct RunArgs {
  #[command(subcommand)]
  pub command: RunCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum RunCommand {
  /// Create an explicit Run correlation and control scope.
  Create(CreateRunArgs),
  /// List Runs visible to the current caller.
  #[command(visible_alias = "ls")]
  List(ListRunsArgs),
  /// Get one Run.
  Get(GetRunArgs),
  /// Finish a Run and release its daemon-internal Runner attachments.
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

#[derive(TableRow)]
struct RunTableRow {
  #[table(header = "RUN ID")]
  run_id: String,
  phase: String,
  #[table(header = "DEVICE IDS")]
  device_ids: String,
}

pub async fn run(args: RunArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  match args.command {
    RunCommand::Create(args) => create(args, selection).await,
    RunCommand::List(args) => list(args, selection).await,
    RunCommand::Get(args) => get(args, selection).await,
    RunCommand::Stop(args) => stop(args, selection).await,
  }
}

async fn create(args: CreateRunArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  if selection.run_id.is_some() {
    return Err("root --run cannot be combined with `auv run create`".to_string());
  }
  let (client, resolved) = auv::Client::selected(args.endpoint.as_deref(), selection)
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "no AUV daemon was discovered".to_string())?;
  let devices = if args.device_ids.is_empty() {
    resolved.device.into_iter().map(|device| device.id).collect()
  } else {
    args
      .device_ids
      .iter()
      .map(|value| auv::resource::DeviceId::from_str(value).map_err(|error| error.to_string()))
      .collect::<Result<_, _>>()?
  };
  let run = client
    .runs()
    .create(auv::runs::CreateRun {
      devices,
      labels: Default::default(),
    })
    .await
    .map_err(|error| error.to_string())?;
  print_run(&run, args.json)?;
  Ok(0)
}

async fn list(args: ListRunsArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  let Some((client, resolved)) = auv::Client::selected(args.endpoint.as_deref(), selection).await.map_err(|error| error.to_string())? else {
    if args.json {
      println!("[]");
    } else {
      print_table(&Vec::<RunTableRow>::new(), "(no runs)");
    }
    return Ok(0);
  };
  let mut runs = client.runs().list().await.map_err(|error| error.to_string())?;
  if let Some(device) = resolved.device {
    runs.retain(|run| run.devices.contains(&device.id));
  }
  if let Some(selected_run) = resolved.run {
    runs.retain(|run| run.id == selected_run.id);
  }
  if args.json {
    println!("{}", serde_json::to_string_pretty(&runs.iter().map(run_json).collect::<Vec<_>>()).map_err(|error| error.to_string())?);
  } else {
    print_table(&runs.iter().map(run_table_row).collect::<Vec<_>>(), "(no runs)");
  }
  Ok(0)
}

async fn get(args: GetRunArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  let (client, resolved) = auv::Client::selected(args.endpoint.as_deref(), selection)
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "no AUV daemon was discovered".to_string())?;
  let selector = auv::resource::RunSelector::parse(&args.run_id).map_err(|error| error.to_string())?;
  let run = client.runs().get(&selector).await.map_err(|error| error.to_string())?;
  run.validate_selection(resolved.run.as_ref()).map_err(|error| error.to_string())?;
  run.validate_device(resolved.device.as_ref().map(|device| &device.id)).map_err(|error| error.to_string())?;
  print_run(&run, args.json)?;
  Ok(0)
}

async fn stop(args: StopRunArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  let (client, resolved) = auv::Client::selected(args.endpoint.as_deref(), selection)
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "no AUV daemon was discovered".to_string())?;
  let selector = auv::resource::RunSelector::parse(&args.run_id).map_err(|error| error.to_string())?;
  let existing = client.runs().get(&selector).await.map_err(|error| error.to_string())?;
  existing.validate_selection(resolved.run.as_ref()).map_err(|error| error.to_string())?;
  existing.validate_device(resolved.device.as_ref().map(|device| &device.id)).map_err(|error| error.to_string())?;
  let outcome = match args.outcome {
    RunOutcomeArg::Succeeded => auv::runs::RunOutcome::Succeeded,
    RunOutcomeArg::Failed => auv::runs::RunOutcome::Failed,
    RunOutcomeArg::Canceled => auv::runs::RunOutcome::Canceled,
  };
  let canonical = auv::resource::RunSelector::parse(existing.id.as_str()).map_err(|error| error.to_string())?;
  let run = client.runs().stop(&canonical, outcome).await.map_err(|error| error.to_string())?;
  print_run(&run, args.json)?;
  Ok(0)
}

fn print_run(run: &auv::runs::Run, json: bool) -> Result<(), String> {
  if json {
    println!("{}", serde_json::to_string_pretty(&run_json(run)).map_err(|error| error.to_string())?);
  } else {
    print_table(&[run_table_row(run)], "(no run)");
  }
  Ok(())
}

fn run_table_row(run: &auv::runs::Run) -> RunTableRow {
  RunTableRow {
    run_id: run.id.short(),
    phase: run_phase_name(run.phase, false),
    device_ids: run.devices.iter().map(auv::resource::DeviceId::short).collect::<Vec<_>>().join(","),
  }
}

fn run_json(run: &auv::runs::Run) -> serde_json::Value {
  serde_json::json!({
    "run_id": run.id.as_str(),
    "phase": run_phase_name(run.phase, true),
    "device_ids": run.devices.iter().map(auv::resource::DeviceId::as_str).collect::<Vec<_>>(),
    "labels": run.labels,
    "created_at": run.created_at.map(|value| serde_json::json!({ "seconds": value.seconds, "nanos": value.nanos })),
  })
}

fn run_phase_name(phase: auv::runs::RunPhase, wire: bool) -> String {
  let name = match phase {
    auv::runs::RunPhase::Unspecified => "UNSPECIFIED",
    auv::runs::RunPhase::Pending => "PENDING",
    auv::runs::RunPhase::Running => "RUNNING",
    auv::runs::RunPhase::Succeeded => "SUCCEEDED",
    auv::runs::RunPhase::Failed => "FAILED",
    auv::runs::RunPhase::Canceled => "CANCELED",
  };
  if wire {
    format!("RUN_PHASE_{name}")
  } else {
    name.to_ascii_lowercase()
  }
}

fn print_table<R: table::TableRow>(rows: &[R], empty_message: &'static str) {
  println!("{}", table::render(rows, TableOptions::default().empty_message(empty_message)));
}
