use std::env;
use std::path::PathBuf;

use auv_cli::build_runtime_with_store_root;
use auv_cli::osu::{QueryWiredLiveActionInputs, run_osu_query_wired_live_action};
use auv_game_osu::{CapturePhase, ObjectKind};

struct Args {
  semantic_manifest: PathBuf,
  object_index: usize,
  capture_phase: CapturePhase,
  object_kind: Option<ObjectKind>,
  output_dir: PathBuf,
  target_app: String,
  target_title: String,
  store_root: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let args = parse_args(env::args().skip(1).collect())?;
  let project_root = env::current_dir()?;
  let store_root = args
    .store_root
    .clone()
    .unwrap_or_else(|| project_root.join(".auv"));
  let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())?;

  let output = run_osu_query_wired_live_action(
    &runtime.recording().handle(),
    QueryWiredLiveActionInputs {
      visual_truth_semantic_manifest_path: args.semantic_manifest,
      object_index: args.object_index,
      capture_phase: args.capture_phase,
      object_kind: args.object_kind,
      output_dir: args.output_dir,
      target_app: args.target_app,
      target_title: args.target_title,
    },
  )?;

  println!("run_id={}", output.run_id);
  println!(
    "attempted={} action_eligibility={}",
    output.value.wiring.attempted,
    output.value.wiring.action_eligibility.as_str()
  );
  if let Some((x, y)) = output.value.wiring.pixel_point {
    println!("pixel_point={x},{y}");
  }
  if let Some(point) = &output.value.wiring.window_point {
    println!("window_point={:.3},{:.3}", point.0.x, point.0.y);
  }
  if let Some(summary) = &output.value.wiring.click_summary {
    println!("click_summary={summary}");
  }
  if let Some(refusal) = &output.value.wiring.refusal_reason {
    println!("refusal_reason={refusal}");
  }
  println!(
    "operation_result_artifact_id={}",
    output.value.operation_result_artifact_id
  );
  println!("store_root={}", store_root.display());
  Ok(())
}

fn parse_args(args: Vec<String>) -> Result<Args, String> {
  let mut semantic_manifest = None;
  let mut object_index = None;
  let mut capture_phase = CapturePhase::BeforeDispatch;
  let mut object_kind = None;
  let mut output_dir = None;
  let mut target_app = None;
  let mut target_title = None;
  let mut store_root = None;

  let mut iter = args.into_iter();
  while let Some(flag) = iter.next() {
    let value = iter
      .next()
      .ok_or_else(|| format!("{flag} requires a value"))?;
    match flag.as_str() {
      "--semantic-manifest" => semantic_manifest = Some(PathBuf::from(value)),
      "--object-index" => {
        object_index = Some(
          value
            .parse()
            .map_err(|error| format!("invalid --object-index {value:?}: {error}"))?,
        )
      }
      "--capture-phase" => capture_phase = parse_capture_phase(&value)?,
      "--object-kind" => object_kind = Some(parse_object_kind(&value)?),
      "--output-dir" => output_dir = Some(PathBuf::from(value)),
      "--target-app" => target_app = Some(value),
      "--target-title" => target_title = Some(value),
      "--store-root" => store_root = Some(PathBuf::from(value)),
      other => return Err(format!("unknown argument: {other}")),
    }
  }

  Ok(Args {
    semantic_manifest: semantic_manifest.ok_or("--semantic-manifest is required")?,
    object_index: object_index.ok_or("--object-index is required")?,
    capture_phase,
    object_kind,
    output_dir: output_dir.ok_or("--output-dir is required")?,
    target_app: target_app.ok_or("--target-app is required")?,
    target_title: target_title.ok_or("--target-title is required")?,
    store_root,
  })
}

fn parse_capture_phase(raw: &str) -> Result<CapturePhase, String> {
  match raw.to_ascii_lowercase().as_str() {
    "before_dispatch" => Ok(CapturePhase::BeforeDispatch),
    "after_dispatch" => Ok(CapturePhase::AfterDispatch),
    other => Err(format!("invalid --capture-phase {other:?}")),
  }
}

fn parse_object_kind(raw: &str) -> Result<ObjectKind, String> {
  match raw.to_ascii_lowercase().as_str() {
    "circle" => Ok(ObjectKind::Circle),
    "slider" => Ok(ObjectKind::Slider),
    "spinner" => Ok(ObjectKind::Spinner),
    "hold" => Ok(ObjectKind::Hold),
    other => Err(format!("invalid --object-kind {other:?}")),
  }
}
