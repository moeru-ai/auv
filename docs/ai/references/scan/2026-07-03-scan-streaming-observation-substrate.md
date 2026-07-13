# S Line: Streaming Observation Substrate Roadmap

**Date:** 2026-07-03  
**Status:** roadmap / direction note — docs-only; does not approve downstream implementation slices  
**Scope:** S-line direction after A-line scoped completion and before model-backend work  
**Related:** [S0 charter](2026-07-02-scan-charter.md), [S1 temporal core plan](2026-07-03-scan-temporal-core-landed.md), [S1 frame contract handoff](2026-07-03-scan-temporal-core-landed.md)

## One-line summary

The S line should be treated as a **streaming observation substrate**:

```text
frame stream
  -> frame binding / calibration
  -> tracking / temporal association
  -> keyframes
  -> coverage ledger
  -> scene state product
  -> optional model backend
```

It is not `screenshot -> 3DGS`, and it is not merely a stronger static screenshot
scanner. The core product value is that AUV can explain what it observed over time,
how observations are bound to surfaces and coordinates, which identities persisted,
where coverage is sufficient, and which conclusions are weak or stale.

## Current lane map

| Lane | Status | Boundary |
| --- | --- | --- |
| **A line** | Scoped complete through A8 | NetEase ViewMemory + reacquire + machine-readable proof. Donor for anchor lifecycle, not the place to grow the generic substrate. |
| **B line** | Required product/read-side follow-up | Converts A-line and S-line proof into inspect/product consumption. B should consume scan semantics, not invent them. |
| **S line** | Active direction | Owns streaming observation contracts: frames, binding, temporal association, coverage, diagnostics, and scene-state products. |
| **M line** | Deferred | Model backends such as SLAM, dense mapping, 3DGS, or Gaussian memory. M consumes S outputs; it must not own the hot-path scan contract. |
| **G line** | Deferred / adapter-validation lane | Game or telemetry adapters such as Minecraft. G can validate S with pose/world-tick advantages, but must not bind core design to one game. |

The practical rule: **S first, M/G later**. AUV needs a trustworthy observation
ledger before it needs a pretty reconstructed world.

## S0-S6 direction

These labels are roadmapping labels, not blanket implementation approval. Each code
slice still needs a narrow owner-approved plan, fixture, and validation path.

### S0 — Charter and vocabulary

S0 defines the S-line question set and keeps the first target honest: continuous,
single-viewport, auditable 2D temporal observation.

Fixed questions:

- What is visible in the current viewport?
- Which visible objects are the same thing across frames?
- How did the viewport move?
- Why was a target lost or reacquired?
- Which conclusions are trustworthy, weak, stale, or ambiguous?

S0 is already represented by the scan charter. This document broadens the roadmap
without replacing that charter.

### S1 — Frame binding and artifact contract

S1 turns a raw capture into a versioned, inspectable frame artifact. The important
step is not image storage; it is binding the observation to a surface, timestamp,
viewport, coordinate system, and quality envelope.

Current seed: `crates/auv-scan` has the first `scan-frame-v0` slice. Future S1
work should extend from that seed instead of inventing a parallel frame schema.

Required properties:

- versioned schema and strict reader validation
- artifact lineage back to the image or capture source
- viewport bounds and coordinate basis
- timestamp and sequence ordering
- quality flags for missing, stale, cropped, unstable, or non-calibrated inputs

### S2 — Motion and temporal association

S2 estimates how a single viewport changed between frames and starts associating
observations across time.

Initial scope should stay 2D:

- two-frame viewport motion fixtures
- stable object or region association across adjacent frames
- explicit uncertainty for ambiguous matches
- no silent identity continuity when evidence is missing

This is not yet SLAM. It is temporal scan mechanics: motion, association, and
diagnostics over a bounded frame sequence.

### S3 — Coverage ledger

S3 records what has been observed, when it was observed, and whether that coverage
is enough to support a claim.

Coverage should capture:

- observed regions or anchors
- last-seen timestamps / frame ids
- freshness and staleness
- negative evidence such as no new observation after motion
- completeness or incompleteness reasons
- confidence and falsifiers

This is where scan stops being a screenshot loop and becomes an auditable ledger.

### S4 — Anchor lifecycle and reacquire generalization

S4 generalizes the A-line reacquire lesson into a reusable anchor lifecycle.

Provisional lifecycle:

```text
observed -> tracked -> stale -> reacquiring -> reacquired | lost | ambiguous
```

The A8 NetEase proof is a donor because it proves a product object can be remembered
and reacquired. S4 should extract the lifecycle and evidence requirements, not copy
NetEase-specific semantics into core.

### S5 — Scene state product

S5 packages scan evidence into product-consumable state.

The output should answer operational questions, not just expose detector rows:

- Is the target still present?
- Is it likely the same target?
- Is it visible, occluded, stale, or out of viewport?
- Is there enough coverage to act?
- What observation would reduce uncertainty next?

This is the main bridge to B-line inspect/product work. B-line UI and read-side cards
should consume this state rather than parse raw frame artifacts directly.

### S6 — Optional model backend boundary

S6 is where heavier model backends can be attached after S1-S5 are stable enough.

Allowed backends later:

- visual odometry / pose graph
- app or game telemetry pose adapters
- dense map or 3DGS representation
- background model assimilation
- predictive visibility / affordance models

Hard boundary: model backends are **cold-path consumers**. They must not block frame
ingestion, tracking, coverage updates, or basic product inspection.

## Core types

Names are provisional unless already landed in code. The intent is to show ownership
and data flow, not to approve a final Rust API.

```rust
struct ObservedFrame {
  schema_version: String,
  frame_id: String,
  sequence_index: u64,
  captured_at_ms: u64,
  image_ref: ArtifactRef,
  binding: FrameBinding,
  source: FrameSource,
  quality_flags: Vec<FrameQualityFlag>,
}

struct FrameBinding {
  surface_ref: Option<SurfaceRef>,
  viewport_bounds: Bounds2D,
  coordinate_space: CoordinateSpace,
  display_scale: Option<f64>,
  window_epoch: Option<String>,
  capture_latency_ms: Option<u64>,
  pose: Option<CameraPose>,
  projection: Option<ProjectionMatrix>,
}

struct TemporalObservation {
  observation_id: String,
  frame_id: String,
  region: Bounds2D,
  source_kind: ObservationSourceKind,
  evidence: EvidenceRef,
  confidence: Confidence,
}

struct AnchorTrack {
  track_id: String,
  observations: Vec<String>,
  lifecycle_state: AnchorLifecycleState,
  identity_confidence: Confidence,
  diagnostics: Vec<ScanDiagnostic>,
}

struct ScanKeyframe {
  keyframe_id: String,
  frame_id: String,
  selected_reason: KeyframeReason,
  tracked_anchors: Vec<String>,
  coverage_delta: CoverageDelta,
}

struct CoverageLedger {
  ledger_id: String,
  frame_range: FrameRange,
  entries: Vec<CoverageEntry>,
  completeness_claims: Vec<CompletenessClaim>,
  open_uncertainties: Vec<ScanDiagnostic>,
}

struct SceneStateProduct {
  product_id: String,
  as_of_frame_id: String,
  tracks: Vec<AnchorTrackSummary>,
  coverage: CoverageSummary,
  action_readiness: Option<ActionReadiness>,
  recommended_observations: Vec<ObservationRequest>,
  diagnostics: Vec<ScanDiagnostic>,
}
```

Type placement should follow ownership:

- frame artifacts and scan-local readers belong in `crates/auv-scan`
- cross-crate durable vocabulary may graduate into shared contracts only after a
  second producer or reader proves the need
- product presentation belongs in B-line inspect/read-side code
- app-specific pose or telemetry belongs in adapter crates until graduated

## Explicit non-goals

- 3DGS-first architecture
- mandatory SLAM, depth, pose telemetry, or game-specific world truth
- replacing the existing S0/S1 docs or landed `scan-frame-v0` seed
- replacing `ObservationSnapshot` or scroll-scan page-loop evidence in one broad refactor
- app-specific semantic closure inside S core
- action planning, policy, or autonomous agent behavior
- cross-session global memory as an S1-S5 requirement
- live capture implementation before artifact contracts and fixtures are stable
- inspect-server API expansion unless B line explicitly approves the consumption slice
- broad runtime rewrites or compatibility shims unrelated to the current scan slice

## First acceptance batch

The first batch should prove that S-line evidence can be produced, validated, read,
and consumed without jumping to 3D or app-specific semantics.

| Gate | Acceptance standard |
| --- | --- |
| **S1 frame contract** | Versioned frame artifact writes and reads round-trip through golden fixtures; invalid schema version, missing schema version, and invalid bounds are rejected. |
| **S2 two-frame motion** | A hermetic two-frame fixture produces an explicit viewport/motion estimate or an explicit `motion_unknown` diagnostic. |
| **S2 association** | A simple stable object across two frames keeps identity; an ambiguous case emits an ambiguity diagnostic instead of silently merging tracks. |
| **S3 coverage ledger** | A fixture records observed regions, freshness, no-new-observation evidence, and an honest incomplete/complete claim. |
| **S4 anchor lifecycle** | A target can move from observed to stale to reacquired or lost with evidence attached to every transition. |
| **S5 read-side product** | A reader can answer the five S0 questions from artifacts without re-running the scanner. |
| **B-line bridge** | Inspect/product consumption uses a summary product or reader output, not ad-hoc parsing of raw frame JSON. |
| **M/G guardrail** | Any pose, telemetry, SLAM, or 3DGS prototype is cold-path and optional; it cannot become required for S1-S5 validation. |

Minimum validation for docs-only changes:

```sh
git diff --check
```

Minimum validation once code lands:

```sh
cargo fmt --check
cargo check -p auv-scan
cargo test -p auv-scan
git diff --check
```

## Decision record

For now, S-line work should stay in core as **streaming observation infrastructure**.

Rejected alternatives:

- **Make S a 3DGS lane.** Rejected because it puts a cold-path representation ahead of
  frame binding, tracking, and coverage proof.
- **Keep S as screenshot scan only.** Rejected because it repeats batch scan patterns and
  does not solve temporal identity, reacquire, or coverage.
- **Use Minecraft/game telemetry as the core contract.** Rejected because it is a strong
  adapter and validation target, not a general AUV substrate.

Chosen direction:

```text
S line = streaming observation substrate
B line = product/read-side consumption
M line = optional model backend
G line = adapter validation, especially telemetry-rich games
```

