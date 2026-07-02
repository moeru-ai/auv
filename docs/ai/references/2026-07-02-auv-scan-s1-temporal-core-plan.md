# AUV Scan S1: 2D Temporal Scan Core — Implementation Plan

**Date:** 2026-07-02  
**Status:** implementation plan — **not started**  
**Prerequisite:** [S0 charter](2026-07-02-auv-scan-s0-charter.md)  
**Server API needed:** **No** (S1 v0 — client-side artifacts + implicit run recording only)

## One-line summary

S1 plans the first implementation slice for **time-continuous 2D observation** from a
single-viewport frame sequence: capture, viewport motion, evidence fusion, temporal
association, and confidence/diagnostics — all inspectable via artifacts.

This document is a **plan only**. No code ships with this file.

## Blocked until

- [S0 charter](2026-07-02-auv-scan-s0-charter.md) merged
- Owner names a concrete **S1 implementation slice** (crate boundary, first fixture)

## Five core modules

### 1. Frame capture

Normalize every input frame into a replayable `ScanFrame` (provisional name):

| Field | Type (provisional) | Notes |
|-------|-------------------|-------|
| `frame_id` | string | Stable within one scan run |
| `timestamp` | number / ISO string | Ordering and delta reasoning |
| `window_bounds` | rect | Window or crop coordinate space |
| `image_ref` | artifact path or run artifact key | Must survive inspect replay |

**Rules:**

- Capture logic must not hide inside a live-only adapter without artifact emission.
- Each frame must be independently inspectable (`scan-frame-NNNN.json` + optional png).

**Donor:** scroll_scan page capture metadata; driver screenshot artifacts.

### 2. Motion / viewport tracking

Estimate motion between adjacent frames:

| Output | Purpose |
|--------|---------|
| `ViewportTransform` | Scroll / pan / crop delta between frame *t* and *t+1* |
| `confidence` | Motion estimate reliability |

Motion is a **prior for association**, not UI semantics. Unstable motion must surface
`motion_unstable` rather than silently corrupting tracks.

**Donor:** `ScreenshotDiffStability` patterns in [`src/scroll_scan/mod.rs`](../../../src/scroll_scan/mod.rs).

### 3. OCR / detector fusion

Merge per-frame OCR boxes and optional detector boxes into unified `EvidenceNode` records:

| Field | Purpose |
|-------|---------|
| `source` | `ocr` / `detector` / … — provenance required |
| `score` | Donor confidence |
| `raw_text` / `normalized_text` | Text evidence when present |
| `box` | Geometry in frame coordinate space |

**NOTICE:** **Detector is an optional donor**, not a mandatory external-model backend for
S1. The first implementation slice may use **OCR + geometry only**; detector fusion can
land in a follow-up slice without blocking temporal association.

Fusion must **not** erase provenance — consumers must see which donor produced each node.

**Donor:** [`ViewEvidenceNode`](../../../crates/auv-view/src/lib.rs) shape and source enum;
extend or parallel only after owner picks owning crate.

### 4. Temporal association

Associate `EvidenceNode` instances across frames into `TemporalTrack` records using:

- geometry overlap / motion-compensated position
- text similarity (when text present)
- viewport motion prior

**Track states (minimum):**

| State | Meaning |
|-------|---------|
| `active` | Visible and associated this frame |
| `occluded` | Likely still exists but not observed |
| `exited_viewport` | Left visible region (scroll away) |
| `lost` | Identity uncertain; no silent drop |

Output **stable track ids** across frames — not per-frame ephemeral labels.

### 5. Confidence / diagnostics

Emit confidence at frame, track, and aggregate result levels. Every weak path must emit
`ScanDiagnostic` entries.

**Minimum diagnostic codes:**

| Code | When |
|------|------|
| `no_capture` | Frame image or metadata missing |
| `no_ocr_evidence` | No OCR nodes on frame when OCR was expected |
| `motion_unstable` | Viewport transform confidence below threshold |
| `ambiguous_association` | Multiple equally valid track matches |
| `track_split` | One track incorrectly split into two |
| `track_gone_from_viewport` | Track exited visible region |
| `low_text_confidence` | Text present but below usable threshold |

Silent empty output when evidence is missing is **forbidden**.

## Minimal contract types (provisional vocabulary only)

The following are **design vocabulary** for planning and fixtures — not approved Rust or
wire types.

### `ScanFrame`

Per-frame capture record (see Frame capture table).

### `EvidenceNode`

Fused per-frame observation with provenance (see fusion table).

### `ViewportTransform`

| Field | Purpose |
|-------|---------|
| `from_frame_id` / `to_frame_id` | Pair |
| `delta_x` / `delta_y` | Translation estimate |
| `scale` | Optional zoom |
| `confidence` | Estimate quality |

### `TemporalTrack`

| Field | Purpose |
|-------|---------|
| `track_id` | Stable id |
| `state` | `active` / `occluded` / `exited_viewport` / `lost` |
| `evidence_refs` | Links to frame-local nodes |
| `confidence` | Track-level |

### `TemporalScanResult`

Aggregate scan output: frames, timeline, tracks, confidences, diagnostics.

### `ScanDiagnostic`

| Field | Purpose |
|-------|---------|
| `code` | From diagnostic table |
| `frame_id` / `track_id` | Optional scope |
| `message` | Human-readable detail |
| `severity` | `info` / `warn` / `error` |

### Owning crate (deferred)

| Option | Tradeoff |
|--------|----------|
| Extend `auv-view` | Reuses view evidence vocabulary |
| New `scan` / `temporal_scan` module under runtime | Clearer boundary from view-parser IR |

**Owner must lock boundary in the first implementation slice** — this plan lists options only.

### Reuse vs new types

| Existing | S1 stance |
|----------|-----------|
| `ViewEvidenceNode` | Donor for provenance fields; do not fork cosmetically |
| `ScrollScanArtifact` / `ObservationSnapshot` | Page-level donor; temporal tracks are **new concern** |
| `view_parser` IR | **Out of scope** for S1 — S2 projection may consume tracks later |

## Minimal artifacts

| Artifact | Content |
|----------|---------|
| `scan-frame-0001.json` | `ScanFrame` + frame-local `EvidenceNode[]` |
| `scan-timeline.json` | `ViewportTransform[]` |
| `scan-tracks.json` | `TemporalTrack[]` + rollup diagnostics |
| Optional png/crop | Visual replay |

Persist via implicit run recording ([`src/runtime.rs`](../../../src/runtime.rs) pattern) —
same run inspect path as scroll scan; **no new HTTP compare API**.

## S1 non-goals

- Query-aware select (→ **S2**)
- ViewMemory durable contract (→ **S3**)
- Cross-run compare ([B2c deferred](2026-06-30-auv-scenebridge-b2c-inspect-cross-run-compare-deferred.md))
- 3D geometry recovery (→ **S4+ research**)
- App-specific parser rewrite
- Mandatory detector / YOLO backend wiring
- New inspect server endpoints
- Producer changes on SceneBridge A line

## Acceptance criteria (hermetic-first)

On a recorded fixture sequence:

1. **Stable track id** — same visible object keeps one id across frames when evidence supports it
2. **Scroll exit** — after viewport scroll, track enters `exited_viewport` or `occluded`, not silent deletion
3. **Reacquire** — on reappearance, reconnects to prior track **or** emits `ambiguous_association`
4. **OCR gap** — missing OCR yields `no_ocr_evidence` / `low_text_confidence`, not empty scan
5. **Replay** — every conclusion traceable from artifacts without re-running live capture

## Test plan (documentation level — no tests in this slice)

### Hermetic fixtures

- Directory convention (future): `tests/fixtures/scan/temporal/<scenario>/`
- Each scenario: frame images + optional ground-truth `tracks.json` for metrics
- Scenarios to plan: steady scroll, bounce-back scroll, duplicate text rows, brief occlusion

### Metrics

| Metric | Definition |
|--------|------------|
| ID switch rate | Track id changes / object-ground-truth-segment |
| track continuity | Fraction of frames where gt object maps to same track id |
| viewport motion accuracy | Delta error vs fixture metadata or diff corroboration |
| diagnostics completeness | Fraction of injected failure modes that emit expected code |

### Live protocol (label: **live**)

- One high-scroll list UI (e.g. playlist sidebar class surface)
- Manual checklist: five S0 auditable questions answerable from artifacts
- Not a merge gate until hermetic coverage exists

## Future implementation order (owner-approved slices)

1. Frame + artifact contract + hermetic single-frame fixture
2. Motion estimation between two frames
3. Evidence fusion (OCR-only path first)
4. Temporal association + track states
5. Diagnostics + confidence rollup
6. Hermetic multi-frame regression suite → optional live probe

Each step is a **separate commit slice** with its own tests.

## Iron rule (contract vocabulary)

**English:** These names are **provisional design vocabulary only**; owning crate, Rust type
names, and wire stability remain **deferred** until an implementation slice is
owner-approved.

**中文：** 上文 `ScanFrame`、`EvidenceNode`、`ViewportTransform`、`TemporalTrack`、
`TemporalScanResult`、`ScanDiagnostic` 等名称**不构成** owner 批准的 wire / runtime
contract。实现 agent **不得**据此直接开 type 或扩 proto。

## Related

- [S0 charter](2026-07-02-auv-scan-s0-charter.md)
- [Scroll scan design](2026-05-21-scroll-scan-design.md)
- [SceneBridge A1 charter](2026-06-30-auv-scenebridge-a1-design-charter.md)

## Validation (this document only)

```sh
git diff --check
```
