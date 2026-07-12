# DetectionSet Adapter Boundary

Date: 2026-06-05

Status: docs/test-only boundary

Base: `fbd7374`

## Purpose

Define what is still missing before `DetectionSet` can become durable AUV
evidence that later feeds `RecognitionResult`, `Candidate`, or another
runtime-facing contract.

This slice does **not** implement:

- `DetectionSet -> RecognitionResult`
- `DetectionSet -> contract::Candidate`
- runtime artifact recording
- driver capture integration
- window/screen projection
- app validation or action consumption

The only implemented route on `main` remains:

```text
local image path or frame
  -> ultralytics-inference backend
  -> upstream Results
  -> AUV DetectionSet
  -> JSON / annotated debug evidence
```

## Current DetectionSet Boundary

`DetectionSet` currently carries only inference output:

- `model_id`
- `image_size`
- `detections[]`
- per detection: `class_id`, `label`, `confidence`, `bbox`

Current bbox semantics:

- source-image pixel coordinates
- after upstream preprocessing/postprocessing has already happened
- not normalized coordinates
- not model-input coordinates
- not window coordinates
- not screen coordinates

This means `DetectionSet` can prove "the model saw these boxes in this source
image" and nothing stronger.

## Why DetectionSet Is Not Candidate Yet

`contract::Candidate` is a replayable action target. `DetectionSet` is not.

The gap is structural, not cosmetic:

| Missing requirement | Why DetectionSet alone is insufficient |
| --- | --- |
| Durable source artifact identity | `DetectionSet` does not say which persisted capture artifact produced the image. |
| Projection basis | Source-image pixels cannot later become window/screen coordinates without capture geometry and transform metadata. |
| Freshness/liveness basis | `DetectionSet` has no statement about whether the detected thing is still present or re-checkable. |
| Action semantics | A class label like `button_play` or `joker_card` is not yet an action-ready target contract. |
| Known limits | Model uncertainty, occlusion, crop limits, or detector confusion do not currently travel with each detection set. |
| Runtime failure classification | There is no place to distinguish grounding failure, stale capture, projection failure, semantic mismatch, or action-side verification failure. |

If someone writes:

```text
Detection bbox center -> Candidate -> click
```

without filling these gaps, that is not a typed evidence bridge. It is just
coordinate clicking with nicer nouns.

## Minimum Future Adapter Inputs

Before a future adapter can produce runtime-facing evidence, it must add all of
the following outside `DetectionSet` itself.

### 1. Source Image Artifact Reference

The adapter must know which durable capture artifact produced the image.

Minimum requirement:

- an `ArtifactRef`-like identity that points to the stored source image
- enough run/span/artifact identity to let inspect/read surfaces find that
  image again

Without this, downstream consumers only have anonymous pixels.

### 2. Coordinate Space and Projection Basis

The adapter must carry explicit projection context:

- source image size
- capture origin and bounds
- the coordinate space of the capture
- how source-image pixels would map back to window/display/screen coordinates

This does **not** mean the future bridge must immediately output click points.
It means the bridge must at least preserve enough projection basis that a later
layer can compute them honestly.

### 3. Freshness and Liveness Basis

The adapter must say why the evidence is still meaningful later.

Examples of what a future bridge may need:

- capture timestamp or capture artifact lineage
- foreground/window assumptions
- re-observation requirements
- "evidence-only" status when no liveness claim exists

If there is no freshness or re-check basis, the output cannot honestly become a
stable `Candidate`.

### 4. Known Limits

The adapter must preserve model-side caveats, for example:

- detector sees only the captured crop
- class confusion risk
- overlap/occlusion uncertainty
- stale screenshot risk
- no semantic proof that the box is actionable

These belong in the bridge output, not by silently widening `DetectionSet`.

### 5. Stable Meaning of model_id / label / confidence

The future bridge must keep the following meanings explicit:

- `model_id`: which detector vocabulary produced the result
- `class_id` / `label`: detector vocabulary, not automatically domain meaning
- `confidence`: detector score, not action confidence and not semantic success

Detector confidence is a model-side ranking signal. It is not verification.

## Preferred Future Direction

The safest future route is still two-step:

```text
DetectionSet
  -> DetectionEvidenceManifest
  -> recognition evidence record
  -> Candidate only when action/liveness/projection gates are satisfied
```

That keeps "the model saw this" separate from "AUV can act on this safely."

This document intentionally does not lock the exact bridge type name. It only
locks the requirements that must exist before a runtime-facing bridge is honest.

## Non-Goals For This Slice

- No new field on `DetectionSet`
- No `src/contract.rs` edits
- No runtime or driver integration
- No app-layer consumer integration
- No Balatro game-state interpretation
- No click-point synthesis

## Test Guidance

Boundary tests in this phase should only lock:

- `DetectionSet` JSON stays inference-only
- bbox remains source-image pixel coordinates in the serialized shape
- runtime bridge fields are absent
- roundtrip does not require action/runtime metadata

Tests in this slice should **not** pretend to prove candidate readiness.

## Follow-Up

The next inference-scoped step after this boundary is
`DetectionEvidenceManifest` v0:

```text
DetectionSet
  + source image evidence
  + explicit coordinate space
  + explicit projection availability
  + model run metadata
  + known limits
```

That remains inference-only and still does not authorize a runtime or Candidate
bridge by itself.
