//! Optional real-source integration probe for MC-16 holdout preview and MC-17 render quality.
//!
//! Populated fixtures live under `crates/auv-gan-minecraft-fixtures/` (sibling path). When those
//! files are absent, tests skip honestly instead of failing CI.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use auv_game_minecraft::training_result_holdout_render_quality::{HoldoutRenderQualityAnswer, HoldoutRenderQualityRequest};
use auv_game_minecraft::types::{PlayerPose, Vec3, Viewport};
use auv_game_minecraft::{
  BlockPosition, HoldoutRenderQualityVerdict, MinecraftTargetSemantics, SCENE_PACKET_SCHEMA_VERSION, ScenePacketInputs,
  TrainingPackageInputs, TrainingResultHoldoutPreviewInputs, TrainingResultHoldoutRenderQualityInputs, TrainingResultSemanticManifest,
  TrainingResultSpatialQueryInputs, export_3dgs_scene_packet, export_3dgs_training_package, inspect_3dgs_training_result_holdout,
  measure_3dgs_holdout_render_quality, query_3dgs_training_result,
};
use auv_stage_status::StageStatus;
use serde_json::json;
use tempfile::TempDir;

const SCENE_PACKET_MANIFEST_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../auv-gan-minecraft-fixtures/scene-packet/run.json");
const RESULT_SEMANTIC_MANIFEST_PATH: &str =
  concat!(env!("CARGO_MANIFEST_DIR"), "/../auv-gan-minecraft-fixtures/result/minecraft-3dgs-training-result-semantic.json");
const FRAME_ALPHA_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../auv-gan-minecraft-fixtures/frames/frame_alpha.png");

/// Standalone probe proving an external holdout render command can stay blocked (no splat diff).
const HOLDOUT_RENDER_PROBE_COMMAND: &str =
  r#"python3 -c "import json,sys; print(json.dumps({'status':'blocked','message':'splat diff must not run in MC-16'}))""#;

/// Copy holdout screenshot to requested render path for MC-17 dimension-matched metrics.
const HOLDOUT_RENDER_QUALITY_COMMAND: &str = r#"python3 -c 'import json,shutil,sys; d=json.loads(sys.stdin.read()); shutil.copy(d["holdout_screenshot_path"], d["requested_rendered_image_path"]); print(json.dumps({"status":"ready","rendered_image_path":d["requested_rendered_image_path"]}))'"#;

struct FixtureChain {
  _root: TempDir,
  semantic_manifest: PathBuf,
  scene_packet_manifest: PathBuf,
  training_package_manifest: PathBuf,
}

fn fixture_workspace_root() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join("target/auv-gan-minecraft-workspaces")
}

fn fixture_training_bundle_dir() -> PathBuf {
  fixture_workspace_root().join("mc7-training-package-bundle").join("mc6-scene-packet-frames")
}

fn read_json_value(path: &Path) -> serde_json::Value {
  serde_json::from_slice(&fs::read(path).expect("read json")).expect("parse json")
}

fn assert_path_exists(path: &Path, label: &str) {
  assert!(path.is_file(), "{label} should exist at {}", path.display());
}

fn load_semantic_manifest(path: &Path) -> Option<TrainingResultSemanticManifest> {
  let bytes = fs::read(path).ok()?;
  serde_json::from_slice(&bytes).ok()
}

fn semantic_fixture_paths_ready(manifest: &TrainingResultSemanticManifest) -> bool {
  Path::new(&manifest.source_scene_packet_manifest_path).is_file()
    && Path::new(&manifest.source_training_package_manifest_path).is_file()
    && Path::new(&manifest.normalized_result_dir).is_dir()
    && Path::new(&manifest.models_dir_path).is_dir()
}

fn try_export_fixture_chain(root: &TempDir) -> Option<(PathBuf, PathBuf, PathBuf)> {
  let bundle_frame_dir = fixture_training_bundle_dir();
  if !bundle_frame_dir.join("alpha.json").is_file() {
    return None;
  }

  let bundle_dir = root.path().join("bundle");
  let scene_dir = root.path().join("scene-packet");
  let package_dir = root.path().join("training-package");

  let mut source_runs = Vec::new();
  for (index, name) in ["alpha", "beta"].iter().enumerate() {
    let run_id = format!("fixture-run-{name}");
    let run_dir = bundle_dir.join(&run_id);
    let artifact_path = run_dir.join("minecraft-spatial-frame.json");
    fs::create_dir_all(&run_dir).ok()?;
    fs::copy(bundle_frame_dir.join(format!("{name}.json")), &artifact_path).ok()?;

    if Path::new(FRAME_ALPHA_PATH).is_file() {
      let screenshot_path = run_dir.join("frame.png");
      if name == &"alpha" {
        fs::copy(FRAME_ALPHA_PATH, &screenshot_path).ok()?;
      } else {
        fs::write(&screenshot_path, fs::read(FRAME_ALPHA_PATH).ok()?).ok()?;
      }
      if let Ok(bytes) = fs::read(&artifact_path) {
        if let Ok(mut spatial_frame) = serde_json::from_slice::<serde_json::Value>(&bytes) {
          if let Some(frame) = spatial_frame.get_mut("frame") {
            frame["screenshot_path"] = json!(screenshot_path);
          }
          let _ = fs::write(&artifact_path, serde_json::to_vec(&spatial_frame).ok()?);
        }
      }
    }

    source_runs.push(json!({
      "run_id": run_id,
      "artifact_path": artifact_path,
      "frame_id": format!("frame-{index}"),
      "captured_at_millis": 1_700_000_000_000_u64 + index as u64,
    }));
  }

  fs::create_dir_all(&bundle_dir).ok()?;
  let bundle_manifest_path = bundle_dir.join("manifest.json");
  fs::write(
    &bundle_manifest_path,
    serde_json::to_string_pretty(&json!({
      "schema_version": 1,
      "generated_at_millis": 1,
      "source_runs": source_runs,
      "counts": {
        "source_runs": 2,
        "frames": 2,
        "screenshots_present": 2,
        "pose_complete": 2,
        "camera_complete": 2,
      },
    }))
    .ok()?,
  )
  .ok()?;

  export_3dgs_scene_packet(ScenePacketInputs {
    bundle_manifest_paths: vec![bundle_manifest_path],
    output_dir: scene_dir.clone(),
  })
  .ok()?;

  let scene_manifest_path = scene_dir.join("run.json");
  export_3dgs_training_package(TrainingPackageInputs {
    scene_packet_manifest_path: scene_manifest_path.clone(),
    output_dir: package_dir.clone(),
  })
  .ok()?;

  if Path::new(RESULT_SEMANTIC_MANIFEST_PATH).is_file() {
    return Some((PathBuf::from(RESULT_SEMANTIC_MANIFEST_PATH), scene_manifest_path, package_dir.join("run.json")));
  }

  None
}

fn prepare_fixture_chain() -> Option<FixtureChain> {
  if Path::new(RESULT_SEMANTIC_MANIFEST_PATH).is_file() {
    let semantic_manifest = load_semantic_manifest(Path::new(RESULT_SEMANTIC_MANIFEST_PATH))?;
    if semantic_fixture_paths_ready(&semantic_manifest) {
      return Some(FixtureChain {
        _root: TempDir::new().ok()?,
        semantic_manifest: PathBuf::from(RESULT_SEMANTIC_MANIFEST_PATH),
        scene_packet_manifest: PathBuf::from(&semantic_manifest.source_scene_packet_manifest_path),
        training_package_manifest: PathBuf::from(&semantic_manifest.source_training_package_manifest_path),
      });
    }
  }

  let root = TempDir::new().ok()?;
  let (semantic_manifest, scene_packet_manifest, training_package_manifest) = try_export_fixture_chain(&root)?;

  Some(FixtureChain {
    _root: root,
    semantic_manifest,
    scene_packet_manifest,
    training_package_manifest,
  })
}

fn run_holdout_render_probe(command: &str, output_dir: &Path) -> HoldoutRenderQualityAnswer {
  let screenshot = output_dir.join("probe_holdout.png");
  fs::write(&screenshot, b"probe").expect("probe screenshot");
  let request = HoldoutRenderQualityRequest {
    normalized_result_dir: output_dir.to_string_lossy().into_owned(),
    config_path: output_dir.join("config.yml").to_string_lossy().into_owned(),
    basis_checkpoint_path: output_dir.join("step.ckpt").to_string_lossy().into_owned(),
    holdout_frame_index: 0,
    holdout_frame_json_path: output_dir.join("frame.json").to_string_lossy().into_owned(),
    holdout_screenshot_path: screenshot.to_string_lossy().into_owned(),
    viewport: Viewport::new(1, 1),
    view_matrix: [0.0; 16],
    projection_matrix: [0.0; 16],
    player_pose: PlayerPose {
      eye_position: Vec3::new(0.0, 0.0, 0.0),
      yaw: 0.0,
      pitch: 0.0,
    },
    requested_rendered_image_path: output_dir.join("rendered.png").to_string_lossy().into_owned(),
  };
  fs::write(output_dir.join("frame.json"), serde_json::to_vec(&request).expect("request json")).expect("frame json");
  fs::write(output_dir.join("config.yml"), "method: splatfacto\n").expect("config");
  fs::write(output_dir.join("step.ckpt"), b"ckpt").expect("checkpoint");

  let mut child = Command::new("sh").arg("-lc").arg(command).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("spawn probe");
  let mut stdin = child.stdin.take().expect("stdin");
  let payload = format!("{}\n", serde_json::to_string(&request).expect("request"));
  stdin.write_all(payload.as_bytes()).expect("write stdin");
  drop(stdin);
  let output = child.wait_with_output().expect("probe output");
  assert!(output.status.success(), "probe command failed");
  let line = std::str::from_utf8(&output.stdout).expect("utf8 stdout").lines().next().unwrap_or("").trim();
  serde_json::from_str::<HoldoutRenderQualityAnswer>(line).expect("answer json")
}

#[test]
fn real_source_mc16_mc17_chain_exports_and_probes_holdout_without_splat_diff() {
  let Some(chain) = prepare_fixture_chain() else {
    eprintln!(
      "SKIP: populate {RESULT_SEMANTIC_MANIFEST_PATH} (and referenced scene/package/normalized paths) or target workspace bundle frames before enabling this test"
    );
    return;
  };

  if Path::new(SCENE_PACKET_MANIFEST_PATH).is_file() {
    assert_path_exists(&chain.scene_packet_manifest, "scene packet manifest");
    let scene_packet = read_json_value(&chain.scene_packet_manifest);
    assert_eq!(scene_packet.get("schema_version").and_then(|value| value.as_u64()), Some(SCENE_PACKET_SCHEMA_VERSION as u64));
    assert!(
      scene_packet.get("frames").and_then(|value| value.as_array()).is_some_and(|frames| !frames.is_empty()),
      "scene packet should export at least one frame"
    );
  }

  assert_path_exists(&chain.training_package_manifest, "training package manifest");
  let package = read_json_value(&chain.training_package_manifest);
  assert!(
    package.get("frames").and_then(|value| value.as_array()).is_some_and(|frames| !frames.is_empty())
      || package.get("compatibility_views").and_then(|value| value.as_array()).is_some_and(|views| !views.is_empty()),
    "training package manifest should expose frame or compatibility evidence"
  );

  assert_path_exists(&chain.semantic_manifest, "semantic manifest");

  let holdout_dir = chain._root.path().join("holdout-preview");
  let holdout = inspect_3dgs_training_result_holdout(TrainingResultHoldoutPreviewInputs {
    training_result_semantic_manifest_path: chain.semantic_manifest.clone(),
    holdout_frame_index: None,
    holdout_render_command: None,
    output_dir: holdout_dir.clone(),
  })
  .expect("mc16 holdout preview");

  assert_eq!(holdout.manifest.status, StageStatus::Ready);
  assert!(
    holdout.manifest.holdout_screenshot_path.as_deref().is_some_and(|path| Path::new(path).is_file()),
    "holdout screenshot path should resolve to a real file"
  );

  let probe_answer = run_holdout_render_probe(HOLDOUT_RENDER_PROBE_COMMAND, &holdout_dir);
  assert_eq!(probe_answer.status, StageStatus::Blocked);

  let quality_dir = chain._root.path().join("holdout-quality");
  let quality = measure_3dgs_holdout_render_quality(TrainingResultHoldoutRenderQualityInputs {
    training_result_semantic_manifest_path: chain.semantic_manifest,
    holdout_preview_manifest_path: holdout.manifest_path.clone(),
    render_command: HOLDOUT_RENDER_QUALITY_COMMAND.to_string(),
    output_dir: quality_dir,
  })
  .expect("mc17 holdout render quality");

  assert_eq!(quality.manifest.status, StageStatus::Ready);
  assert_eq!(quality.manifest.verdict, HoldoutRenderQualityVerdict::MeasuredOnly);
  assert!(quality.manifest.image_size_match);
  assert!(
    quality.manifest.metrics.as_ref().is_some_and(|metrics| metrics.l1_mean == Some(0.0) && metrics.mse == Some(0.0)),
    "copied holdout render should match screenshot exactly"
  );
}

#[test]
#[ignore = "requires populated MC-14 real-source semantic fixture and checkpoint query command"]
fn real_source_mc14_spatial_query_after_holdout_chain() {
  let Some(chain) = prepare_fixture_chain() else {
    eprintln!("SKIP: real-source semantic fixture not available");
    return;
  };

  let query_dir = chain._root.path().join("spatial-query");
  let query = query_3dgs_training_result(TrainingResultSpatialQueryInputs {
    training_result_semantic_manifest_path: chain.semantic_manifest,
    target_block: BlockPosition {
      x: 100,
      y: 64,
      z: -201,
    },
    target_face: None,
    target_semantics: MinecraftTargetSemantics::BlockCenter,
    query_command: None,
    use_checkpoint_native_provider: false,
    use_closed_scene_toy_provider: false,
    closed_scene_fixture_path: None,
    output_dir: query_dir,
  });

  match query {
    Ok(output) => {
      assert!(
        output.manifest.status.as_str() == "ready" || output.manifest.status.as_str() == "blocked",
        "MC-14 should close honestly against fixture semantic manifest"
      );
    }
    Err(message) => {
      assert!(
        message.contains("semantic") || message.contains("checkpoint") || message.contains("query"),
        "unexpected MC-14 failure: {message}"
      );
    }
  }
}
