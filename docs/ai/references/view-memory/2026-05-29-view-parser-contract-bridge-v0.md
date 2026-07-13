# AUV View Parser Contract Bridge v0

Date: 2026-05-29

Status: v0 bridge spec. Scope-narrowing, not implementation guide.

Audience: owner, reviewers, and any agent (Codex, Claude, others) implementing
view parser primitives or NetEase example code.

## Purpose

The view parser design doc
(`docs/ai/references/view-memory/2026-05-28-view-parser-ir-netease-playlist-example-design.md`)
covers a complete view parsing model: `ViewObservation`,
`ViewReconstruction`, `ViewProjection`, `ViewMemory`, anchors, landmarks,
scroll boundaries.

It does **not** specify how those types relate to AUV's existing contract
surface in `src/contract.rs` (`ArtifactRef`, `RecognitionResult`,
`SurfaceNode`, `Candidate`, `CandidateRef`, `VerificationResult`) or to the
surface-analyze v0 boundary in
`docs/ai/references/ops/2026-05-28-surface-analyze-v0.md`.

Without that bridge, a v0 implementation is at risk of:

- Inventing a third evidence-ref schema parallel to `ArtifactRef`. The view
  parser doc already hedges with `ArtifactRefLike`.
- Inventing a third candidate schema parallel to `AppSurfaceCandidate` and
  `contract::Candidate`.
- Letting view parser output produce action-grade candidates directly,
  bypassing the promotion gate defined in `surface-analyze-v0.md`.
- Re-parsing AX evidence and recognition output instead of consuming the
  existing `SurfaceNode` and `RecognitionResult` shapes.

This document closes those four questions for v0.

## Type taxonomy: three distinct candidate types, one promotion gate

AUV v0 carries three distinct types for "thing observed that might be acted
on". They are **not** synonyms.

| Type                   | Lifecycle                                       | Audience                                  | Promotion                                                  |
|------------------------|-------------------------------------------------|-------------------------------------------|------------------------------------------------------------|
| `ViewCandidate`        | Parser-internal, single viewport, pre-merge     | View parser implementation only           | Lifted into `ViewNode` after cross-viewport merge          |
| `AppSurfaceCandidate`  | Probe-scoped, explainable                       | `app analyze` reports, distillation review| Gated promotion to `contract::Candidate`                   |
| `contract::Candidate`  | Operation-scoped, machine-consumable            | `ActionResolver`, operation handlers      | Consumed via `CandidateRef`                                |

`ViewCandidate` is internal to the parser's cross-viewport stitching
algorithm. It is not the type that downstream consumers see. View parser may
project a `ViewNode` into an `AppSurfaceCandidate` when the node should
appear in `app analyze` output, but it must not mint `contract::Candidate`
directly from any view parser type.

## Evidence ref: one schema, `ArtifactRef`

The view parser design uses `ArtifactRefLike` as a placeholder on
`SidebarViewportObservation.source_artifacts`. v0 resolves this.

`source_artifacts` on `ViewObservation`, `ViewReconstruction`, and
`ViewProjection` must be `Vec<ArtifactRef>`. Do not introduce
`ViewArtifactRef`, `SourceArtifactRef`, or any other `ArtifactRefLike`
variant. The placeholder name is treated as resolved to `ArtifactRef`.

View-specific context — `coordinate_space`, `viewport_fingerprint`,
`viewport_index`, `bounds`, capture/window context — lives on the view
parser record types, not inside `ArtifactRef`. This mirrors the rule already
stated in `surface-analyze-v0.md` for `AppSurfaceCandidate`: refs carry
trace identity, candidates carry observation context.

## Single promotion gate

The surface-analyze v0 promotion gate is the **only** legal path from any
"observed thing" — whether produced by `app analyze` directly or by a view
parser — to `contract::Candidate`. View parser-derived candidates reuse the
same gate.

```text
ViewReconstruction
  └─ ViewNode
       │  (view parser projects into surface candidate when the node should be actionable)
       ▼
     AppSurfaceCandidate
       │  (surface-analyze v0 promotion gate: re-location, evidence, action contract,
       │   verification contract, liveness, control, failure layer)
       ▼
     contract::Candidate
       │
       ▼
     ActionResolver / operation
```

The gate's seven conditions apply unchanged. Adding a view-parser-specific
gate is forbidden in v0. If a NetEase node cannot satisfy the existing gate,
the node stays at `ViewNode` and the failure is recorded as a `known_limit`
on the projection — not papered over by a parallel gate.

## Consume existing contract types, do not re-invent

The view parser design doc does not mention `RecognitionResult` or
`SurfaceNode`. Bridge rule:

- **`RecognitionResult`** is the existing contract type for OCR / icon /
  visual recognition output. View parser's evidence-collection layer must
  produce or consume `RecognitionResult` records, not invent a parallel
  recognition record.
- **`SurfaceNode`** is the existing AX-evidence type. View parser must
  consume `SurfaceNode` records produced by AX capture rather than
  re-parsing AX trees from scratch.
- **`ArtifactRef`** is the only evidence reference type (see above).

If the view parser needs richer recognition or surface evidence than these
existing types provide, the gap is filed against the existing types — not
patched by inventing a parallel evidence type. File the gap in
`surface-analyze-v0.md` or in this document and stop until the owner
approves an extension.

## What view parser may not invent (in addition to surface-analyze rules)

- No `ViewArtifactRef`, `ViewSourceRef`, or `ArtifactRefLike`. Use
  `ArtifactRef`.
- No second promotion gate. Use the surface-analyze v0 gate.
- No direct construction of `contract::Candidate` from any view parser
  type. Route through `AppSurfaceCandidate`.
- No new operation namespace for view-derived actions in v0. The NetEase
  example consumes existing `catalog.rs` operations or stays in example
  code.
- No NetEase-specific contract types in `src/contract.rs`. Per the view
  parser design doc's Non-Goals, NetEase types live in the example.
- No parallel recognition or AX-evidence schema. Use `RecognitionResult`
  and `SurfaceNode`.

## How view parser output reaches durable storage

A view parser run produces these durable artifacts through existing
`crate::contract` / trace machinery:

| View parser output            | Stored as                                                        |
|-------------------------------|------------------------------------------------------------------|
| `ViewObservation` (per pass)  | one artifact, role `view-observation`                            |
| `ViewReconstruction`          | one artifact, role `view-reconstruction`                         |
| `ViewProjection` (per domain) | one artifact, role `view-projection-<domain>` (e.g. `netease`)   |
| `ViewMemory`                  | one per clean parse run when enabled (see view-memory-v0)        |
| Boundary / scroll evidence    | embedded in the observation or reconstruction artifact           |

Each of these artifacts receives an `ArtifactRef` like any other artifact
in the run. Cross-references between view parser artifacts use
`ArtifactRef`, not internal numeric IDs or path strings.

## v0 done criteria for the bridge

The bridge is v0-complete when:

1. Every `source_artifacts` field across view parser types serializes as
   `Vec<ArtifactRef>`. No `Like` / `Variant` / wrapper suffix.
2. No view parser code path constructs `contract::Candidate` directly.
   Action targets reach `ActionResolver` only via `AppSurfaceCandidate` and
   the surface-analyze v0 gate.
3. View parser evidence consumed from OCR / icon / AX uses the existing
   `RecognitionResult` and `SurfaceNode` shapes, or records the missing
   field as a gap.
4. The NetEase example does not add types to `src/contract.rs`.
5. Artifact roles (`view-observation`, `view-reconstruction`,
   `view-projection-<domain>`) are recorded in the run trace with proper
   `ArtifactRef`s and visible to `list_*` read-side APIs.
6. The human-readable view parser report explains, for each domain item,
   which `ViewNode` backs it, which `AppSurfaceCandidate` (if any) is
   projected, and whether that projection passes the promotion gate.

## Non-goals for this bridge

Intentionally deferred. They are not gaps; they are the wall this bridge
defines.

- Promoting `ViewNode` to `contract::Candidate` without going through
  `AppSurfaceCandidate`.
- A view-parser-specific promotion gate or evidence schema.
- `ViewMemory` persistence, anchor reacquisition, and viewport pose memory.
- (Inspect viewer integration for view parser artifacts — covered by `2026-05-29-view-parser-inspect-viewer-v0.md`.)
- Promotion of NetEase-specific types into `src/contract.rs`.
- Cross-app view composition.

## How this document is meant to be used

When implementing view parser primitives or the NetEase example:

- Default to `ArtifactRef` for every "where did this come from" field.
  Stop and ask the owner before introducing any new ref type.
- Default to projecting `ViewNode` to `AppSurfaceCandidate` when a node
  should become actionable. Stop and ask the owner before designing a
  parallel projection.
- Default to the surface-analyze v0 promotion gate. Stop and ask before
  adding a second gate.
- When a NetEase need cannot be expressed through existing contract types,
  record the gap in this document or in `surface-analyze-v0.md` instead of
  patching with a new schema.

This document is part of the convergence phase. Revisions are allowed; they
must be explicit, dated, and owner-approved.
