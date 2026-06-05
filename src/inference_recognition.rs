use std::collections::BTreeSet;

use auv_inference_common::{
  BoundingBox, ClassLabelSource, DetectionCoordinateSpace, DetectionEvidenceManifest, ImageSize,
  ProjectionBasis,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::contract::{
  ArtifactRef, RecognitionBox, RecognitionResult, RecognitionScope, RecognitionSource,
  RecognizedItem,
};

const BRIDGE_POLICY_VERSION: &str = "detector-manifest-recognitionresult.v0";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BestSelectionStrategy {
  None,
  SingleFilteredItem,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectorRecognitionBridgePolicy {
  pub allowed_labels: Option<BTreeSet<String>>,
  pub best_selection: BestSelectionStrategy,
}

impl Default for DetectorRecognitionBridgePolicy {
  fn default() -> Self {
    Self {
      allowed_labels: None,
      best_selection: BestSelectionStrategy::None,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectorRecognitionRuntimeContext {
  pub recognition_id: String,
  pub scope: RecognitionScope,
  pub evidence: Vec<ArtifactRef>,
  pub source_image_size: ImageSize,
  pub projection: RuntimeProjection,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeProjection {
  pub kind: RuntimeProjectionKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeProjectionKind {
  Unavailable { reason: String },
  IdentitySourceImagePixels,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DetectorRecognitionMappingError {
  MissingRuntimeEvidence,
  MissingCaptureArtifact,
  ProjectionUnavailable,
  SourceImageSizeMismatch {
    manifest: ImageSize,
    runtime: ImageSize,
  },
  UnsupportedCoordinateSpace(DetectionCoordinateSpace),
}

impl std::fmt::Display for DetectorRecognitionMappingError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::MissingRuntimeEvidence => {
        write!(
          f,
          "detector manifest mapping requires runtime ArtifactRef evidence"
        )
      }
      Self::MissingCaptureArtifact => {
        write!(
          f,
          "detector manifest mapping requires scope.capture_artifact"
        )
      }
      Self::ProjectionUnavailable => {
        write!(
          f,
          "detector manifest mapping requires honest runtime projection context"
        )
      }
      Self::SourceImageSizeMismatch { manifest, runtime } => write!(
        f,
        "detector manifest image size {}x{} does not match runtime source image size {}x{}",
        manifest.width, manifest.height, runtime.width, runtime.height
      ),
      Self::UnsupportedCoordinateSpace(space) => write!(
        f,
        "detector manifest coordinate space {space:?} is not supported by the v0 recognition bridge"
      ),
    }
  }
}

pub fn map_detector_manifest_to_recognition_result(
  manifest: &DetectionEvidenceManifest,
  context: &DetectorRecognitionRuntimeContext,
  policy: &DetectorRecognitionBridgePolicy,
) -> Result<RecognitionResult, DetectorRecognitionMappingError> {
  validate_mapping_inputs(manifest, context)?;

  let all = manifest
    .detection_set
    .detections
    .iter()
    .enumerate()
    .map(|(index, detection)| projected_item(index, detection, manifest, context))
    .collect::<Vec<_>>();
  let filtered = all
    .iter()
    .filter(|item| item_passes_filter(item, policy))
    .cloned()
    .collect::<Vec<_>>();
  let best = select_best(&filtered, policy);

  Ok(RecognitionResult {
    recognition_id: context.recognition_id.clone(),
    source: RecognitionSource::Custom,
    scope: context.scope.clone(),
    best,
    filtered,
    all,
    detail: json!({
      "bridge_policy_version": BRIDGE_POLICY_VERSION,
      "bridge_filter_strategy": bridge_filter_strategy(policy),
      "bridge_best_selection_strategy": policy.best_selection,
      "manifest_version": "detection_evidence_manifest_v0",
      "backend": manifest.model_run.backend,
      "model_id": manifest.model_run.model_id.0,
      "confidence_threshold": manifest.model_run.confidence_threshold,
      "iou_threshold": manifest.model_run.iou_threshold,
      "class_label_source": class_label_source_detail(&manifest.model_run.class_label_source),
      "execution_provider": manifest.model_run.execution_provider,
      "runtime_projection": context.projection.kind,
      "source_image_coordinate_space": manifest.source_image.coordinate_space,
    }),
    evidence: context.evidence.clone(),
    known_limits: carried_known_limits(&manifest.known_limits),
  })
}

fn validate_mapping_inputs(
  manifest: &DetectionEvidenceManifest,
  context: &DetectorRecognitionRuntimeContext,
) -> Result<(), DetectorRecognitionMappingError> {
  if context.evidence.is_empty() {
    return Err(DetectorRecognitionMappingError::MissingRuntimeEvidence);
  }
  if context.scope.capture_artifact.is_none() {
    return Err(DetectorRecognitionMappingError::MissingCaptureArtifact);
  }
  if manifest.detection_set.image_size != context.source_image_size {
    return Err(DetectorRecognitionMappingError::SourceImageSizeMismatch {
      manifest: manifest.detection_set.image_size,
      runtime: context.source_image_size,
    });
  }
  if manifest.source_image.coordinate_space != DetectionCoordinateSpace::SourceImagePixels {
    return Err(DetectorRecognitionMappingError::UnsupportedCoordinateSpace(
      manifest.source_image.coordinate_space,
    ));
  }
  match (
    &manifest.source_image.projection_basis,
    &context.projection.kind,
  ) {
    (_, RuntimeProjectionKind::Unavailable { .. }) => {
      Err(DetectorRecognitionMappingError::ProjectionUnavailable)
    }
    (ProjectionBasis::Unavailable { .. }, RuntimeProjectionKind::IdentitySourceImagePixels) => {
      Ok(())
    }
  }
}

fn projected_item(
  index: usize,
  detection: &auv_inference_common::Detection,
  manifest: &DetectionEvidenceManifest,
  context: &DetectorRecognitionRuntimeContext,
) -> RecognizedItem {
  let box_ = project_box(&detection.bbox, &context.projection.kind);
  RecognizedItem {
    item_id: format!("detector:{}:{}", manifest.detection_set.model_id.0, index),
    kind: detection.label.clone(),
    box_,
    text: None,
    provider_score: Some(detection.confidence as f64),
    detail: json!({
      "class_id": detection.class_id,
      "label": detection.label,
      "model_id": manifest.detection_set.model_id.0,
      "backend": manifest.model_run.backend,
      "class_label_source": class_label_source_detail(&manifest.model_run.class_label_source),
      "bridge_policy_version": BRIDGE_POLICY_VERSION,
    }),
  }
}

fn project_box(bounds: &BoundingBox, projection: &RuntimeProjectionKind) -> RecognitionBox {
  match projection {
    RuntimeProjectionKind::Unavailable { reason } => {
      unreachable!("project_box called without runtime projection: {reason}")
    }
    // NOTICE(detector-runtime-identity-projection-v0): v0 only permits mapping
    // when the caller truthfully states that runtime capture space is identical
    // to source-image pixel space. General display/window projection is
    // deferred until an owner-approved capture-integrated slice lands.
    RuntimeProjectionKind::IdentitySourceImagePixels => RecognitionBox {
      x: bounds.x1.floor() as i64,
      y: bounds.y1.floor() as i64,
      width: (bounds.x2 - bounds.x1).max(0.0).round() as i64,
      height: (bounds.y2 - bounds.y1).max(0.0).round() as i64,
    },
  }
}

fn item_passes_filter(item: &RecognizedItem, policy: &DetectorRecognitionBridgePolicy) -> bool {
  match &policy.allowed_labels {
    Some(allowed) => allowed.contains(&item.kind),
    None => true,
  }
}

fn select_best(
  filtered: &[RecognizedItem],
  policy: &DetectorRecognitionBridgePolicy,
) -> Option<RecognizedItem> {
  match policy.best_selection {
    BestSelectionStrategy::None => None,
    BestSelectionStrategy::SingleFilteredItem if filtered.len() == 1 => filtered.first().cloned(),
    BestSelectionStrategy::SingleFilteredItem => None,
  }
}

fn bridge_filter_strategy(policy: &DetectorRecognitionBridgePolicy) -> &'static str {
  if policy.allowed_labels.is_some() {
    "allowed_labels"
  } else {
    "pass_through"
  }
}

fn carried_known_limits(manifest_limits: &[String]) -> Vec<String> {
  let mut known_limits = manifest_limits.to_vec();
  known_limits.push(
    "detector provider_score preserves model confidence semantics, not semantic success"
      .to_string(),
  );
  known_limits.push(
    "detector RecognitionResult is recognition evidence only, not candidate-ready output"
      .to_string(),
  );
  known_limits
}

fn class_label_source_detail(source: &ClassLabelSource) -> serde_json::Value {
  match source {
    ClassLabelSource::OverrideFile { path } => {
      json!({ "kind": "override_file", "path": path })
    }
    ClassLabelSource::EmbeddedModelMetadata => json!({ "kind": "embedded_model_metadata" }),
    ClassLabelSource::InlineList => json!({ "kind": "inline_list" }),
    ClassLabelSource::Unknown => json!({ "kind": "unknown" }),
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeSet;
  use std::path::PathBuf;

  use auv_inference_common::{
    BoundingBox, ClassLabelSource, Detection, DetectionCoordinateSpace, DetectionEvidenceManifest,
    DetectionSet, ImageSize, ModelId, ModelRunMetadata, ProjectionBasis, SourceImageEvidence,
    SourceImageRef,
  };
  use serde_json::json;

  use super::{
    BestSelectionStrategy, DetectorRecognitionBridgePolicy, DetectorRecognitionMappingError,
    DetectorRecognitionRuntimeContext, RuntimeProjection, RuntimeProjectionKind,
    map_detector_manifest_to_recognition_result,
  };
  use crate::contract::{ArtifactRef, RatioRegion, RecognitionScope, RecognitionSurface};
  use crate::trace::{ArtifactId, EventId, RunId, SpanId};

  fn sample_manifest() -> DetectionEvidenceManifest {
    DetectionEvidenceManifest {
      detection_set: DetectionSet {
        model_id: ModelId("games-balatro-2024-yolo-ui-detection".to_string()),
        image_size: ImageSize {
          width: 1280,
          height: 660,
        },
        detections: vec![
          Detection {
            class_id: 28,
            label: "ui_score_chips".to_string(),
            confidence: 0.9867,
            bbox: BoundingBox {
              x1: 108.67,
              y1: 357.69,
              x2: 213.18,
              y2: 411.45,
            },
          },
          Detection {
            class_id: 19,
            label: "ui_score_mult".to_string(),
            confidence: 0.9102,
            bbox: BoundingBox {
              x1: 220.0,
              y1: 357.0,
              x2: 288.0,
              y2: 411.0,
            },
          },
        ],
      },
      source_image: SourceImageEvidence {
        source_image_ref: SourceImageRef::LocalPath {
          path: PathBuf::from("/tmp/balatro-ui.png"),
        },
        coordinate_space: DetectionCoordinateSpace::SourceImagePixels,
        projection_basis: ProjectionBasis::Unavailable {
          reason: "manifest alone does not carry runtime projection".to_string(),
        },
      },
      model_run: ModelRunMetadata {
        backend: "ultralytics-inference".to_string(),
        model_id: ModelId("games-balatro-2024-yolo-ui-detection".to_string()),
        confidence_threshold: 0.25,
        iou_threshold: 0.45,
        class_label_source: ClassLabelSource::OverrideFile {
          path: PathBuf::from("/tmp/classes.txt"),
        },
        execution_provider: Some("cpu".to_string()),
      },
      known_limits: vec![
        "source image identity is inference-scoped, not a runtime artifact".to_string(),
        "source-image coordinate basis requires projection before action".to_string(),
      ],
    }
  }

  fn sample_artifact_ref() -> ArtifactRef {
    ArtifactRef {
      run_id: RunId::new("run_123"),
      artifact_id: ArtifactId::new("artifact_capture"),
      span_id: SpanId::new("span_01"),
      captured_event_id: Some(EventId::new("event_01")),
    }
  }

  fn sample_context() -> DetectorRecognitionRuntimeContext {
    let capture_artifact = sample_artifact_ref();
    DetectorRecognitionRuntimeContext {
      recognition_id: "recognition_detector_balatro_ui".to_string(),
      scope: RecognitionScope {
        surface: RecognitionSurface::Region,
        display_ref: Some("display-main".to_string()),
        native_display_id: Some("69733248".to_string()),
        app_bundle_id: Some("com.playstack.balatro".to_string()),
        window_title: Some("Balatro".to_string()),
        window_number: Some(42),
        region_hint: Some(RatioRegion {
          left: 0.0,
          top: 0.0,
          right: 1.0,
          bottom: 1.0,
        }),
        capture_artifact: Some(capture_artifact.clone()),
        capture_contract_artifact: Some(ArtifactRef {
          run_id: RunId::new("run_123"),
          artifact_id: ArtifactId::new("artifact_contract"),
          span_id: SpanId::new("span_01"),
          captured_event_id: Some(EventId::new("event_02")),
        }),
      },
      evidence: vec![
        capture_artifact,
        ArtifactRef {
          run_id: RunId::new("run_123"),
          artifact_id: ArtifactId::new("artifact_contract"),
          span_id: SpanId::new("span_01"),
          captured_event_id: Some(EventId::new("event_02")),
        },
      ],
      source_image_size: ImageSize {
        width: 1280,
        height: 660,
      },
      projection: RuntimeProjection {
        kind: RuntimeProjectionKind::IdentitySourceImagePixels,
      },
    }
  }

  #[test]
  fn mapping_requires_runtime_projection_context() {
    let manifest = sample_manifest();
    let mut context = sample_context();
    context.projection = RuntimeProjection {
      kind: RuntimeProjectionKind::Unavailable {
        reason: "manifest-only smoke has no runtime projection".to_string(),
      },
    };

    let error = map_detector_manifest_to_recognition_result(
      &manifest,
      &context,
      &DetectorRecognitionBridgePolicy::default(),
    )
    .expect_err("projection-unavailable runtime context should be rejected");

    assert_eq!(
      error,
      DetectorRecognitionMappingError::ProjectionUnavailable
    );
  }

  #[test]
  fn manifest_only_mapping_requires_runtime_evidence() {
    let manifest = sample_manifest();
    let mut context = sample_context();
    context.evidence.clear();

    let error = map_detector_manifest_to_recognition_result(
      &manifest,
      &context,
      &DetectorRecognitionBridgePolicy::default(),
    )
    .expect_err("manifest-only input should be rejected");

    assert_eq!(
      error,
      DetectorRecognitionMappingError::MissingRuntimeEvidence
    );
  }

  #[test]
  fn mapping_requires_scope_capture_artifact() {
    let manifest = sample_manifest();
    let mut context = sample_context();
    context.scope.capture_artifact = None;

    let error = map_detector_manifest_to_recognition_result(
      &manifest,
      &context,
      &DetectorRecognitionBridgePolicy::default(),
    )
    .expect_err("scope without capture artifact should be rejected");

    assert_eq!(
      error,
      DetectorRecognitionMappingError::MissingCaptureArtifact
    );
  }

  #[test]
  fn mapping_rejects_source_image_size_mismatch() {
    let manifest = sample_manifest();
    let mut context = sample_context();
    context.source_image_size = ImageSize {
      width: 1440,
      height: 900,
    };

    let error = map_detector_manifest_to_recognition_result(
      &manifest,
      &context,
      &DetectorRecognitionBridgePolicy::default(),
    )
    .expect_err("size mismatch should be rejected");

    assert_eq!(
      error,
      DetectorRecognitionMappingError::SourceImageSizeMismatch {
        manifest: ImageSize {
          width: 1280,
          height: 660,
        },
        runtime: ImageSize {
          width: 1440,
          height: 900,
        },
      }
    );
  }

  #[test]
  fn mapping_projects_detector_manifest_into_recognition_result() {
    let manifest = sample_manifest();
    let context = sample_context();
    let policy = DetectorRecognitionBridgePolicy {
      allowed_labels: Some(BTreeSet::from(["ui_score_chips".to_string()])),
      best_selection: BestSelectionStrategy::None,
    };

    let result = map_detector_manifest_to_recognition_result(&manifest, &context, &policy)
      .expect("valid runtime context should map into RecognitionResult");

    assert_eq!(result.source, crate::contract::RecognitionSource::Custom);
    assert_eq!(result.evidence, context.evidence);
    assert_eq!(result.best, None);
    assert_eq!(result.all.len(), 2);
    assert_eq!(result.filtered.len(), 1);
    assert_eq!(result.filtered[0].kind, "ui_score_chips");
    assert_eq!(result.filtered[0].text, None);
    let provider_score = result.filtered[0]
      .provider_score
      .expect("projected detection should preserve provider score");
    assert!((provider_score - 0.9867_f64).abs() < 0.000_001);
    assert_eq!(
      result.filtered[0].box_,
      crate::contract::RecognitionBox {
        x: 108,
        y: 357,
        width: 105,
        height: 54,
      }
    );
    assert_eq!(result.detail["backend"], json!("ultralytics-inference"));
    assert_eq!(
      result.detail["model_id"],
      json!("games-balatro-2024-yolo-ui-detection")
    );
    assert_eq!(
      result.detail["class_label_source"]["kind"],
      json!("override_file")
    );
    assert_eq!(
      result.detail["bridge_policy_version"],
      json!("detector-manifest-recognitionresult.v0")
    );
    let detail_string = serde_json::to_string(&result.detail).expect("detail should serialize");
    assert!(!detail_string.contains("candidate"));
    assert!(!detail_string.contains("action"));
    assert!(!detail_string.contains("click"));
    assert!(
      result
        .known_limits
        .contains(&"source image identity is inference-scoped, not a runtime artifact".to_string())
    );
    assert!(
      result.known_limits.contains(
        &"detector RecognitionResult is recognition evidence only, not candidate-ready output"
          .to_string()
      )
    );
  }

  #[test]
  fn mapping_only_selects_best_when_policy_explicitly_allows_single_winner() {
    let mut manifest = sample_manifest();
    manifest.detection_set.detections.truncate(1);
    let context = sample_context();
    let policy = DetectorRecognitionBridgePolicy {
      allowed_labels: Some(BTreeSet::from(["ui_score_chips".to_string()])),
      best_selection: BestSelectionStrategy::SingleFilteredItem,
    };

    let result = map_detector_manifest_to_recognition_result(&manifest, &context, &policy)
      .expect("single filtered item should map");

    assert_eq!(result.filtered.len(), 1);
    assert!(result.best.is_some());
    assert_eq!(
      result.best.as_ref().map(|item| item.item_id.as_str()),
      Some("detector:games-balatro-2024-yolo-ui-detection:0")
    );
  }
}
