# Surface Analyze Closure — 2026-05-28

Status: v0 boundary mostly closed

PR branch tip before this closure note was added:

- `codex/surface-analyze-evidence-refs` at `915dbff`

Current `origin/main` during this closure note:

- `5c67054`

## Purpose

This note records the **actual closure state** of the `surface analyze v0`
work after PR #8 was extended beyond the original evidence-ref / promotion-gate
 slice.

It exists to answer three questions plainly:

1. what has actually been closed
2. what is still open
3. whether the next step should stay in `surface analyze` or move to the newer
   `View*` parsing line

This is not a new design spec. It is a repo-state closure note for handoff and
review.

## What Is Closed

The important `surface analyze v0` seam is now largely closed:

```text
surface candidate
  != direct executable candidate
```

More specifically, the following behaviors are now locked by code and tests.

### 1. Analyze output is honest about row and OCR candidate status

`AppSurfaceCandidate` now carries:

- `candidate_query`
- `evidence_refs`
- `promotion_gate`
- compatibility metadata

Current gate behavior is intentionally conservative:

- OCR visible text stays blocked as semantic result selection
- row/list grouping stays blocked as semantic result selection
- window region stays `distill_strategy_only`

This means `app analyze` can emit reviewable surfaces without pretending they
are already `contract::Candidate`.

### 2. Distillation shape does not smuggle row candidates into direct inputs

For row/context-only cases:

- `direct_candidate_ids` stays empty
- `context_candidate_ids` carries the row ids
- `provided_inputs` stays empty
- shape notes explicitly record that no direct candidate shape exists

This closes the easiest accidental bug: using "was suggested in distill" as if
it meant "is safe to execute".

### 3. Grounding does not treat row-only candidates as OCR anchors

The result-selection grounding path still requires a real anchor source.

If only row/context evidence exists:

- row-only candidates are not consumed as anchor grounding
- unresolved inputs remain unresolved
- validate fails instead of inventing a target

This is the core behavior that keeps structural evidence from being mistaken
for semantic identity.

### 4. Read-side reports preserve review-only semantics

All three report layers now keep the same distinction:

- `render_app_analysis_report`
- `render_app_distillation_report`
- `render_app_validation_report`

For row/context-only cases, the reports now show review-only suggestions,
blocked promotion, unresolved grounding, and failure context without inventing
used annotations or direct candidate claims.

### 5. JSON persistence preserves review-only semantics

The persisted JSON artifacts now have coverage for the same seam:

- `analysis.json`
- `distillation.json`
- `validation.json`

Round-trip and persistence assertions now cover:

- review-only `suggested_annotation_ids`
- empty `direct_candidate_ids`
- context-only candidate ids
- empty `provided_inputs`
- rejected validation output with unresolved grounding
- empty `used_annotation_ids` for row/context-only failures

This matters because the v0 contract promised machine-readable output, not only
Markdown prose.

## What Is Not Closed

The closure above is **boundary closure**, not product completion.

The following remain open.

### 1. `AppSurfaceCandidate -> contract::Candidate`

This is still the main unclosed seam.

The project now has a stronger definition of:

- what surface candidates are
- why they are blocked
- how review-only candidates survive distill/validate/report/json

But there is still no implemented promotion path from
`AppSurfaceCandidate` into an action-grade `contract::Candidate`.

That is the next real architecture slice if the team wants surface analyze to
feed runtime consumers directly.

### 2. Row action + semantic verification do not exist

Row/list grouping remains structural evidence because two concrete contracts are
still missing:

- row action contract
- semantic verification contract

Until those exist, rows should stay exactly where they are now: visible,
reviewable, machine-readable, but not executable.

### 3. OCR anchor is still evidence-first, not semantic success

`anchor-text` is closer to consumption than row grouping, but it is still not a
semantic result-selection guarantee.

Current result-selection candidate scaffolding still uses
`captureEvidence`, not a semantic verifier. That is an intentional limitation.

### 4. Surface type coverage is still narrow

Implemented v0 surface kinds are effectively:

- AX focus/query candidates
- OCR visible-text candidates
- grouped row candidates
- window region candidates

The v0 doc mentions icon/template matches, menu/shortcut affordances, future
DOM/CDP, and future CV/YOLO as reserved or future directions. Those are still
not closed here.

### 5. `surface analyze` is still a candidate pipeline, not a view model

The current app workflow is:

```text
probe -> analyze -> distill -> validate
```

That pipeline is now much more honest and much better defended, but it is still
about candidate surfacing and recipe scaffolding. It is not yet a generic
reconstruction/view substrate.

## Relationship To The New View Parser Document

Since this closure pass started, `origin/main` advanced with a new document
stack ending at:

- `5c67054 docs: clarify view reconstruction model`

Resulting file:

- `docs/ai/references/2026-05-28-view-parser-ir-netease-playlist-example-design.md`

That document is **not** a tail task for PR #8.

It opens a new direction:

- `ViewObservation`
- `ViewEvidenceNode`
- `ViewReconstruction`
- `ViewProjection`
- `ViewMemory`

In plain terms:

- `surface analyze v0` closes an evidence/promotion boundary
- `View*` parsing opens a new reconstruction substrate

They are related, but they are not the same slice.

So the right reading is:

- do not keep grinding row/context-only report details forever
- do not pretend `surface analyze` is now a complete view parser
- treat `View*` parsing as the next line, not as "one more small follow-up" to
  PR #8

This closure note assumes the PR branch stays focused on `surface analyze v0`
boundary work and does **not** absorb the newer `View*` planning documents.

## Recommended Next Step

The next step should **not** be more `surface analyze` read-side polish by
default.

The two serious next options are:

1. implement a narrow promotion seam for one candidate family that can honestly
   satisfy `contract::Candidate`
2. start the new `View*` parsing vertical slice described in the NetEase
   playlist example design

If the goal is to finish PR #8 cleanly, the first option should stay very
small.

If the goal is to move the product forward, the second option is likely more
valuable now that the `surface analyze v0` boundary is mostly defended.

## Bottom Line

`surface analyze v0` is **mostly closed as a boundary**.

It is **not closed as a full product capability**.

What is closed:

- evidence refs
- selector attachment
- promotion blockers
- row/context-only honesty across analyze, distill, validate, report, and JSON

What is not closed:

- action-grade promotion
- row action + semantic verification
- OCR anchor semantic success
- broader surface kinds
- generic view reconstruction

That is the real state of the repo after this closure pass.
