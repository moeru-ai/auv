use auv_cli::build_runtime_with_store_root;
use auv_cli::contract::{RecognitionScope, RecognitionSurface};
use auv_cli::inference_recognition::{
  BestSelectionStrategy, DetectorRecognitionArtifactRequest, DetectorRecognitionBridgePolicy,
  RuntimeProjection, RuntimeProjectionKind, record_detector_manifest_recognition_artifact,
};
use auv_inference_common::{
  BoundingBox, ClassLabelSource, Detection, DetectionCoordinateSpace, DetectionEvidenceManifest,
  DetectionSet, ImageSize, ModelId, ModelRunMetadata, ProjectionBasis, SourceImageEvidence,
  SourceImageRef,
};
use auv_tracing_driver::run_builder::RunSpec;
use auv_tracing_driver::trace::RunType;
use serde_json::{Value, json};
use std::error::Error;
use std::fs;
use std::path::PathBuf;

#[test]
fn slay_the_spire_manual_fixture_stays_observe_only_and_emits_readable_lineage()
-> Result<(), Box<dyn Error>> {
  let root = temp_dir("slay-the-spire-observe-only");
  let project_root = root.join("project");
  let store_root = root.join("store");
  fs::create_dir_all(&project_root)?;

  let screenshot_path = project_root.join("slay-the-spire-fixture.png");
  fs::write(&screenshot_path, "fixture image bytes")?;

  let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())?;
  let manifest = DetectionEvidenceManifest {
    detection_set: DetectionSet {
      model_id: ModelId("slay-the-spire-manual-fixture-v0".to_string()),
      image_size: ImageSize {
        width: 1920,
        height: 1080,
      },
      detections: vec![
        Detection {
          class_id: 0,
          label: "card_region".to_string(),
          confidence: 1.0,
          bbox: BoundingBox {
            x1: 340.0,
            y1: 742.0,
            x2: 1572.0,
            y2: 1030.0,
          },
        },
        Detection {
          class_id: 1,
          label: "enemy_region".to_string(),
          confidence: 1.0,
          bbox: BoundingBox {
            x1: 1160.0,
            y1: 210.0,
            x2: 1720.0,
            y2: 560.0,
          },
        },
        Detection {
          class_id: 2,
          label: "energy_region".to_string(),
          confidence: 1.0,
          bbox: BoundingBox {
            x1: 88.0,
            y1: 790.0,
            x2: 222.0,
            y2: 934.0,
          },
        },
        Detection {
          class_id: 3,
          label: "end_turn_button_region".to_string(),
          confidence: 1.0,
          bbox: BoundingBox {
            x1: 1638.0,
            y1: 792.0,
            x2: 1866.0,
            y2: 970.0,
          },
        },
      ],
    },
    source_image: SourceImageEvidence {
      source_image_ref: SourceImageRef::LocalPath {
        path: screenshot_path.clone(),
      },
      coordinate_space: DetectionCoordinateSpace::SourceImagePixels,
      projection_basis: ProjectionBasis::Unavailable {
        reason: "manual Slay the Spire fixture does not provide display/window projection"
          .to_string(),
      },
    },
    model_run: ModelRunMetadata {
      backend: "manual-fixture".to_string(),
      model_id: ModelId("slay-the-spire-manual-fixture-v0".to_string()),
      confidence_threshold: 1.0,
      iou_threshold: 1.0,
      class_label_source: ClassLabelSource::InlineList,
      execution_provider: None,
    },
    known_limits: vec![
      "manual fixture regions are observe-only recognition evidence".to_string(),
      "no trained detector or semantic game-state mapping is implied".to_string(),
      "no action or clickability semantics are attached to end_turn_button_region".to_string(),
    ],
  };

  let recording = runtime.recording().handle();
  let recorded = recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.game.slay_the_spire.observe_only"),
    "Slay the Spire observe-only recognition fixture",
    |context| {
      let mut request =
        DetectorRecognitionArtifactRequest::new("recognition_slay_the_spire_fixture");
      request.scope = RecognitionScope {
        surface: RecognitionSurface::Region,
        display_ref: None,
        native_display_id: None,
        app_bundle_id: Some("com.megacrit.cardcrawl".to_string()),
        window_title: Some("Slay the Spire".to_string()),
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
      request.artifact_label = "slay-the-spire-observe-only".to_string();
      request.artifact_note =
        "Observe-only Slay the Spire detector-backed RecognitionResult fixture.".to_string();
      record_detector_manifest_recognition_artifact(
        context,
        &manifest,
        &screenshot_path,
        "capture-image",
        "slay-the-spire-fixture.png",
        Some("Synthetic Slay the Spire screenshot fixture.".to_string()),
        &request,
      )
    },
  )?;

  let inspect_text =
    auv_cli::inspect::inspect_run(runtime.recording().store(), recorded.run_id.as_str())?;
  assert!(
    inspect_text.contains("Detector Recognition Lineage:"),
    "inspect text should expose detector recognition lineage"
  );
  assert!(
    inspect_text.contains("backend=manual-fixture"),
    "inspect text should preserve manual fixture backend provenance"
  );
  assert!(
    inspect_text.contains("model=slay-the-spire-manual-fixture-v0"),
    "inspect text should preserve manual fixture model identity"
  );

  let lineage = auv_cli::inspect::list_detector_recognition_lineage(
    runtime.recording().store(),
    recorded.run_id.as_str(),
  )?;
  assert_eq!(lineage.len(), 1);
  let lineage = &lineage[0];
  assert_eq!(serde_json::to_value(&lineage.status)?, json!("ready"));
  assert_eq!(lineage.backend.as_deref(), Some("manual-fixture"));
  assert_eq!(
    lineage.model_id.as_deref(),
    Some("slay-the-spire-manual-fixture-v0")
  );
  assert_eq!(lineage.all_count, Some(4));
  assert_eq!(lineage.filtered_count, Some(4));
  assert_eq!(
    lineage
      .capture_artifact
      .as_ref()
      .and_then(|artifact| artifact.role.as_deref()),
    Some("capture-image")
  );
  assert!(
    !lineage.evidence_artifacts.is_empty(),
    "lineage should preserve evidence artifact refs"
  );
  assert!(lineage.known_limits.contains(
    &"no action or clickability semantics are attached to end_turn_button_region".to_string()
  ));

  assert_no_forbidden_keys(
    "slay_the_spire_observe_only",
    &serde_json::to_value(lineage)?,
    &["candidate", "candidate_ref", "action", "click"],
  );

  let _ = fs::remove_dir_all(root);
  Ok(())
}

fn temp_dir(label: &str) -> PathBuf {
  let path = std::env::temp_dir().join(format!("auv-{}-{}", label, auv_cli::model::now_millis()));
  let _ = fs::remove_dir_all(&path);
  fs::create_dir_all(&path).expect("temp dir should be creatable");
  path
}

fn assert_no_forbidden_keys(fixture_name: &str, value: &Value, forbidden_keys: &[&str]) {
  match value {
    Value::Object(map) => {
      for (key, nested) in map {
        assert!(
          !forbidden_keys.contains(&key.as_str()),
          "{fixture_name}: observe-only lineage JSON must not contain forbidden key {key:?}"
        );
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
