# Surface Analyze v0

Date: 2026-05-28

Status: contract note, v0 boundary

Implementation baseline: `b702be0`

## Purpose

`app analyze` is the bridge between an app probe and later distillation. Its job
is to turn probe evidence into honest surface candidates, not to pretend that
every visible text box, OCR fragment, or row-like band is already an
action-grade semantic target.

This document freezes the v0 boundary for:

- what "surface" means in `app analyze`
- what `AppSurfaceCandidate` is allowed to represent
- when a surface candidate may be promoted into an action-grade `Candidate`
- how evidence refs should be represented
- what counts as "analyze is done" for v0

This is a technical boundary, not another governance layer.

## Definitions

### Surface

A surface is an app-exposed area or structure that can be observed, queried, or
re-located in the current probe/run.

Examples:

- AX nodes and AX trees
- OCR text fragments
- OCR or visual row bands
- window bounds and regions
- icon/template matches
- menus, shortcuts, or command affordances
- future DOM/CDP or detector outputs

The v0 implementation only has selector clauses for AX, OCR, and row. Future
backends such as DOM, icon, menu, shortcut, or CV should be added as new
selector/recognition producers, not as a reason to weaken the v0 boundary.

### Surface Observation

A surface observation is the raw or structured evidence that something was seen:
AX tree, OCR match list, row bands, window bounds, screenshot/capture contract,
or a recognition artifact.

Observation does not imply actionability.

### Surface Selector

A surface selector describes how to re-find a surface candidate. In v0 this is
`CandidateQuery` with `SurfaceSelectorClause::{Ax,Ocr,Row}`.

Selector examples:

- AX role/label/path
- OCR text anchor and provider score threshold
- row index or row containing text within the target window

Selector existence does not imply semantic success. It only means there is a
candidate re-location strategy that can be attempted later.

### Surface Candidate

`AppSurfaceCandidate` is the `app analyze` report object. It is explainable,
probe-scoped, and reviewable.

It may describe:

- an AX focus target
- an OCR-visible text anchor
- a grouped row candidate
- a window region
- future recognition-backed surface candidates

It is not automatically the same thing as `contract::Candidate`.

### Action-Grade Candidate

`contract::Candidate` is an operation/runtime object that an action can consume
through `CandidateRef`.

Action-grade candidates must satisfy the promotion gate below. They carry enough
evidence, liveness, and control information for a later operation to re-check
the target before acting.

## Analyze Responsibilities

`app analyze` may:

- read probe artifacts
- classify observable surfaces
- emit `AppSurfaceCandidate` records
- attach `CandidateQuery` when a candidate can be re-located
- attach evidence references when available
- recommend strategies only when an existing action and verification contract
  can express the path
- write known limits when a candidate is visible but not semantically proven

`app analyze` must not:

- promote raw OCR text into semantic result selection
- promote row/list bands into a result-selection recipe without a row action and
  semantic verification path
- treat icon recognition as action success
- invent a strategy taxonomy that recipe rendering cannot validate
- create a second runtime `Candidate` schema outside `src/contract.rs`
- hide provider uncertainty behind a single top-level confidence number

## Promotion Gate

Promotion is the transition:

```text
AppSurfaceCandidate -> contract::Candidate -> ActionResolver / operation
```

Promotion is allowed only when all required gates are satisfied:

| Gate | Requirement |
| --- | --- |
| Re-location | The candidate has a `CandidateQuery` or an equivalent stable target spec. |
| Evidence | The candidate links to probe/run evidence, preferably via `ArtifactRef`. |
| Action contract | An existing operation can act on the target without inventing a new action path. |
| Verification contract | A verifier exists, or the result is explicitly marked evidence-only. |
| Liveness | Preconditions can be expressed and re-checked before action. |
| Control | Required focus, foreground, clipboard, keyboard, pointer, or AX behavior is explicit. |
| Failure layer | Failure can be classified as grounding, candidate expiration, control, verification, or semantic mismatch. |

If any gate is missing, keep the item as a surface candidate and record the
blocker in `AppSurfaceCandidate.promotion_gate.missing_gates`. Use candidate
notes or known boundaries for prose context, but do not rely on Markdown-only
text as the machine-readable gate.

This gate is the seam between `app analyze` and `ActionResolver`. Do not bypass
it by letting a driver action consume `AppSurfaceCandidate` directly.

## Evidence Refs

Do not add a new `SurfaceEvidenceRef` type in v0.

Use the existing `contract::ArtifactRef` shape for durable evidence identity:

- `source_run_id`
- `source_span_id`
- `source_artifact_id`
- `captured_event_id`

Surface-specific context should remain next to the candidate, not inside the
ref:

- coordinate space
- bounds or click point
- capture/window context
- source step id
- known limits
- provider detail or recognition detail

This keeps evidence references aligned with the runtime trace model and avoids a
third incompatible reference schema.

## Surface Kind Rules

| Surface kind | Analyze output allowed | Promotion precondition | Known limit |
| --- | --- | --- | --- |
| Search-entry AX text input | Surface candidate + candidate query + search-entry strategy when supported | Existing search-entry action path and evidence capture contract | Search submission success still requires validation. |
| Native AX text | Surface candidate + candidate query + native-text strategy when supported | Existing native-text action path and `verify.axText` contract | Requires AX-readable text after action. |
| Window point / region | Surface candidate + evidence-only window action strategy when no semantic surface exists | Existing window-relative click/capture path | Must not be promoted to semantic selection. |
| OCR visible text | Surface candidate + OCR `CandidateQuery` | A semantic verifier or action-grade candidate producer must exist | Visible text is title-level evidence, not result selection. |
| Row/list band | Surface candidate + row `CandidateQuery` | Row action plus semantic verification or explicit evidence-only status | Structural row evidence is not semantic identity. |
| Icon/template match | Recognition candidate only | Action contract plus verifier or explicit evidence-only status | Seeing an icon does not prove the intended control was activated. |
| Menu/shortcut affordance | Candidate or strategy only when the command path is explicit | Existing menu/shortcut operation and verification/evidence contract | Shortcut availability is not the same as current focus safety. |
| Future DOM/CDP | Reserved | DOM backend plus artifact/evidence mapping into `Candidate` | Do not call the whole selector layer "DOM". |
| Future CV/YOLO | Reserved | Detector output must become `RecognitionResult` before promotion | Detector confidence is not semantic success. |

## Relationship To Existing Contracts

`AppSurfaceCandidate` is an app-analysis report object.

`CandidateQuery`, `RecognitionResult`, `SurfaceNode`, `Candidate`,
`CandidateRef`, `VerificationResult`, and `ArtifactRef` live in `src/contract.rs`
and define the runtime/action contract.

The v0 direction is:

```text
probe artifact
  -> app analyze surface candidate
  -> CandidateQuery / evidence refs / known limits
  -> promoted contract::Candidate only after the gate
  -> action consumes CandidateRef
  -> VerificationResult records outcome and failure layer
```

Do not collapse these lifecycles too early. `app analyze` is for review and
distillation decisions; operation results are for machine consumption and replay.

## V0 Done Criteria

`app analyze` is v0-complete when it can do all of the following for a probe:

- emit surface candidates with `area`, `kind`, `source`, `status`, text/bounds,
  and known limits
- attach `CandidateQuery` whenever a candidate can be re-located
- attach or point toward durable evidence refs without inventing a parallel ref
  schema
- keep OCR-visible text as text evidence unless a semantic verifier exists
- keep row/list bands as structural evidence unless a row action and verifier
  exist
- recommend only strategies that current recipe/action/verification contracts can
  express
- write a report that makes promotion blockers visible to a human reviewer
- write JSON that another operation can use without scraping Markdown

Anything beyond this, including full DOM selectors, YOLO, broad visual
segmentation, or a new orchestration language, is outside v0.

## Implementation Status

The `surface analyze v0` evidence/promotion boundary is now partially
implemented on top of the `b702be0` selector baseline:

1. Evidence references on `AppSurfaceCandidate` landed in `6e0c7fd`.
   Candidates derived from current probe artifacts may now carry durable
   `ArtifactRef` values instead of Markdown-only evidence prose.
2. Promotion blockers on `AppSurfaceCandidate.promotion_gate` landed in
   `899ac79`. Blocked OCR/row candidates and strategy-only candidates are now
   explicit in the analyze report contract instead of being inferred from
   reviewer notes.
3. OCR visible text and row/list grouping remain surface candidates, not
   action-grade runtime candidates. The v0 boundary still treats them as
   observable evidence unless a later slice provides the missing action,
   liveness, and verification contracts.
4. Coordinate and capture context remain candidate-side detail rather than being
   pushed into a separate evidence-ref schema.

The remaining future step is still the same seam:

```text
AppSurfaceCandidate -> contract::Candidate
```

That promotion work is intentionally outside the current v0 analyze closure.
