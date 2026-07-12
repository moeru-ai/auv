# Recognition Evidence Boundary v0

Date: 2026-06-05

Status: docs-only boundary

Base: `origin/main` after `69a00ba`

## Purpose

Define the next inference-scoped boundary after `DetectionEvidenceManifest` v0:
what must be true before detector evidence can honestly become runtime-side
recognition evidence.

This slice does **not** implement:

- `DetectionEvidenceManifest -> RecognitionResult`
- `DetectionSet -> RecognitionResult`
- `DetectionSet -> contract::Candidate`
- `DetectionEvidenceManifest -> contract::Candidate`
- `src/contract.rs` changes
- runtime artifact recording
- driver capture integration
- app validation or action consumption

It only defines the bridge boundary and terminology so later implementation
work does not drift into parallel schemas or premature Candidate plumbing.

## Current State

`main` now has three relevant pieces:

```text
DetectionSet
  -> inference-only detector output

DetectionEvidenceManifest
  -> DetectionSet + source image evidence + model run metadata + known limits

RecognitionResult
  -> stable runtime-side structured recognition contract already used by OCR,
     icon-match, row observations, and downstream candidate evidence chains
```

What is still missing is the boundary between the first two and the third.

## Naming Rule

`RecognitionEvidence` in this document is a **design-layer term**, not a new
Rust contract type.

That distinction is deliberate.

The repository already has a stable runtime-facing contract name:

```text
src/contract.rs::RecognitionResult
```

If a future slice needs a bridge from detector evidence into runtime-side
recognition, the default convergence target is still `RecognitionResult`
unless the owner explicitly approves a different contract shape.

This document therefore uses:

- `RecognitionEvidence` to describe the **conceptual bridge stage**
- `RecognitionResult` to describe the **existing runtime contract target**

Do **not** read this spec as approval to add:

- `struct RecognitionEvidence`
- `enum RecognitionEvidence`
- `recognition_evidence_ref`
- a second runtime-facing recognition schema parallel to `RecognitionResult`

If the bridge cannot fit `RecognitionResult`, the gap must be recorded first.
It must not be papered over by inventing a parallel contract.

## Why DetectionEvidenceManifest Is Still Not Runtime Recognition

`DetectionEvidenceManifest` proves:

- which model vocabulary produced the boxes
- which source image the boxes came from
- which thresholds/provider/backend shaped the result
- which inference-only limits still apply

It does **not** yet prove:

- which items should count as runtime-recognized objects
- how detector classes map to stable semantic kinds
- what `best / filtered / all` should mean for detector output
- how item identity should be assigned across overlapping detections
- how detector boxes become `RecognitionBox` in a runtime scope
- what runtime artifact/evidence refs should replace inference-only source image
  refs
- whether the result is evidence-only or eligible for downstream candidate
  promotion

That missing layer is what this document calls the RecognitionEvidence
boundary.

## Bridge Output Requirements

Before any implementation can produce runtime-side recognition evidence from
detector manifests, it must answer all of the following.

### 1. Semantic Class Mapping

The bridge must define how detector vocabulary becomes stable runtime meaning.

Minimum questions:

- Which detector classes are recognized at all?
- Which classes remain detector-only and should never enter runtime contracts?
- Does one detector class map directly to one runtime `kind`, or is additional
  filtering required?
- Are entities and UI detections allowed in the same bridge output, or must
  they stay split by model/source?

Without this, a detector label is still only model vocabulary.

### 2. Runtime Scope Mapping

The bridge must define what runtime scope can honestly be claimed.

`RecognitionResult` requires a runtime `scope` with fields like:

- `surface`
- `display_ref`
- `app_bundle_id`
- `window_title`
- `window_number`
- `capture_artifact`
- `capture_contract_artifact`

Current Balatro smoke manifests do not have this information.

So the bridge must say one of two things explicitly:

- this evidence remains inference-only because runtime scope is unavailable, or
- a future capture-integrated path supplied the missing runtime scope and
  artifact refs

There is no honest middle state where inference-local image paths silently
pretend to be runtime capture scope.

### 3. Item Projection Rules

The bridge must define how detections become recognized items.

For each future item:

- how `item_id` is derived
- how `kind` is assigned
- how `box` is converted from detector bbox into runtime `RecognitionBox`
- what text, if any, is attached
- how detector confidence maps to `provider_score`
- what detector/raw detail survives in `detail`

This projection must be deterministic enough that inspect/read-side tools can
explain where the runtime item came from.

### 4. best / filtered / all Semantics

`RecognitionResult` is not just "a list of boxes". It already carries:

- `best`
- `filtered`
- `all`

The bridge must define what those buckets mean for detector output.

Examples of unresolved policy:

- Is `all` every raw detection after upstream postprocess?
- Is `filtered` the subset that passes semantic/runtime eligibility filters?
- Is `best` the top-ranked item per requested class, per region, or globally?
- Can there be no `best` even when `filtered` is non-empty?

Until those answers exist, emitting detector output as `RecognitionResult`
would be under-specified.

### 5. Evidence and Known Limits Carry-Forward

The bridge must preserve inference caveats when crossing into runtime-side
recognition evidence.

At minimum it must define:

- which `known_limits` move forward unchanged
- which new runtime limits are added
- how source-image evidence is represented once runtime artifacts exist
- how detector detail survives for inspect/debug consumers

The bridge must not erase:

- source-image pixel coordinate basis
- lack of projection
- detector score semantics
- crop/occlusion/model-vocabulary limits

## Preferred Shape

The preferred future convergence path remains:

```text
DetectionSet
  -> DetectionEvidenceManifest
  -> recognition evidence bridge policy
  -> RecognitionResult
  -> Candidate only when existing promotion/liveness/action gates are satisfied
```

That is intentionally different from:

```text
DetectionSet -> Candidate
DetectionEvidenceManifest -> Candidate
```

The runtime recognition contract should be the first landing zone, not
actionable candidates.

## Evidence-Only Outcomes Are Valid

A future bridge slice does not need to prove every detector output is candidate
eligible.

Valid outcomes include:

- produce runtime recognition evidence only
- produce no `best` item
- keep certain classes in `all` but not `filtered`
- reject a manifest because runtime scope/projection/artifact identity is
  missing

The important rule is that the bridge must say so explicitly instead of
silently upgrading detector output into action targets.

## Non-Goals

This slice does not approve:

- a new contract type parallel to `RecognitionResult`
- `src/contract.rs` edits
- detector-to-candidate projection
- Balatro game-state interpretation
- click point generation
- runtime integration in `src/app/*`, `src/driver/*`, or `src/runtime.rs`

## Follow-Up Unlock Condition

A future implementation slice is only eligible when it can name the exact
bridge boundary it will close, for example:

```text
docs/test(inference): define detector-manifest to RecognitionResult mapping
```

or

```text
test(inference): prove capture-integrated detector manifest can emit runtime RecognitionResult
```

Before that, this spec should be treated as a guardrail against naming drift
and premature Candidate work, not as permission to start runtime bridge code.
