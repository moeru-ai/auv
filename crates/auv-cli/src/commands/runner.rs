use clap::{Args, Subcommand, ValueEnum};
use std::str::FromStr;
use std::time::Duration;

use auv_cli_common::{
  TableRow,
  outputs::formats::table::{self, TableOptions},
};

#[derive(Clone, Debug, Args)]
pub struct RunnerArgs {
  #[command(subcommand)]
  pub command: RunnerCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum RunnerCommand {
  /// Create a daemon-owned Runner from a trusted RunnerClass.
  Create(CreateRunnerArgs),
  /// List daemon-owned Runners.
  #[command(visible_alias = "ls")]
  List(ListRunnersArgs),
  /// List trusted RunnerClasses registered by the selected Device.
  Classes(ListRunnersArgs),
  /// Get one Runner.
  Get(GetRunnerArgs),
  /// Stop and reap one Runner process.
  #[command(visible_alias = "delete")]
  Stop(StopRunnerArgs),
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum LifecycleArg {
  Ephemeral,
  UnlessIdle,
  UnlessShutdown,
}

#[derive(Clone, Debug, Args)]
pub struct CreateRunnerArgs {
  #[arg(long = "class")]
  pub runner_class: String,
  #[arg(long, value_enum, default_value = "unless-shutdown")]
  pub lifecycle: LifecycleArg,
  #[arg(long)]
  pub endpoint: Option<String>,
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct ListRunnersArgs {
  #[arg(long)]
  pub endpoint: Option<String>,
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct GetRunnerArgs {
  pub runner_id: String,
  #[arg(long)]
  pub endpoint: Option<String>,
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct StopRunnerArgs {
  pub runner_id: String,
  /// Maximum seconds to wait before terminating a daemon-owned executable.
  /// Without this option, graceful draining has no deadline.
  #[arg(long, value_name = "SECONDS", conflicts_with = "force")]
  pub timeout: Option<u64>,
  /// Immediately terminate a daemon-owned executable. Remote endpoints are
  /// only detached and are never terminated by AUV.
  #[arg(long)]
  pub force: bool,
  #[arg(long)]
  pub endpoint: Option<String>,
  #[arg(long)]
  pub json: bool,
}

#[derive(TableRow)]
struct RunnerClassTableRow {
  #[table(header = "CLASS")]
  runner_class: String,
  name: String,
  available: bool,
  #[table(header = "DEVICE ID")]
  device_id: Option<String>,
  lifecycles: String,
}

#[derive(TableRow)]
struct RunnerTableRow {
  #[table(header = "RUNNER ID")]
  runner_id: String,
  class: String,
  phase: String,
  pid: Option<u32>,
  #[table(header = "DEVICE ID")]
  device_id: Option<String>,
  lifecycle: String,
  #[table(header = "OPERATIONS")]
  active_operations: u64,
}

pub async fn run(args: RunnerArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  match args.command {
    RunnerCommand::Create(args) => create(args, selection).await,
    RunnerCommand::List(args) => list(args, selection).await,
    RunnerCommand::Classes(args) => classes(args, selection).await,
    RunnerCommand::Get(args) => get(args, selection).await,
    RunnerCommand::Stop(args) => stop(args, selection).await,
  }
}

async fn create(args: CreateRunnerArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  let (client, resolved) = auv::Client::selected(args.endpoint.as_deref(), selection)
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "no AUV daemon was discovered".to_string())?;
  let runner = client
    .runners()
    .create(auv::runners::CreateRunner {
      device: resolved.device.map(|device| device.id),
      class: auv::resource::RunnerClassId::from_str(&args.runner_class).map_err(|error| error.to_string())?,
      labels: Default::default(),
      lifecycle: lifecycle(args.lifecycle),
      idle_timeout: None,
    })
    .await
    .map_err(|error| error.to_string())?;
  print_runner(&runner, args.json)?;
  Ok(0)
}

async fn list(args: ListRunnersArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  let Some((client, resolved)) = auv::Client::selected(args.endpoint.as_deref(), selection).await.map_err(|error| error.to_string())? else {
    if args.json {
      println!("[]")
    } else {
      print_table(&Vec::<RunnerTableRow>::new(), "(no runners)")
    }
    return Ok(0);
  };
  let mut runners = client.runners().list().await.map_err(|error| error.to_string())?;
  if let Some(device) = resolved.device {
    runners.retain(|runner| runner.device == device.id);
  }
  if args.json {
    println!("{}", serde_json::to_string_pretty(&runners.iter().map(runner_json).collect::<Vec<_>>()).map_err(|error| error.to_string())?);
  } else {
    print_table(&runners.iter().map(runner_table_row).collect::<Vec<_>>(), "(no runners)");
  }
  Ok(0)
}

async fn classes(args: ListRunnersArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  let Some((client, resolved)) = auv::Client::selected(args.endpoint.as_deref(), selection).await.map_err(|error| error.to_string())? else {
    if args.json {
      println!("[]")
    } else {
      print_table(&Vec::<RunnerClassTableRow>::new(), "(no runner classes)")
    }
    return Ok(0);
  };
  let classes = client.runners().classes(resolved.device.as_ref().map(|device| &device.id)).await.map_err(|error| error.to_string())?;
  if args.json {
    println!(
      "{}",
      serde_json::to_string_pretty(&classes.iter().map(runner_class_json).collect::<Vec<_>>()).map_err(|error| error.to_string())?
    );
  } else {
    print_table(&classes.iter().map(runner_class_table_row).collect::<Vec<_>>(), "(no runner classes)");
  }
  Ok(0)
}

async fn get(args: GetRunnerArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  let (client, resolved) = auv::Client::selected(args.endpoint.as_deref(), selection)
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "no AUV daemon was discovered".to_string())?;
  let selector = auv::resource::RunnerSelector::parse(&args.runner_id).map_err(|error| error.to_string())?;
  let runner =
    client.runners().get(&selector, resolved.device.as_ref().map(|device| &device.id)).await.map_err(|error| error.to_string())?;
  print_runner(&runner, args.json)?;
  Ok(0)
}

async fn stop(args: StopRunnerArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  let (client, resolved) = auv::Client::selected(args.endpoint.as_deref(), selection)
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "no AUV daemon was discovered".to_string())?;
  let selector = auv::resource::RunnerSelector::parse(&args.runner_id).map_err(|error| error.to_string())?;
  let runner = client
    .runners()
    .stop(
      &selector,
      resolved.device.as_ref().map(|device| &device.id),
      auv::runners::StopRunner {
        grace_period: args.timeout.map(Duration::from_secs),
        force: args.force,
      },
    )
    .await
    .map_err(|error| error.to_string())?;
  print_runner(&runner, args.json)?;
  Ok(0)
}

fn lifecycle(value: LifecycleArg) -> auv::runners::RunnerLifecycle {
  match value {
    LifecycleArg::Ephemeral => auv::runners::RunnerLifecycle::Ephemeral,
    LifecycleArg::UnlessIdle => auv::runners::RunnerLifecycle::UnlessIdle,
    LifecycleArg::UnlessShutdown => auv::runners::RunnerLifecycle::UnlessShutdown,
  }
}

fn print_runner(runner: &auv::runners::Runner, json: bool) -> Result<(), String> {
  if json {
    println!("{}", serde_json::to_string_pretty(&runner_json(runner)).map_err(|error| error.to_string())?);
  } else {
    print_table(&[runner_table_row(runner)], "(no runner)");
  }
  Ok(())
}

fn runner_table_row(runner: &auv::runners::Runner) -> RunnerTableRow {
  RunnerTableRow {
    runner_id: runner.id.short(),
    class: runner.class.to_string(),
    phase: runner_phase_name(runner.phase, false),
    pid: runner.process_id,
    device_id: Some(runner.device.short()),
    lifecycle: lifecycle_name(runner.lifecycle, false),
    active_operations: runner.active_operations,
  }
}

fn runner_json(runner: &auv::runners::Runner) -> serde_json::Value {
  serde_json::json!({
    "runner_id": runner.id.as_str(),
    "device_id": runner.device.as_str(),
    "runner_class": runner.class.as_str(),
    "phase": runner_phase_name(runner.phase, true),
    "process_id": runner.process_id.unwrap_or(0),
    "labels": runner.labels,
  })
}

fn runner_class_table_row(class: &auv::runners::RunnerClass) -> RunnerClassTableRow {
  RunnerClassTableRow {
    runner_class: class.id.to_string(),
    name: class.display_name.clone(),
    available: class.available,
    device_id: class.device.as_ref().map(auv::resource::DeviceId::short),
    lifecycles: class.supported_lifecycles.iter().map(|value| lifecycle_name(*value, false)).collect::<Vec<_>>().join(","),
  }
}

fn runner_class_json(class: &auv::runners::RunnerClass) -> serde_json::Value {
  serde_json::json!({
    "runner_class": class.id.as_str(),
    "device_id": class.device.as_ref().map(auv::resource::DeviceId::as_str),
    "display_name": class.display_name,
    "available": class.available,
    "supported_lifecycles": class.supported_lifecycles.iter().map(|value| lifecycle_name(*value, true)).collect::<Vec<_>>(),
  })
}

fn runner_phase_name(value: auv::runners::RunnerPhase, wire: bool) -> String {
  let name = match value {
    auv::runners::RunnerPhase::Unspecified => "UNSPECIFIED",
    auv::runners::RunnerPhase::Starting => "STARTING",
    auv::runners::RunnerPhase::Ready => "READY",
    auv::runners::RunnerPhase::Draining => "DRAINING",
    auv::runners::RunnerPhase::Stopped => "STOPPED",
    auv::runners::RunnerPhase::Failed => "FAILED",
  };
  if wire {
    format!("RUNNER_PHASE_{name}")
  } else {
    name.to_ascii_lowercase()
  }
}

fn lifecycle_name(value: auv::runners::RunnerLifecycle, wire: bool) -> String {
  let name = match value {
    auv::runners::RunnerLifecycle::Ephemeral => "EPHEMERAL",
    auv::runners::RunnerLifecycle::UnlessIdle => "UNLESS_IDLE",
    auv::runners::RunnerLifecycle::UnlessShutdown => "UNLESS_SHUTDOWN",
  };
  if wire {
    format!("RUNNER_LIFECYCLE_{name}")
  } else {
    name.to_ascii_lowercase().replace('_', "-")
  }
}

fn print_table<R: table::TableRow>(rows: &[R], empty_message: &'static str) {
  println!("{}", table::render(rows, TableOptions::default().empty_message(empty_message)));
}
