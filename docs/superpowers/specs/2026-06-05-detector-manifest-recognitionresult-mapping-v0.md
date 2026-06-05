# Detector Manifest to RecognitionResult Mapping v0

Date: 2026-06-05

Status: partial implementation landed

Base: `origin/main` after `1dff479`

## Purpose

Define the next narrow question after `DetectionEvidenceManifest` v0 and the
RecognitionEvidence boundary:

```text
If a future slice is allowed to emit runtime-side recognition evidence from
detector manifests, how would that evidence populate existing
RecognitionResult semantics?
```

This slice does **not** implement:

- `DetectionEvidenceManifest -> RecognitionResult`
- `DetectionSet -> RecognitionResult`
- `DetectionSet -> contract::Candidate`
- `DetectionEvidenceManifest -> contract::Candidate`
- `src/contract.rs` changes
- runtime artifact recording
- capture integration
- driver or app consumer integration

It only defines the mapping questions and preferred semantics so later work can
converge on `RecognitionResult` instead of inventing a parallel detector-side
runtime schema.

## Implementation Status

`main` now contains a narrow root-crate mapper that proves the smallest honest
bridge shape:

```text
DetectionEvidenceManifest
  + runtime RecognitionScope
  + runtime ArtifactRef evidence
  + explicit runtime projection context
  -> RecognitionResult
```

Implemented in this slice:

- pure mapping function only
- explicit rejection for missing runtime evidence
- explicit rejection for missing `scope.capture_artifact`
- explicit rejection for source-image size mismatch
- `RecognitionSource::Custom` bridge policy
- carry-forward of detector-side `known_limits`
- synthetic tests for failure and success cases

Still not implemented:

- runtime capture production
- display/window projection beyond caller-declared identity mapping
- Candidate promotion
- runtime artifact recording
- driver/app/runtime integration

## Convergence Rule

The current repository already has a stable runtime recognition contract:

```text
RecognitionResult {
  recognition_id,
  source,
  scope,
  best,
  filtered,
  all,
  detail,
  evidence,
  known_limits,
}
```

So a future detector-manifest bridge should target that shape directly unless a
separate owner-approved slice proves it cannot.

This document therefore answers:

- how a detector manifest would populate `RecognitionResult`
- which fields are still blocked today
- what remains evidence-only even after such a mapping exists

It does **not** approve creating `DetectorRecognitionResult`,
`RecognitionEvidenceResult`, or any other parallel runtime contract.

## Current Inputs And Output

### Input: DetectionEvidenceManifest

Current inference-side manifest carries:

- `detection_set`
- `source_image`
- `model_run`
- `known_limits`

From `detection_set`:

- `model_id`
- `image_size`
- `detections[]`
- per detection: `class_id`, `label`, `confidence`, `bbox`

### Output Target: RecognitionResult

Current runtime-side contract expects:

- one `recognition_id`
- one `RecognitionSource`
- one `RecognitionScope`
- one optional `best`
- many `filtered`
- many `all`
- one `detail` object
- `evidence: Vec<ArtifactRef>`
- `known_limits: Vec<String>`

That means the bridge question is not "should we add detector buckets?" The
buckets already exist. The question is how detector output would populate them
honestly.

## Preferred Mapping

The preferred future mapping is:

```text
DetectionEvidenceManifest
  -> classify manifest as runtime-mappable or inference-only
  -> project detections into RecognizedItem values
  -> assign RecognitionResult.source
  -> assign RecognitionResult.scope
  -> derive best / filtered / all
  -> carry forward evidence + known_limits
```

If any required runtime field is unavailable, the manifest stays
inference-only. The bridge must not fill runtime fields with placeholders just
to make the shape serialize.

## Field Mapping Questions

### 1. recognition_id

The bridge must define a deterministic or at least audit-friendly
`recognition_id`.

Current acceptable directions:

- runtime-generated UUID at bridge time
- stable hash over runtime capture artifact + model id + mapping policy version

Current non-goal:

- minting a `recognition_id` in inference-only smoke paths that still lack
  runtime scope and artifact identity

The important invariant is that `recognition_id` belongs to the runtime-side
recognition record, not to the raw detector output.

### 2. source

`RecognitionResult.source` is an enum, not a free-form string.

Current enum values are:

- `ocr_text`
- `ocr_row`
- `visual_row`
- `segmented_region`
- `icon_match`
- `custom`

Because there is no detector-specific source variant on `main`, the bridge must
not silently pretend detector output is OCR or icon match.

So v0 bridge policy should assume:

- use `custom` for detector-backed runtime recognition until a separate contract
  slice adds a stable detector-specific source variant
- record the exact detector provenance in `detail`

This keeps the runtime contract honest without editing `src/contract.rs` in
this slice.

### 3. scope

`RecognitionScope` is the largest current blocker.

A valid runtime `RecognitionResult` needs a real scope with fields such as:

- `surface`
- optional display/window/app context
- optional `capture_artifact`
- optional `capture_contract_artifact`

Current local Balatro smoke manifests do not contain those runtime artifact
refs or capture scope fields.

Therefore the bridge policy must be:

- if runtime capture scope is unavailable, do **not** emit `RecognitionResult`
- if runtime capture scope exists, prefer the narrowest honest surface
  (`region`, `window`, `display`, then `screen`) and carry the capture
  artifact refs explicitly

There is no approved fallback where a local image path becomes a fake runtime
scope.

### 4. all

Preferred meaning:

```text
all = every detection from DetectionSet that survived upstream detector
postprocess and was accepted into bridge projection as a RecognizedItem
```

This means:

- `all` is not raw pre-NMS model output
- `all` is not "only semantically eligible runtime items"
- `all` may still omit detections the bridge refuses to project at all, but the
  rejection reason must be explained in bridge policy or `known_limits`

Each projected detection should become one `RecognizedItem` with:

- `item_id`
- `kind`
- `box_`
- `text = None` unless another approved source adds text
- `provider_score = Some(confidence as f64)`
- `detail` carrying detector-specific metadata such as `class_id`,
  `label_source`, `model_id`, and optional bridge-local rejection notes

### 5. filtered

Preferred meaning:

```text
filtered = the subset of projected items that pass the runtime-side semantic
filter for the current bridge policy
```

Examples:

- class allowlist for this model
- region-specific filter
- overlap suppression at bridge policy level
- domain-specific runtime eligibility checks

Important constraint:

- `filtered` is not allowed to mean "all detections above the detector score
  threshold", because the detector already applied its own threshold before the
  manifest existed

The runtime bridge must make its own filter semantics explicit.

### 6. best

Preferred meaning:

```text
best = the single bridge-selected winner from filtered, only when the bridge
policy actually needs or can justify a winner
```

This implies:

- `best` may be `None` even when `filtered` is non-empty
- detector confidence alone is not sufficient reason to pick `best`
- if bridge policy chooses `best`, the selection rule must be explicit in
  `detail`

Examples of valid future rules:

- top-ranked item after semantic filtering
- one winner per requested class when a caller asked for exactly one class
- `None` when the bridge is only producing evidence and no single winner is
  justified

This matches existing `RecognitionResult` practice better than forcing every
detector manifest to nominate a winner.

### 7. RecognizedItem.kind

Preferred meaning:

```text
kind = runtime semantic kind chosen by bridge policy, not blindly copied model label
```

That means one of two explicit approaches must be chosen in a future slice:

- use the detector label itself as the provisional runtime `kind`
- map detector labels/classes into a separate runtime vocabulary

Both are legal design directions. What is not legal is leaving the policy
implicit.

### 8. RecognizedItem.box_

Preferred meaning:

```text
box_ = runtime-space box derived from detector bbox after the bridge has a real
runtime capture scope and projection basis
```

Until projection exists, detector bbox remains source-image pixel evidence only.

So the bridge must not:

- copy source-image pixels straight into runtime `RecognitionBox`
- claim screen/window coordinates without projection basis

If projection cannot be performed honestly, the manifest stays inference-only.

### 9. detail

Preferred top-level `RecognitionResult.detail` should record bridge policy, not
just raw detector trivia.

At minimum, a future mapped `detail` should explain:

- source manifest version
- detector backend/model id
- class label source
- bridge policy version
- filter strategy
- best-selection strategy

Per-item `detail` should preserve:

- detector `class_id`
- raw detector `label`
- detector confidence
- any bridge-local notes relevant to why the item is in `all`, `filtered`, or
  excluded from `best`

### 10. evidence

Preferred meaning:

```text
evidence = runtime artifact refs that let inspect/read-side consumers locate the
capture source again
```

Current blocker:

- `DetectionEvidenceManifest.source_image_ref` is inference-scoped and is not
  `ArtifactRef`

Therefore:

- manifest-only smoke cannot yet produce valid runtime `evidence`
- a future capture-integrated bridge must replace inference-only image identity
  with runtime artifact refs before emitting `RecognitionResult`

### 11. known_limits

Preferred carry-forward rule:

- start with manifest `known_limits`
- append bridge-local runtime limits
- do not erase detector-side caveats

Expected detector-side limits that should survive:

- detector score is not semantic success
- source-image coordinate basis requires projection before action
- inference-only source image identity is not runtime artifact identity
- class vocabulary is detector-local until mapped

Expected new runtime-side limits that may be added later:

- bridge emitted evidence-only recognition, not candidate-ready output
- runtime scope came from capture integration and may still be stale
- `best` omitted because no single winner was justified

## Evidence-Only Versus Candidate-Eligible

Even if a future slice successfully maps a detector manifest into
`RecognitionResult`, that still does **not** imply Candidate promotion.

The safe chain remains:

```text
DetectionEvidenceManifest
  -> RecognitionResult
  -> existing candidate promotion / liveness / control gates
  -> Candidate only if those gates are separately satisfied
```

This document is about recognition mapping, not action readiness.

## Test Guidance

When implementation is later approved, tests for that slice should prove:

- mapped detector outputs populate `RecognitionResult.best/filtered/all`
  intentionally, not accidentally
- `source=custom` is used until a dedicated detector source variant exists
- runtime scope is required; manifest-only local smoke does not fake it
- projected item `provider_score` preserves detector confidence semantics
- `known_limits` carry forward detector-side caveats
- absence of runtime artifact refs keeps the output inference-only

What tests in that future slice should **not** claim:

- Candidate readiness
- click-point validity
- app/runtime action success
- capture integration if no capture artifact path actually exists

## Non-Goals

This slice does not approve:

- new runtime contract types
- `src/contract.rs` edits
- a detector-specific `RecognitionSource` variant
- `DetectionEvidenceManifest -> Candidate`
- Balatro domain interpretation
- runtime/app/driver integration

## Follow-Up

The next honest implementation slice, if approved, is:

```text
docs/test(inference): prove capture-integrated detector manifest can map into RecognitionResult
```

Not:

```text
feat(inference): promote detector output directly to Candidate
```
