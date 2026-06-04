use auv_inference_common::{BoundingBox, Detection, DetectionOptions, ModelId};
use auv_inference_ultralytics::{InferenceDevice, UltralyticsDetector, UltralyticsModelConfig};
use serde::Deserialize;
use std::error::Error;
use std::path::{Path, PathBuf};

const BALATRO_ROOT: &str = "/Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro";
const CONFIDENCE_TOLERANCE: f32 = 0.01;
const BBOX_TOLERANCE: f32 = 3.0;

#[derive(Debug, Deserialize)]
struct Fixture {
  model: FixtureModel,
  classes: FixtureClasses,
  image: Option<FixtureImage>,
  thresholds: FixtureThresholds,
  detection_count: usize,
  detections: Vec<FixtureDetection>,
}

#[derive(Debug, Deserialize)]
struct FixtureModel {
  name: String,
  balatro_asset: String,
}

#[derive(Debug, Deserialize)]
struct FixtureClasses {
  balatro_asset: String,
  count: usize,
  labels: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FixtureImage {
  width: u32,
  height: u32,
}

#[derive(Debug, Deserialize)]
struct FixtureThresholds {
  confidence: f32,
  iou: f32,
}

#[derive(Clone, Debug, Deserialize)]
struct FixtureDetection {
  class_id: usize,
  label: String,
  confidence: f32,
  bbox: [f32; 4],
}

#[test]
fn balatro_golden_fixtures_are_well_formed() -> Result<(), Box<dyn Error>> {
  let fixture_dir = balatro_fixture_dir();
  assert!(
    fixture_dir.join("balatro.jpg").exists(),
    "Balatro fixture image does not exist"
  );

  for fixture_name in ["entities", "ui"] {
    let fixture = load_fixture(fixture_name)?;
    assert_fixture_metadata(fixture_name, &fixture);
  }

  Ok(())
}

#[test]
fn balatro_golden_fixtures_match_ultralytics_detector() -> Result<(), Box<dyn Error>> {
  let balatro_repo = PathBuf::from(BALATRO_ROOT);
  if !balatro_repo.exists() {
    eprintln!(
      "skipping Balatro fixture parity; repo root does not exist: {}",
      balatro_repo.display()
    );
    return Ok(());
  }

  for fixture_name in ["entities", "ui"] {
    assert_fixture_matches_detector(fixture_name, &balatro_repo)?;
  }

  Ok(())
}

fn assert_fixture_matches_detector(
  fixture_name: &str,
  balatro_repo: &Path,
) -> Result<(), Box<dyn Error>> {
  let fixture_dir = balatro_fixture_dir();
  let fixture = load_fixture(fixture_name)?;
  assert_fixture_metadata(fixture_name, &fixture);

  let model_path = balatro_repo.join(&fixture.model.balatro_asset);
  assert!(
    model_path.exists(),
    "{fixture_name}: model path does not exist: {}",
    model_path.display()
  );

  let class_names = load_class_names(
    &balatro_repo.join(&fixture.classes.balatro_asset),
    fixture_name,
  )?;
  assert_eq!(
    class_names, fixture.classes.labels,
    "{fixture_name}: fixture class labels do not match Balatro classes.txt"
  );

  let detector = UltralyticsDetector::load(UltralyticsModelConfig {
    model_id: ModelId(fixture.model.name.clone()),
    model_path,
    input_size: Some(640),
    options: DetectionOptions {
      confidence_threshold: fixture.thresholds.confidence,
      iou_threshold: fixture.thresholds.iou,
      max_detections: 300,
    },
    device: InferenceDevice::Cpu,
    class_names_override: Some(class_names),
  })?;

  let result = detector.detect_path(fixture_dir.join("balatro.jpg"))?;
  if let Some(image) = &fixture.image {
    assert_eq!(
      result.image_size.width, image.width,
      "{fixture_name}: result image width does not match fixture"
    );
    assert_eq!(
      result.image_size.height, image.height,
      "{fixture_name}: result image height does not match fixture"
    );
  }

  assert_eq!(
    result.detections.len(),
    fixture.detection_count,
    "{fixture_name}: detection count mismatch\nexpected: {}\nactual: {}\nexpected summary:\n{}\nactual summary:\n{}",
    fixture.detection_count,
    result.detections.len(),
    summarize_fixture_detections(&fixture.detections),
    summarize_actual_detections(&result.detections)
  );
  assert_unordered_detections_match(fixture_name, &fixture.detections, &result.detections);

  Ok(())
}

fn balatro_fixture_dir() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("tests")
    .join("fixtures")
    .join("balatro")
}

fn load_fixture(fixture_name: &str) -> Result<Fixture, Box<dyn Error>> {
  let fixture_path = balatro_fixture_dir().join(format!("{fixture_name}.json"));
  let fixture: Fixture = serde_json::from_str(&std::fs::read_to_string(&fixture_path)?)?;
  Ok(fixture)
}

fn assert_fixture_metadata(fixture_name: &str, fixture: &Fixture) {
  assert_eq!(
    fixture.detection_count,
    fixture.detections.len(),
    "{fixture_name}: fixture detection_count does not match detections length"
  );
  assert_eq!(
    fixture.classes.count,
    fixture.classes.labels.len(),
    "{fixture_name}: fixture class count does not match labels length"
  );
}

fn load_class_names(path: &Path, fixture_name: &str) -> Result<Vec<String>, Box<dyn Error>> {
  let contents = std::fs::read_to_string(path).map_err(|err| {
    format!(
      "{fixture_name}: failed to read Balatro class file {}: {err}",
      path.display()
    )
  })?;
  Ok(
    contents
      .lines()
      .map(str::trim)
      .filter(|line| !line.is_empty())
      .map(ToOwned::to_owned)
      .collect(),
  )
}

fn assert_unordered_detections_match(
  fixture_name: &str,
  expected: &[FixtureDetection],
  actual: &[Detection],
) {
  let mut unmatched = actual.to_vec();

  for expected_detection in expected {
    let match_index = unmatched
      .iter()
      .position(|actual_detection| detections_match(expected_detection, actual_detection));
    let Some(match_index) = match_index else {
      panic!(
        "{fixture_name}: missing matching detection for {}\nunmatched actual detections:\n{}",
        summarize_fixture_detection(expected_detection),
        summarize_actual_detections(&unmatched)
      );
    };
    unmatched.remove(match_index);
  }

  assert!(
    unmatched.is_empty(),
    "{fixture_name}: unexpected extra detections:\n{}",
    summarize_actual_detections(&unmatched)
  );
}

fn detections_match(expected: &FixtureDetection, actual: &Detection) -> bool {
  expected.class_id == actual.class_id
    && expected.label == actual.label
    && (expected.confidence - actual.confidence).abs() <= CONFIDENCE_TOLERANCE
    && bbox_within_tolerance(expected.bbox, actual.bbox)
}

fn bbox_within_tolerance(expected: [f32; 4], actual: BoundingBox) -> bool {
  [
    (expected[0], actual.x1),
    (expected[1], actual.y1),
    (expected[2], actual.x2),
    (expected[3], actual.y2),
  ]
  .into_iter()
  .all(|(expected, actual)| (expected - actual).abs() <= BBOX_TOLERANCE)
}

fn summarize_fixture_detections(detections: &[FixtureDetection]) -> String {
  detections
    .iter()
    .map(summarize_fixture_detection)
    .collect::<Vec<_>>()
    .join("\n")
}

fn summarize_fixture_detection(detection: &FixtureDetection) -> String {
  format!(
    "{}:{} conf={:.3} bbox=[{:.1},{:.1},{:.1},{:.1}]",
    detection.class_id,
    detection.label,
    detection.confidence,
    detection.bbox[0],
    detection.bbox[1],
    detection.bbox[2],
    detection.bbox[3]
  )
}

fn summarize_actual_detections(detections: &[Detection]) -> String {
  detections
    .iter()
    .map(|detection| {
      format!(
        "{}:{} conf={:.3} bbox=[{:.1},{:.1},{:.1},{:.1}]",
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
    .join("\n")
}
