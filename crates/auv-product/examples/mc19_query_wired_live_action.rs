//! Historical thin wrapper around the canonical MC-20 D2 CLI entry.
//! Prefer `auv-minecraft query-wired-live-click`.

use std::env;
use std::path::PathBuf;

use auv_cli::build_runtime_with_store_root;
use auv_product::integrations::minecraft::{
  QueryWiredLiveActionInputs, QueryWiredLiveActionTelemetryWitness, run_minecraft_query_wired_live_action,
};

fn main() -> Result<(), String> {
  eprintln!("notice: canonical entry is `auv-minecraft query-wired-live-click`; this example is a thin library wrapper");
  let mut args = env::args().skip(1);
  let mut semantic_manifest = None;
  let mut target_block = None;
  let mut target_face = None;
  let mut target_semantics = auv_game_minecraft::MinecraftTargetSemantics::HitFaceCenter;
  let mut use_checkpoint_native_provider = false;
  let mut use_closed_scene_toy_provider = false;
  let mut closed_scene_fixture = None;
  let mut output_dir = None;
  let mut target_app = None;
  let mut target_title = None;
  let mut store_root = None;
  let mut telemetry_sample = None;
  let mut post_telemetry_sample = None;
  let mut verification_expected_item_id = None;

  while let Some(flag) = args.next() {
    match flag.as_str() {
      "--semantic-manifest" | "--training-result-semantic-manifest" => {
        semantic_manifest = args.next();
      }
      "--target-block" => target_block = args.next(),
      "--target-face" => {
        let value = args.next().ok_or("--target-face requires a value")?;
        target_face = Some(parse_block_face(&value)?);
      }
      "--target-semantics" => {
        let value = args.next().ok_or("--target-semantics requires a value")?;
        target_semantics = parse_target_semantics(&value)?;
      }
      "--query-provider" => {
        let value = args.next().ok_or("--query-provider requires a value")?;
        match value.as_str() {
          "checkpoint-native" => use_checkpoint_native_provider = true,
          "closed-scene-toy" => use_closed_scene_toy_provider = true,
          other => {
            return Err(format!("invalid --query-provider {other:?}; expected checkpoint-native or closed-scene-toy"));
          }
        }
      }
      "--closed-scene-fixture" => closed_scene_fixture = args.next().map(PathBuf::from),
      "--output-dir" => output_dir = args.next().map(PathBuf::from),
      "--target-app" => target_app = args.next(),
      "--target-title" => target_title = args.next(),
      "--store-root" => store_root = args.next().map(PathBuf::from),
      "--sample" => telemetry_sample = args.next().map(PathBuf::from),
      "--post-sample" => post_telemetry_sample = args.next().map(PathBuf::from),
      "--verification-expected-item-id" => {
        verification_expected_item_id = args.next();
      }
      other => return Err(format!("unexpected argument {other}")),
    }
  }

  if use_checkpoint_native_provider && use_closed_scene_toy_provider {
    return Err("--query-provider checkpoint-native and --query-provider closed-scene-toy are mutually exclusive".to_string());
  }
  if use_closed_scene_toy_provider && closed_scene_fixture.is_none() {
    return Err("--closed-scene-fixture is required when --query-provider closed-scene-toy".to_string());
  }
  if closed_scene_fixture.is_some() && !use_closed_scene_toy_provider {
    return Err("--closed-scene-fixture requires --query-provider closed-scene-toy".to_string());
  }
  if post_telemetry_sample.is_some() && telemetry_sample.is_none() {
    return Err("--post-sample requires --sample".to_string());
  }
  if verification_expected_item_id.is_some() && telemetry_sample.is_none() {
    return Err("--verification-expected-item-id requires --sample".to_string());
  }

  let project_root = env::current_dir().map_err(|error| error.to_string())?;
  let store_root = store_root.unwrap_or_else(|| project_root.join(".auv"));
  let runtime = build_runtime_with_store_root(project_root, store_root)?;
  let target_block = parse_block_position(&target_block.ok_or("--target-block is required")?)?;
  let telemetry_witness = telemetry_sample.map(|pre| QueryWiredLiveActionTelemetryWitness {
    pre_telemetry_sample: pre,
    post_telemetry_sample,
  });
  let output = run_minecraft_query_wired_live_action(
    &runtime.recording().handle(),
    QueryWiredLiveActionInputs {
      training_result_semantic_manifest_path: PathBuf::from(semantic_manifest.ok_or("--semantic-manifest is required")?),
      target_block,
      target_face,
      target_semantics,
      query_command: None,
      use_checkpoint_native_provider,
      use_closed_scene_toy_provider,
      closed_scene_fixture_path: closed_scene_fixture,
      output_dir: output_dir.ok_or("--output-dir is required")?.into(),
      target_app: target_app.ok_or("--target-app is required")?,
      target_title: target_title.ok_or("--target-title is required")?,
      telemetry_witness,
      verification_expected_item_id,
    },
  )?;
  println!("runId: {}", output.run_id);
  println!("queryStatus: {}", output.value.query.manifest.status.as_str());
  println!("wiringAttempted: {}", output.value.wiring.attempted);
  println!("actionEligibility: {}", output.value.wiring.action_eligibility.as_str());
  println!("operationResultArtifact: {}", output.value.operation_result_artifact_id);
  Ok(())
}

fn parse_block_position(raw: &str) -> Result<auv_game_minecraft::BlockPosition, String> {
  let mut parts = raw.split(',');
  let x = parts
    .next()
    .ok_or_else(|| format!("invalid target block {raw:?}"))?
    .parse::<i32>()
    .map_err(|error| format!("invalid target block x: {error}"))?;
  let y = parts
    .next()
    .ok_or_else(|| format!("invalid target block {raw:?}"))?
    .parse::<i32>()
    .map_err(|error| format!("invalid target block y: {error}"))?;
  let z = parts
    .next()
    .ok_or_else(|| format!("invalid target block {raw:?}"))?
    .parse::<i32>()
    .map_err(|error| format!("invalid target block z: {error}"))?;
  Ok(auv_game_minecraft::BlockPosition::new(x, y, z))
}

fn parse_block_face(raw: &str) -> Result<auv_game_minecraft::BlockFace, String> {
  match raw {
    "up" => Ok(auv_game_minecraft::BlockFace::Up),
    "down" => Ok(auv_game_minecraft::BlockFace::Down),
    "north" => Ok(auv_game_minecraft::BlockFace::North),
    "south" => Ok(auv_game_minecraft::BlockFace::South),
    "east" => Ok(auv_game_minecraft::BlockFace::East),
    "west" => Ok(auv_game_minecraft::BlockFace::West),
    other => Err(format!("invalid --target-face {other:?}")),
  }
}

fn parse_target_semantics(raw: &str) -> Result<auv_game_minecraft::MinecraftTargetSemantics, String> {
  match raw {
    "hit_face_center" => Ok(auv_game_minecraft::MinecraftTargetSemantics::HitFaceCenter),
    "block_center" => Ok(auv_game_minecraft::MinecraftTargetSemantics::BlockCenter),
    other => Err(format!("invalid --target-semantics {other:?}")),
  }
}
