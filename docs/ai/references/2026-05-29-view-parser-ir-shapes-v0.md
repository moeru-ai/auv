# AUV View Parser IR Shapes v0

Date: 2026-05-29

Status: v0 IR types spec. Concrete shapes, **not** a re-statement of the
design rationale.

Audience: owner, reviewers, and any agent (Codex, Claude, others)
implementing view parser primitives, the NetEase example, or downstream
readers of view parser artifacts.

## Purpose

This document pins the concrete IR types for the view parser. It is a
sibling of:

- `docs/ai/references/2026-05-28-view-parser-ir-netease-playlist-example-design.md`
  — the high-level design (what & why, principles, taxonomy, NetEase
  example).
- `docs/ai/references/2026-05-28-surface-analyze-v0.md` — the surface
  candidate model and promotion gate.
- `docs/ai/references/2026-05-29-view-parser-contract-bridge-v0.md` —
  the rule that view parser uses `ArtifactRef`, `RecognitionResult`,
  `SurfaceNode`, and the surface-analyze v0 promotion gate.

The design doc says **what** the IR is. The bridge says **what existing
types it must consume**. This document says **what the IR types look
like** so the implementation has one canonical shape to target.

Cross-references:

```text
view-parser-ir-netease-playlist-example-design.md   what + why + principles
view-parser-contract-bridge-v0.md                    must-use existing contracts
view-parser-ir-shapes-v0.md          (this doc)      concrete types
surface-analyze-v0.md                                promotion gate consumed here
```

## Scope and non-scope

In scope:

- Concrete shapes for `ViewScope`, `ViewRegion`, `ViewViewport`,
  `ViewObservation`, `ViewEvidenceNode`, `ViewCandidate`, `ViewNode`,
  `ViewReconstruction`, `ViewProjection`, `ViewAnchor`, `ViewLandmark`.
- Identity / ID derivation.
- Cross-viewport merge keys.
- Artifact roles and JSON serialization rules.
- Schema versioning.
- v0 done criteria.

Out of scope:

- `ViewMemory` persistence — reserve the shape, do not implement.
- Inspect viewer rendering of these types.
- Domain-specific projection record shapes (e.g. NetEase
  `PlaylistSidebarProjection`) — those live with the example, not the
  generic IR.
- Action execution semantics — `ActionResolver` is covered by
  contract + surface-analyze docs.

## Identity types

Newtypes keep callers from mixing IDs. All serialize as plain strings.

```rust
pub struct ViewNodeId(pub String);
pub struct ViewCandidateId(pub String);
pub struct ViewEvidenceId(pub String);
pub struct ObservationIndex(pub u32);
pub struct ViewportFingerprint(pub String); // SHA-256 over normalized viewport content
```

ID derivation rules (v0, deterministic where possible):

| ID | Derivation |
|---|---|
| `ViewNodeId` for `Item` | `hash(region_id, section_hint, normalized_label)` |
| `ViewNodeId` for `Section` | `hash(region_id, section_kind_hint, position_among_sections)` |
| `ViewNodeId` for `Container` / `Collection` | `hash(region_id, kind, layout, position_in_parent)` |
| `ViewNodeId` for `Text` / `Icon` | `hash(region_id, normalized_label_or_icon_hash, parent_id)` |
| `ViewNodeId` for `Unknown` | `hash(region_id, observation_index, candidate_local_index)` — **not stable across runs** |
| `ViewCandidateId` | `hash(observation_index, candidate_local_index)` — parser-internal, not stable |
| `ViewEvidenceId` | `hash(observation_index, source, source_ref.artifact_id, evidence_local_index)` |
| `ViewportFingerprint` | SHA-256 of viewport raster region after normalization (gray + downscale + quantize) |

Hash function: SHA-256 truncated to 16 hex chars, prefixed with a short
type tag (e.g. `node-`, `cand-`, `evi-`). Implementation may choose a
cheaper hash for `ViewCandidateId` since it is parser-internal.

> **NOTICE(unknown-node-id):** `ViewNodeId` for `Unknown` is intentionally
> not stable across runs. Stabilizing it would require classifying the
> evidence first; doing so silently would turn `Unknown` into a misleading
> bucket that hides classification uncertainty. Implementations must
> surface unknowns rather than re-stabilize them.

## Coordinate space and bounds

```rust
pub enum CoordinateSpace {
  WindowLocal,
  RegionLocal,
  ViewportLocal,
  DisplayPhysical,
}

pub struct ViewBounds {
  pub origin_space: CoordinateSpace,
  pub x: i32,
  pub y: i32,
  pub width: u32,
  pub height: u32,
}

pub struct ScrollOffset {
  pub region_id: String,
  pub axis: ScrollAxis,
  pub logical_offset: i32, // relative to region top/left, can be negative for overscroll
}

pub enum ScrollAxis { Vertical, Horizontal, Both }
```

Translating between spaces is an implementation responsibility; the IR
records the space each `ViewBounds` was captured in and never silently
mixes them.

## Scope, region, viewport

```rust
pub struct ViewScope {
  pub scope_id: String,                       // e.g. "netease.main_window.sidebar"
  pub app_bundle_id: String,
  pub window_title_hint: Option<String>,
  pub source_artifacts: Vec<ArtifactRef>,     // capture, AX dump, window enumeration
}

pub struct ViewRegion {
  pub region_id: String,                      // e.g. "netease.sidebar.body"
  pub scope_id: String,
  pub bounds: ViewBounds,                     // window-local
  pub bounds_evidence: Vec<ArtifactRef>,
  pub known_limits: Vec<String>,
}

pub struct ViewViewport {
  pub viewport_id: String,
  pub region_id: String,
  pub observation_index: ObservationIndex,
  pub bounds: ViewBounds,                     // window-local
  pub scroll_offset: ScrollOffset,
  pub fingerprint: ViewportFingerprint,
}
```

## Observation

```rust
pub struct ViewObservation {
  pub observation_index: ObservationIndex,
  pub viewport: ViewViewport,
  pub source_artifacts: Vec<ArtifactRef>,     // capture, OCR result, AX dump for this pass
  pub evidence_nodes: Vec<ViewEvidenceNode>,
  pub candidates: Vec<ViewCandidate>,
  pub parser_notes: Vec<String>,
  pub schema_version: String,                 // "view-ir-v0"
}

pub struct ViewEvidenceNode {
  pub evidence_id: ViewEvidenceId,
  pub source: EvidenceSource,
  pub bounds: ViewBounds,
  pub source_ref: ArtifactRef,                // producing artifact (capture / OCR / AX)
  pub raw: serde_json::Value,                 // source-specific payload
}

pub enum EvidenceSource {
  Ax,            // payload conforms to contract::SurfaceNode
  Ocr,           // payload conforms to contract::RecognitionResult
  IconMatch,
  WindowGeometry,
  ScrollPose,
  // TODO(evidence-source-v1): Dom, CdpAccessibility, VisualSegmentation,
  // VisionLanguageModel are reserved future variants. Per bridge spec,
  // adding any requires owner approval and a corresponding row in
  // surface-analyze-v0.md's kind table. Do not silently extend.
}
```

`EvidenceSource::Ax` and `Ocr` payloads must serialize as
`contract::SurfaceNode` and `contract::RecognitionResult` respectively;
they are **not** parallel schemas. See bridge spec.

## Candidate (parser-internal)

```rust
pub struct ViewCandidate {
  pub candidate_id: ViewCandidateId,
  pub observation_index: ObservationIndex,
  pub kind_hint: ViewNodeKind,
  pub label: Option<String>,                  // normalized form, not raw OCR
  pub bounds: ViewBounds,
  pub evidence_refs: Vec<ViewEvidenceId>,     // local to this observation
  pub confidence: Confidence,
  pub parser_notes: Vec<String>,
}

pub struct Confidence {
  pub level: ConfidenceLevel,
  pub provider_scores: BTreeMap<String, f64>, // e.g. {"ocr": 0.92, "icon": 0.71}
}

pub enum ConfidenceLevel {
  Confirmed,
  Likely,
  Unknown,
  Contradicted,
}
```

`ViewCandidate` is parser-internal. It is **not** the type readers see;
it is consumed by the merge step that produces `ViewNode`. Per the
bridge, it is **not** `AppSurfaceCandidate` and must not leak into
`app analyze` reports directly.

## Reconstruction node

```rust
pub struct ViewNode {
  pub node_id: ViewNodeId,
  pub kind: ViewNodeKind,
  pub layout: ViewLayout,
  pub domain_kind: Option<String>,            // e.g. "netease.playlist_item"
  pub bounds: Option<ViewBounds>,             // containers may have no stable bounds
  pub label: Option<String>,
  pub parent: Option<ViewNodeId>,
  pub children: Vec<ViewNodeId>,              // tree edges by ID
  pub evidence_refs: Vec<EvidenceRef>,        // spans multiple observations
  pub anchors: Vec<ViewAnchor>,
  pub landmarks: Vec<ViewLandmark>,
  pub capabilities: Vec<ViewCapability>,
  pub actions: Vec<ViewAction>,
  pub confidence: Confidence,
  pub known_limits: Vec<String>,
}

pub struct EvidenceRef {
  pub observation_index: ObservationIndex,
  pub evidence_id: ViewEvidenceId,
}

pub enum ViewNodeKind {
  Container,
  Collection,
  Section,
  Item,
  Text,
  Icon,
  Unknown,
}

pub enum ViewLayout {
  VStack,
  HStack,
  Group,
}

pub enum ViewCapability {
  Scrollable(ViewScrollable),
}

pub struct ViewScrollable {
  pub axis: ScrollAxis,
  pub observed_viewports: Vec<ObservationIndex>,
  pub boundary: ScrollBoundary,
}

pub struct ScrollBoundary {
  pub top: BoundaryState,
  pub bottom: BoundaryState,
  pub repeated_viewport_fingerprints: Vec<ViewportFingerprint>,
}

pub enum BoundaryState {
  Confirmed,
  Likely,
  Unknown,
  Contradicted,
}
// TODO(boundary-promotion-rules-v1): the rules for promoting Likely to
// Confirmed (or demoting either to Contradicted) are deferred. v0
// carries the enum tag plus repeated_viewport_fingerprints evidence;
// the promotion algorithm is a separate slice that should not be
// embedded in the IR types.
```

`BoundaryState` mirrors the four-value confidence in the design doc
verbatim. Reuse it; do not invent `ScrollBoundaryConfidence`.

## Anchors and landmarks

```rust
pub struct ViewAnchor {
  pub anchor_id: String,
  pub label_hint: Option<String>,
  pub section_hint: Option<String>,
  pub bounds_hint: Option<ViewBounds>,
  pub viewport_fingerprint_hint: Option<ViewportFingerprint>,
  pub evidence_refs: Vec<EvidenceRef>,
  pub reacquire_strategy: ReacquireStrategy,
}

pub enum ReacquireStrategy {
  LabelMatch,
  LabelPlusSectionContext,
  ViewportFingerprintNeighborhood,
  AxPath,
  Mixed,
}
// TODO(reacquisition-algorithm-v1): the matching algorithm behind each
// strategy is deferred. v0 IR carries only the enum tag + hint fields;
// runtime reacquisition logic is a separate slice. Do not embed
// selection heuristics in the IR types — keep the IR descriptive, not
// procedural.

pub struct ViewLandmark {
  pub landmark_id: String,
  pub origin_node_id: ViewNodeId,
  pub purpose: LandmarkPurpose,
  pub evidence_refs: Vec<EvidenceRef>,
}

pub enum LandmarkPurpose {
  PoseEstimation,
  SectionBoundary,
  UniqueIcon,
  StableFirstVisible,
  StableLastVisible,
}
```

Per the design doc, anchors and landmarks live on nodes (and, when
useful, on evidence records). `anchor_index` and `landmark_index` in
`ViewReconstruction` are lookup indexes over these attached values, not
new owning collections.

## Action and action target

```rust
pub enum ViewAction {
  Open(ViewActionTarget),
  Select(ViewActionTarget),
  Scroll(ScrollAxis),
  ObserveOnly,
}

pub struct ViewActionTarget {
  pub kind: ViewActionTargetKind,
  pub payload: serde_json::Value,
}

pub enum ViewActionTargetKind {
  AxPath,
  WindowPoint,
  CandidateQuery,  // serialized contract::CandidateQuery from src/contract.rs
  // NOTICE(closed-action-target-kinds): the variant set is intentionally
  // limited to forms that already exist in the contract surface or that
  // the bridge spec endorses. RegionScroll, MenuShortcut, KeyboardCombo,
  // and similar are reserved for owner-approved future expansions and
  // must not be added silently.
}
```

`ViewActionTargetKind::CandidateQuery` is the bridge to surface-analyze:
when a view parser wants the action to reach `ActionResolver`, it embeds
the existing `contract::CandidateQuery` as the payload. No new selector
schema is invented.

## Reconstruction

```rust
pub struct ViewReconstruction {
  pub reconstruction_id: String,
  pub scope: ViewScope,
  pub root_node_id: ViewNodeId,
  pub nodes: BTreeMap<ViewNodeId, ViewNode>,  // flat node store
  pub anchor_index: BTreeMap<String, ViewNodeId>,
  pub landmark_index: BTreeMap<String, ViewNodeId>,
  pub observations: Vec<ObservationIndex>,    // contributing observations
  pub diagnostics: Vec<ParserDiagnostic>,
  pub known_limits: Vec<String>,
  pub schema_version: String,                 // "view-ir-v0"
}

pub struct ParserDiagnostic {
  pub kind: ParserDiagnosticKind,
  pub message: String,
  pub node_id: Option<ViewNodeId>,
  pub observation_index: Option<ObservationIndex>,
  pub evidence_refs: Vec<EvidenceRef>,
}

pub enum ParserDiagnosticKind {
  ConflictingEvidence,
  IncompleteEvidence,
  ScrollStuck,
  RepeatedViewport,
  SectionAmbiguous,
  ItemPartiallyVisible,
  ModalBlocked,
  RegionNotFound,
  RegionResized,
  RegionCollapsed,
}
```

Flat node store + tree edges by ID is the canonical shape. Do not nest
`Vec<ViewNode>` recursively in the JSON artifact; reads must work with a
single dictionary lookup.

## Projection

```rust
pub struct ViewProjection<P> {
  pub projection_id: String,                  // e.g. "netease.playlist_sidebar"
  pub reconstruction_ref: ArtifactRef,        // points to the reconstruction artifact
  pub domain: String,                         // e.g. "netease"
  pub records: P,                             // domain-typed
  pub diagnostics: Vec<ParserDiagnostic>,
}
// NOTICE(generic-projection-envelope): `P` is parameterized on purpose.
// Domain projection records (e.g. NetEase `PlaylistSidebarProjection`)
// live with the example, not in the generic IR. The envelope here is
// the only piece the IR owns; the `records` type is owner-approved per
// domain. Do not promote a domain record into the IR crate to make
// imports easier — that path collapses the boundary the design doc
// drew between framework and example.
```

`P` is domain-typed and lives with the example (NetEase
`PlaylistSidebarProjection`). The generic IR provides the envelope only.

## Cross-viewport merge (candidate → node)

Two `ViewCandidate`s from observations N and N+1 merge into one
`ViewNode` when **all** of the following hold:

1. Same `kind_hint`.
2. Normalized label equality. Normalization: lowercase, NFKC, collapse
   internal whitespace, trim. Empty labels never auto-merge.
3. Bounds overlap after translating both candidates' bounds to
   `WindowLocal`. v0 threshold: IoU ≥ 0.5 along the merge axis.
   **REVIEW(merge-iou-threshold-v1):** 0.5 is a v0 placeholder. Tune
   against NetEase sidebar fixtures and record measured precision /
   recall in the example tests before promoting it to a stable
   default. Treat the constant as instrumentable, not load-bearing.
4. Compatible section context. If both have a `section_hint`, they must
   match. If only one has a hint, the other must not have evidence of a
   different section.
5. Neither candidate has `Confidence::Contradicted`.

When merge succeeds:

- `ViewNode.evidence_refs` is the union of both candidates'
  `evidence_refs` lifted to `EvidenceRef { observation_index,
  evidence_id }`.
- `ViewNode.bounds` is the union (axis-aligned bounding box) after
  translation to `WindowLocal`.
- `ViewNode.label` is the longest non-empty observed label.
- `ViewNode.confidence` is the max-level pair-wise; provider scores are
  the per-provider max.

When merge fails on (2)–(4), emit a `ConflictingEvidence` diagnostic and
keep candidates as separate nodes. Do **not** silently pick a winner.

Merging across more than two observations applies pairwise, left to
right by `observation_index`.

## Artifact roles and JSON serialization

| IR type | Role | One per |
|---|---|---|
| `ViewObservation` | `view-observation` | observation pass |
| `ViewReconstruction` | `view-reconstruction` | parse run |
| `ViewProjection<P>` | `view-projection-<domain>` | domain projection |
| `ViewMemory` (deferred) | `view-memory` (reserved) | — |

```rust
// TODO(view-memory-v1): ViewMemory persistence is reserved, not
// implemented. No struct is defined in v0; the `view-memory` artifact
// role is reserved here so downstream readers do not collide with it.
// A separate, owner-approved spec must cover reacquisition algorithm,
// eviction policy, cross-run identity, and on-disk vs in-process
// scope before any ViewMemory type lands. Until then there is no
// type and no migration path.
```

Serialization rules:

- JSON via `serde_json` with `serde(rename_all = "snake_case")`.
- `ArtifactRef` serializes as `{ run_id, span_id, artifact_id,
  captured_event_id }` per `src/contract.rs`. No alternate form.
- Newtype IDs serialize as plain strings.
- `BTreeMap` serializes as a JSON object with sorted keys.
- Every top-level artifact (`ViewObservation`,
  `ViewReconstruction`) carries `schema_version: "view-ir-v0"`.
- `serde_json::Value` payloads (`ViewEvidenceNode.raw`,
  `ViewActionTarget.payload`) must round-trip; readers parse them with
  the contract type indicated by `source` / `kind`.

## Schema versioning

Single field: `schema_version: String` on `ViewObservation` and
`ViewReconstruction`. v0 value: `"view-ir-v0"`.

Bumping policy:

- Additive optional fields → no version bump; note the addition in this
  doc with a date.
- Renamed / removed / semantically changed fields → bump to
  `"view-ir-v1"` and update this doc.
- Readers must reject artifacts whose major version they do not
  understand. v0 readers accept exactly `"view-ir-v0"`.

## Forbidden invariants

Per bridge spec, re-stated here so implementers do not have to
cross-reference:

- `source_artifacts: Vec<ArtifactRef>`. No `ArtifactRefLike`, no
  `ViewArtifactRef`.
- `ViewCandidate` ≠ `AppSurfaceCandidate` ≠ `contract::Candidate`.
  Three types, three lifecycles.
- `ViewNode` → `contract::Candidate` requires a projection through
  `AppSurfaceCandidate` and the surface-analyze v0 promotion gate.
- `ViewActionTargetKind::CandidateQuery` payload is
  `contract::CandidateQuery`, serialized verbatim.
- `EvidenceSource::Ax` raw payload is `contract::SurfaceNode`.
  `EvidenceSource::Ocr` raw payload is `contract::RecognitionResult`.
- No NetEase-specific types in the generic IR. NetEase types live in
  the example projection.

Implementations that violate any of the above fail v0 acceptance.

## v0 done criteria

The IR is v0-complete when:

1. All concrete types in this document are implemented as a Rust crate
   or module (location to be approved by the owner; suggested
   `crates/auv-view` with re-exports through workspace).
2. Round-trip JSON serialization works for `ViewObservation`,
   `ViewReconstruction`, and a domain-typed `ViewProjection`.
3. Every top-level artifact carries `schema_version = "view-ir-v0"`.
4. `ViewNodeId` derivation is deterministic across runs for all node
   kinds except `Unknown`.
5. Cross-viewport merge passes unit tests with synthetic OCR fixtures
   covering: full match, partial overlap, conflicting sections, repeat
   fingerprint, and clipped-label edge case.
6. NetEase example emits a valid `ViewReconstruction` plus a
   `ViewProjection<PlaylistSidebarProjection>` whose records reference
   only valid `ViewNodeId`s.
7. All `ArtifactRef`s in serialized artifacts resolve to real artifacts
   in the same run (no dangling refs).
8. Anchors and landmarks are reachable from `ViewReconstruction` via
   `anchor_index` / `landmark_index`, and every entry's
   `evidence_refs` resolves within the same reconstruction.

## Non-goals for this spec

- `ViewMemory` persistence beyond reserving the artifact role.
- Cross-application reconstruction merging.
- Generic visual segmentation backend integration (`Future CV/YOLO`
  remains a reserved row in `surface-analyze-v0.md`).
- DOM / CDP backend (`Future DOM/CDP` likewise reserved).
- Inspect viewer panels.
- Anchor reacquisition algorithm — design doc lists strategies; v0
  IR carries only the strategy enum and hint fields.

## How to use this spec

When implementing view parser primitives or the NetEase example:

- Treat the type list above as **closed for v0**. Adding a field, a
  variant, or a new IR type requires a dated revision of this document
  and owner approval.
- Treat the cross-viewport merge rule as **the** v0 merge semantics.
  Do not branch a second merge heuristic.
- Treat schema_version as a wire contract. Never ship an artifact
  without it.
- When a real parser need cannot be expressed by these types, record
  the gap in this document instead of patching by inventing a new
  type. Multiple narrow gap notes are preferable to one quiet
  extension.

This document is part of the convergence phase. Revisions are explicit,
dated, and owner-approved.
