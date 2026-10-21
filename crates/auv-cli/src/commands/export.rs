//! Exports recorded Runs into interchange formats.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};

/// Export a recorded Run from its trace stores.
#[derive(Clone, Debug, Args)]
pub struct ExportArgs {
  #[command(subcommand)]
  pub command: ExportCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum ExportCommand {
  /// Write one Run as an ATIF v1.8 trajectory with its screenshots.
  #[command(
    after_long_help = "Steps are the Run's invoke commands (CLI, MCP) and the Runner calls made outside any command (SDK). Each store's runner-traces/* sub-stores are read too, so pass the daemon's --store-root to include SDK calls.\n\nExamples:\n  # List the Runs recorded in the default store\n  auv export atif --list\n\n  # Export one Run\n  auv export atif --run 0192c3a8-7b1e-7cc0-9b4e-3f2a1d0c9e8b --out episode/trajectory"
  )]
  Atif(AtifArgs),
}

#[derive(Clone, Debug, Args)]
pub struct AtifArgs {
  /// Trace store to read; repeat for several [default: ./.auv/store].
  #[arg(long = "store", value_name = "PATH")]
  pub stores: Vec<PathBuf>,
  /// Run to export, as `auv invoke --json` or `auv run list` shows it; repeat
  /// to merge several, such as one Run per MCP call, into one trajectory.
  #[arg(long = "run", value_name = "ID")]
  pub run_ids: Vec<String>,
  /// Output directory for trajectory.json and images/.
  #[arg(long, value_name = "PATH", required_unless_present = "list")]
  pub out: Option<PathBuf>,
  /// List the Runs found in the stores instead of exporting.
  #[arg(long, conflicts_with_all = ["run_ids", "out"])]
  pub list: bool,
}

pub fn run(args: ExportArgs, selected_run: Option<&str>, project_root: &Path) -> Result<i32, String> {
  let ExportCommand::Atif(args) = args.command;
  let stores = if args.stores.is_empty() {
    vec![project_root.join(".auv").join("store")]
  } else {
    args
      .stores
      .iter()
      .map(|store| {
        if store.is_absolute() {
          store.clone()
        } else {
          project_root.join(store)
        }
      })
      .collect()
  };
  if args.list {
    for (run_id, started_at, spans) in crate::atif::runs(&stores)? {
      println!("{run_id}\t{}\t{spans} steps", started_at.unix_seconds());
    }
    return Ok(0);
  }
  let mut run_ids = args.run_ids;
  if run_ids.is_empty() {
    run_ids.extend(selected_run.map(str::to_string));
  }
  if run_ids.is_empty() {
    return Err("--run is required; list the recorded Runs with --list".to_string());
  }
  // Accepts the trace id `auv invoke --json` reports and the daemon id
  // `auv run list` shows: both name the same Run.
  let run_ids = run_ids
    .iter()
    .map(|run_id| auv_tracing::RunId::from_daemon_run_id(run_id).map_err(|error| format!("invalid Run id {run_id:?}: {error}")))
    .collect::<Result<Vec<_>, String>>()?;
  let out = args.out.ok_or("--out is required")?;
  let exported = crate::atif::export(&stores, &run_ids, &out)?;
  println!("trajectory: {}", exported.trajectory.display());
  println!("steps: {}", exported.steps);
  println!("images: {}", exported.images);
  if exported.skipped_records > 0 {
    println!("skipped records: {}", exported.skipped_records);
  }
  Ok(0)
}
