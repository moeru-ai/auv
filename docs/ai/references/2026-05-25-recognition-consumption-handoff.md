# Recognition Consumption Handoff

Date: 2026-05-25

Status: current handoff for the next coding agent

Current HEAD when written: `24b62ca`

## Start Here

Read these files first, in this order:

1. `AGENTS.md`
2. `docs/TERMS_AND_CONCEPTS.md`
3. `docs/ai/references/2026-05-24-structured-observation-roadmap.md`
4. `docs/ai/references/2026-05-23-surface-selector-contract.md`
5. `docs/ai/references/2026-05-21-scroll-scan-design.md`
6. `src/contract.rs`
7. `src/driver/macos/support/artifacts.rs`
8. `src/driver/macos/support/recognition.rs`
9. `src/driver/macos/control/music.rs`
10. `src/scroll_scan.rs`

Before editing, verify the live repository state:

```bash
git status --short --branch
git log --oneline --decorate -10
```

Do not rely on this document as a substitute for live `git` state. It records
the intended continuation point after `24b62ca`.

## Product Goal

AUV should expose application operations as structured, inspectable packages,
not as raw coordinate scripts or pure CLI text.

The long-term agent-facing shape is:

```text
getter:
  search.results
  playlists.list
  resources.snapshot
  selectedObject.get

action:
  candidate.select
  song.play
  fleet.move
  panel.open

verification:
  stateChanged
  semanticMatched
  activationOnly
  failedWithEvidence
```

The core chain is:

```text
raw provider output
  -> RecognitionResult
  -> Candidate / CandidateRef
  -> action consuming CandidateRef
  -> VerificationResult
  -> inspectable trace/artifacts
```

This is the concrete bridge between "large model explores UI once" and
"smaller agents call stable skills later". The skill product should return
structured state, candidates, and verification results with evidence, not just
coordinates and screenshots.

## Current Facts At `24b62ca`

The previous structured-observation handoff is no longer current. It was
written around `37ff6e2`. Since then, several phases have landed.

Important current commits:

```text
24b62ca feat(macos): wire icon_match recognition evidence chain
39791b0 feat(macos): wire screen-rows recognition evidence chain
53719a0 feat(macos): wire window OCR row recognition evidence chain
954ea61 feat(macos): wire observe_window_region recognition evidence chain
0df7ba8 feat(macos): wire recognition evidence chain through DriverArtifactBuilder
c0a8b13 refactor(macos): replace magic artifact_id constants with DriverArtifactBuilder
4c9af00 refactor(driver): replace _auv_run_id/_auv_span_id input smuggling with typed DriverRunContext
c96fd08 fix(scroll-scan): surface RecognitionResult deserialize errors instead of swallowing them
a944ae2 Revert "feat(macos): add ONNX neural network detect command (Phase 7c)"
```

What is already done:

- `src/contract.rs` defines `ArtifactRef`, `CandidateRef`,
  `OperationResult`, `Candidate`, `VerificationResult`, `FailureLayer`,
  `RecognitionResult`, `RecognizedItem`, `RecognitionBox`, and the v0
  `SurfaceSelector` types.
- The ONNX / neural detect command was reverted. There should be no
  `debug.findNeuralDetect` command and no `RecognitionSource::NeuralNetworkDetect`
  enum value on this mainline.
- `DriverRunContext` now carries run/span identifiers into driver calls
  without smuggling `_auv_run_id` or `_auv_span_id` through user inputs.
- `DriverArtifactBuilder` exists so drivers can create `ArtifactRef`s that
  line up with the runtime's staged artifact order.
- Row-like producers now emit `RecognitionResult` artifacts:
  - `debug.findWindowRows`
  - `debug.findScreenRows`
  - `debug.observeWindowRegion`
- `debug.findIconMatch` emits `RecognitionResult(source=icon_match)` using NCC
  template matching. It is evidence-only and must not directly click.
- Recognition artifacts now cite their source screenshot artifact through
  `scope.capture_artifact` and `evidence`.
- `scroll_scan` now prefers `RecognitionResult` artifacts over legacy row JSON
  when parsing observation pages.
- If a JSON object looks like a `RecognitionResult` but fails to deserialize,
  `scroll_scan` surfaces the serde error instead of silently falling back to
  legacy rows.

## What Is Not Done Yet

The next gap is not another detector.

The gap is proving that downstream consumers can reliably consume
`RecognitionResult` items and preserve the evidence chain.

Known incomplete areas:

- `segmented_region` and `icon_match` are still mostly producer-side evidence.
  They are not validated product control paths.
- `music.search.results` links candidates to recognition evidence, but the
  consumer path still needs tighter tests proving the exact item/ref survives
  into later action and verification artifacts.
- `scroll_scan` can parse `RecognitionResult`, but it still needs better
  end-to-end evidence that observation records preserve recognition source,
  item id, and artifact provenance in scan outputs.
- `ArtifactRef.captured_event_id` is still generally `None` inside driver-built
  JSON because the runtime mints artifact capture events while staging the
  response. Do not pretend event ids are available unless the runtime fills or
  patches them later.
- `capture_contract_artifact` is still usually `None` for recognition scope.
  The capture contract detail may be embedded in provider detail, but that is
  not the same as a separate artifact ref.
- The inspect viewer may preview the JSON artifacts, but there is no dedicated
  recognition overlay viewer contract yet.

## Immediate Next Goal

Do this next:

```text
Prove RecognitionResult can be consumed by downstream operations without losing
source artifact, recognition_id, recognized item id, and failure context.
```

This should be a narrow consumer-focused pass. It should not add new
recognition backends.

Recommended commit shape:

```text
feat(scan): preserve recognition item refs in scan observations
```

or, if starting from the music loop:

```text
test(music): prove candidate action preserves recognition evidence
```

Pick one lane, not both at once.

## Lane A: Scroll Scan Consumer Pass

Use this lane if the next agent wants a generic consumer before touching a real
music action.

Goal:

```text
RecognitionResult artifact
  -> scroll_scan observation
  -> scan artifact
  -> preserved source_artifact + recognition_id + recognized_item_id
```

Implementation targets:

- `src/scroll_scan.rs`
- Existing tests around:
  - `parse_observe_json_prefers_recognition_result_filtered_items`
  - `observations_from_json_artifacts_prefers_recognition_result_over_legacy_rows`
  - scan artifact serialization tests

Concrete tasks:

1. Ensure each `CollectionObservation` created from a `RecognizedItem` preserves:
   - source artifact path
   - `recognition_id`
   - `recognition_source`
   - `recognition_surface`
   - `recognized_item_id`
   - `recognized_item_kind`
   - provider score when present
2. Ensure scan artifact serialization includes these attributes.
3. Add a regression test that feeds a synthetic `RecognitionResult` with two
   filtered items into scan parsing and asserts the final scan artifact can
   identify the exact recognized item used.
4. If a malformed recognition artifact is encountered, preserve the current
   behavior: fail clearly instead of falling back to legacy rows.

Do not:

- add YOLO / ONNX / new detector sources
- add a new orchestration language
- make scroll scan domain-specific for music
- claim top/bottom list completeness unless scroll boundary evidence actually
  supports it

Acceptance:

```bash
cargo test scroll_scan
cargo test recognition
cargo fmt --check
git diff --check
```

## Lane B: Music Candidate Consumer Pass

Use this lane if the next agent wants the first product-like getter/action loop
to consume recognition evidence more explicitly.

Goal:

```text
music.search.results
  -> OperationResult(Candidates)
  -> CandidateEvidence.recognition_result_ref
  -> music.result.play
  -> OperationResult(Verification)
  -> evidence includes candidate source and recognition source
```

Implementation targets:

- `src/driver/macos/control/music.rs`
- `recipes/macos/qqmusic/*` only if needed for validation
- tests around candidate evidence and `music.result.play`

Concrete tasks:

1. Keep `CandidateRef` as the authoritative cross-operation handle.
2. Ensure `music.search.results` candidates include:
   - candidate-local id
   - candidate evidence artifact ref
   - recognition result ref when available
   - inline summary for CLI readability
3. Ensure `music.result.play` reads the candidate source artifact from the
   run store, not from in-memory state.
4. Ensure `music.result.play` verification artifact records:
   - candidate ref consumed
   - recognition ref consumed, if present
   - whether success is semantic success, state change only, or activation only
   - failure layer if verification is not trustworthy
5. Add tests that do not require QQ Music to be open.

Do not:

- use icon matching as an automatic QQ Music fallback yet
- claim semantic playback success from row activation alone
- add new live desktop probes unless the user explicitly allows disturbance

Acceptance:

```bash
cargo test music
cargo test contract
cargo fmt --check
git diff --check
```

## Why Not Continue Phase 7c

Do not re-add ONNX / YOLO right now.

Reason:

```text
More producers do not solve the current bottleneck.
The current bottleneck is consumer trust: can actions and verifiers cite exactly
which recognized item they used, and can failures identify grounding/control/
verification/semantic mismatch?
```

Only consider ONNX / YOLO after:

- `RecognitionResult` consumption is proven in at least one generic scan path
  or product loop.
- Evidence refs survive from capture to recognition to candidate/action.
- The inspect output makes it clear which candidate/item was selected.
- There is an explicit runner protocol and a real model/evidence pack.

## Rules For The Next Agent

- Verify the live `git` state before editing.
- Keep the next commit consumer-focused.
- Do not add detection backends.
- Do not wire `icon_match` into clicking unless the user explicitly asks.
- Do not remove legacy text artifacts or CLI-readable summaries.
- Do not make `Candidate.confidence` top-level; keep provider scores inside
  recognition item/detail.
- Do not treat heuristic visual segmentation as proof.
- If a doc says something stale about `NeuralNetworkDetect`, fix the doc before
  coding against it.
- If changing or introducing a core term, update `docs/TERMS_AND_CONCEPTS.md`.

## Validation Commands

For Rust changes, run:

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
cargo run --quiet -- list-commands
cargo run --quiet -- skill cases list
cargo run --quiet -- skill bundle list
```

For docs-only changes, at minimum run:

```bash
git diff --check
```

## Suggested Prompt For The Next Codex Agent

```text
You are working in /Users/liuziheng/https-github-com-moeru-ai-auv.

Read:
- AGENTS.md
- docs/ai/references/2026-05-25-recognition-consumption-handoff.md
- docs/ai/references/2026-05-24-structured-observation-roadmap.md
- src/contract.rs
- src/scroll_scan.rs

Current direction:
Do not add a new detector. Do not revive ONNX/YOLO. Do not wire icon_match into
clicking. The next task is to prove RecognitionResult can be consumed by a
downstream operation while preserving source artifact, recognition_id,
recognized_item_id, and failure context.

Start with Lane A unless instructed otherwise:
Add tests and minimal code so scroll_scan observations produced from
RecognitionResult preserve recognition item provenance in the final scan
artifact.

Run:
cargo fmt --check
cargo check
cargo test
git diff --check
cargo run --quiet -- list-commands
cargo run --quiet -- skill cases list
cargo run --quiet -- skill bundle list
```
