use std::env;
use std::path::PathBuf;

use auv_cli::balatro::run_balatro_consumption_probe_chain;
use auv_cli::build_runtime_with_store_root;
use auv_game_balatro::{ObjectZone, SlotId};

struct Args {
  bundle_input: PathBuf,
  expected_slots: PathBuf,
  work_dir: PathBuf,
  store_root: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let args = parse_args(env::args().skip(1).collect())?;
  let project_root = env::current_dir()?;
  let store_root = args
    .store_root
    .clone()
    .unwrap_or_else(|| project_root.join(".auv"));
  let runtime = build_runtime_with_store_root(project_root, store_root.clone())?;

  let output = run_balatro_consumption_probe_chain(
    &runtime.recording().handle(),
    args.bundle_input,
    args.expected_slots,
    SlotId::new(ObjectZone::Hand, 0),
    args.work_dir,
  )?;

  println!("run_id={}", output.run_id);
  println!(
    "semantic_status={}",
    output.value.semantic.manifest.semantic_status
  );
  println!(
    "query_status={}",
    output.value.query.manifest.status.as_str()
  );
  println!(
    "witness_status={}",
    output.value.witness.manifest.status.as_str()
  );
  println!(
    "quality_verdict={}",
    output.value.quality.manifest.verdict.as_str()
  );
  if let Some(backend) = output.value.quality.manifest.quality_backend {
    println!("quality_backend={}", backend.as_str());
  }
  println!("store_root={}", store_root.display());
  Ok(())
}

fn parse_args(args: Vec<String>) -> Result<Args, String> {
  let mut bundle_input = None;
  let mut expected_slots = None;
  let mut work_dir = None;
  let mut store_root = None;

  let mut iter = args.into_iter();
  while let Some(flag) = iter.next() {
    let value = iter
      .next()
      .ok_or_else(|| format!("missing value for {flag}"))?;
    match flag.as_str() {
      "--bundle" => bundle_input = Some(PathBuf::from(value)),
      "--expected-slots" => expected_slots = Some(PathBuf::from(value)),
      "--work-dir" => work_dir = Some(PathBuf::from(value)),
      "--store-root" => store_root = Some(PathBuf::from(value)),
      other => return Err(format!("unknown flag: {other}")),
    }
  }

  Ok(Args {
    bundle_input: bundle_input.ok_or("--bundle is required")?,
    expected_slots: expected_slots.ok_or("--expected-slots is required")?,
    work_dir: work_dir.ok_or("--work-dir is required")?,
    store_root,
  })
}
