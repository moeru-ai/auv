use auv_inference_common::{
  render_annotated_image, BoundingBox, ClassLabelSource, Detection, DetectionCoordinateSpace,
  DetectionEvidenceManifest, DetectionOptions, DetectionSet, ModelId, ModelRunMetadata,
  ProjectionBasis, SourceImageEvidence, SourceImageRef,
};
use auv_inference_ultralytics::{InferenceDevice, UltralyticsDetector, UltralyticsModelConfig};
use image::ImageReader;
use serde::Deserialize;
use std::error::Error;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

const BALATRO_ROOT_ENV: &str = "AUV_BALATRO_ROOT";
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
  path: String,
  source_balatro_asset: String,
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

#[derive(Debug)]
struct LocalSmokeConfig {
  balatro_root: PathBuf,
  output_dir: PathBuf,
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

// NOTICE: This is a local/gated smoke test, not default CI proof of real model
// execution. If the Balatro checkout is absent, the test must skip explicitly
// instead of pretending that workspace `cargo test` validated model inference.
#[test]
fn balatro_golden_fixtures_match_ultralytics_detector() -> Result<(), Box<dyn Error>> {
  let Some(config) = local_smoke_config()? else {
    return Ok(());
  };

  for fixture_name in ["entities", "ui"] {
    assert_fixture_matches_detector(fixture_name, &config)?;
  }

  Ok(())
}

fn local_smoke_config() -> Result<Option<LocalSmokeConfig>, Box<dyn Error>> {
  let Some(balatro_root) = balatro_root_from_env_value(std::env::var_os(BALATRO_ROOT_ENV)) else {
    eprintln!(
      "skipping Balatro smoke; {} is not set to an existing directory, so local model/image evidence is unavailable",
      BALATRO_ROOT_ENV
    );
    return Ok(None);
  };

  let output_dir = smoke_output_dir();
  fs::create_dir_all(&output_dir)?;

  Ok(Some(LocalSmokeConfig {
    balatro_root,
    output_dir,
  }))
}

fn assert_fixture_matches_detector(
  fixture_name: &str,
  config: &LocalSmokeConfig,
) -> Result<(), Box<dyn Error>> {
  let fixture = load_fixture(fixture_name)?;
  assert_fixture_metadata(fixture_name, &fixture);

  let model_path = config.balatro_root.join(&fixture.model.balatro_asset);
  assert!(
    model_path.exists(),
    "{fixture_name}: model path does not exist: {}",
    model_path.display()
  );

  let class_path = config.balatro_root.join(&fixture.classes.balatro_asset);
  let class_names = load_class_names(&class_path, fixture_name)?;
  assert_eq!(
    class_names.len(),
    fixture.classes.count,
    "{fixture_name}: Balatro class file count does not match fixture count"
  );
  assert_eq!(
    class_names, fixture.classes.labels,
    "{fixture_name}: fixture class labels do not match Balatro classes.txt"
  );

  let image = fixture
    .image
    .as_ref()
    .ok_or_else(|| format!("{fixture_name}: fixture image metadata is missing"))?;
  let source_image_path = config.balatro_root.join(&image.source_balatro_asset);
  if !source_image_path.exists() {
    eprintln!(
      "skipping Balatro smoke for {fixture_name}; source image is missing: {}",
      source_image_path.display()
    );
    return Ok(());
  }

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
    class_names_override: Some(class_names.clone()),
  })?;

  let result = detector.detect_path(&source_image_path)?;
  write_smoke_evidence(
    fixture_name,
    &source_image_path,
    &result,
    &class_path,
    &fixture.thresholds,
    &config.output_dir,
  )?;

  let decoded_image = ImageReader::open(&source_image_path)?.decode()?.to_rgb8();
  assert_eq!(
    result.image_size.width,
    decoded_image.width(),
    "{fixture_name}: result image width does not match decoded input image"
  );
  assert_eq!(
    result.image_size.height,
    decoded_image.height(),
    "{fixture_name}: result image height does not match decoded input image"
  );
  assert_eq!(
    result.image_size.width, image.width,
    "{fixture_name}: result image width does not match fixture metadata"
  );
  assert_eq!(
    result.image_size.height, image.height,
    "{fixture_name}: result image height does not match fixture metadata"
  );

  assert_detection_set_invariants(fixture_name, &result, &class_names);

  if result.detections.is_empty() {
    eprintln!(
      "{fixture_name}: detector returned 0 detections; model_id={}, image={}, class_count={}, confidence_threshold={}, iou_threshold={}",
      result.model_id.0,
      source_image_path.display(),
      class_names.len(),
      fixture.thresholds.confidence,
      fixture.thresholds.iou
    );
  }

  assert_eq!(
    result.detections.len(),
    fixture.detection_count,
    "{fixture_name}: detection count mismatch\nmodel: {}\nimage: {}\nfixture_image: {}\nclass_count: {}\nexpected: {}\nactual: {}\nexpected summary:\n{}\nactual summary:\n{}",
    fixture.model.name,
    source_image_path.display(),
    image.path,
    class_names.len(),
    fixture.detection_count,
    result.detections.len(),
    summarize_fixture_detections(&fixture.detections),
    summarize_actual_detections(&result.detections)
  );
  assert_unordered_detections_match(fixture_name, &fixture.detections, &result.detections);

  Ok(())
}

fn assert_detection_set_invariants(
  fixture_name: &str,
  result: &DetectionSet,
  class_names: &[String],
) {
  for detection in &result.detections {
    assert!(
      (0.0..=1.0).contains(&detection.confidence),
      "{fixture_name}: confidence must stay in 0..=1, got {} for {}",
      detection.confidence,
      detection.label
    );
    assert!(
      detection.class_id < class_names.len(),
      "{fixture_name}: class id {} is out of range for {} labels",
      detection.class_id,
      class_names.len()
    );
    assert_eq!(
      detection.label, class_names[detection.class_id],
      "{fixture_name}: label must come from class list for class id {}",
      detection.class_id
    );
    assert!(
      bbox_is_within_source_image(
        detection.bbox,
        result.image_size.width,
        result.image_size.height
      ),
      "{fixture_name}: bbox must stay in source-image pixel space, got {:?} within {}x{}",
      detection.bbox,
      result.image_size.width,
      result.image_size.height
    );
  }
}

fn bbox_is_within_source_image(bbox: BoundingBox, width: u32, height: u32) -> bool {
  let max_x = width as f32;
  let max_y = height as f32;
  bbox.x1.is_finite()
    && bbox.y1.is_finite()
    && bbox.x2.is_finite()
    && bbox.y2.is_finite()
    && bbox.x1 >= 0.0
    && bbox.y1 >= 0.0
    && bbox.x2 >= 0.0
    && bbox.y2 >= 0.0
    && bbox.x1 <= max_x
    && bbox.x2 <= max_x
    && bbox.y1 <= max_y
    && bbox.y2 <= max_y
    && bbox.x1 <= bbox.x2
    && bbox.y1 <= bbox.y2
}

fn write_smoke_evidence(
  fixture_name: &str,
  source_image_path: &Path,
  result: &DetectionSet,
  class_path: &Path,
  thresholds: &FixtureThresholds,
  output_dir: &Path,
) -> Result<(), Box<dyn Error>> {
  let json_path = output_dir.join(format!("{fixture_name}-detections.json"));
  let manifest_path = output_dir.join(format!("{fixture_name}-manifest.json"));
  let image_path = output_dir.join(format!("{fixture_name}-annotated.png"));

  let file = File::create(&json_path)?;
  let mut writer = BufWriter::new(file);
  serde_json::to_writer_pretty(&mut writer, result)?;
  writer.write_all(b"\n")?;

  let manifest = DetectionEvidenceManifest {
    detection_set: result.clone(),
    source_image: SourceImageEvidence {
      source_image_ref: SourceImageRef::LocalPath {
        path: source_image_path.to_path_buf(),
      },
      coordinate_space: DetectionCoordinateSpace::SourceImagePixels,
      projection_basis: ProjectionBasis::Unavailable {
        reason: "local Balatro smoke does not capture display/window projection".to_string(),
      },
    },
    model_run: ModelRunMetadata {
      backend: "ultralytics-inference".to_string(),
      model_id: result.model_id.clone(),
      confidence_threshold: thresholds.confidence,
      iou_threshold: thresholds.iou,
      class_label_source: ClassLabelSource::OverrideFile {
        path: class_path.to_path_buf(),
      },
      execution_provider: Some("cpu".to_string()),
    },
    known_limits: vec![
      "source image reference is inference-scoped, not a runtime artifact".to_string(),
      "projection basis is unavailable in local Balatro smoke".to_string(),
      "annotated image is a debug aid only".to_string(),
    ],
  };
  let manifest_file = File::create(&manifest_path)?;
  let mut manifest_writer = BufWriter::new(manifest_file);
  serde_json::to_writer_pretty(&mut manifest_writer, &manifest)?;
  manifest_writer.write_all(b"\n")?;

  let source_image = ImageReader::open(source_image_path)?.decode()?.to_rgb8();
  let annotated = render_annotated_image(&source_image, &result.detections);
  annotated.save(&image_path)?;

  eprintln!(
    "{fixture_name}: wrote smoke evidence json={} manifest={} annotated={}",
    json_path.display(),
    manifest_path.display(),
    image_path.display()
  );

  Ok(())
}

fn smoke_output_dir() -> PathBuf {
  let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
  let repo_root = manifest_dir
    .parent()
    .and_then(|path| path.parent())
    .expect("crate manifest should be nested under repo root");
  repo_root.join("target/auv-inference-smoke/balatro")
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
  let class_names = contents
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .map(ToOwned::to_owned)
    .collect::<Vec<_>>();
  assert!(
    !class_names.is_empty(),
    "{fixture_name}: Balatro class file is empty: {}",
    path.display()
  );
  Ok(class_names)
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

#[test]
fn local_smoke_skips_when_env_is_missing_or_path_is_missing() -> Result<(), Box<dyn Error>> {
  assert!(balatro_root_from_env_value(None).is_none());
  assert!(
    balatro_root_from_env_value(Some(OsString::from("/definitely-missing-auv-balatro-root")))
      .is_none()
  );
  let temp_dir = std::env::temp_dir().join("auv-inference-ultralytics-existing-dir-check");
  fs::create_dir_all(&temp_dir)?;
  assert!(balatro_root_from_env_value(Some(temp_dir.into_os_string())).is_some());
  Ok(())
}

fn balatro_root_from_env_value(value: Option<OsString>) -> Option<PathBuf> {
  let value = value?;
  let path = PathBuf::from(value);
  if path.exists() && path.is_dir() {
    Some(path)
  } else {
    None
  }
}
