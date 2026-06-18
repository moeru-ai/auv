use image::RgbImage;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ModelId(pub String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImageSize {
  pub width: u32,
  pub height: u32,
}

/// Inference-scoped RGB frame input.
///
/// NOTICE: This is currently an image-backed helper for inference crates, not a
/// general AUV media/shared-contract type.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageFrame {
  pub image: RgbImage,
}

impl ImageFrame {
  pub fn new(image: RgbImage) -> Self {
    Self { image }
  }

  pub fn size(&self) -> ImageSize {
    ImageSize {
      width: self.image.width(),
      height: self.image.height(),
    }
  }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct BoundingBox {
  pub x1: f32,
  pub y1: f32,
  pub x2: f32,
  pub y2: f32,
}

impl BoundingBox {
  pub fn width(&self) -> f32 {
    self.x2 - self.x1
  }

  pub fn height(&self) -> f32 {
    self.y2 - self.y1
  }

  pub fn area(&self) -> f32 {
    self.width().max(0.0) * self.height().max(0.0)
  }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Detection {
  pub class_id: usize,
  pub label: String,
  pub confidence: f32,
  /// Bounding box in source-image pixel coordinates after upstream
  /// preprocessing/postprocessing has already been applied.
  ///
  /// NOTICE: This is inference evidence only. These coordinates are not
  /// screen-space, window-space, or replayable click coordinates, and they do
  /// not imply that AUV has capture/projection metadata for action delivery.
  pub bbox: BoundingBox,
}

/// Structured inference output produced by inference crates.
///
/// NOTICE: `DetectionSet` is not `contract::Candidate`, not
/// `OperationResult`, and does not carry action semantics, freshness,
/// liveness, or screen/window coordinate projection.
///
/// TODO(inference-candidate-bridge): If detections later need a runtime bridge,
/// add it as a separate contract slice instead of growing action semantics
/// directly into this type. The future bridge must attach durable source-image
/// artifact identity, projection basis, freshness/liveness context, and known
/// limits outside `DetectionSet`; see
/// `docs/superpowers/specs/2026-06-05-detectionset-candidate-adapter-boundary.md`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DetectionSet {
  pub model_id: ModelId,
  pub image_size: ImageSize,
  pub detections: Vec<Detection>,
}

/// Coordinate space used by detection bounding boxes inside
/// [`DetectionEvidenceManifest`].
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionCoordinateSpace {
  SourceImagePixels,
  ProjectedScreenPixels,
  ProjectedWindowPixels,
}

/// Inference-scoped source-image identity paired with a [`DetectionSet`].
///
/// NOTICE: This identifies where the source image came from for inference
/// evidence review. It is not `ArtifactRef`, not a runtime storage handle, and
/// does not imply replay or inspect support outside inference slices.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceImageRef {
  LocalPath { path: PathBuf },
  OpaqueId { id: String },
}

/// Projection context that explains whether source-image pixel detections can
/// be mapped back into a richer capture/display/window space.
///
/// TODO(inference-projection-basis-v1): If a future inference slice produces
/// detections from AUV-owned capture artifacts, add explicit capture/display or
/// window projection variants instead of overloading `DetectionSet`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectionBasis {
  Unavailable {
    reason: String,
  },
  Projected {
    basis_id: String,
    derivation_family: String,
  },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceImageEvidence {
  pub source_image_ref: SourceImageRef,
  pub coordinate_space: DetectionCoordinateSpace,
  pub projection_basis: ProjectionBasis,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClassLabelSource {
  OverrideFile { path: PathBuf },
  EmbeddedModelMetadata,
  InlineList,
  Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModelRunMetadata {
  pub backend: String,
  pub model_id: ModelId,
  pub confidence_threshold: f32,
  pub iou_threshold: f32,
  pub class_label_source: ClassLabelSource,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub execution_provider: Option<String>,
}

/// Inference-scoped evidence manifest that bundles a [`DetectionSet`] with the
/// minimum source-image metadata needed before any future runtime bridge can be
/// discussed.
///
/// NOTICE: This type is still inference-only. It is not `contract::Candidate`,
/// not `RecognitionResult`, not `OperationResult`, and not action evidence.
/// It may explain where a detection set came from, but it does not authorize
/// clicking, targeting, or runtime action consumption.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DetectionEvidenceManifest {
  pub detection_set: DetectionSet,
  pub source_image: SourceImageEvidence,
  pub model_run: ModelRunMetadata,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub known_limits: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct DetectionOptions {
  pub confidence_threshold: f32,
  pub iou_threshold: f32,
  pub max_detections: usize,
}

impl Default for DetectionOptions {
  fn default() -> Self {
    Self {
      confidence_threshold: 0.25,
      iou_threshold: 0.45,
      max_detections: 300,
    }
  }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelConfig {
  pub model_id: ModelId,
  pub model_path: PathBuf,
  pub input_size: Option<u32>,
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  fn sample_manifest() -> DetectionEvidenceManifest {
    DetectionEvidenceManifest {
      detection_set: DetectionSet {
        model_id: ModelId("games-balatro-2024-yolo-ui-detection".to_string()),
        image_size: ImageSize {
          width: 1280,
          height: 660,
        },
        detections: vec![Detection {
          class_id: 28,
          label: "ui_score_chips".to_string(),
          confidence: 0.9867,
          bbox: BoundingBox {
            x1: 108.67,
            y1: 357.69,
            x2: 213.18,
            y2: 411.45,
          },
        }],
      },
      source_image: SourceImageEvidence {
        source_image_ref: SourceImageRef::OpaqueId {
          id: "balatro-smoke-image-001".to_string(),
        },
        coordinate_space: DetectionCoordinateSpace::SourceImagePixels,
        projection_basis: ProjectionBasis::Unavailable {
          reason: "local Balatro smoke does not capture window/display projection".to_string(),
        },
      },
      model_run: ModelRunMetadata {
        backend: "ultralytics-inference".to_string(),
        model_id: ModelId("games-balatro-2024-yolo-ui-detection".to_string()),
        confidence_threshold: 0.25,
        iou_threshold: 0.45,
        class_label_source: ClassLabelSource::InlineList,
        execution_provider: Some("cpu".to_string()),
      },
      known_limits: vec![
        "source image identity is inference-scoped, not a runtime artifact".to_string(),
        "projection basis is unavailable in local smoke".to_string(),
      ],
    }
  }

  #[test]
  fn detection_evidence_manifest_serializes_source_image_pixel_space() {
    let value =
      serde_json::to_value(sample_manifest()).expect("manifest should serialize as JSON value");

    assert_eq!(
      value["source_image"]["coordinate_space"],
      json!("source_image_pixels"),
      "manifest must serialize bbox coordinate space explicitly"
    );
    assert_eq!(
      value["source_image"]["projection_basis"]["kind"],
      json!("unavailable"),
      "manifest must serialize unavailable projection explicitly"
    );
    assert_eq!(
      value["detection_set"]["model_id"],
      json!("games-balatro-2024-yolo-ui-detection"),
      "DetectionSet shape should remain nested inside the manifest"
    );
    assert_eq!(
      value["model_run"]["backend"],
      json!("ultralytics-inference"),
      "manifest must record backend identity"
    );
    assert_eq!(
      value["model_run"]["execution_provider"],
      json!("cpu"),
      "manifest must record provider information when available"
    );

    let object = value
      .as_object()
      .expect("manifest should serialize as a top-level object");
    for forbidden in [
      "candidate_ref",
      "contract_candidate",
      "action",
      "click",
      "runtime",
      "operation_result",
      "recognition_result",
    ] {
      assert!(
        !object.contains_key(forbidden),
        "manifest must stay inference-scoped and omit `{forbidden}`"
      );
    }
    assert!(
      object.get("annotated_image").is_none(),
      "annotated images remain debug aids and are not required manifest fields"
    );
  }

  #[test]
  fn detection_evidence_manifest_roundtrip_does_not_require_local_assets() {
    let value = json!({
      "detection_set": {
        "model_id": "games-balatro-2024-yolo-entities-detection",
        "image_size": {
          "width": 1280,
          "height": 660
        },
        "detections": [
          {
            "class_id": 0,
            "label": "card_description",
            "confidence": 0.9706,
            "bbox": {
              "x1": 798.98,
              "y1": 213.76,
              "x2": 963.18,
              "y2": 350.84
            }
          }
        ]
      },
      "source_image": {
        "source_image_ref": {
          "kind": "local_path",
          "path": "/private/tmp/example.png"
        },
        "coordinate_space": "source_image_pixels",
        "projection_basis": {
          "kind": "unavailable",
          "reason": "projection data not captured"
        }
      },
      "model_run": {
        "backend": "ultralytics-inference",
        "model_id": "games-balatro-2024-yolo-entities-detection",
        "confidence_threshold": 0.25,
        "iou_threshold": 0.45,
        "class_label_source": {
          "kind": "override_file",
          "path": "/private/tmp/classes.txt"
        },
        "execution_provider": "cpu"
      },
      "known_limits": [
        "projection basis is unavailable"
      ]
    });

    let parsed: DetectionEvidenceManifest = serde_json::from_value(value)
      .expect("manifest should deserialize without requiring real local assets");

    assert_eq!(
      parsed.source_image.coordinate_space,
      DetectionCoordinateSpace::SourceImagePixels
    );
    assert_eq!(parsed.detection_set.detections.len(), 1);
    assert_eq!(parsed.model_run.backend, "ultralytics-inference");
    assert_eq!(parsed.model_run.execution_provider.as_deref(), Some("cpu"));
  }

  #[test]
  fn projected_coordinate_spaces_serialize_explicitly() {
    assert_eq!(
      serde_json::to_value(DetectionCoordinateSpace::ProjectedScreenPixels)
        .expect("serialize screen coordinate space"),
      json!("projected_screen_pixels")
    );
    assert_eq!(
      serde_json::to_value(DetectionCoordinateSpace::ProjectedWindowPixels)
        .expect("serialize window coordinate space"),
      json!("projected_window_pixels")
    );
  }
}
