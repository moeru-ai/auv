use auv_cli::build_runtime_with_store_root;
use auv_cli::contract::{RecognitionResult, RecognitionScope, RecognitionSurface};
use auv_cli::inference_recognition::{
  BestSelectionStrategy, DetectorRecognitionArtifactRequest, DetectorRecognitionBridgePolicy, RuntimeProjection, RuntimeProjectionKind,
  record_detector_manifest_recognition_artifact,
};
use auv_cli::inspect_server;
use auv_inference_common::{
  BoundingBox, ClassLabelSource, Detection, DetectionCoordinateSpace, DetectionEvidenceManifest, DetectionOptions, DetectionSet, ModelId,
  ModelRunMetadata, ProjectionBasis, SourceImageEvidence, SourceImageRef, render_annotated_image,
};
use auv_inference_ultralytics::{InferenceDevice, UltralyticsDetector, UltralyticsModelConfig};
use auv_tracing_driver::run_builder::RunSpec;
use auv_tracing_driver::store::LocalStore;
use auv_tracing_driver::trace::RunType;
use auv_tracing_driver::{BroadcastRunRecorder, RunRecordingBackend};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use image::ImageReader;
use serde::Deserialize;
use serde_json::Value;
use std::error::Error;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower::util::ServiceExt;

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

#[derive(Debug)]
struct SmokeEvidencePaths {
  detection_json: PathBuf,
  manifest_json: PathBuf,
  recognition_json: PathBuf,
  annotated_image: PathBuf,
  runtime_store_root: PathBuf,
  runtime_run_id: String,
}

#[test]
fn balatro_golden_fixtures_are_well_formed() -> Result<(), Box<dyn Error>> {
  let fixture_dir = balatro_fixture_dir();
  assert!(fixture_dir.join("balatro.jpg").exists(), "Balatro fixture image does not exist");

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

fn assert_fixture_matches_detector(fixture_name: &str, config: &LocalSmokeConfig) -> Result<(), Box<dyn Error>> {
  let fixture = load_fixture(fixture_name)?;
  assert_fixture_metadata(fixture_name, &fixture);

  let model_path = config.balatro_root.join(&fixture.model.balatro_asset);
  assert!(model_path.exists(), "{fixture_name}: model path does not exist: {}", model_path.display());

  let class_path = config.balatro_root.join(&fixture.classes.balatro_asset);
  let class_names = load_class_names(&class_path, fixture_name)?;
  assert_eq!(class_names.len(), fixture.classes.count, "{fixture_name}: Balatro class file count does not match fixture count");
  assert_eq!(class_names, fixture.classes.labels, "{fixture_name}: fixture class labels do not match Balatro classes.txt");

  let image = fixture.image.as_ref().ok_or_else(|| format!("{fixture_name}: fixture image metadata is missing"))?;
  let source_image_path = config.balatro_root.join(&image.source_balatro_asset);
  if !source_image_path.exists() {
    eprintln!("skipping Balatro smoke for {fixture_name}; source image is missing: {}", source_image_path.display());
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
  let evidence_paths =
    write_smoke_evidence(fixture_name, &source_image_path, &result, &class_path, &fixture.thresholds, &config.output_dir)?;
  assert_smoke_evidence_outputs(fixture_name, &evidence_paths, &result, &source_image_path, &class_path, &fixture.thresholds)?;

  let decoded_image = ImageReader::open(&source_image_path)?.decode()?.to_rgb8();
  assert_eq!(result.image_size.width, decoded_image.width(), "{fixture_name}: result image width does not match decoded input image");
  assert_eq!(result.image_size.height, decoded_image.height(), "{fixture_name}: result image height does not match decoded input image");
  assert_eq!(result.image_size.width, image.width, "{fixture_name}: result image width does not match fixture metadata");
  assert_eq!(result.image_size.height, image.height, "{fixture_name}: result image height does not match fixture metadata");

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

fn assert_detection_set_invariants(fixture_name: &str, result: &DetectionSet, class_names: &[String]) {
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
      bbox_is_within_source_image(detection.bbox, result.image_size.width, result.image_size.height),
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
) -> Result<SmokeEvidencePaths, Box<dyn Error>> {
  let json_path = output_dir.join(format!("{fixture_name}-detections.json"));
  let manifest_path = output_dir.join(format!("{fixture_name}-manifest.json"));
  let image_path = output_dir.join(format!("{fixture_name}-annotated.png"));
  let runtime_store_root = output_dir.join(format!("{fixture_name}-runtime-store"));

  let file = File::create(&json_path)?;
  let mut writer = BufWriter::new(file);
  serde_json::to_writer_pretty(&mut writer, result)?;
  writer.write_all(b"\n")?;
  writer.flush()?;

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
  manifest_writer.flush()?;

  let source_image = ImageReader::open(source_image_path)?.decode()?.to_rgb8();
  let annotated = render_annotated_image(&source_image, &result.detections);
  annotated.save(&image_path)?;

  let store = LocalStore::new(runtime_store_root.clone())?;
  let recording = RunRecordingBackend::local_only(store.clone()).handle();
  let recorded = recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.inference.detector_smoke"),
    format!("Balatro detector recognition smoke {fixture_name}"),
    |context| {
      let mut request = DetectorRecognitionArtifactRequest::new(format!("recognition_balatro_smoke_{fixture_name}"));
      request.scope = RecognitionScope {
        surface: RecognitionSurface::Region,
        display_ref: None,
        native_display_id: None,
        app_bundle_id: None,
        window_title: None,
        window_number: None,
        region_hint: Some(auv_cli::contract::RatioRegion {
          left: 0.0,
          top: 0.0,
          right: 1.0,
          bottom: 1.0,
        }),
        capture_artifact: None,
        capture_contract_artifact: None,
      };
      request.projection = RuntimeProjection {
        kind: RuntimeProjectionKind::IdentitySourceImagePixels,
      };
      request.policy = DetectorRecognitionBridgePolicy {
        allowed_labels: None,
        best_selection: BestSelectionStrategy::None,
      };
      request.artifact_label = format!("{fixture_name}-recognition");
      request.artifact_note = "Detector-backed RecognitionResult runtime artifact from gated Balatro smoke.".to_string();
      record_detector_manifest_recognition_artifact(
        context,
        &manifest,
        source_image_path,
        "capture-image",
        &format!("{fixture_name}-capture.png"),
        Some("Balatro source image staged as capture artifact for detector smoke.".to_string()),
        &request,
      )
    },
  )?;
  let recognition_path = {
    let run = store.read_run(recorded.run_id.as_str())?;
    let detector_artifact = run
      .artifacts
      .iter()
      .find(|artifact| artifact.role == "detector-recognition")
      .ok_or("detector-recognition artifact missing from recorded Balatro smoke run")?;
    recorded.run_dir.join(&detector_artifact.path)
  };

  eprintln!(
    "{fixture_name}: wrote smoke evidence json={} manifest={} recognition={} annotated={} run_id={}",
    json_path.display(),
    manifest_path.display(),
    recognition_path.display(),
    image_path.display(),
    recorded.run_id
  );

  Ok(SmokeEvidencePaths {
    detection_json: json_path,
    manifest_json: manifest_path,
    recognition_json: recognition_path,
    annotated_image: image_path,
    runtime_store_root,
    runtime_run_id: recorded.run_id.to_string(),
  })
}

fn assert_smoke_evidence_outputs(
  fixture_name: &str,
  evidence_paths: &SmokeEvidencePaths,
  result: &DetectionSet,
  source_image_path: &Path,
  class_path: &Path,
  thresholds: &FixtureThresholds,
) -> Result<(), Box<dyn Error>> {
  let detections_name = format!("{fixture_name}-detections.json");
  let manifest_name = format!("{fixture_name}-manifest.json");
  let annotated_name = format!("{fixture_name}-annotated.png");

  assert_eq!(
    evidence_paths.detection_json.file_name().and_then(|name| name.to_str()),
    Some(detections_name.as_str()),
    "{fixture_name}: detections evidence path should use the expected file name"
  );
  assert_eq!(
    evidence_paths.manifest_json.file_name().and_then(|name| name.to_str()),
    Some(manifest_name.as_str()),
    "{fixture_name}: manifest evidence path should use the expected file name"
  );
  assert!(
    evidence_paths.recognition_json.file_name().and_then(|name| name.to_str()).map(|name| name.contains(fixture_name)).unwrap_or(false),
    "{fixture_name}: detector-recognition runtime artifact file name should retain fixture identity"
  );
  assert_eq!(
    evidence_paths.annotated_image.file_name().and_then(|name| name.to_str()),
    Some(annotated_name.as_str()),
    "{fixture_name}: annotated evidence path should use the expected file name"
  );

  assert!(
    evidence_paths.detection_json.is_file(),
    "{fixture_name}: detections evidence file was not written: {}",
    evidence_paths.detection_json.display()
  );
  assert!(
    evidence_paths.manifest_json.is_file(),
    "{fixture_name}: manifest evidence file was not written: {}",
    evidence_paths.manifest_json.display()
  );
  assert!(
    evidence_paths.recognition_json.is_file(),
    "{fixture_name}: recognition evidence file was not written: {}",
    evidence_paths.recognition_json.display()
  );
  assert!(
    evidence_paths.annotated_image.is_file(),
    "{fixture_name}: annotated image evidence file was not written: {}",
    evidence_paths.annotated_image.display()
  );

  let written_detections: DetectionSet = serde_json::from_str(&fs::read_to_string(&evidence_paths.detection_json)?)?;
  assert_eq!(written_detections, *result, "{fixture_name}: detections evidence JSON should round-trip back to the detector result");

  let manifest: DetectionEvidenceManifest = serde_json::from_str(&fs::read_to_string(&evidence_paths.manifest_json)?)?;
  assert_eq!(manifest.detection_set, *result, "{fixture_name}: manifest must embed the same DetectionSet written to detections JSON");
  assert_eq!(
    manifest.source_image.source_image_ref,
    SourceImageRef::LocalPath {
      path: source_image_path.to_path_buf()
    },
    "{fixture_name}: manifest must point at the source image used for smoke"
  );
  assert_eq!(
    manifest.source_image.coordinate_space,
    DetectionCoordinateSpace::SourceImagePixels,
    "{fixture_name}: manifest coordinate space must stay in source-image pixels"
  );
  assert_eq!(
    manifest.source_image.projection_basis,
    ProjectionBasis::Unavailable {
      reason: "local Balatro smoke does not capture display/window projection".to_string()
    },
    "{fixture_name}: manifest projection basis must stay unavailable for local smoke"
  );
  assert_eq!(manifest.model_run.backend, "ultralytics-inference", "{fixture_name}: manifest backend must identify the ultralytics adapter");
  assert_eq!(manifest.model_run.model_id, result.model_id, "{fixture_name}: manifest model_id must match the DetectionSet model_id");
  assert_eq!(
    manifest.model_run.confidence_threshold, thresholds.confidence,
    "{fixture_name}: manifest confidence threshold must match the fixture thresholds"
  );
  assert_eq!(manifest.model_run.iou_threshold, thresholds.iou, "{fixture_name}: manifest IoU threshold must match the fixture thresholds");
  assert_eq!(
    manifest.model_run.class_label_source,
    ClassLabelSource::OverrideFile {
      path: class_path.to_path_buf()
    },
    "{fixture_name}: manifest must record override-file label provenance"
  );
  assert_eq!(
    manifest.model_run.execution_provider.as_deref(),
    Some("cpu"),
    "{fixture_name}: manifest must record the CPU execution provider used by the smoke"
  );
  assert_eq!(
    manifest.known_limits,
    vec![
      "source image reference is inference-scoped, not a runtime artifact".to_string(),
      "projection basis is unavailable in local Balatro smoke".to_string(),
      "annotated image is a debug aid only".to_string(),
    ],
    "{fixture_name}: manifest known limits should stay explicit and inference-scoped"
  );

  let recognition: RecognitionResult = serde_json::from_str(&fs::read_to_string(&evidence_paths.recognition_json)?)?;
  assert_eq!(
    recognition.source,
    auv_cli::contract::RecognitionSource::Custom,
    "{fixture_name}: recognition source must stay custom until a detector-specific source variant lands"
  );
  assert!(!recognition.evidence.is_empty(), "{fixture_name}: recognition evidence must not be empty");
  assert!(recognition.scope.capture_artifact.is_some(), "{fixture_name}: recognition scope must carry capture_artifact");
  assert_eq!(recognition.all.len(), result.detections.len(), "{fixture_name}: recognition all[] should contain every accepted detection");
  assert_eq!(
    recognition.filtered.len(),
    result.detections.len(),
    "{fixture_name}: pass-through bridge policy should keep filtered[] aligned with accepted detections"
  );
  assert!(recognition.best.is_none(), "{fixture_name}: smoke recognition should keep best unset by default");
  assert_eq!(
    recognition.detail["backend"],
    Value::String("ultralytics-inference".to_string()),
    "{fixture_name}: recognition detail must carry backend provenance"
  );
  assert_eq!(
    recognition.detail["model_id"],
    Value::String(result.model_id.0.clone()),
    "{fixture_name}: recognition detail must carry model_id provenance"
  );
  assert_eq!(
    recognition.detail["class_label_source"]["kind"],
    Value::String("override_file".to_string()),
    "{fixture_name}: recognition detail must carry class_label_source provenance"
  );
  assert_eq!(
    recognition.detail["bridge_policy_version"],
    Value::String("detector-manifest-recognitionresult.v0".to_string()),
    "{fixture_name}: recognition detail must carry bridge policy version"
  );
  assert_eq!(
    recognition.detail["runtime_projection"]["kind"],
    Value::String("identity_source_image_pixels".to_string()),
    "{fixture_name}: smoke recognition must use identity source-image projection only"
  );
  assert!(
    recognition.known_limits.starts_with(&manifest.known_limits),
    "{fixture_name}: recognition known_limits must preserve manifest known_limits as a prefix"
  );
  assert!(
    recognition.known_limits.contains(&"detector RecognitionResult is recognition evidence only, not candidate-ready output".to_string()),
    "{fixture_name}: recognition known_limits must append the bridge evidence-only limit"
  );
  assert_no_forbidden_keys(fixture_name, &serde_json::to_value(&recognition)?, &["candidate", "candidate_ref", "action", "click"]);
  let runtime = build_runtime_with_store_root(
    evidence_paths.runtime_store_root.parent().unwrap().join(format!("{fixture_name}-runtime-project")),
    evidence_paths.runtime_store_root.clone(),
  )?;
  let inspect_text = auv_cli::inspect::inspect_run(runtime.recording().store(), &evidence_paths.runtime_run_id)?;
  assert!(
    inspect_text.contains("Detector Recognition Lineage:"),
    "{fixture_name}: inspect text must expose detector recognition lineage section"
  );
  assert!(inspect_text.contains("backend=ultralytics-inference"), "{fixture_name}: inspect text must expose detector backend provenance");
  assert!(inspect_text.contains("capture-image"), "{fixture_name}: inspect text must mention capture-image lineage");
  let lineage = auv_cli::inspect::list_detector_recognition_lineage(runtime.recording().store(), &evidence_paths.runtime_run_id)?;
  assert_eq!(lineage.len(), 1, "{fixture_name}: runtime read-side should expose exactly one detector recognition lineage record");
  let lineage = &lineage[0];
  assert_eq!(
    serde_json::to_value(&lineage.status)?,
    Value::String("ready".to_string()),
    "{fixture_name}: detector recognition lineage must be ready"
  );
  assert_eq!(
    lineage.source,
    Some(auv_cli::contract::RecognitionSource::Custom),
    "{fixture_name}: lineage must preserve custom recognition source"
  );
  assert_eq!(lineage.backend.as_deref(), Some("ultralytics-inference"), "{fixture_name}: lineage must preserve backend provenance");
  assert_eq!(lineage.model_id.as_deref(), Some(result.model_id.0.as_str()), "{fixture_name}: lineage must preserve model_id provenance");
  assert_eq!(
    lineage.runtime_projection_kind.as_deref(),
    Some("identity_source_image_pixels"),
    "{fixture_name}: lineage must preserve runtime projection policy"
  );
  assert_eq!(lineage.filtered_count, Some(result.detections.len()), "{fixture_name}: lineage filtered_count must match accepted detections");
  assert_eq!(lineage.all_count, Some(result.detections.len()), "{fixture_name}: lineage all_count must match accepted detections");
  assert!(
    lineage.capture_artifact.as_ref().map(|artifact| artifact.resolved).unwrap_or(false),
    "{fixture_name}: lineage capture_artifact must resolve to a real runtime artifact"
  );
  assert_eq!(
    lineage.capture_artifact.as_ref().and_then(|artifact| artifact.role.as_deref()),
    Some("capture-image"),
    "{fixture_name}: lineage capture_artifact role must be capture-image"
  );
  assert!(!lineage.evidence_artifacts.is_empty(), "{fixture_name}: lineage evidence artifacts must not be empty");
  assert!(
    lineage.known_limits.contains(&"detector RecognitionResult is recognition evidence only, not candidate-ready output".to_string()),
    "{fixture_name}: lineage must preserve detector evidence-only known limit"
  );
  let store = LocalStore::new(evidence_paths.runtime_store_root.clone())?;
  let app = inspect_server::router(store, Arc::new(BroadcastRunRecorder::new(16)));
  let async_runtime = tokio::runtime::Runtime::new()?;
  let response = async_runtime
    .block_on(async {
      app
        .oneshot(
          Request::builder().uri(format!("/runs/{}", evidence_paths.runtime_run_id)).body(Body::empty()).expect("request should build"),
        )
        .await
    })
    .expect("inspect_server /runs route should respond");
  assert_eq!(response.status(), StatusCode::OK, "{fixture_name}: inspect_server /runs route should return 200 for recorded smoke run");
  let body =
    async_runtime.block_on(async { to_bytes(response.into_body(), usize::MAX).await }).expect("inspect_server /runs body should read");
  let run: Value = serde_json::from_slice(&body)?;
  assert_eq!(
    run["run_id"],
    Value::String(evidence_paths.runtime_run_id.clone()),
    "{fixture_name}: inspect_server /runs JSON should preserve runtime run_id"
  );
  assert_eq!(
    run["detector_recognition_lineage"][0]["status"],
    Value::String("ready".to_string()),
    "{fixture_name}: inspect_server /runs JSON should expose ready detector lineage"
  );
  assert_eq!(
    run["detector_recognition_lineage"][0]["source"],
    Value::String("custom".to_string()),
    "{fixture_name}: inspect_server /runs JSON should expose custom recognition source"
  );
  assert_eq!(
    run["detector_recognition_lineage"][0]["backend"],
    Value::String("ultralytics-inference".to_string()),
    "{fixture_name}: inspect_server /runs JSON should expose detector backend provenance"
  );
  assert_eq!(
    run["detector_recognition_lineage"][0]["model_id"],
    Value::String(result.model_id.0.clone()),
    "{fixture_name}: inspect_server /runs JSON should expose detector model_id provenance"
  );
  assert_eq!(
    run["detector_recognition_lineage"][0]["capture_artifact"]["role"],
    Value::String("capture-image".to_string()),
    "{fixture_name}: inspect_server /runs JSON should expose capture-image artifact role"
  );
  assert_eq!(
    run["detector_recognition_lineage"][0]["filtered_count"],
    Value::from(result.detections.len()),
    "{fixture_name}: inspect_server /runs JSON should expose filtered_count"
  );
  assert_eq!(
    run["detector_recognition_lineage"][0]["all_count"],
    Value::from(result.detections.len()),
    "{fixture_name}: inspect_server /runs JSON should expose all_count"
  );
  assert!(
    run["detector_recognition_lineage"][0].get("best_item_id").is_some(),
    "{fixture_name}: inspect_server /runs JSON should expose best_item_id field even when null"
  );

  let annotated = ImageReader::open(&evidence_paths.annotated_image)?.decode()?.to_rgb8();
  assert_eq!(annotated.width(), result.image_size.width, "{fixture_name}: annotated image width must match the source image width");
  assert_eq!(annotated.height(), result.image_size.height, "{fixture_name}: annotated image height must match the source image height");

  Ok(())
}

fn assert_no_forbidden_keys(fixture_name: &str, value: &Value, forbidden_keys: &[&str]) {
  match value {
    Value::Object(map) => {
      for (key, nested) in map {
        assert!(!forbidden_keys.contains(&key.as_str()), "{fixture_name}: smoke recognition JSON must not contain forbidden key {key:?}");
        assert_no_forbidden_keys(fixture_name, nested, forbidden_keys);
      }
    }
    Value::Array(values) => {
      for nested in values {
        assert_no_forbidden_keys(fixture_name, nested, forbidden_keys);
      }
    }
    _ => {}
  }
}

fn smoke_output_dir() -> PathBuf {
  let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
  let repo_root = manifest_dir.parent().and_then(|path| path.parent()).expect("crate manifest should be nested under repo root");
  repo_root.join("target/auv-inference-smoke/balatro")
}

fn balatro_fixture_dir() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("balatro")
}

fn load_fixture(fixture_name: &str) -> Result<Fixture, Box<dyn Error>> {
  let fixture_path = balatro_fixture_dir().join(format!("{fixture_name}.json"));
  let fixture: Fixture = serde_json::from_str(&fs::read_to_string(&fixture_path)?)?;
  Ok(fixture)
}

fn assert_fixture_metadata(fixture_name: &str, fixture: &Fixture) {
  assert_eq!(fixture.detection_count, fixture.detections.len(), "{fixture_name}: fixture detection_count does not match detections length");
  assert_eq!(fixture.classes.count, fixture.classes.labels.len(), "{fixture_name}: fixture class count does not match labels length");
}

fn load_class_names(path: &Path, fixture_name: &str) -> Result<Vec<String>, Box<dyn Error>> {
  let contents =
    fs::read_to_string(path).map_err(|err| format!("{fixture_name}: failed to read Balatro class file {}: {err}", path.display()))?;
  let class_names = contents.lines().map(str::trim).filter(|line| !line.is_empty()).map(ToOwned::to_owned).collect::<Vec<_>>();
  assert!(!class_names.is_empty(), "{fixture_name}: Balatro class file is empty: {}", path.display());
  Ok(class_names)
}

fn assert_unordered_detections_match(fixture_name: &str, expected: &[FixtureDetection], actual: &[Detection]) {
  let mut unmatched = actual.to_vec();

  for expected_detection in expected {
    let match_index = unmatched.iter().position(|actual_detection| detections_match(expected_detection, actual_detection));
    let Some(match_index) = match_index else {
      panic!(
        "{fixture_name}: missing matching detection for {}\nunmatched actual detections:\n{}",
        summarize_fixture_detection(expected_detection),
        summarize_actual_detections(&unmatched)
      );
    };
    unmatched.remove(match_index);
  }

  assert!(unmatched.is_empty(), "{fixture_name}: unexpected extra detections:\n{}", summarize_actual_detections(&unmatched));
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
  detections.iter().map(summarize_fixture_detection).collect::<Vec<_>>().join("\n")
}

fn summarize_fixture_detection(detection: &FixtureDetection) -> String {
  format!(
    "{}:{} conf={:.3} bbox=[{:.1},{:.1},{:.1},{:.1}]",
    detection.class_id, detection.label, detection.confidence, detection.bbox[0], detection.bbox[1], detection.bbox[2], detection.bbox[3]
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
  assert!(balatro_root_from_env_value(Some(OsString::from("/definitely-missing-auv-balatro-root"))).is_none());
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
