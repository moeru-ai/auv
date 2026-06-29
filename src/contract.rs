// File: src/contract.rs
//! Shared observation + verification contracts used across AUV.
//!
//! This module defines typed evidence objects (e.g. `RecognitionResult`,
//! `CandidateRef`, `SurfaceNode`, `NodeRef`, `VerificationResult`) that can be
//! persisted as artifacts and consumed by higher-level recipes.
//!
//! Intentionally data-only: these structs describe what was observed or
//! verified, but do not execute actions. A `NodeRef` existing here does *not*
//! mean AUV has a generic node-aware action runtime.
//!
//! # Seam map
//!
//! These records terminate the v0 execution seam called out in `CLAUDE.md`:
//!
//! ```text
//! recognition / AX / candidates
//!   -> ActionResolver
//!        (src/driver/macos/control/action_resolver.rs;
//!         `ActionResolverDecision`, pub(crate), serialize-only,
//!         records WHICH input method got picked + fallback policy)
//!   -> auv-driver InputActionResult
//!        (crates/auv-driver/src/input.rs;
//!         `InputActionResult`, pub, full serde,
//!         records the actual delivery attempts + disturbance levels)
//!   -> OperationResult / VerificationResult / ObservationSnapshot
//!        (this file; the persisted, reader-consumable records)
//!   -> trace artifacts
//!        (src/run_read.rs reads them back via `extract_verifications`
//!         and `extract_observation_snapshots`; src/recorded_operation.rs
//!         is the orchestration bridge, not a contract record)
//! ```
//!
//! `ActionResolverDecision` and `InputActionResult` are the only two
//! action-result schemas in v0. Per CLAUDE.md, a third action-result
//! schema must not be introduced beside them — extend one or escalate
//! to the owner before adding another.
//!
//! Reader-side `api_version` rejection is deferred; see
//! `NOTICE(contract-api-version-reader-check)` immediately below.

use serde::{Deserialize, Serialize};

pub use auv_tracing_driver::ArtifactRef;

use auv_tracing_driver::trace::{ArtifactId, RunId, SpanId};

// NOTICE(contract-api-version-reader-check): producer-side stamping landed
// in commit be0aab7 but the reader side does not yet reject artifacts
// whose api_version is unknown. `run_read::extract_*` deserializes any
// shape that satisfies `serde(default = "...")`, which means a future
// `auv.*.v1alpha2` artifact would currently parse as v1alpha1 by accident
// instead of being skipped. The check is deferred until either (a) a
// non-additive v1alpha2 actually needs to land, or (b) the owner asks
// for the reader-side discriminator as its own slice. Adding it now
// without a real second version would be untestable.

/// Wire-shape version of [`OperationResult`] JSON artifacts. Stamped onto
/// every produced record so readers can reject artifacts whose shape they do
/// not understand. Historical artifacts without the field deserialize as this
/// version because the shape was already `v1alpha1` before the field existed.
pub const OPERATION_RESULT_API_VERSION: &str = "auv.operation_result.v1alpha1";

/// Wire-shape version of persisted `operation-summary` JSON artifacts (API-P11).
///
/// Covers the `InvokeResult`-sourced half of the `GetOperation` projection:
/// `status`, `output_summary`, `signals`, and `failure_message`.
pub const OPERATION_SUMMARY_API_VERSION: &str = "auv.operation_summary.v1alpha2";

/// Artifact role for persisted operation summary projections (API-P11).
pub const OPERATION_SUMMARY_ARTIFACT_ROLE: &str = "operation-summary";

/// Wire-shape version of [`VerificationResult`] JSON artifacts. Same semantics
/// as [`OPERATION_RESULT_API_VERSION`].
pub const VERIFICATION_RESULT_API_VERSION: &str = "auv.verification_result.v1alpha1";

/// Wire-shape version of [`ObservationSnapshot`] JSON artifacts. Same
/// semantics as [`OPERATION_RESULT_API_VERSION`].
pub const OBSERVATION_SNAPSHOT_API_VERSION: &str = "auv.observation_snapshot.v1alpha1";

/// Artifact role for telemetry sample payloads.
pub const TELEMETRY_SAMPLE_ARTIFACT_ROLE: &str = "telemetry-sample";

/// Artifact role for Minecraft projection payloads.
pub const MINECRAFT_PROJECTION_ARTIFACT_ROLE: &str = "minecraft-projection";

fn default_operation_result_api_version() -> String {
  OPERATION_RESULT_API_VERSION.to_string()
}

fn default_verification_result_api_version() -> String {
  VERIFICATION_RESULT_API_VERSION.to_string()
}

fn default_observation_snapshot_api_version() -> String {
  OBSERVATION_SNAPSHOT_API_VERSION.to_string()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateRef {
  pub source_run_id: RunId,
  pub source_span_id: SpanId,
  pub source_operation_id: String,
  pub source_artifact_id: ArtifactId,
  pub candidate_local_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
  Completed,
  Failed,
}

/// Persisted, reader-consumable record of one typed command's outcome.
///
/// # Seam role
///
/// - **Produced by** typed driver / runtime command handlers via
///   `Runtime::record_operation` (see [`auv_tracing_driver::recorded_operation`]).
///   Action commands record the `InputActionResult` from
///   `crates/auv-driver` through the macos `ActionResolverDecision`
///   layer, then attach the resulting `OperationResult` here.
/// - **Consumed by** `run_read::extract_verifications` (which scans
///   `operation-result` JSON artifacts and lifts both top-level
///   `verifications` and the legacy `OperationOutput::Verification`),
///   and by the inspect HTTP/viewer surface that reads the same
///   artifacts.
///
/// Wire `api_version` is stamped on write but readers do not reject
/// unknown values yet — see `NOTICE(contract-api-version-reader-check)`
/// at the top of this file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OperationResult {
  /// Wire-shape version. See [`OPERATION_RESULT_API_VERSION`]. Defaults so
  /// historical artifacts without the field still parse as the current shape.
  #[serde(default = "default_operation_result_api_version")]
  pub api_version: String,
  pub run_id: RunId,
  pub status: OperationStatus,
  pub operation_id: String,
  pub evidence_artifacts: Vec<ArtifactRef>,
  pub output: OperationOutput,
  /// First-class verification claims attached to this operation. Independent
  /// of [`OperationOutput`]: any operation — observe, action, or dedicated
  /// verify — can attach one or more verifications when the world enters an
  /// expected state. Consumers MAY scan this field directly instead of pattern-
  /// matching on `output`. Serialized as empty when no claims were produced,
  /// and accepted as missing for back-compat with older OperationResult JSON.
  ///
  /// [`OperationOutput::Verification`] is retained for now so single-claim
  /// verify-only commands stay one-shape; new producers SHOULD prefer this
  /// top-level field, especially when an action wants to record a verification
  /// alongside its acknowledged output.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub verifications: Vec<VerificationResult>,
  pub freshness_basis: Option<FreshnessBasis>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OperationOutput {
  Candidates {
    candidates: Vec<Candidate>,
  },
  // `VerificationResult` is the largest variant by far (~440 B vs. ~24 B for
  // `Candidates`), and `OperationOutput` / `OperationResult` move across the
  // seam by value, so leaving it unboxed inflates every sibling. `Box<T>` is
  // serde-transparent (serializes exactly as `T`), so the wire shape on
  // `OperationResult.output` stays identical.
  Verification {
    verification: Box<VerificationResult>,
  },
  Acknowledged {
    message: Option<String>,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FreshnessBasis {
  pub source_artifact: Option<ArtifactRef>,
  pub source_operation_id: Option<String>,
  pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecognitionResult {
  pub recognition_id: String,
  pub source: RecognitionSource,
  pub scope: RecognitionScope,
  pub best: Option<RecognizedItem>,
  pub filtered: Vec<RecognizedItem>,
  pub all: Vec<RecognizedItem>,
  pub detail: serde_json::Value,
  pub evidence: Vec<ArtifactRef>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeRef {
  pub run_id: RunId,
  pub span_id: SpanId,
  pub node_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceNode {
  pub node_ref: NodeRef,
  pub kind: String,
  pub label: Option<String>,
  #[serde(rename = "box")]
  pub box_: RecognitionBox,
  pub source_artifacts: Vec<String>,
  pub recognition_id: Option<String>,
  pub recognition_source: Option<RecognitionSource>,
  pub recognition_surface: Option<RecognitionSurface>,
  pub recognized_item_id: Option<String>,
  pub recognized_item_kind: Option<String>,
  pub provider_score: Option<f64>,
  pub detail: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecognizedItem {
  pub item_id: String,
  pub kind: String,
  #[serde(rename = "box")]
  pub box_: RecognitionBox,
  pub text: Option<String>,
  pub provider_score: Option<f64>,
  pub detail: serde_json::Value,
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
  pub capture_artifact: Option<ArtifactRef>,
  pub capture_contract_artifact: Option<ArtifactRef>,
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
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidateQuery {
  pub query_id: String,
  pub selector: SurfaceSelector,
  pub output_kind: Option<String>,
  pub known_limits: Vec<String>,
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
///   JSON is full of this shape, and `OPERATION_RESULT_API_VERSION`
///   readers (see `NOTICE(contract-api-version-reader-check)` above)
///   silently fall back to v1alpha1 on unknown shapes — switching the
///   wire layout now would parse historical artifacts incorrectly.
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
pub struct CandidateEvidence {
  pub artifact_ref: ArtifactRef,
  pub observation: serde_json::Value,
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

/// First-class claim that the observed world matches an asserted state.
///
/// # Seam role
///
/// - **Produced by** any operation that wants to record a verification
///   claim — legacy `verify.*` operation-result producers, action commands
///   that succeeded semantically, or observe commands that incidentally
///   confirmed a property. Producers attach claims to
///   [`OperationResult::verifications`] (preferred) or wrap a single
///   claim into [`OperationOutput::Verification`] (legacy single-claim
///   shape).
/// - **Consumed by** `run_read::extract_verifications`, the inspect
///   server's verification panel, and the trace viewer's per-run
///   verification list.
///
/// The verification is identity-agnostic: it carries the
/// [`VerificationMethod`] taxonomy and observed-label hints, but does
/// not own the candidate or node that was verified — provenance lives
/// in the optional `consumed_*` fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerificationResult {
  /// Wire-shape version. See [`VERIFICATION_RESULT_API_VERSION`]. Defaults so
  /// historical artifacts without the field still parse as the current shape.
  #[serde(default = "default_verification_result_api_version")]
  pub api_version: String,
  /// Taxonomy of the assertion. Lets downstream tooling reason about coverage
  /// without parsing the producing command id. See [`VerificationMethod`].
  #[serde(default = "VerificationMethod::default_legacy")]
  pub method: VerificationMethod,
  pub executed: bool,
  pub state_changed: bool,
  pub semantic_matched: Option<bool>,
  pub failure_layer: Option<FailureLayer>,
  pub evidence: Vec<ArtifactRef>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub consumed_candidate_ref: Option<CandidateRef>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub consumed_node_ref: Option<NodeRef>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub consumed_recognition_artifact_ref: Option<ArtifactRef>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub consumed_recognition_id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub consumed_recognized_item_id: Option<String>,
  pub observed_label: Option<String>,
}

/// Taxonomy of [`VerificationResult`]s. AUV's value isn't "the action ran";
/// it's "the world is in the expected state, and here is the evidence". A
/// typed method makes that claim explicit instead of leaking through the
/// producing command id.
///
/// **Provisional.** Variant set may grow; `Custom` exists so producers can
/// emit verifications outside the standard set without forking the enum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VerificationMethod {
  /// Assertion: a specific text fragment is visible on the captured surface.
  /// Evidence: OCR pass over the capture.
  TextVisible,
  /// Assertion: an AX node carries the expected label / value / role.
  /// Evidence: AX snapshot.
  AxText,
  /// Assertion: the UI state changed between two captures (delta visible).
  /// Evidence: pre/post screenshots, AX diff, or both.
  StateChanged,
  /// Assertion: a previously emitted candidate is still alive (visible,
  /// addressable, with matching anchors).
  /// Evidence: re-observation of the candidate's anchor context.
  CandidateAlive,
  /// Assertion: the broader semantic goal of an action was achieved (e.g.
  /// "the track titled X is now playing").
  /// Evidence: domain-specific signals (Now Playing AX, server state, etc.).
  SemanticMatch,
  /// Assertion: a scroll/scan has reached a content boundary (top/bottom/
  /// next-section) and no further progress is expected.
  /// Evidence: stop reason + screenshot diff stability + completeness claim.
  NoProgressBoundary,
  /// Producer-defined verification kind. The `name` carries the producer's
  /// taxonomy hint (e.g. `"music.now_playing"`); downstream consumers must
  /// not pattern-match on the string for safety-critical decisions.
  Custom { name: String },
}

impl VerificationMethod {
  /// Default used when an older snapshot is deserialized without a method
  /// field. Returns [`Self::Custom { name: "legacy" }`] so the carve-out is
  /// explicit rather than silently picking a real method.
  pub fn default_legacy() -> Self {
    Self::Custom {
      name: "legacy".to_string(),
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureLayer {
  GroundingFailed,
  CandidateExpired,
  ControlFailed,
  VerificationUnreliable,
  StateChangedNoMatch,
  SemanticMismatch,
}

/// Coarse evidence source for an [`ObservationSnapshot`]. AX trees, OCR
/// fragments, visual detectors, and fused outputs all project into one
/// [`SurfaceNode`] shape — the snapshot still tags its origin so downstream
/// callers can reason about coverage and uncertainty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
  /// Snapshot derived from the macOS accessibility tree.
  Ax,
  /// Snapshot derived from OCR over a captured image.
  Ocr,
  /// Snapshot derived from visual detectors (template match, row detector,
  /// segmentation, icon match).
  Visual,
  /// Snapshot that fuses multiple sources into one merged projection.
  Merged,
}

/// **Provisional contract.** A normalized snapshot of UI observations captured
/// at one moment within a run/span. The projection target for AX trees, OCR
/// fragments, visual detector outputs, and scroll-scan list-item candidates so
/// consumers can read evidence without knowing which producer generated it.
///
/// Snapshot layout: the record carries source/scope/coordinate context, raw
/// provider blob, and known limits. The per-item observations live in the
/// `nodes` field as a list of [`SurfaceNode`]s; each node still carries its
/// own `recognition_source` so the unified shape doesn't lose finer-grained
/// origin information.
///
/// **Status: v0, first producer landed.** `scroll_scan` now emits per-page
/// `ObservationSnapshot` records inside `ScrollScanArtifact.snapshots`.
/// Other producers still emit their existing authoritative shapes
/// (`RecognitionResult`, AX snapshots, raw detector JSON). The type exists so
/// those producers can converge on one stable observed-UI projection rather
/// than diverging per producer. Field set and semantics may still shift before
/// this is marked stable.
///
/// # Seam role
///
/// - **Produced by** `scroll_scan` per page; wrapped inside the
///   `ScrollScanArtifact.snapshots` field. New producers should emit
///   `ObservationSnapshot` directly so consumers can read evidence
///   without knowing the producer.
/// - **Consumed by** `run_read::extract_observation_snapshots`, which
///   scans `scroll-scan`-role JSON artifacts and flattens their
///   `snapshots` list. The inspect HTTP/viewer surface reads from
///   that same path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObservationSnapshot {
  /// Wire-shape version. See [`OBSERVATION_SNAPSHOT_API_VERSION`]. Defaults so
  /// historical artifacts without the field still parse as the current shape.
  #[serde(default = "default_observation_snapshot_api_version")]
  pub api_version: String,
  /// Stable identifier for this snapshot. Producers should use a deterministic
  /// format such as `snapshot_{run_id}_{span_id}_{seq}` so events and
  /// artifacts can reference snapshots after the fact.
  pub snapshot_id: String,
  /// Run that captured this snapshot.
  pub run_id: RunId,
  /// Span that captured this snapshot.
  pub span_id: SpanId,
  /// Wall-clock millis-since-epoch when the snapshot was captured.
  pub captured_at_millis: u64,
  /// Coarse evidence source (`ax` / `ocr` / `visual` / `merged`).
  pub source: ObservationSource,
  /// Capture scope: surface, display, app/window, optional region.
  pub scope: RecognitionScope,
  /// Reference to the capture contract artifact that defines the coordinate
  /// system, scale, and source bounds of this snapshot. Optional because
  /// AX-only snapshots may have no pixel capture to anchor.
  pub capture_contract_ref: Option<ArtifactRef>,
  /// Evidence artifacts produced alongside the snapshot (screenshots, raw
  /// OCR JSON, AX snapshot files, detector outputs, etc.).
  pub evidence: Vec<ArtifactRef>,
  /// Per-item observed UI nodes. The unified projection.
  pub nodes: Vec<SurfaceNode>,
  /// Raw provider-specific detail blob. Consumers should not rely on this
  /// shape; it exists for debugging and forward-compat.
  pub detail: serde_json::Value,
  /// Known limitations of this snapshot. Examples: "AX tree partial:
  /// accessibility permission missing", "OCR provider score below threshold",
  /// "visual rows detected without baseline".
  pub known_limits: Vec<String>,
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  use auv_tracing_driver::trace::EventId;

  fn artifact_ref() -> ArtifactRef {
    ArtifactRef {
      run_id: RunId::new("run_123"),
      artifact_id: ArtifactId::new("artifact_01"),
      span_id: SpanId::new("span_01"),
      captured_event_id: Some(EventId::new("event_01")),
    }
  }

  #[test]
  fn artifact_ref_is_owned_by_tracing_driver_boundary() {
    fn accepts_driver_ref(_value: auv_tracing_driver::ArtifactRef) {}

    let artifact_ref = ArtifactRef {
      run_id: RunId::new("run_type_identity"),
      artifact_id: ArtifactId::new("artifact_type_identity"),
      span_id: SpanId::new("span_type_identity"),
      captured_event_id: Some(EventId::new("event_type_identity")),
    };

    accepts_driver_ref(artifact_ref);
  }

  #[test]
  fn artifact_ref_round_trips_without_inline_timestamp() {
    let value = serde_json::to_value(artifact_ref()).expect("artifact ref should serialize");

    assert_eq!(value["run_id"], json!("run_123"));
    assert_eq!(value["artifact_id"], json!("artifact_01"));
    assert_eq!(value["span_id"], json!("span_01"));
    assert_eq!(value["captured_event_id"], json!("event_01"));
    assert!(value.get("captured_at_millis").is_none());

    let parsed: ArtifactRef =
      serde_json::from_value(value).expect("artifact ref should deserialize");
    assert_eq!(parsed, artifact_ref());
  }

  #[test]
  fn artifact_ref_serializes_missing_capture_event_as_null() {
    let artifact_ref = ArtifactRef {
      run_id: RunId::new("run_123"),
      artifact_id: ArtifactId::new("artifact_01"),
      span_id: SpanId::new("span_01"),
      captured_event_id: None,
    };

    let value = serde_json::to_value(&artifact_ref).expect("artifact ref should serialize");

    assert!(value.get("captured_event_id").is_some());
    assert_eq!(value["captured_event_id"], serde_json::Value::Null);
  }

  #[test]
  fn candidate_ref_round_trips_as_cross_operation_handle() {
    let reference = CandidateRef {
      source_run_id: RunId::new("run_getter"),
      source_span_id: SpanId::new("span_getter"),
      source_operation_id: "music.search.results".to_string(),
      source_artifact_id: ArtifactId::new("artifact_candidates"),
      candidate_local_id: "row#1".to_string(),
    };

    let value = serde_json::to_value(&reference).expect("candidate ref should serialize");
    assert_eq!(value["source_run_id"], json!("run_getter"));
    assert_eq!(value["source_span_id"], json!("span_getter"));
    assert_eq!(value["source_operation_id"], json!("music.search.results"));
    assert_eq!(value["source_artifact_id"], json!("artifact_candidates"));
    assert_eq!(value["candidate_local_id"], json!("row#1"));
    assert!(value.get("candidate_id").is_none());

    let parsed: CandidateRef =
      serde_json::from_value(value).expect("candidate ref should deserialize");
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
      known_limits: vec!["dom and visual-icon backends are not part of v0".to_string()],
    };

    let value = serde_json::to_value(&query).expect("candidate query should serialize");
    assert_eq!(value["selector"]["within"], json!("target_window"));
    assert_eq!(value["selector"]["any_of"][0]["source"], json!("ax"));
    assert_eq!(value["selector"]["any_of"][1]["source"], json!("ocr"));
    assert_eq!(value["selector"]["any_of"][2]["source"], json!("row"));
    assert_eq!(
      value["selector"]["any_of"][1]["min_provider_score"],
      json!(0.75)
    );
    assert!(value["selector"]["any_of"][1].get("confidence").is_none());

    let parsed: CandidateQuery =
      serde_json::from_value(value).expect("candidate query should deserialize");
    assert_eq!(parsed, query);
  }

  #[test]
  fn recognition_result_round_trips_populated_best_filtered_and_all() {
    let capture_artifact = artifact_ref();
    let contract_artifact = ArtifactRef {
      run_id: RunId::new("run_123"),
      artifact_id: ArtifactId::new("artifact_contract"),
      span_id: SpanId::new("span_01"),
      captured_event_id: Some(EventId::new("event_02")),
    };
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
      detail: json!({
        "provider": "vision_ocr",
        "fragments": ["Cure", "For", "Me"],
      }),
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
      detail: json!({
        "provider": "vision_ocr",
        "fragments": ["A", "Temporary", "High"],
      }),
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
      detail: json!({
        "provider": "vision_ocr",
        "reject_reason": "below_min_provider_score",
      }),
    };
    let result = RecognitionResult {
      recognition_id: "recognition_window_rows_01".to_string(),
      source: RecognitionSource::OcrRow,
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
        capture_artifact: Some(capture_artifact.clone()),
        capture_contract_artifact: Some(contract_artifact.clone()),
      },
      best: Some(best.clone()),
      filtered: vec![best.clone(), filtered.clone()],
      all: vec![best.clone(), filtered.clone(), rejected.clone()],
      detail: json!({
        "provider": "vision_ocr.window_rows",
        "strategy": "ocr-first",
        "raw_match_count": 3,
      }),
      evidence: vec![capture_artifact.clone(), contract_artifact.clone()],
      known_limits: vec![
        "provider score is detector-local, not semantic truth".to_string(),
        "window scope depends on the capture contract".to_string(),
      ],
    };

    let value = serde_json::to_value(&result).expect("recognition result should serialize");
    assert_eq!(value["source"], json!("ocr_row"));
    assert_eq!(value["scope"]["surface"], json!("window"));
    assert_eq!(value["best"]["box"]["x"], json!(2155));
    assert_eq!(value["filtered"][1]["box"]["width"], json!(196));
    assert_eq!(
      value["all"][2]["detail"]["reject_reason"],
      json!("below_min_provider_score")
    );
    assert_eq!(value["best"]["provider_score"], json!(0.97));
    assert!(value["best"].get("box_").is_none());
    assert!(value.get("confidence").is_none());

    let parsed: RecognitionResult =
      serde_json::from_value(value).expect("recognition result should deserialize");
    assert_eq!(parsed, result);
  }

  #[test]
  fn recognition_result_round_trips_with_empty_filtered_and_all() {
    let result = RecognitionResult {
      recognition_id: "recognition_empty".to_string(),
      source: RecognitionSource::VisualRow,
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
        capture_artifact: None,
        capture_contract_artifact: None,
      },
      best: None,
      filtered: Vec::new(),
      all: Vec::new(),
      detail: json!({
        "provider": "visual_rows",
        "strategy": "visual-bands",
      }),
      evidence: Vec::new(),
      known_limits: vec!["no rows detected on this page".to_string()],
    };

    let value = serde_json::to_value(&result).expect("empty recognition result should serialize");
    assert_eq!(value["source"], json!("visual_row"));
    assert_eq!(value["scope"]["surface"], json!("region"));
    assert_eq!(value["best"], serde_json::Value::Null);
    assert_eq!(value["filtered"], json!([]));
    assert_eq!(value["all"], json!([]));

    let parsed: RecognitionResult =
      serde_json::from_value(value).expect("empty recognition result should deserialize");
    assert_eq!(parsed, result);
  }

  #[test]
  fn node_ref_round_trips_as_stable_scan_handle() {
    let reference = NodeRef {
      run_id: RunId::new("run_scan"),
      span_id: SpanId::new("span_scan"),
      node_id: "obs_0001_0001".to_string(),
    };

    let value = serde_json::to_value(&reference).expect("node ref should serialize");
    assert_eq!(value["run_id"], json!("run_scan"));
    assert_eq!(value["span_id"], json!("span_scan"));
    assert_eq!(value["node_id"], json!("obs_0001_0001"));

    let parsed: NodeRef = serde_json::from_value(value).expect("node ref should deserialize");
    assert_eq!(parsed, reference);
  }

  #[test]
  fn surface_node_round_trips_with_recognition_provenance() {
    let node = SurfaceNode {
      node_ref: NodeRef {
        run_id: RunId::new("run_scan"),
        span_id: SpanId::new("span_scan"),
        node_id: "obs_0001_0001".to_string(),
      },
      kind: "search_result_row".to_string(),
      label: Some("Cure For Me".to_string()),
      box_: RecognitionBox {
        x: 2155,
        y: 1402,
        width: 170,
        height: 24,
      },
      source_artifacts: vec!["artifacts/page.png".to_string()],
      recognition_id: Some("recognition_window_rows_01".to_string()),
      recognition_source: Some(RecognitionSource::OcrRow),
      recognition_surface: Some(RecognitionSurface::Window),
      recognized_item_id: Some("row#1".to_string()),
      recognized_item_kind: Some("ocr_text".to_string()),
      provider_score: Some(0.97),
      detail: json!({
        "raw_text": "Cure For Me",
        "page_index": 0,
        "text_fragments": ["Cure", "For", "Me"],
      }),
    };

    let value = serde_json::to_value(&node).expect("surface node should serialize");
    assert_eq!(value["node_ref"]["node_id"], json!("obs_0001_0001"));
    assert_eq!(value["kind"], json!("search_result_row"));
    assert_eq!(value["box"]["width"], json!(170));
    assert_eq!(value["recognition_source"], json!("ocr_row"));
    assert_eq!(value["recognized_item_id"], json!("row#1"));
    assert_eq!(value["provider_score"], json!(0.97));

    let parsed: SurfaceNode =
      serde_json::from_value(value).expect("surface node should deserialize");
    assert_eq!(parsed, node);
  }

  #[test]
  fn operation_result_with_candidate_round_trips() {
    let artifact = artifact_ref();
    let result = OperationResult {
      api_version: OPERATION_RESULT_API_VERSION.to_string(),
      run_id: RunId::new("run_123"),
      status: OperationStatus::Completed,
      operation_id: "music.search.results".to_string(),
      evidence_artifacts: vec![artifact.clone()],
      verifications: Vec::new(),
      output: OperationOutput::Candidates {
        candidates: vec![Candidate {
          candidate_local_id: "row#1".to_string(),
          kind: "search_result_row".to_string(),
          label: Some("Cure For Me".to_string()),
          target_spec: TargetSpec {
            grounding: TargetGrounding::OcrAnchor,
            anchor_text: Some("Cure For Me".to_string()),
            region_hint: Some(RatioRegion {
              left: 0.2,
              top: 0.3,
              right: 0.8,
              bottom: 0.9,
            }),
            row_index: None,
          },
          evidence: CandidateEvidence {
            artifact_ref: artifact.clone(),
            observation: json!({
              "provider": "vision_ocr",
              "text": "Cure For Me",
              "bounds": { "x": 2155, "y": 1402, "width": 170, "height": 24 }
            }),
          },
          liveness: CandidateLiveness {
            preconditions: LivenessPreconditions {
              window_ref: Some(WindowRefPrecondition {
                app_bundle_id: "com.tencent.QQMusicMac".to_string(),
                window_title_substring: None,
                window_number: Some(42),
              }),
              anchor_recheck: Some(AnchorRecheckPrecondition {
                text: "Cure For Me".to_string(),
                region_hint: None,
                expected_min_confidence: 0.5,
                max_pixel_distance: 32.0,
              }),
            },
            ttl_hint_ms: Some(5000),
          },
          control: ControlRequirements {
            requires_app_frontmost: true,
            requires_window_focus: true,
          },
          known_limits: vec!["validated only for visible ASCII anchors".to_string()],
        }],
      },
      freshness_basis: Some(FreshnessBasis {
        source_artifact: Some(artifact),
        source_operation_id: Some("debug.findWindowRows".to_string()),
        notes: vec!["window-scoped OCR rows".to_string()],
      }),
      known_limits: Vec::new(),
    };

    let value = serde_json::to_value(&result).expect("operation result should serialize");
    assert_eq!(value["status"], json!("completed"));
    assert_eq!(value["output"]["kind"], json!("candidates"));
    assert_eq!(
      value["output"]["candidates"][0]["target_spec"]["grounding"],
      json!("ocr_anchor")
    );

    let parsed: OperationResult =
      serde_json::from_value(value).expect("operation result should deserialize");
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
        artifact_ref: artifact,
        observation: json!({
          "provider": "vision_ocr.window_rows",
          "source": "visual-bands"
        }),
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
      known_limits: vec!["visual row index may drift after scrolling".to_string()],
    };

    let value = serde_json::to_value(&candidate).expect("candidate should serialize");
    assert_eq!(value["target_spec"]["grounding"], json!("visual_row"));
    assert_eq!(value["target_spec"]["row_index"], json!(2));
    assert_eq!(
      value["liveness"]["preconditions"]["anchor_recheck"],
      serde_json::Value::Null
    );

    let parsed: Candidate = serde_json::from_value(value).expect("candidate should deserialize");
    assert_eq!(parsed, candidate);
  }

  #[test]
  fn verification_result_failure_layer_uses_snake_case_contract() {
    let result = VerificationResult {
      api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
      method: VerificationMethod::SemanticMatch,
      executed: true,
      state_changed: true,
      semantic_matched: Some(false),
      failure_layer: Some(FailureLayer::StateChangedNoMatch),
      evidence: vec![artifact_ref()],
      consumed_candidate_ref: Some(CandidateRef {
        source_run_id: RunId::new("run_getter"),
        source_span_id: SpanId::new("span_getter"),
        source_operation_id: "music.search.results".to_string(),
        source_artifact_id: ArtifactId::new("artifact_candidates"),
        candidate_local_id: "row#1".to_string(),
      }),
      consumed_node_ref: Some(NodeRef {
        run_id: RunId::new("run_getter"),
        span_id: SpanId::new("span_getter"),
        node_id: "obs_0001_0001".to_string(),
      }),
      consumed_recognition_artifact_ref: Some(ArtifactRef {
        run_id: RunId::new("run_getter"),
        artifact_id: ArtifactId::new("artifact_recognition"),
        span_id: SpanId::new("span_getter"),
        captured_event_id: None,
      }),
      consumed_recognition_id: Some("music_search_results".to_string()),
      consumed_recognized_item_id: Some("row#1".to_string()),
      observed_label: Some("天空仍灿烂".to_string()),
    };

    let value = serde_json::to_value(&result).expect("verification result should serialize");
    assert_eq!(value["failure_layer"], json!("state_changed_no_match"));
    assert_eq!(
      value["consumed_candidate_ref"]["candidate_local_id"],
      json!("row#1")
    );
    assert_eq!(
      value["consumed_node_ref"]["node_id"],
      json!("obs_0001_0001")
    );
    assert_eq!(
      value["consumed_recognition_id"],
      json!("music_search_results")
    );

    let parsed: VerificationResult =
      serde_json::from_value(value).expect("verification result should deserialize");
    assert_eq!(parsed, result);
  }

  #[test]
  fn verification_method_round_trips_each_taxonomy_variant() {
    let methods = [
      VerificationMethod::TextVisible,
      VerificationMethod::AxText,
      VerificationMethod::StateChanged,
      VerificationMethod::CandidateAlive,
      VerificationMethod::SemanticMatch,
      VerificationMethod::NoProgressBoundary,
      VerificationMethod::Custom {
        name: "music.now_playing".to_string(),
      },
    ];
    for method in methods {
      let value = serde_json::to_value(&method).expect("method should serialize");
      let parsed: VerificationMethod =
        serde_json::from_value(value).expect("method should deserialize");
      assert_eq!(parsed, method);
    }
  }

  #[test]
  fn verification_method_built_in_variants_serialize_as_snake_case_kind() {
    let value =
      serde_json::to_value(&VerificationMethod::TextVisible).expect("method should serialize");
    assert_eq!(value, json!({ "kind": "text_visible" }));

    let value = serde_json::to_value(&VerificationMethod::NoProgressBoundary)
      .expect("method should serialize");
    assert_eq!(value, json!({ "kind": "no_progress_boundary" }));
  }

  #[test]
  fn verification_method_custom_variant_carries_producer_hint() {
    let value = serde_json::to_value(&VerificationMethod::Custom {
      name: "music.now_playing".to_string(),
    })
    .expect("custom method should serialize");
    assert_eq!(
      value,
      json!({ "kind": "custom", "name": "music.now_playing" })
    );
  }

  #[test]
  fn legacy_verification_result_without_method_decodes_as_custom_legacy() {
    let legacy = json!({
      "executed": true,
      "state_changed": false,
      "semantic_matched": null,
      "failure_layer": null,
      "evidence": [],
      "observed_label": null
    });
    let parsed: VerificationResult =
      serde_json::from_value(legacy).expect("legacy verification should decode");
    assert_eq!(
      parsed.method,
      VerificationMethod::Custom {
        name: "legacy".to_string()
      }
    );
  }

  fn sample_node(node_id: &str, source: RecognitionSource) -> SurfaceNode {
    SurfaceNode {
      node_ref: NodeRef {
        run_id: RunId::new("run_snapshot"),
        span_id: SpanId::new("span_snapshot"),
        node_id: node_id.to_string(),
      },
      kind: "observation".to_string(),
      label: Some("Play".to_string()),
      box_: RecognitionBox {
        x: 100,
        y: 200,
        width: 80,
        height: 24,
      },
      source_artifacts: vec!["artifacts/scan-page-0001.png".to_string()],
      recognition_id: Some("recognition_001".to_string()),
      recognition_source: Some(source),
      recognition_surface: Some(RecognitionSurface::Window),
      recognized_item_id: Some("item_001".to_string()),
      recognized_item_kind: Some("button".to_string()),
      provider_score: Some(0.91),
      detail: json!({}),
    }
  }

  fn snapshot_scope() -> RecognitionScope {
    RecognitionScope {
      surface: RecognitionSurface::Window,
      display_ref: None,
      native_display_id: None,
      app_bundle_id: Some("com.netease.163music".to_string()),
      window_title: Some("网易云音乐".to_string()),
      window_number: Some(42),
      region_hint: Some(RatioRegion {
        left: 0.0,
        top: 0.18,
        right: 1.0,
        bottom: 0.72,
      }),
      capture_artifact: Some(artifact_ref()),
      capture_contract_artifact: Some(artifact_ref()),
    }
  }

  #[test]
  fn observation_source_serializes_as_snake_case() {
    for (source, wire) in [
      (ObservationSource::Ax, "ax"),
      (ObservationSource::Ocr, "ocr"),
      (ObservationSource::Visual, "visual"),
      (ObservationSource::Merged, "merged"),
    ] {
      let value = serde_json::to_value(source).expect("source should serialize");
      assert_eq!(value, json!(wire));
      let parsed: ObservationSource =
        serde_json::from_value(json!(wire)).expect("source should deserialize");
      assert_eq!(parsed, source);
    }
  }

  #[test]
  fn observation_snapshot_round_trips_with_unified_projection() {
    let snapshot = ObservationSnapshot {
      api_version: OBSERVATION_SNAPSHOT_API_VERSION.to_string(),
      snapshot_id: "snapshot_run_001_span_001_0001".to_string(),
      run_id: RunId::new("run_001"),
      span_id: SpanId::new("span_001"),
      captured_at_millis: 1_779_090_000_000,
      source: ObservationSource::Merged,
      scope: snapshot_scope(),
      capture_contract_ref: Some(artifact_ref()),
      evidence: vec![artifact_ref()],
      nodes: vec![
        sample_node("node_ax_play", RecognitionSource::Custom),
        sample_node("node_ocr_play", RecognitionSource::OcrText),
      ],
      detail: json!({
        "fusion_strategy": "ax_then_ocr",
        "ax_node_count": 1,
        "ocr_row_count": 1,
      }),
      known_limits: vec![
        "AX tree was partial: window title bar missing".to_string(),
        "OCR provider score min was 0.40, lower than recipe expectation".to_string(),
      ],
    };

    let value = serde_json::to_value(&snapshot).expect("snapshot should serialize");
    assert_eq!(
      value["snapshot_id"],
      json!("snapshot_run_001_span_001_0001")
    );
    assert_eq!(value["source"], json!("merged"));
    assert_eq!(value["scope"]["surface"], json!("window"));
    assert_eq!(value["nodes"].as_array().expect("nodes array").len(), 2);
    assert_eq!(
      value["known_limits"]
        .as_array()
        .expect("limits array")
        .len(),
      2
    );

    let parsed: ObservationSnapshot =
      serde_json::from_value(value).expect("snapshot should deserialize");
    assert_eq!(parsed, snapshot);
  }

  #[test]
  fn observation_snapshot_allows_empty_nodes_for_negative_evidence() {
    let snapshot = ObservationSnapshot {
      api_version: OBSERVATION_SNAPSHOT_API_VERSION.to_string(),
      snapshot_id: "snapshot_negative".to_string(),
      run_id: RunId::new("run_002"),
      span_id: SpanId::new("span_002"),
      captured_at_millis: 1_779_090_001_000,
      source: ObservationSource::Ocr,
      scope: snapshot_scope(),
      capture_contract_ref: None,
      evidence: vec![artifact_ref()],
      nodes: Vec::new(),
      detail: json!({ "reason": "no_recognized_items" }),
      known_limits: vec!["OCR ran but produced no rows above min_confidence".to_string()],
    };

    let value = serde_json::to_value(&snapshot).expect("snapshot should serialize");
    let parsed: ObservationSnapshot =
      serde_json::from_value(value).expect("snapshot should deserialize");
    assert_eq!(parsed, snapshot);
    assert!(parsed.nodes.is_empty());
  }

  #[test]
  fn observation_snapshot_ax_source_can_omit_capture_contract() {
    let snapshot = ObservationSnapshot {
      api_version: OBSERVATION_SNAPSHOT_API_VERSION.to_string(),
      snapshot_id: "snapshot_ax_only".to_string(),
      run_id: RunId::new("run_003"),
      span_id: SpanId::new("span_003"),
      captured_at_millis: 1_779_090_002_000,
      source: ObservationSource::Ax,
      scope: snapshot_scope(),
      capture_contract_ref: None,
      evidence: vec![artifact_ref()],
      nodes: vec![sample_node("ax_button", RecognitionSource::Custom)],
      detail: json!({ "ax_root": "AXApplication" }),
      known_limits: Vec::new(),
    };

    let value = serde_json::to_value(&snapshot).expect("snapshot should serialize");
    assert_eq!(value["source"], json!("ax"));
    assert!(
      value["capture_contract_ref"].is_null(),
      "ax snapshot may not have a capture contract"
    );

    let parsed: ObservationSnapshot =
      serde_json::from_value(value).expect("snapshot should deserialize");
    assert_eq!(parsed, snapshot);
  }

  fn sample_verification(method: VerificationMethod) -> VerificationResult {
    VerificationResult {
      api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
      method,
      executed: true,
      state_changed: true,
      semantic_matched: Some(true),
      failure_layer: None,
      evidence: vec![artifact_ref()],
      consumed_candidate_ref: None,
      consumed_node_ref: None,
      consumed_recognition_artifact_ref: None,
      consumed_recognition_id: None,
      consumed_recognized_item_id: None,
      observed_label: Some("Now playing X".to_string()),
    }
  }

  #[test]
  fn operation_result_carries_first_class_verifications_alongside_acknowledged_output() {
    let verification = sample_verification(VerificationMethod::StateChanged);
    let result = OperationResult {
      api_version: OPERATION_RESULT_API_VERSION.to_string(),
      run_id: RunId::new("run_action"),
      status: OperationStatus::Completed,
      operation_id: "music.result.play".to_string(),
      evidence_artifacts: vec![artifact_ref()],
      output: OperationOutput::Acknowledged {
        message: Some("Issued play".to_string()),
      },
      verifications: vec![verification.clone()],
      freshness_basis: None,
      known_limits: Vec::new(),
    };

    let value = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(value["output"]["kind"], json!("acknowledged"));
    assert_eq!(
      value["verifications"][0]["method"]["kind"],
      json!("state_changed"),
      "first-class verifications must serialize with their typed method"
    );

    let parsed: OperationResult = serde_json::from_value(value).expect("result should deserialize");
    assert_eq!(parsed.verifications, vec![verification]);
  }

  #[test]
  fn legacy_operation_result_without_verifications_field_decodes_with_empty_vec() {
    let json = json!({
      "run_id": "run_legacy",
      "status": "completed",
      "operation_id": "music.search.results",
      "evidence_artifacts": [],
      "output": { "kind": "acknowledged", "message": null },
      "freshness_basis": null,
      "known_limits": []
    });

    let parsed: OperationResult =
      serde_json::from_value(json).expect("legacy result should deserialize");
    assert!(
      parsed.verifications.is_empty(),
      "missing verifications field must default to an empty list, preserving back-compat"
    );

    let reserialized = serde_json::to_value(&parsed).expect("result should re-serialize");
    assert!(
      reserialized.get("verifications").is_none(),
      "empty verifications must skip serialize to keep wire compact for legacy producers"
    );
  }

  #[test]
  fn operation_result_supports_multiple_verification_claims() {
    let result = OperationResult {
      api_version: OPERATION_RESULT_API_VERSION.to_string(),
      run_id: RunId::new("run_multi"),
      status: OperationStatus::Completed,
      operation_id: "music.result.play".to_string(),
      evidence_artifacts: vec![artifact_ref()],
      output: OperationOutput::Acknowledged { message: None },
      verifications: vec![
        sample_verification(VerificationMethod::StateChanged),
        sample_verification(VerificationMethod::SemanticMatch),
      ],
      freshness_basis: None,
      known_limits: Vec::new(),
    };

    let value = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(
      value["verifications"].as_array().map(|a| a.len()),
      Some(2),
      "multi-claim verifications must round-trip"
    );
    let parsed: OperationResult = serde_json::from_value(value).expect("result should deserialize");
    assert_eq!(parsed.verifications.len(), 2);
  }
}
