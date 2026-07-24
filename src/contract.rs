// File: src/contract.rs
//! Shared recognition and candidate contracts used across AUV.
//!
//! This module defines typed evidence objects such as `RecognitionResult` and
//! `CandidateRef` that can be persisted as artifacts and consumed by
//! higher-level operations.
//!
//! Intentionally data-only: these structs describe recognition and candidate
//! lineage, but do not execute actions or define app-specific verification.
//!
//! # Seam map
//!
//! These records terminate the v0 execution seam called out in `CLAUDE.md`:
//!
//! ```text
//! recognition / AX / candidates
//!   -> auv-driver InputActionResult
//!        (standalone `input-action-result` artifact; read via
//!         `run_read::extract_input_action_results`)
//!   -> app-owned typed result/events
//!   -> tracing artifacts
//!        (src/run_read/mod.rs reads them back via
//!         typed purpose-specific readers)
//! ```
//!
//! The archived candidate-action `ActionResolverDecision` schema was removed.
//! Current input delivery evidence is the standalone `InputActionResult`
//! artifact plus separate app-owned verification records.
//! Do not introduce a replacement action-result schema without owner approval.
//!
//! Reader-side `api_version` rejection is deferred; see
//! `NOTICE(contract-api-version-reader-check)` immediately below.

use auv_tracing::ArtifactUri;
use serde::{Deserialize, Serialize};

// NOTICE(contract-api-version-reader-check): producer-side stamping landed
// in commit be0aab7 but the reader side does not yet reject artifacts
// whose api_version is unknown. `run_read::extract_*` deserializes any
// shape that satisfies `serde(default = "...")`, which means a future
// `auv.*.v1alpha2` artifact would currently parse as v1alpha1 by accident
// instead of being skipped. The check is deferred until either (a) a
// non-additive v1alpha2 actually needs to land, or (b) the owner asks
// for the reader-side discriminator as its own slice. Adding it now
// without a real second version would be untestable.

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateRef {
  pub source_scan_uri: ArtifactUri,
  pub candidate_local_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FreshnessBasis {
  pub source_artifact_uri: ArtifactUri,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecognitionResult {
  pub recognition_id: String,
  pub source: RecognitionSource,
  pub provenance: RecognitionProvenance,
  pub scope: RecognitionScope,
  pub best: Option<RecognizedItem>,
  pub filtered: Vec<RecognizedItem>,
  pub all: Vec<RecognizedItem>,
  pub evidence_artifacts: Vec<ArtifactUri>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecognitionProvenance {
  pub producer: String,
  pub model_id: Option<String>,
  pub execution_provider: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecognizedItem {
  pub item_id: String,
  pub kind: String,
  #[serde(rename = "box")]
  pub box_: RecognitionBox,
  pub text: Option<String>,
  pub provider_score: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecognitionBox {
  pub x: i64,
  pub y: i64,
  pub width: i64,
  pub height: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecognitionScope {
  pub surface: RecognitionSurface,
  pub display_ref: Option<String>,
  pub native_display_id: Option<String>,
  pub app_bundle_id: Option<String>,
  pub window_title: Option<String>,
  pub window_number: Option<i64>,
  pub region_hint: Option<RatioRegion>,
  pub capture_artifact_uri: Option<ArtifactUri>,
  pub capture_contract_artifact_uri: Option<ArtifactUri>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecognitionSource {
  OcrText,
  OcrRow,
  VisualRow,
  SegmentedRegion,
  IconMatch,
  Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecognitionSurface {
  Screen,
  Display,
  Window,
  Region,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
  pub candidate_local_id: String,
  pub kind: String,
  pub label: Option<String>,
  pub target_spec: TargetSpec,
  pub evidence: CandidateEvidence,
  pub liveness: CandidateLiveness,
  pub control: ControlRequirements,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidateQuery {
  pub query_id: String,
  pub selector: SurfaceSelector,
  pub output_kind: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceSelector {
  pub any_of: Vec<SurfaceSelectorClause>,
  pub within: SelectorScope,
  pub require_visible: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum SurfaceSelectorClause {
  Ax {
    role: Option<String>,
    label: Option<String>,
    path: Option<String>,
    enabled: Option<bool>,
    visible: Option<bool>,
  },
  Ocr {
    text: String,
    region_hint: Option<RatioRegion>,
    min_provider_score: Option<f64>,
  },
  Row {
    row_index: Option<usize>,
    contains_text: Option<String>,
    region_hint: Option<RatioRegion>,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorScope {
  ActiveWindow,
  TargetWindow,
  CaptureRegion,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TargetSpec {
  pub grounding: TargetGrounding,
  pub anchor_text: Option<String>,
  pub region_hint: Option<RatioRegion>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub row_index: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetGrounding {
  OcrAnchor,
  VisualRow,
  AxNode,
  Coordinate,
}

/// NOTICE(contract-ratio-region-rect-duplication-v0):
///
/// `RatioRegion` and `auv_driver::geometry::RatioRect` carry the same
/// concept (axis-aligned rectangle expressed as ratios of a containing
/// space) with the same f64 storage size, which the workspace
/// primitive-reuse guideline (AGENTS.md, commit 7b520c0) calls out as
/// a duplicate that should normally be collapsed onto the existing
/// primitive.
///
/// v0 keeps both because the **wire shapes differ**:
///
/// - `RatioRegion` serializes LRBT:
///   `{"left":…,"top":…,"right":…,"bottom":…}`. Stored
///   `CandidateQuery` / `SurfaceSelectorClause::Ocr.region_hint`
///   JSON uses this shape, so switching the wire layout would parse
///   historical artifacts incorrectly.
/// - `auv_driver::geometry::RatioRect` serializes XYWH:
///   `{"x":…,"y":…,"width":…,"height":…}`. It is used by driver
///   capture / window geometry APIs and was reused by
///   `auv-netease-music` for the CLI `--sidebar-region` flag in
///   commit `3196cfe`.
///
/// Mirrors the same trade-off documented for `ViewBounds` vs `Rect` in
/// `crates/auv-view/src/lib.rs::ViewBounds`
/// (`NOTICE(view-bounds-rect-duplication-v0)`, commit `2c1a382`).
///
/// Unification therefore needs a wire-shape migration plan (versioned
/// reader, fixture re-records, possibly a serde adapter) before this
/// duplicate type can be deleted. Until that lands, do not "fix" this
/// by adding a `From<RatioRect>` for `RatioRegion` (or vice versa)
/// here — `contract.rs` must stay free of `auv-driver` so the type
/// surface stays platform-agnostic, and an automatic conversion would
/// hide the wire-shape boundary that a future migration needs to
/// preserve.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RatioRegion {
  pub left: f64,
  pub top: f64,
  pub right: f64,
  pub bottom: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateEvidence {
  pub source_artifact_uri: ArtifactUri,
  pub recognition_id: String,
  pub item_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidateLiveness {
  pub preconditions: LivenessPreconditions,
  pub ttl_hint_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LivenessPreconditions {
  pub window_ref: Option<WindowRefPrecondition>,
  pub anchor_recheck: Option<AnchorRecheckPrecondition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowRefPrecondition {
  pub app_bundle_id: String,
  pub window_title_substring: Option<String>,
  pub window_number: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnchorRecheckPrecondition {
  pub text: String,
  pub region_hint: Option<RatioRegion>,
  pub expected_min_confidence: f64,
  pub max_pixel_distance: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlRequirements {
  pub requires_app_frontmost: bool,
  pub requires_window_focus: bool,
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_tracing::{ArtifactId, RunId};
  use serde_json::json;

  fn artifact_ref() -> ArtifactUri {
    ArtifactUri::from_ids(RunId::new(), ArtifactId::new())
  }

  #[test]
  fn artifact_ref_is_owned_by_canonical_tracing_boundary() {
    fn accepts_canonical_ref(_value: ArtifactUri) {}

    let artifact_ref = ArtifactUri::from_ids(RunId::new(), ArtifactId::new());

    accepts_canonical_ref(artifact_ref);
  }

  #[test]
  fn artifact_ref_round_trips_without_inline_timestamp() {
    let reference = artifact_ref();
    let value = serde_json::to_value(&reference).expect("artifact ref should serialize");

    assert_eq!(value, json!(reference.to_string()));

    let parsed: ArtifactUri = serde_json::from_value(value).expect("artifact ref should deserialize");
    assert_eq!(parsed, reference);
  }

  #[test]
  fn artifact_ref_serializes_only_the_typed_uri() {
    let artifact_ref = ArtifactUri::from_ids(RunId::new(), ArtifactId::new());

    let value = serde_json::to_value(&artifact_ref).expect("artifact ref should serialize");

    assert!(value.is_string());
    assert_eq!(value, json!(artifact_ref.to_string()));
  }

  #[test]
  fn candidate_ref_round_trips_with_one_typed_source_uri() {
    let reference = CandidateRef {
      source_scan_uri: ArtifactUri::from_ids(RunId::new(), ArtifactId::new()),
      candidate_local_id: "row#1".to_string(),
    };

    let value = serde_json::to_value(&reference).expect("candidate ref should serialize");
    assert_eq!(value["source_scan_uri"], json!(reference.source_scan_uri.to_string()));
    for duplicate in [
      "source_run_id",
      "source_span_id",
      "source_operation_id",
      "source_artifact_id",
    ] {
      assert!(value.get(duplicate).is_none(), "CandidateRef must not split source identity into {duplicate}");
    }
    assert_eq!(value["candidate_local_id"], json!("row#1"));
    assert!(value.get("candidate_id").is_none());

    let parsed: CandidateRef = serde_json::from_value(value).expect("candidate ref should deserialize");
    assert_eq!(parsed, reference);
  }

  #[test]
  fn candidate_query_round_trips_minimal_cross_surface_selector() {
    let query = CandidateQuery {
      query_id: "play-control".to_string(),
      selector: SurfaceSelector {
        any_of: vec![
          SurfaceSelectorClause::Ax {
            role: Some("AXButton".to_string()),
            label: Some("播放".to_string()),
            path: None,
            enabled: Some(true),
            visible: Some(true),
          },
          SurfaceSelectorClause::Ocr {
            text: "播放".to_string(),
            region_hint: Some(RatioRegion {
              left: 0.18,
              top: 0.28,
              right: 0.60,
              bottom: 0.42,
            }),
            min_provider_score: Some(0.75),
          },
          SurfaceSelectorClause::Row {
            row_index: Some(1),
            contains_text: None,
            region_hint: None,
          },
        ],
        within: SelectorScope::TargetWindow,
        require_visible: true,
      },
      output_kind: Some("button".to_string()),
    };

    let value = serde_json::to_value(&query).expect("candidate query should serialize");
    assert_eq!(value["selector"]["within"], json!("target_window"));
    assert_eq!(value["selector"]["any_of"][0]["source"], json!("ax"));
    assert_eq!(value["selector"]["any_of"][1]["source"], json!("ocr"));
    assert_eq!(value["selector"]["any_of"][2]["source"], json!("row"));
    assert_eq!(value["selector"]["any_of"][1]["min_provider_score"], json!(0.75));
    assert!(value["selector"]["any_of"][1].get("confidence").is_none());

    let parsed: CandidateQuery = serde_json::from_value(value).expect("candidate query should deserialize");
    assert_eq!(parsed, query);
  }

  #[test]
  fn recognition_result_round_trips_populated_best_filtered_and_all() {
    let capture_artifact_uri = artifact_ref();
    let contract_artifact = ArtifactUri::from_ids(RunId::new(), ArtifactId::new());
    let best = RecognizedItem {
      item_id: "item_best".to_string(),
      kind: "ocr_text".to_string(),
      box_: RecognitionBox {
        x: 2155,
        y: 1402,
        width: 170,
        height: 24,
      },
      text: Some("Cure For Me".to_string()),
      provider_score: Some(0.97),
    };
    let filtered = RecognizedItem {
      item_id: "item_filtered".to_string(),
      kind: "ocr_text".to_string(),
      box_: RecognitionBox {
        x: 2140,
        y: 1440,
        width: 196,
        height: 22,
      },
      text: Some("A Temporary High".to_string()),
      provider_score: Some(0.84),
    };
    let rejected = RecognizedItem {
      item_id: "item_rejected".to_string(),
      kind: "ocr_text".to_string(),
      box_: RecognitionBox {
        x: 1980,
        y: 1328,
        width: 140,
        height: 19,
      },
      text: None,
      provider_score: Some(0.31),
    };
    let result = RecognitionResult {
      recognition_id: "recognition_window_rows_01".to_string(),
      source: RecognitionSource::OcrRow,
      provenance: RecognitionProvenance {
        producer: "vision_ocr.window_rows".to_string(),
        model_id: None,
        execution_provider: None,
      },
      scope: RecognitionScope {
        surface: RecognitionSurface::Window,
        display_ref: Some("display-main".to_string()),
        native_display_id: Some("69733248".to_string()),
        app_bundle_id: Some("com.tencent.QQMusicMac".to_string()),
        window_title: Some("QQ音乐".to_string()),
        window_number: Some(42),
        region_hint: Some(RatioRegion {
          left: 0.18,
          top: 0.28,
          right: 0.82,
          bottom: 0.92,
        }),
        capture_artifact_uri: Some(capture_artifact_uri.clone()),
        capture_contract_artifact_uri: Some(contract_artifact.clone()),
      },
      best: Some(best.clone()),
      filtered: vec![best.clone(), filtered.clone()],
      all: vec![best.clone(), filtered.clone(), rejected.clone()],
      evidence_artifacts: vec![capture_artifact_uri.clone(), contract_artifact.clone()],
    };

    let value = serde_json::to_value(&result).expect("recognition result should serialize");
    assert_eq!(value["source"], json!("ocr_row"));
    assert_eq!(value["scope"]["surface"], json!("window"));
    assert_eq!(value["provenance"]["producer"], json!("vision_ocr.window_rows"));
    assert_eq!(value["best"]["box"]["x"], json!(2155));
    assert_eq!(value["filtered"][1]["box"]["width"], json!(196));
    assert!(value["all"][2].get("detail").is_none());
    assert_eq!(value["best"]["provider_score"], json!(0.97));
    assert!(value["best"].get("box_").is_none());
    assert!(value.get("confidence").is_none());

    let parsed: RecognitionResult = serde_json::from_value(value).expect("recognition result should deserialize");
    assert_eq!(parsed, result);
  }

  #[test]
  fn recognition_result_round_trips_with_empty_filtered_and_all() {
    let result = RecognitionResult {
      recognition_id: "recognition_empty".to_string(),
      source: RecognitionSource::VisualRow,
      provenance: RecognitionProvenance {
        producer: "visual_rows".to_string(),
        model_id: None,
        execution_provider: None,
      },
      scope: RecognitionScope {
        surface: RecognitionSurface::Region,
        display_ref: None,
        native_display_id: None,
        app_bundle_id: Some("com.tencent.QQMusicMac".to_string()),
        window_title: None,
        window_number: None,
        region_hint: Some(RatioRegion {
          left: 0.22,
          top: 0.30,
          right: 0.88,
          bottom: 0.76,
        }),
        capture_artifact_uri: None,
        capture_contract_artifact_uri: None,
      },
      best: None,
      filtered: Vec::new(),
      all: Vec::new(),
      evidence_artifacts: Vec::new(),
    };

    let value = serde_json::to_value(&result).expect("empty recognition result should serialize");
    assert_eq!(value["source"], json!("visual_row"));
    assert_eq!(value["scope"]["surface"], json!("region"));
    assert_eq!(value["best"], serde_json::Value::Null);
    assert_eq!(value["filtered"], json!([]));
    assert_eq!(value["all"], json!([]));

    let parsed: RecognitionResult = serde_json::from_value(value).expect("empty recognition result should deserialize");
    assert_eq!(parsed, result);
  }

  #[test]
  fn visual_row_candidate_serializes_row_index_without_anchor_recheck() {
    let artifact = artifact_ref();
    let candidate = Candidate {
      candidate_local_id: "row#2".to_string(),
      kind: "search_result_row".to_string(),
      label: Some("Visual row 2".to_string()),
      target_spec: TargetSpec {
        grounding: TargetGrounding::VisualRow,
        anchor_text: None,
        region_hint: Some(RatioRegion {
          left: 0.1,
          top: 0.2,
          right: 0.9,
          bottom: 0.3,
        }),
        row_index: Some(2),
      },
      evidence: CandidateEvidence {
        source_artifact_uri: artifact,
        recognition_id: "recognition-window-rows".to_string(),
        item_id: "row#2".to_string(),
      },
      liveness: CandidateLiveness {
        preconditions: LivenessPreconditions {
          window_ref: Some(WindowRefPrecondition {
            app_bundle_id: "com.tencent.QQMusicMac".to_string(),
            window_title_substring: None,
            window_number: None,
          }),
          anchor_recheck: None,
        },
        ttl_hint_ms: Some(5000),
      },
      control: ControlRequirements {
        requires_app_frontmost: true,
        requires_window_focus: true,
      },
    };

    let value = serde_json::to_value(&candidate).expect("candidate should serialize");
    assert_eq!(value["target_spec"]["grounding"], json!("visual_row"));
    assert_eq!(value["target_spec"]["row_index"], json!(2));
    assert_eq!(value["liveness"]["preconditions"]["anchor_recheck"], serde_json::Value::Null);

    let parsed: Candidate = serde_json::from_value(value).expect("candidate should deserialize");
    assert_eq!(parsed, candidate);
  }

  #[test]
  fn candidate_evidence_rejects_legacy_observation_bag() {
    let value = json!({
      "artifact_ref": artifact_ref(),
      "recognition_id": "recognition-1",
      "item_id": "row-2",
      "observation": {
        "item_id": "row-2",
        "provider": "vision_ocr.window_rows"
      }
    });

    assert!(serde_json::from_value::<CandidateEvidence>(value).is_err());
  }

  #[test]
  fn recognition_result_rejects_legacy_detail_bag() {
    let value = json!({
      "recognition_id": "recognition-1",
      "source": "visual_row",
      "provenance": {
        "producer": "fixture",
        "model_id": null,
        "execution_provider": null
      },
      "scope": {
        "surface": "window",
        "display_ref": null,
        "native_display_id": null,
        "app_bundle_id": null,
        "window_title": null,
        "window_number": null,
        "region_hint": null,
        "capture_artifact_uri": null,
        "capture_contract_artifact_uri": null
      },
      "best": null,
      "filtered": [],
      "all": [],
      "detail": { "backend": "fixture" },
      "evidence_artifacts": []
    });

    assert!(serde_json::from_value::<RecognitionResult>(value).is_err());
  }

  #[test]
  fn recognized_item_rejects_legacy_detail_bag() {
    let value = json!({
      "item_id": "row-2",
      "kind": "visual_row",
      "box": { "x": 0, "y": 10, "width": 20, "height": 5 },
      "text": "target",
      "provider_score": 0.8,
      "detail": { "fragments": ["target"] }
    });

    assert!(serde_json::from_value::<RecognizedItem>(value).is_err());
  }
}
