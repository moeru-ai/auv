# Game Recognition To Recipe Consumer Seam (M0)

Date: 2026-06-10

Status: docs-only alignment note for the next zero-AX app-family slice

## Purpose

Pin one shared seam before any Slay the Spire code lands:

```text
detector / OCR evidence
  -> RecognitionResult artifact
  -> typed consumer command
  -> recipe step signals / OperationResult / VerificationResult
```

This note exists to stop Balatro and Slay the Spire from each wiring a
different detector-consumer path.

It does **not** approve:

- autoplay or combat planning
- detector output clicking directly
- a second recognition schema beside `RecognitionResult`
- a third action-result schema beside
  `ActionResolverDecision -> InputActionResult -> OperationResult / VerificationResult`
- `if app == sts` branches inside AUV core

## Current Repo Truth

Two halves already exist on `main`.

Bottom half, detector-side evidence:

```text
DetectionEvidenceManifest
  + runtime scope / artifact refs / projection context
  -> RecognitionResult
  -> detector-recognition artifact
  -> run_read / inspect lineage
```

Anchors:

- `src/inference_recognition.rs`
- `docs/superpowers/specs/2026-06-05-detector-manifest-recognitionresult-mapping-v0.md`
- `docs/superpowers/specs/2026-06-06-game-slay-the-spire-observe-only-recognition-fixture-boundary.md`

Top half, runtime-side invoke/recipe execution:

```text
bundle command
  -> Runtime::invoke_in_span(...)
  -> SkillRecipeRunner::run_into_existing_run(...)
  -> run / spans / artifacts / signals
```

Anchors:

- `src/runtime.rs`
- `src/skill/recipe.rs`
- `src/skill/mod.rs`
- historical `native-app-skill-tree` bundle manifest, retired 2026-06-11

What is still missing is the middle seam:

```text
RecognitionResult artifact
  -> recipe-consumable typed domain operation
```

Today no recipe step consumes detector-backed `RecognitionResult` as step
evidence.

## Existing Pattern To Reuse

The current AUV product-like consumer pattern is **not** "recipe parses JSON
artifacts directly". It is:

```text
producer command
  -> typed artifact
  -> string-serialized handle exported in step signals
  -> downstream typed consumer command resolves the handle itself
```

Canonical example:

```text
music.search.results
  -> OperationResult::Candidates
  -> CandidateRef serialized into
     music.search.results.selected_candidate_ref
  -> recipe passes ${step_*_signal_*}
  -> music.result.play resolves CandidateRef back into typed evidence
```

Anchors:

- `src/driver/macos/control/music.rs`
- `recipes/macos/qqmusic/play-search-result-candidate.v0.json`

This is the seam to copy.

The important property is ownership:

- recipe runner remains a scalar-template orchestrator
- typed consumers own parsing and validating structured handles
- run-store lookup stays inside runtime/domain commands, not in recipe JSON glue

## M0 Decision

For zero-AX game families, the first consumer seam should follow the same
shape:

```text
RecognitionResult producer
  -> export a typed handle as a signal string
  -> downstream typed consumer resolves it from run/artifact lineage
```

Concretely:

1. `RecognitionResult` remains the only runtime-facing recognition evidence
   contract.
2. Recipe steps continue to exchange **strings**, not in-memory structs.
3. If a downstream step needs one recognized item, the producer step should
   export a string handle for that item.
4. The downstream typed consumer command is responsible for:
   - decoding that handle
   - loading the referenced artifact/run data
   - validating that the referenced item still exists in the source evidence
   - translating it into domain output, candidate promotion input, or refusal

This keeps the seam consistent with the current `CandidateRef` pattern and
avoids teaching the recipe runner to understand detector-specific JSON.

## Ownership Boundary

The seam is split across these layers:

### 1. Recognition Producer

Owns:

- detector/OCR output
- runtime artifact staging
- `RecognitionResult` construction
- evidence and `known_limits`

Current owner examples:

- `src/inference_recognition.rs`
- `src/ax_recognition.rs`
- row-recognition producers under `src/driver/macos/support/recognition.rs`

### 2. Recipe Runtime

Owns:

- step execution order
- signal export
- artifact-path export
- dry-run behavior
- run/span recording

It does **not** own:

- parsing `RecognitionResult` artifacts
- domain selection policy
- candidate liveness logic

Current owner:

- `src/skill/mod.rs`
- `src/skill/recipe.rs`

### 3. Typed Consumer Command

Owns:

- decoding the producer-exported handle
- reloading typed evidence from run/artifact lineage
- domain-specific interpretation
- optional promotion/gating into action-grade state
- refusal when evidence is missing, stale, or ambiguous

This is where the first StS and Balatro consumers should converge.

## Type Flow To Preserve

### Read / Observe Commands

For commands like:

- `sts.readPlayerHp.v0`
- `sts.readEnergy.v0`
- `sts.listHandCards.v0`

the intended shape is:

```text
capture-image
  -> DetectionEvidenceManifest
  -> RecognitionResult artifact
  -> typed read consumer
  -> OperationResult / InvokeResult signals
  -> recipe-level expectations
```

These commands do not need `CandidateRef` if they are read-only. They still
must consume `RecognitionResult` through one typed command boundary rather than
having the recipe parse raw detector output.

### Future Gated Click

For a later command like:

- `sts.clickEndTurn.v0`

the shape must stay:

```text
capture-image
  -> DetectionEvidenceManifest
  -> RecognitionResult artifact
  -> typed selector consumer
  -> existing promotion / gating seam
  -> ActionResolver
  -> auv-driver InputActionResult
  -> VerificationResult
```

Detector boxes remain evidence only. They do **not** become click coordinates
by themselves.

## Shared Rule For Balatro And StS

Balatro and StS should share:

- `RecognitionResult`
- runtime artifact recording
- string-handle export through recipe signals
- typed consumer-side reload from run/artifact lineage
- existing promotion/gating seam before action

They should **not** each invent:

- separate detector-recognition record types
- recipe-local JSON parsing conventions
- game-specific core runtime branches
- detector-direct-to-click shortcuts

## What The First Consumer Handle Should Not Be

Do **not** start by adding a new generic `RecognitionItemRef` contract just
because it sounds tidy.

That may become right later, but `M0` does not prove the shared shape yet.
The cheaper and safer move is:

- first prove one producer-exported string handle pattern for game recognition
- keep the typed decode/validate logic in the consumer command
- only extract a shared `RecognitionItemRef` contract if both Balatro and StS
  produce the same pressure

The repo already has this exact extraction pattern with `CandidateRef`.

## First Code Slice After M0

The next implementation slice should be narrow:

```text
observe-only game recognition producer
  + one typed read consumer
  + one recipe step path that proves signal-exported handle -> consumer reload
```

Suggested order:

1. StS observe/capture fixture or live narrow producer
2. one read consumer, not click
3. only then discuss action-grade promotion for `End Turn`

This keeps the first proof on the missing seam instead of jumping straight to
action.

## Deferred On Purpose

- generic `RecognitionItemRef` in `src/contract.rs`
- recipe-runner support for non-scalar structured arguments
- detector-backed direct candidate promotion without an explicit typed consumer
- game-state planner contracts
- multi-step autonomous gameplay

Those are valid later topics. They are not needed to prove the current seam.
