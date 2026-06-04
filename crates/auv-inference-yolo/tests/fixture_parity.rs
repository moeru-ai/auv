use auv_inference_yolo::{
  Detection, DetectionOptions, ImageFrame, ModelId, YoloDetector, YoloFamily, YoloModelConfig,
};
use serde::Deserialize;
use std::{
  error::Error,
  fs,
  path::{Path, PathBuf},
};

const BALATRO_REPO: &str = "/Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro";
const INPUT_SIZE: u32 = 640;
const CONFIDENCE_TOLERANCE: f32 = 0.002;
const BBOX_TOLERANCE: f32 = 2.0;

#[derive(Debug, Deserialize)]
struct Fixture {
  classes: FixtureClasses,
  detection_count: usize,
  detections: Vec<FixtureDetection>,
  image: Option<FixtureImage>,
  model: FixtureModel,
  thresholds: FixtureThresholds,
}

impl Fixture {
  fn model(&self) -> &str {
    &self.model.name
  }

  fn model_path(&self, balatro_repo: &Path) -> PathBuf {
    balatro_repo.join(&self.model.balatro_asset)
  }

  fn classes_path(&self, balatro_repo: &Path) -> PathBuf {
    balatro_repo.join(&self.classes.balatro_asset)
  }

  fn confidence_threshold(&self) -> f32 {
    self.thresholds.confidence
  }

  fn iou_threshold(&self) -> f32 {
    self.thresholds.iou
  }
}

#[derive(Debug, Deserialize)]
struct FixtureClasses {
  #[serde(rename = "balatro_asset")]
  balatro_asset: PathBuf,
  count: usize,
  labels: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FixtureDetection {
  class_id: usize,
  label: String,
  confidence: f32,
  bbox: [f32; 4],
}

#[derive(Debug, Deserialize)]
struct FixtureImage {
  height: u32,
  width: u32,
}

#[derive(Debug, Deserialize)]
struct FixtureModel {
  #[serde(rename = "balatro_asset")]
  balatro_asset: PathBuf,
  name: String,
}

#[derive(Debug, Deserialize)]
struct FixtureThresholds {
  confidence: f32,
  iou: f32,
}

#[test]
fn balatro_fixtures_match_reference_detections() -> Result<(), Box<dyn Error>> {
  let balatro_repo = Path::new(BALATRO_REPO);
  if !balatro_repo.exists() {
    eprintln!("skipping Balatro fixture parity: {BALATRO_REPO} does not exist");
    return Ok(());
  }

  let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/balatro");
  let image_path = fixture_dir.join("balatro.jpg");
  let required_fixture_paths = [
    image_path.clone(),
    fixture_dir.join("entities.json"),
    fixture_dir.join("ui.json"),
  ];
  let missing_fixture_paths = required_fixture_paths
    .iter()
    .filter(|path| !path.exists())
    .map(|path| path.display().to_string())
    .collect::<Vec<_>>();
  if !missing_fixture_paths.is_empty() {
    // NOTICE: Old YOLO parity fixtures were migrated to `auv-inference-ultralytics`;
    // keeping this old test fully runnable is deferred because `auv-inference-yolo`
    // is scheduled for removal in Task 7 of
    // `docs/superpowers/plans/2026-06-04-ultralytics-inference-adapter.md`.
    eprintln!(
      "skipping old YOLO Balatro fixture parity: migrated fixtures are missing from {}: {}",
      fixture_dir.display(),
      missing_fixture_paths.join(", ")
    );
    return Ok(());
  }
  let frame = ImageFrame::new(image::open(&image_path)?.to_rgb8());

  for fixture_name in ["entities", "ui"] {
    assert_fixture_matches(&fixture_dir, fixture_name, balatro_repo, &frame)?;
  }

  Ok(())
}

fn assert_fixture_matches(
  fixture_dir: &Path,
  fixture_name: &str,
  balatro_repo: &Path,
  frame: &ImageFrame,
) -> Result<(), Box<dyn Error>> {
  let fixture = load_fixture(&fixture_dir.join(format!("{fixture_name}.json")))?;
  assert_eq!(
    fixture.detection_count,
    fixture.detections.len(),
    "{fixture_name} fixture detection_count does not match detections length"
  );
  assert_eq!(
    fixture.classes.count,
    fixture.classes.labels.len(),
    "{fixture_name} fixture class count does not match labels length"
  );

  let model_path = fixture.model_path(balatro_repo);
  let classes_path = fixture.classes_path(balatro_repo);
  assert!(
    model_path.exists(),
    "{fixture_name} fixture model path is missing: {}",
    model_path.display()
  );
  assert!(
    classes_path.exists(),
    "{fixture_name} fixture classes path is missing: {}",
    classes_path.display()
  );

  let class_names = load_class_names(&classes_path)?;
  assert_eq!(
    fixture.classes.labels, class_names,
    "{fixture_name} class names differ from fixture metadata"
  );

  let detector = YoloDetector::load(YoloModelConfig {
    model_id: ModelId(fixture.model().to_string()),
    model_path,
    class_names,
    input_size: INPUT_SIZE,
    family: YoloFamily::UltralyticsV8Like,
  })?;
  let result = detector.detect(
    frame,
    DetectionOptions {
      confidence_threshold: fixture.confidence_threshold(),
      iou_threshold: fixture.iou_threshold(),
    },
  )?;

  assert_eq!(result.model_id, ModelId(fixture.model().to_string()));
  if let Some(image) = &fixture.image {
    assert_eq!(result.image_size.width, image.width);
    assert_eq!(result.image_size.height, image.height);
  }
  assert_eq!(
    result.detections.len(),
    fixture.detections.len(),
    "{fixture_name} detection count differs\nexpected: {}\nactual: {}",
    summarize_fixture_detections(&fixture.detections),
    summarize_detections(&result.detections)
  );

  assert_detection_set_matches(fixture_name, &fixture.detections, &result.detections);

  Ok(())
}

fn load_fixture(path: &Path) -> Result<Fixture, Box<dyn Error>> {
  Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn load_class_names(path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
  Ok(
    fs::read_to_string(path)?
      .lines()
      .filter_map(|line| {
        let label = line.trim();
        (!label.is_empty()).then(|| label.to_string())
      })
      .collect(),
  )
}

fn assert_detection_set_matches(
  fixture_name: &str,
  expected: &[FixtureDetection],
  actual: &[Detection],
) {
  let mut unmatched_actual = vec![true; actual.len()];
  for (expected_index, expected_detection) in expected.iter().enumerate() {
    let Some(actual_index) = actual
      .iter()
      .enumerate()
      .position(|(actual_index, detection)| {
        unmatched_actual[actual_index] && detection_matches(expected_detection, detection)
      })
    else {
      panic!(
        "{fixture_name} detection {expected_index} had no matching actual detection: expected {} actual {}",
        summarize_fixture_detection(expected_detection),
        summarize_detections(actual)
      );
    };
    unmatched_actual[actual_index] = false;
  }
}

fn detection_matches(expected: &FixtureDetection, actual: &Detection) -> bool {
  actual.class_id == expected.class_id
    && actual.label == expected.label
    && (actual.confidence - expected.confidence).abs() < CONFIDENCE_TOLERANCE
    && bbox_matches(expected.bbox, actual)
}

fn bbox_matches(expected: [f32; 4], actual: &Detection) -> bool {
  let actual_bbox = [
    actual.bbox.x1,
    actual.bbox.y1,
    actual.bbox.x2,
    actual.bbox.y2,
  ];
  expected
    .into_iter()
    .zip(actual_bbox)
    .all(|(expected, actual)| (actual - expected).abs() < BBOX_TOLERANCE)
}

fn summarize_fixture_detection(detection: &FixtureDetection) -> String {
  format!(
    "{}:{}:{:.6}:{:?}",
    detection.class_id, detection.label, detection.confidence, detection.bbox
  )
}

fn summarize_fixture_detections(detections: &[FixtureDetection]) -> String {
  detections
    .iter()
    .map(summarize_fixture_detection)
    .collect::<Vec<_>>()
    .join(", ")
}

fn summarize_detections(detections: &[Detection]) -> String {
  detections
    .iter()
    .map(|detection| {
      format!(
        "{}:{}:{:.6}:[{:.3}, {:.3}, {:.3}, {:.3}]",
        detection.class_id,
        detection.label,
        detection.confidence,
        detection.bbox.x1,
        detection.bbox.y1,
        detection.bbox.x2,
        detection.bbox.y2
      )
    })
    .collect::<Vec<_>>()
    .join(", ")
}
