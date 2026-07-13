# Detection Evidence Manifest v0

Date: 2026-06-05

Status: inference-scoped design + test boundary with gated Balatro runtime-artifact smoke consumer

Base: `origin/main` after `809c787`

## Purpose

Define the minimum inference-scoped evidence package that must exist before any
future bridge from `DetectionSet` into AUV runtime contracts can even be
discussed.

This slice does **not** implement:

- `DetectionSet -> RecognitionResult` as a generic runtime production path
- `DetectionSet -> contract::Candidate`
- driver capture integration
- window/display projection beyond identity source-image smoke projection
- app validation or action consumption

A gated Balatro smoke now proves one narrow real path:

- local Balatro source image can be staged as `capture-image` runtime artifact
- `DetectionEvidenceManifest` can feed detector-backed `RecognitionResult` artifact recording
- read-side lineage can resolve the resulting `detector-recognition` artifact

It only introduces an inference evidence manifest that answers:

- which source image the detections came from
- what coordinate space the bbox uses
- whether any projection basis exists
- what model/backend thresholds produced the result
- what limits still apply

## Manifest Shape

`DetectionEvidenceManifest` is an inference-scoped package:

```text
DetectionEvidenceManifest
  - detection_set: DetectionSet
  - source_image: SourceImageEvidence
  - model_run: ModelRunMetadata
  - known_limits: Vec<String>
```

### DetectionSet

Still carries only structured detector output:

- `model_id`
- `image_size`
- `detections[]`

Per detection:

- `class_id`
- `label`
- `confidence`
- `bbox`

### SourceImageEvidence

Binds the detection set to a source image without pretending that the image is
already a runtime artifact.

Current v0 fields:

- `source_image_ref`
- `coordinate_space`
- `projection_basis`

`SourceImageRef` is intentionally inference-scoped:

- `local_path`
- `opaque_id`

It is **not** `ArtifactRef`.

### DetectionCoordinateSpace

Current v0 supports only:

- `source_image_pixels`

This makes bbox semantics explicit and keeps later layers from mistaking them
for window/screen coordinates.

### ProjectionBasis

Current v0 supports only:

- `unavailable { reason }`

That is deliberate. The current Balatro smoke path has no AUV-owned capture
artifact, no display transform, no window mapping, and no replayable click
basis. The manifest must say that explicitly instead of silently implying a
projection exists.

### ModelRunMetadata

Captures the detector-side run parameters that shape the result:

- `backend`
- `model_id`
- `confidence_threshold`
- `iou_threshold`
- `class_label_source`
- optional `execution_provider`

This is still not runtime metadata. It only explains detector provenance.

### known_limits

`known_limits` remains explicit and serializable so the inference layer can
admit what it does **not** prove, for example:

- local source image identity is not a runtime artifact
- projection basis is unavailable
- detector score is not semantic success
- annotated image is a debug aid only

## Why This Exists

Without this manifest, future bridge work would be forced to jump straight from:

```text
DetectionSet -> Candidate
```

That is too early and too unsafe.

The safer future chain is:

```text
DetectionSet
  + SourceImageEvidence
  + ProjectionBasis
  + ModelRunMetadata
  -> inference-scoped evidence manifest
  -> future RecognitionEvidence design
  -> maybe Candidate later
```

This keeps "the model saw a box" separate from "AUV can safely consume this as
actionable evidence."

## Non-Goals

- No `src/contract.rs` edits
- No runtime bridge fields
- No action semantics
- No click-point synthesis
- No app/runtime/driver integration
- No provider support expansion claims

## Debug Artifacts

Annotated images remain debug aids only.

The manifest does **not** require:

- annotated PNG path
- preview asset path
- rendered overlay metadata

Those may be emitted next to the manifest for debugging, but they are not part
of the required inference evidence package.

## Empty Detection Reporting

An empty detection set is still valid manifest input.

The manifest does not turn "0 detections" into success or failure by itself.
Callers should preserve:

- thresholds
- backend
- model id
- source image reference
- known limits

so later readers can tell the difference between "nothing detected" and "no
evidence recorded."

## Future Unlock Conditions

A later bridge into AUV runtime contracts is only eligible when a separate
slice can answer all of these:

- what durable artifact identity replaces inference-only source image refs
- how source-image pixels project to capture/display/window coordinates
- what freshness/liveness basis exists
- when detections remain evidence-only versus candidate-eligible
- how verification and failure layers are represented

Until then, `DetectionEvidenceManifest` remains inference evidence only.
