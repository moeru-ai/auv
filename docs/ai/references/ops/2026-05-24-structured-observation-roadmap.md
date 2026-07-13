# Structured Observation Roadmap

Date: 2026-05-24

Status: implementation handoff, future plan

Implementation baseline before this roadmap: `fa8bdca`

## Purpose

This document is the next-phase plan after the scroll-scan PR and directional
boundary candidate patch. It is written so another agent can continue without
reconstructing the design from chat logs.

The core direction is:

```text
observe surface -> structured recognition result -> candidate/query layer
-> action consuming CandidateRef -> verification result with failure layer
```

Do not widen this into a universal GUI framework in one step. The current
project already has enough moving parts. The next work should turn existing
OCR rows, visual rows, scroll scan pages, and candidate evidence into one
inspectable result chain.

## Current Facts

- `src/contract.rs` already contains `OperationResult`, `Candidate`,
  `CandidateRef`, `VerificationResult`, `FailureLayer`, and the v0
  `SurfaceSelector` contract.
- `docs/ai/references/ops/2026-05-23-surface-selector-contract.md` defines the
  cross-surface selector boundary: v0 supports AX, OCR, and row selectors only.
- `src/scroll_scan.rs` already records page observations, row candidates,
  hook decisions, and heuristic `scroll_boundary_candidates`.
- `docs/ai/references/view-memory/2026-05-21-scroll-scan-design.md` is still the main
  scroll-scan design source.
- The first `music.search.results -> CandidateRef -> music.result.play`
  candidate/action loop exists. It should remain the first real consumer.
- MaaFramework references in old notes and comments are useful research leads,
  but they are not verified project facts. Verify before copying names,
  schema, or semantics.

## Non-Goals For The Next Phase

- Do not replace the recipe system with a new JS/Rust orchestration layer yet.
- Do not add YOLO, icon matching, or visual segmentation as runtime features
  before the recognition result contract exists.
- Do not make scroll scan domain-specific for music, playlists, games, or
  chats.
- Do not add top-level `Candidate.confidence`; keep provider scores inside
  recognition/provider detail.
- Do not treat heuristic scroll boundaries as proven top/bottom evidence.
- Do not introduce a DOM/CDP selector backend in this phase.

## Phase 1: Recognition Result Contract

Goal: create the common box + detail contract that observation commands can
return and inspect.

### Contract Shape

Add Rust types in `src/contract.rs`:

```rust
pub struct RecognitionResult {
  pub recognition_id: String,
  pub source: RecognitionSource,
  pub scope: RecognitionScope,
  pub best: Option<RecognizedItem>,
  pub filtered: Vec<RecognizedItem>,
  pub all: Vec<RecognizedItem>,
  pub detail: serde_json::Value,
  pub evidence: Vec<ArtifactRef>,
  pub known_limits: Vec<String>,
}

pub struct RecognizedItem {
  pub item_id: String,
  pub kind: String,
  pub box_: RecognitionBox,
  pub text: Option<String>,
  pub provider_score: Option<f64>,
  pub detail: serde_json::Value,
}
```

Use `#[serde(rename = "box")]` for the Rust field that serializes as `box`.

Minimum source enum:

- `ocr_text`
- `ocr_row`
- `visual_row`
- `segmented_region`
- `icon_match`
- `custom`

Minimum scope enum or struct:

- `screen`
- `display`
- `window`
- `region`
- include enough reference data to project coordinates back to the capture
  contract when possible.

### Acceptance

- Add serde round-trip tests in `src/contract.rs`.
- Prove that `best`, `filtered`, and `all` can all serialize empty or populated.
- Do not wire live drivers in this commit.
- Update `docs/TERMS_AND_CONCEPTS.md` only if the term changes from the
  provisional definition added with this roadmap.

Suggested commit:

```text
feat(contract): add recognition result types
```

## Phase 2: Project Existing OCR Row Outputs Into RecognitionResult

Goal: stop losing bbox/confidence/detail by hiding row and OCR details inside
loosely named attributes.

### Implementation Targets

Start with the lowest-risk producers:

- `debug.findWindowRows`
- `debug.waitForWindowRows`
- `debug.findScreenRows`
- `debug.waitForScreenRows`
- `debug.observeWindowRegion`

These commands already produce row-like observations. Add a structured JSON
artifact that contains a `RecognitionResult` for the row detection pass.

Do not remove existing text reports or signals. Add the new structured artifact
beside them so old recipes and tests keep working.

### Acceptance

- The artifact contains row `box`, row text, provider score when available,
  raw OCR fragments in `detail`, and source screenshot/capture artifact refs.
- Tests assert JSON markers for `best`, `filtered`, `all`, and row bounds.
- Existing list and recipe commands still pass. Bundle commands were retired on
  2026-06-11.
- Viewer should not be changed unless needed for a basic artifact preview.

Suggested commit:

```text
feat(macos): emit recognition result artifacts for row detection
```

## Phase 3: Use RecognitionResult In Candidate Evidence

Goal: make `music.search.results` and future selector producers point at
structured observations instead of one-off provider blobs.

### Implementation Targets

- Update `music.search.results` candidate evidence to include a
  `recognition_result_ref` or equivalent detail path.
- Keep the authoritative handle as `CandidateRef`.
- Candidate evidence should still include enough inline summary for CLI
  readability, but detailed row/OCR information should live in the recognition
  artifact.

### Acceptance

- `music.search.results` still emits an `OperationResult` with candidates.
- Candidate evidence links back to the recognition artifact.
- `music.result.play` can still consume the candidate without needing in-memory
  lookup.
- Existing QQ Music case manifests validate.

Suggested commit:

```text
feat(music): link candidates to recognition evidence
```

## Phase 4: Stronger Scroll Boundary Evidence

Goal: improve top/bottom detection without pretending heuristics are proof.

The current boundary candidate is:

```text
scroll happened + no new observation signatures -> heuristic boundary candidate
```

Add stronger optional evidence in this order:

1. Repeated row-band overlap across pages.
2. Screenshot-region diff stability after scroll.
3. Scrollbar/thumb geometry if detectable.
4. AX scroll value if exposed by the target app.

### Acceptance

- `scroll_boundary_candidates` gain a `basis` that distinguishes each evidence
  source.
- `confidence` remains a label such as `heuristic`, `corroborated`, or
  `provider_reported`; do not fake a numeric probability.
- Stop messages explain which basis fired.
- Tests cover up and down directions separately.

Suggested commit:

```text
feat(scroll-scan): corroborate boundary candidates with visual stability
```

## Phase 5: Structured Hook Return Contract

Goal: replace `last.scan.hook.action` string plumbing with a typed hook
decision object.

### Contract Shape

Add a typed return payload:

```text
ScanHookDecision {
  hook_name
  stage
  page_index
  action
  reason
  annotations
  adjusted_region
  adjusted_scroll
  retry_policy
  evidence
}
```

Supported actions remain:

- `continue`
- `stop`
- `retry_observe`
- `adjust_region`
- `adjust_scroll`
- `annotate`

Only `continue` and `stop` need to execute first. The other actions may parse
and fail with explicit "not implemented" errors until supported.

### Acceptance

- Existing scalar hook variables keep working as a compatibility path.
- New structured hook result artifact or signal is preferred when present.
- Tests cover scalar fallback and structured result parsing.
- Invalid hook actions fail with a clear error and preserve partial scan
  artifacts.

Suggested commit:

```text
feat(scan): add structured hook decision parsing
```

## Phase 6: Inline Hook / Sub Recipe Declaration

Goal: reduce the awkwardness where a per-item hook must be a separate recipe
manifest.

Do not design a full orchestration language. Add only parent-local hook blocks
for scan workflows.

### Candidate Manifest Shape

```json
{
  "hooks": {
    "per_list_item_candidate": {
      "input_schema": "auv.scan.list_item_candidate_context.v0",
      "return_schema": "auv.scan.hook_decision.v0",
      "steps": []
    }
  }
}
```

### Acceptance

- Parent recipe can declare an inline hook block.
- Hook input and return schema are explicit.
- Existing standalone hook recipe still works.
- Runtime records hook execution as child spans under the scan run.
- No JS orchestration layer yet.

Suggested commit:

```text
feat(skill): allow inline scan hook blocks
```

## Phase 7: Region Segmentation And Icon Matching

Goal: start visual partitioning only after RecognitionResult exists.

This is not the immediate next task.

Initial segmentation should be rule-based:

- repeated row bands
- strong horizontal/vertical separators
- stable sidebars vs content region
- list/table/card containers

Initial icon matching should be optional and evidence-only:

- template or simple image match
- emits `RecognitionResult(source=icon_match)`
- does not directly click

Suggested later commits:

```text
feat(macos): emit rule-based segmented region recognition
feat(macos): add icon match recognition artifacts
```

## Maa Research Task

Before importing any Maa-inspired naming or protocol shape, do one explicit
research commit or doc update:

- verify MaaFramework pipeline protocol terms against the current upstream docs
- verify recognizer result shape and runtime cache behavior from source
- record what AUV will reuse conceptually and what it will not copy

The likely useful ideas are:

- task/pipeline override as scoped execution context
- recognizer output with box + detail
- runtime cache lookup by id
- action wait/stability checks
- debugger launch/runtime visualization

But do not let Maa turn AUV into a static game automation pipeline. AUV still
needs agent-facing structured state, candidates, and verification results.

Suggested commit:

```text
docs(research): evaluate Maa recognition and pipeline patterns
```

## Recommended Claude Execution Order

Use this order unless live validation proves a blocker:

1. `feat(contract): add recognition result types`
2. `feat(macos): emit recognition result artifacts for row detection`
3. `feat(music): link candidates to recognition evidence`
4. `feat(scroll-scan): corroborate boundary candidates with visual stability`
5. `feat(scan): add structured hook decision parsing`
6. `feat(skill): allow inline scan hook blocks`
7. `docs(research): evaluate Maa recognition and pipeline patterns`
8. Rule-based segmentation and icon matching only after the above stabilizes.

Run after every Rust-touching commit:

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
cargo run --quiet -- list-commands
cargo run --quiet -- skill cases list
```

## Review Checklist

- Does this preserve old recipe behavior?
- Does this write evidence artifacts rather than only CLI text?
- Can a later action refer to the observation without guessing coordinates?
- Is the failure layer explicit when grounding/control/verification fails?
- Are heuristic claims labeled as heuristic?
- Did the change avoid adding domain-specific music/game assumptions to core
  scan logic?
- Did docs update `docs/TERMS_AND_CONCEPTS.md` if a core term changed?
