# PR #8 Surface Analyze Handoff

Date: 2026-05-28

Branch: `codex/surface-analyze-evidence-refs`

PR: <https://github.com/moeru-ai/auv/pull/8>

Base at time of handoff: `origin/main` at `baa8997`

Current branch head at time of handoff: `899ac79`

Status: implementation mostly complete; one small documentation/test closure
slice remains if the next agent wants to make the PR review-ready.

## Purpose

This PR closes the `surface analyze v0` evidence/promotion boundary.

The important product decision is:

`app analyze` may emit reviewable surface candidates, selector queries, evidence
refs, and promotion blockers. It must not quietly promote OCR text, row bands,
or window regions into action-grade `contract::Candidate` values.

That separation is intentional. Promotion into runtime/action objects is a later
slice.

## Commits In This PR

### `6e0c7fd feat(app): attach artifact refs to surface candidates`

Implemented durable evidence references for analyze candidates.

Main changes:

- Added `evidence_refs: Vec<contract::ArtifactRef>` to `AppSurfaceCandidate`.
- Added probe-artifact lookup from `AppProbeStep.run_id` and
  `AppProbeStep.artifact_paths`.
- Mapped matching `ArtifactRecordV1Alpha1` entries from
  `.auv/runs/<run_id>/artifacts.jsonl` into `ArtifactRef`.
- Attached refs to candidates derived from:
  - `list-windows`
  - `capture-ax-tree`
  - `ocr-sample`
- Rendered `evidenceRefs` in the app analyze Markdown report.
- Added regression coverage for OCR/row candidates carrying evidence refs.

Design note:

The implementation does not synthesize fake `span_id`, `event_id`, or
`artifact_id` values. If artifact records are unavailable, refs remain empty.
That is deliberate; missing evidence should be visible, not fabricated.

### `899ac79 feat(app): report surface promotion gates`

Implemented machine-readable promotion blockers for analyze candidates.

Main changes:

- Added `AppCandidatePromotionGate`.
- Added `AppCandidatePromotionStatus`:
  - `blocked`
  - `distill_strategy_only`
  - `action_grade_candidate`
- Added `promotion_gate: Option<AppCandidatePromotionGate>` to
  `AppSurfaceCandidate`.
- Generated promotion gates for candidates built by `build_annotation_candidates`.
- Rendered `promotionGate` in the app analyze Markdown report.
- Updated `docs/ai/references/ops/2026-05-28-surface-analyze-v0.md` so missing
  gates are recorded in `AppSurfaceCandidate.promotion_gate.missing_gates`
  instead of Markdown-only prose.
- Added tests that:
  - OCR visible-text candidates are `blocked`.
  - OCR candidates require `action_contract` and
    `semantic_verification_contract`.
  - row candidates are `blocked`.
  - row candidates require `row_action_contract`.
  - window-region candidates are only `distill_strategy_only`, not action-grade.

Design note:

This commit intentionally does not convert `AppSurfaceCandidate` into
`contract::Candidate`. The current `contract::Candidate` shape needs stronger
action-time liveness/control fields than analyze can honestly produce for these
surfaces.

## Current Behavior

`app analyze` now emits candidates with these layers:

- observable surface fields: `area`, `kind`, `source`, text, bounds, click point
- optional `CandidateQuery` for re-location
- optional `ArtifactRef` chain for evidence
- `input_bindings` for distillation templates
- compatibility taxonomy IDs
- `promotion_gate` explaining whether the candidate is:
  - blocked
  - usable only as distillation strategy input
  - action-grade candidate

Current generated gates are conservative:

- OCR visible text: `blocked`
- row/list grouping: `blocked`
- known direct taxonomy candidates: `distill_strategy_only`
- candidates with no known action path: `blocked`

No current analyzer-produced candidate should be treated as a validated semantic
target.

## Validation Already Run

The following commands passed after `899ac79`:

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
cargo run --quiet -- list-commands
cargo run --quiet -- skill cases list
```

Observed test count:

```text
395 lib tests passed
25 main/CLI tests passed
```

The working tree was clean after pushing `899ac79`.

## Historical Remaining Work

These items were open when this handoff was written. They are now resolved, but
the section is retained so reviewers can see the closure trail.

### 1. Convert the spec tail from "next slice" to implementation status

File:

```text
docs/ai/references/ops/2026-05-28-surface-analyze-v0.md
```

Problem:

The bottom section still reads like future work:

- add evidence refs
- add promotion gate
- then later convert to `contract::Candidate`

But the first two are already done in PR #8.

Resolution:

- `docs/ai/references/ops/2026-05-28-surface-analyze-v0.md` now has an
  `Implementation Status` section.
- Evidence refs and promotion gates are marked implemented.
- `AppSurfaceCandidate -> contract::Candidate` promotion remains explicitly
  future work.

### 2. Add one JSON serialization regression for `promotion_gate`

Original request: add one narrow test that serializes an analysis/candidate and
asserts the JSON contains:

```json
"promotion_gate": {
  "status": "blocked",
  "missing_gates": [...]
}
```

Resolution:

- `src/app/mod.rs` includes JSON serialization coverage for `promotion_gate`.
- Additional round-trip tests preserve evidence refs, review-only row/context
  shape, unresolved grounding, and validation failure semantics without relying
  on Markdown scraping.

### 3. Re-run the same validation commands

Run:

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
cargo run --quiet -- list-commands
cargo run --quiet -- skill cases list
```

Then push to the same branch:

```bash
git push origin codex/surface-analyze-evidence-refs
```

Do not open a new PR. Push updates directly to PR #8.

## Do Not Expand This PR

Do not add these to PR #8:

- actual `AppSurfaceCandidate -> contract::Candidate` promotion
- ActionResolver consuming analyze candidates
- new surface kinds
- DOM/CDP selector backend
- icon/template matching changes
- YOLO/ONNX/CV backend
- scroll-scan parser or orchestration work
- recipe language redesign

Those are separate slices. This PR should stay focused on the analyze evidence
and promotion-boundary report contract.

## Current Repo State Notes

At the time this handoff was written:

- branch was `codex/surface-analyze-evidence-refs`
- remote branch was `origin/codex/surface-analyze-evidence-refs`
- branch was clean before this handoff document was added
- PR #8 already had the two implementation commits pushed

This document itself may be uncommitted depending on where the next agent starts.
Check `git status --short --branch` before continuing.
