//! `auv-trace-atif`: writes recorded AUV Runs as ATIF v1.8 trajectories.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

/// Write recorded AUV Runs as one ATIF v1.8 trajectory with its screenshots.
#[derive(Debug, Parser)]
#[command(
  name = "auv-trace-atif",
  version,
  after_long_help = "Steps are the Runs' invoke commands (CLI, MCP) and the Runner calls made outside any command (SDK). Each store's runner-traces/* sub-stores are read too, so pass the daemon's --store-root to include SDK calls.\n\nExamples:\n  # List the Runs recorded in a store\n  auv-trace-atif --store .auv/store --list\n\n  # Export one Run\n  auv-trace-atif --store .auv/store --run 56490ab8191b99cef8e18e293fd0ae51 --out episode/trajectory"
)]
struct Args {
  /// Trace store to read; repeat for several [default: ./.auv/store].
  #[arg(long = "store", value_name = "PATH")]
  stores: Vec<PathBuf>,
  /// Run to export, as `auv invoke --json` or `auv run list` shows it; repeat
  /// to merge several, such as one Run per MCP call, into one trajectory.
  #[arg(long = "run", value_name = "ID", required_unless_present = "list")]
  run_ids: Vec<String>,
  /// Output directory for trajectory.json and images/.
  #[arg(long, value_name = "PATH", required_unless_present = "list")]
  out: Option<PathBuf>,
  /// List the Runs found in the stores instead of exporting.
  #[arg(long, conflicts_with_all = ["run_ids", "out"])]
  list: bool,
}

fn main() -> ExitCode {
  match run(Args::parse()) {
    Ok(()) => ExitCode::SUCCESS,
    Err(error) => {
      eprintln!("error: {error}");
      ExitCode::FAILURE
    }
  }
}

fn run(args: Args) -> Result<(), String> {
  let stores = if args.stores.is_empty() {
    vec![PathBuf::from(".auv").join("store")]
  } else {
    args.stores
  };
  if args.list {
    for (run_id, started_at, steps) in auv_trace_atif::runs(&stores)? {
      println!("{run_id}\t{}\t{steps} steps", started_at.unix_seconds());
    }
    return Ok(());
  }
  // Accepts the trace id `auv invoke --json` reports and the daemon id
  // `auv run list` shows: both name the same Run.
  let run_ids = args
    .run_ids
    .iter()
    .map(|run_id| auv_tracing::RunId::from_daemon_run_id(run_id).map_err(|error| format!("invalid Run id {run_id:?}: {error}")))
    .collect::<Result<Vec<_>, String>>()?;
  let out = args.out.ok_or("--out is required")?;
  let exported = auv_trace_atif::export(&stores, &run_ids, &out)?;
  println!("trajectory: {}", exported.trajectory.display());
  println!("steps: {}", exported.steps);
  println!("images: {}", exported.images);
  if exported.skipped_records > 0 {
    println!("skipped records: {}", exported.skipped_records);
  }
  Ok(())
}
