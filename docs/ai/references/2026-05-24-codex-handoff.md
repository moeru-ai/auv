# Codex Handoff: Structured Observation Mainline

Date: 2026-05-24

Status: current handoff for the next coding agent

Current HEAD when written: `37ff6e2`

## Start Here

Read these files first, in this order:

1. `AGENTS.md`
2. `docs/TERMS_AND_CONCEPTS.md`
3. `docs/ai/references/2026-05-24-structured-observation-roadmap.md`
4. `docs/ai/references/2026-05-23-surface-selector-contract.md`
5. `docs/ai/references/2026-05-21-scroll-scan-design.md`
6. `src/contract.rs`
7. `src/scroll_scan.rs`

The current mainline is not "more OCR chasing". The work is now about making
observations structured enough that candidates, actions, and verification can
share one evidence chain.

## Current Repo State

`main` is expected to be synchronized with `origin/main` at:

```text
37ff6e2 docs: add structured observation roadmap
```

Recent commits that matter:

```text
37ff6e2 docs: add structured observation roadmap
fa8bdca feat(scroll-scan): add directional boundary candidates
b79fe78 fix(music): avoid activation before candidate row capture
6455ace Merge pull request #5 from moeru-ai/scroll-scan-2026-05-21
```

Before coding, verify the live state:

```bash
git status --short --branch
git log --oneline --decorate -5
```

If the branch is dirty, inspect the diff before editing. Do not assume the dirt
belongs to you.

## What Is Already Done

- `OperationResult`, `Candidate`, `CandidateRef`, `VerificationResult`,
  `FailureLayer`, and v0 `SurfaceSelector` exist in `src/contract.rs`.
- `music.search.results` and `music.result.play` form the first real
  getter/action candidate loop.
- Scroll scan exists and records page observations, hook decisions,
  conservative observation clusters, and heuristic directional
  `scroll_boundary_candidates`.
- PR #5 is merged. The scroll-scan branch is no longer the active work branch.
- `docs/TERMS_AND_CONCEPTS.md` now has a provisional `Recognition Result`
  term.

## Immediate Next Task

Implement only this first:

```text
feat(contract): add recognition result types
```

Target file:

```text
src/contract.rs
```

Add contract-only Rust types and serde round-trip tests. Do not wire live
drivers in this commit.

The intended shape is documented in:

```text
docs/ai/references/2026-05-24-structured-observation-roadmap.md
```

Important contract constraints:

- `RecognitionResult` must carry `best`, `filtered`, `all`, `detail`,
  `evidence`, and `known_limits`.
- `RecognizedItem` must carry a serialized `box` field, provider-native detail,
  optional text, optional provider score, and a stable item id.
- Keep provider scores inside recognition detail/items. Do not add top-level
  `Candidate.confidence`.
- Keep this independent from live OCR/row commands until the type round trips
  cleanly.

## After That

Recommended commit order:

1. `feat(contract): add recognition result types`
2. `feat(macos): emit recognition result artifacts for row detection`
3. `feat(music): link candidates to recognition evidence`
4. `feat(scroll-scan): corroborate boundary candidates with visual stability`
5. `feat(scan): add structured hook decision parsing`
6. `feat(skill): allow inline scan hook blocks`
7. `docs(research): evaluate Maa recognition and pipeline patterns`

Do not skip straight to region segmentation, icon matching, YOLO, or a new
orchestration language. That would be architecture cosplay before the evidence
contract is stable.

## Things Not To Do

- Do not replace the recipe system yet.
- Do not introduce JS orchestration yet.
- Do not add DOM/CDP selector support in this phase.
- Do not make scroll scan music-player-specific.
- Do not claim heuristic scroll boundaries are proven top/bottom detection.
- Do not remove existing text artifacts or CLI output when adding structured
  artifacts; add the structured artifact alongside them.
- Do not reopen the old QQ Music Chinese OCR-anchor fight unless a fresh task
  explicitly asks for it.

## Validation Commands

For Rust-touching changes, run:

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
cargo run --quiet -- list-commands
cargo run --quiet -- skill cases list
```

The former bundle-list validation command was retired on 2026-06-11.

For docs-only changes, at minimum run:

```bash
git diff --check
```

## Useful Mental Model

AUV should return structured state, candidates, and verification results, not
raw coordinates or pure CLI text.

The target architecture is:

```text
getter:
  search.results
  playlists.list
  resources.snapshot

action:
  candidate.select
  song.play
  panel.open

verification:
  stateChanged
  semanticMatched
  activationOnly
  failedWithEvidence
```

The next implementation work is the observation layer that makes this possible:

```text
raw provider output -> RecognitionResult -> Candidate -> CandidateRef
-> action -> VerificationResult
```

## Maa Context

MaaFramework links mentioned in chat are not verified project facts. Treat them
as research inputs only.

Useful ideas to verify later:

- recognizer output with box + detail
- runtime cache lookup by id
- task/pipeline override as scoped execution context
- wait/stability checks before/after actions
- debugger/runtime visualization

Do not copy Maa naming or protocol shapes before reading upstream source and
writing a short research note.
