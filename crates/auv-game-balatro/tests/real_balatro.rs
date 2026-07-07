use std::path::{Path, PathBuf};

use auv_game_balatro::config::BalatroModelConfig;
use auv_game_balatro::observation::observe_image;
use auv_game_balatro::output::write_json_file;
use auv_task_object_detection::{Detection, render_annotated_image};
use image::ImageReader;

#[test]
#[ignore = "loads or downloads Balatro ONNX models and writes smoke artifacts"]
fn real_balatro_fixture_observes_state_and_writes_artifacts() -> Result<(), Box<dyn std::error::Error>> {
  let config = BalatroModelConfig::default();
  let resolved = config.resolve()?;
  assert_exists(&resolved.entities_model);
  assert_exists(&resolved.entities_classes);
  assert_exists(&resolved.ui_model);
  assert_exists(&resolved.ui_classes);

  let image_path =
    repo_root().join("crates").join("auv-inference-ultralytics").join("tests").join("fixtures").join("balatro").join("balatro.jpg");
  assert_exists(&image_path);

  let state = observe_image(&image_path, &config, false)?;

  assert!(!state.raw_entities.is_empty(), "real Balatro entities model returned no detections");
  assert!(!state.raw_ui.is_empty(), "real Balatro UI model returned no detections");
  assert!(!state.hand.is_empty(), "expected hand cards in fixture");
  assert!(!state.jokers.is_empty(), "expected joker cards in fixture");
  assert!(!state.buttons.is_empty(), "expected buttons in fixture");
  assert_eq!(state.phase, auv_game_balatro::model::BalatroPhase::Playing);

  let json_path = std::env::temp_dir().join("auv-game-balatro-real-state.json");
  write_json_file(&json_path, &state)?;

  let source_image = ImageReader::open(&image_path)?.decode()?.to_rgb8();
  let detections =
    state.raw_entities.iter().chain(state.raw_ui.iter()).map(|evidence| evidence.detection.clone()).collect::<Vec<Detection>>();
  let annotated = render_annotated_image(&source_image, &detections);
  let annotated_path = std::env::temp_dir().join("auv-game-balatro-real-annotated.png");
  annotated.save(&annotated_path)?;

  eprintln!(
    "real Balatro smoke: entities={} ui={} hand={} jokers={} buttons={} state={} annotated={}",
    state.raw_entities.len(),
    state.raw_ui.len(),
    state.hand.len(),
    state.jokers.len(),
    state.buttons.len(),
    json_path.display(),
    annotated_path.display()
  );

  Ok(())
}

fn assert_exists(path: &Path) {
  assert!(path.exists(), "missing required Balatro smoke path: {}", path.display());
}

fn repo_root() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("auv-game-balatro should live under crates/").to_path_buf()
}
