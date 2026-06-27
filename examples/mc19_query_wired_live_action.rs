use std::env;
use std::path::PathBuf;

use auv_cli::build_runtime_with_store_root;
use auv_cli::minecraft::{QueryWiredLiveActionInputs, run_minecraft_query_wired_live_action};
use auv_game_minecraft::{BlockFace, BlockPosition, MinecraftTargetSemantics};

struct Args {
  semantic_manifest: PathBuf,
  target_block: BlockPosition,
  target_face: Option<BlockFace>,
  target_semantics: MinecraftTargetSemantics,
  closed_scene_fixture: Option<PathBuf>,
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

  let output = run_minecraft_query_wired_live_action(
    &runtime.recording().handle(),
    QueryWiredLiveActionInputs {
      training_result_semantic_manifest_path: args.semantic_manifest,
      target_block: args.target_block,
      target_face: args.target_face,
      target_semantics: args.target_semantics,
      query_command: None,
      use_checkpoint_native_provider: false,
      use_closed_scene_toy_provider: args.closed_scene_fixture.is_some(),
      closed_scene_fixture_path: args.closed_scene_fixture,
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
  let mut target_block = None;
  let mut target_face = None;
  let mut target_semantics = MinecraftTargetSemantics::HitFaceCenter;
  let mut closed_scene_fixture = None;
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
      "--target-block" => target_block = Some(parse_block_position(&value)?),
      "--target-face" => target_face = Some(parse_block_face(&value)?),
      "--target-semantics" => target_semantics = parse_target_semantics(&value)?,
      "--closed-scene-fixture" => closed_scene_fixture = Some(PathBuf::from(value)),
      "--output-dir" => output_dir = Some(PathBuf::from(value)),
      "--target-app" => target_app = Some(value),
      "--target-title" => target_title = Some(value),
      "--store-root" => store_root = Some(PathBuf::from(value)),
      other => return Err(format!("unknown argument: {other}")),
    }
  }

  Ok(Args {
    semantic_manifest: semantic_manifest.ok_or("--semantic-manifest is required")?,
    target_block: target_block.ok_or("--target-block is required")?,
    target_face,
    target_semantics,
    closed_scene_fixture,
    output_dir: output_dir.ok_or("--output-dir is required")?,
    target_app: target_app.ok_or("--target-app is required")?,
    target_title: target_title.ok_or("--target-title is required")?,
    store_root,
  })
}

fn parse_block_position(raw: &str) -> Result<BlockPosition, String> {
  let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
  if parts.len() != 3 {
    return Err(format!("invalid --target-block {raw:?}; expected x,y,z"));
  }
  let x = parts[0]
    .parse()
    .map_err(|error| format!("invalid target block x: {error}"))?;
  let y = parts[1]
    .parse()
    .map_err(|error| format!("invalid target block y: {error}"))?;
  let z = parts[2]
    .parse()
    .map_err(|error| format!("invalid target block z: {error}"))?;
  Ok(BlockPosition::new(x, y, z))
}

fn parse_block_face(raw: &str) -> Result<BlockFace, String> {
  match raw.to_ascii_lowercase().as_str() {
    "north" => Ok(BlockFace::North),
    "south" => Ok(BlockFace::South),
    "east" => Ok(BlockFace::East),
    "west" => Ok(BlockFace::West),
    "up" => Ok(BlockFace::Up),
    "down" => Ok(BlockFace::Down),
    other => Err(format!("invalid --target-face {other:?}")),
  }
}

fn parse_target_semantics(raw: &str) -> Result<MinecraftTargetSemantics, String> {
  match raw.to_ascii_lowercase().as_str() {
    "hit_face_center" => Ok(MinecraftTargetSemantics::HitFaceCenter),
    other => Err(format!("invalid --target-semantics {other:?}")),
  }
}
