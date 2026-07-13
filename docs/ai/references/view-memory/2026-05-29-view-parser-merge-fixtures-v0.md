# AUV View Parser Cross-Viewport Merge Fixtures v0

Date: 2026-05-29

Status: v0 fixture spec. Pins the canonical inputs and expected
outputs for the cross-viewport merge algorithm defined in
`view-parser-ir-shapes-v0.md`.

Audience: owner, reviewers, and any agent (Codex, Claude, others)
implementing the merge step or writing regression tests for it.

## Purpose

The IR shapes spec defines the merge rule and lists five required test
cases by name (full match / partial overlap / conflicting sections /
repeat fingerprint / clipped label). It does not pin the inputs or
expected outputs. Without canonical fixtures:

- Different implementations of the merge step pass their own tests
  while diverging in behavior on cases the test author did not think
  of.
- Negative cases ("should not merge but the algorithm is tempted") are
  often missing because they require deliberate construction.
- Tuning the IoU threshold (see `REVIEW(merge-iou-threshold-v1)`)
  cannot be compared across runs without a stable fixture corpus.

This document pins **5 positive** and **4 negative** canonical
fixtures. They are the v0 acceptance contract for the merge step.

## Relationship to other specs

```text
view-parser-ir-netease-playlist-example-design.md   what & why
surface-analyze-v0.md                                surface candidates & gate
view-parser-contract-bridge-v0.md                    must-use existing contracts
view-parser-ir-shapes-v0.md                          IR types + merge rules
view-parser-diagnostic-policy-v0.md                  diagnostic firing rules
view-parser-merge-fixtures-v0.md     (this doc)      canonical merge test cases
```

## Merge rule recap

From `view-parser-ir-shapes-v0.md`, merge requires **all** of:

1. Same `kind_hint`.
2. Normalized label equality (lowercase, NFKC, collapsed whitespace,
   trimmed; empty never auto-merges).
3. IoU ≥ 0.5 along the merge axis after translation to `WindowLocal`.
4. Compatible section context (matching `section_hint` when both
   present; otherwise no contradictory evidence).
5. Neither candidate is `Confidence::Contradicted`.

Failures on (2)–(4) emit `ConflictingEvidence` per
`view-parser-diagnostic-policy-v0.md`.

## Fixture format

Each fixture is named `merge_<case>` and contains:

- **Inputs**: two or more `ViewCandidate`s. Fields not relevant to the
  case are elided (`..default()`-style).
- **Section context**: explicit `section_hint` per candidate.
- **Expected outcome**: one or more `ViewNode`s and a list of
  `ParserDiagnostic`s (empty if none).
- **Asserted invariants**: bullets the test must check beyond
  structural equality (e.g. "merged node's bounds are the union, not
  the intersection").

Common helpers used in fixtures below:

```rust
fn evi(obs: u32, source: EvidenceSource, x: i32, y: i32, w: u32, h: u32, label: &str) -> ViewEvidenceNode { /* ... */ }
fn cand(obs: u32, kind: ViewNodeKind, label: &str, bounds: ViewBounds, section: Option<&str>) -> ViewCandidate { /* ... */ }
fn window_bounds(x: i32, y: i32, w: u32, h: u32) -> ViewBounds {
  ViewBounds { origin_space: CoordinateSpace::WindowLocal, x, y, width: w, height: h }
}
```

Provider scores below use `{"ocr": 0.9}` shorthand for
`BTreeMap::from([("ocr".into(), 0.9)])`. Confidence levels default to
`Confirmed` unless stated.

## Positive cases (must merge)

### Case `merge_full_match`

The same item appears in two observations with near-identical bounds
and the same section. Standard merge.

```rust
// Inputs
let c0 = cand(0, Item, "Liked Songs",     window_bounds(12, 80, 200, 36), Some("netease.my_playlists"));
let c1 = cand(1, Item, "Liked Songs",     window_bounds(12, 78, 200, 36), Some("netease.my_playlists"));

// Expected: one ViewNode
ViewNode {
  kind: ViewNodeKind::Item,
  label: Some("Liked Songs".into()),
  bounds: Some(window_bounds(12, 78, 200, 38)),   // union (y from 78, height to 80+36-78=38)
  evidence_refs: [EvidenceRef(0, _), EvidenceRef(1, _)],
  confidence.provider_scores: {"ocr": max(0.95, 0.93) = 0.95},
  ..
}
// Expected diagnostics: []
```

Asserted invariants:

- Merged node has exactly two `evidence_refs`, one per observation.
- Bounds are the axis-aligned union, not the intersection.
- `confidence.provider_scores["ocr"]` is the per-provider max.
- `node_id` is content-derived; running the fixture twice produces the
  same id.

### Case `merge_partial_overlap`

The item appears slightly higher in observation 1 than observation 0
(the viewport scrolled up). Bounds overlap is around IoU ≈ 0.65 along
the vertical axis. Same label, same section.

```rust
let c0 = cand(0, Item, "Discover Weekly", window_bounds(12, 200, 200, 36), Some("netease.discover"));
let c1 = cand(1, Item, "Discover Weekly", window_bounds(12, 188, 200, 36), Some("netease.discover"));
// Vertical IoU: overlap = 24 / union = 48 → 0.5 (boundary case — accepts).
```

Expected: merge succeeds. One `ViewNode`, bounds = union
`(12, 188, 200, 48)`, two `evidence_refs`, no diagnostics.

Asserted invariants:

- IoU exactly at the 0.5 threshold accepts.
- A separate edge fixture (`merge_partial_overlap_below_threshold`,
  see negative cases) tests IoU just under 0.5 must reject.

### Case `merge_conflicting_sections_resolved_by_evidence`

Same label, IoU ≥ 0.5, but only one candidate has a `section_hint`.
The other has no evidence contradicting the hinted section. Merge
proceeds; the hinted section wins.

```rust
let c0 = cand(0, Item, "Daily Mix 1", window_bounds(12, 300, 200, 36), Some("netease.recommended"));
let c1 = cand(1, Item, "Daily Mix 1", window_bounds(12, 299, 200, 36), None);
// c1 has no contradicting section evidence.
```

Expected: one `ViewNode`, section inherits from c0
(`"netease.recommended"`), no diagnostics.

Asserted invariants:

- The merged node's parent points to the recommended section's node id.
- No `SectionAmbiguous` diagnostic (c1's missing hint is not a
  contradiction).

### Case `merge_repeated_viewport_fingerprint`

Three observations. Observations 0 and 2 share a `ViewportFingerprint`
(non-adjacent). One item is present in all three. Merge produces one
node with three evidence refs; the reconstruction's
`ScrollBoundary.repeated_viewport_fingerprints` lists the shared
fingerprint.

```rust
let fp = ViewportFingerprint("fp-abc123".into());
let viewport0 = viewport(0, fp.clone(), window_bounds(0, 0, 240, 480));
let viewport1 = viewport(1, ViewportFingerprint("fp-def456".into()), window_bounds(0, 36, 240, 480));
let viewport2 = viewport(2, fp.clone(), window_bounds(0, 0, 240, 480));

let c0 = cand(0, Item, "FIP",             window_bounds(12, 100, 200, 36), Some("netease.collected"));
let c1 = cand(1, Item, "FIP",             window_bounds(12, 64,  200, 36), Some("netease.collected"));
let c2 = cand(2, Item, "FIP",             window_bounds(12, 100, 200, 36), Some("netease.collected"));
```

Expected: one `ViewNode` with three `evidence_refs`. The
reconstruction's scrollable container records
`repeated_viewport_fingerprints: ["fp-abc123"]`.

Expected diagnostics:

- One `RepeatedViewport` diagnostic naming observations 0 and 2 with
  both capture artifact refs. (Per diagnostic policy: non-adjacent
  fingerprint repeats fire `RepeatedViewport`.)

Asserted invariants:

- The `RepeatedViewport` diagnostic does not duplicate per non-adjacent
  pair if more than two non-adjacent observations share a fingerprint;
  policy aggregation is per-occurrence at the pair level.

### Case `merge_clipped_label_completed_after_scroll`

Observation 0 sees the item with a clipped label (`"Liked So…"` due to
the item being at the bottom edge of the viewport) and emits
`ItemPartiallyVisible`. Observation 1 sees the full label
(`"Liked Songs"`). Bounds overlap meets IoU ≥ 0.5 after scroll
translation.

```rust
// Observation 0: clipped label, partial visibility
let c0 = cand(0, Item, "Liked So",  window_bounds(12, 444, 200, 12), Some("netease.my_playlists"));
//                                                  ^^^^^^^^^^^^^^^^^^^^^ height 12 of expected 36
// Observation 1: full label visible after scroll
let c1 = cand(1, Item, "Liked Songs", window_bounds(12, 80,  200, 36), Some("netease.my_playlists"));
```

Expected: one `ViewNode`, `label = "Liked Songs"` (longest non-empty),
bounds = union of c1's bounds with c0's translated bounds (or just c1
if c0's partial bounds do not contribute meaningfully), `evidence_refs`
from both observations.

Expected diagnostics:

- **No** `ItemPartiallyVisible` carried into the merged node. The
  merge resolved the partial visibility. Per policy: "if cross-viewport
  merge later combines partial views into a non-clipped node, the
  merged node carries no `ItemPartiallyVisible` diagnostic for the
  clipped observations."

Asserted invariants:

- `label` is `"Liked Songs"`, not `"Liked So"`. Length wins.
- The fixture also asserts that a SEPARATE reconstruction emitted just
  from observation 0 alone DOES carry `ItemPartiallyVisible` — the
  diagnostic only drops after merge resolution.

## Negative cases (must not merge)

### Case `merge_different_labels_high_iou`

Two items with high bounds overlap but distinct normalized labels.
This case catches algorithms that merge purely on geometry.

```rust
let c0 = cand(0, Item, "Liked Songs",  window_bounds(12, 80, 200, 36), Some("netease.my_playlists"));
let c1 = cand(1, Item, "Daily Mix 1",  window_bounds(12, 82, 200, 36), Some("netease.my_playlists"));
// IoU ≈ 0.94 — geometry would tempt merge.
```

Expected: two separate `ViewNode`s, no merge, no diagnostics (this is
just two different items happening to land at the same position
across viewports — a normal occurrence after scroll).

Asserted invariants:

- Two distinct `node_id`s.
- Each node has exactly one `evidence_ref`.
- No `ConflictingEvidence` diagnostic — the merge correctly rejected
  on rule (2) before flagging conflict.

### Case `merge_same_label_different_sections`

Identical label, IoU ≥ 0.5, but the candidates have different
`section_hint` values both with evidence.

```rust
let c0 = cand(0, Item, "Library", window_bounds(12, 80, 200, 36), Some("netease.my_playlists"));
let c1 = cand(1, Item, "Library", window_bounds(12, 80, 200, 36), Some("netease.collected"));
```

Expected: two separate `ViewNode`s.

Expected diagnostics:

- One `ConflictingEvidence` diagnostic naming both candidates, citing
  merge rule (4) (section conflict).

Asserted invariants:

- The two nodes have different parents (one per section).
- The diagnostic's `evidence_refs` lists both candidates' evidence,
  not just one side.

### Case `merge_partial_overlap_below_threshold`

Same label, same section, IoU < 0.5 (e.g. 0.4 along the merge axis).

```rust
let c0 = cand(0, Item, "Podcasts", window_bounds(12, 100, 200, 36), Some("netease.collected"));
let c1 = cand(1, Item, "Podcasts", window_bounds(12, 124, 200, 36), Some("netease.collected"));
// Vertical IoU = 12 / 60 = 0.2 — far below threshold.
```

Expected: two separate `ViewNode`s (the algorithm treated them as
independent occurrences, e.g. one above and one below in a list that
allows the same label twice — rare but possible).

Expected diagnostics: none. The bounds-overlap check failing is the
normal pattern for "this is the same label, different row"; only
section conflict gets a diagnostic.

Asserted invariants:

- The IoU threshold is precise: 0.5 accepts, 0.4 rejects, no diagnostic
  on rejection-by-IoU alone.

### Case `merge_contradicted_confidence_blocks_merge`

All other merge rules satisfied but one candidate has
`Confidence::Contradicted`.

```rust
let c0 = cand(0, Item, "Heart Beat", window_bounds(12, 200, 200, 36), Some("netease.my_playlists"))
  .with_confidence(ConfidenceLevel::Contradicted);
let c1 = cand(1, Item, "Heart Beat", window_bounds(12, 198, 200, 36), Some("netease.my_playlists"));
```

Expected: two separate `ViewNode`s. The Contradicted candidate remains
as its own node so reviewers can see what was contradicted; do not
silently drop it.

Expected diagnostics:

- One `ConflictingEvidence` diagnostic citing rule (5)
  (Contradicted candidate blocked merge).

Asserted invariants:

- The Contradicted node's `confidence.level` remains `Contradicted` in
  the reconstruction.
- The merged-path node carries its own evidence only — no Contradicted
  evidence leaks into the merge target.

## Fixture storage convention

For v0:

- Fixtures live next to the merge unit tests, suggested
  `crates/auv-view/tests/fixtures/merge/` (location pending owner
  approval).
- Each fixture is one Rust function returning
  `(Vec<ViewCandidate>, Vec<ViewNode>, Vec<ParserDiagnostic>)` —
  inputs, expected nodes, expected diagnostics.
- Helper functions for `cand`, `evi`, `window_bounds`, `viewport`
  live in a shared module; each fixture stays focused on what it
  varies.
- JSON-serialized variants of the fixtures (round-trip artifacts) are
  out of scope for v0. The inspect viewer integration spec
  (`2026-05-29-view-parser-inspect-viewer-v0.md`) exercises the JSON
  shape via `Runtime::list_view_*` round-trips; if those tests reveal
  a need for parallel JSON merge fixtures, revisit here.

## Test runner contract

Each fixture test must:

1. Call the merge entry point with the fixture's inputs.
2. Assert that the resulting `Vec<ViewNode>` matches the expected
   nodes (by content, not by `node_id` literal — derive expected ids
   from the same rules the implementation uses).
3. Assert that the resulting diagnostics match the expected kinds and
   their required field set per
   `view-parser-diagnostic-policy-v0.md`.
4. Assert any per-fixture "asserted invariants" bullets above.

The runner must distinguish:

- **Structural mismatch** (the merge produced wrong nodes) — flag as
  a real regression.
- **Diagnostic mismatch** (the merge produced different diagnostics) —
  flag as a real regression.
- **`node_id` mismatch when content is identical** — flag as a
  derivation bug, not a content bug.

## v0 done criteria

The fixture corpus is v0-complete when:

1. All 9 fixtures above are implemented as test functions that pass
   against the merge implementation.
2. Each fixture's "Asserted invariants" bullets are translated into
   explicit assertions.
3. The 5 positive and 4 negative cases together exercise every merge
   rule from `view-parser-ir-shapes-v0.md`: kind_hint mismatch
   indirectly via `merge_different_labels_high_iou`, label mismatch
   directly, IoU threshold both above and below, section conflict and
   resolution, Contradicted blocking.
4. The IoU threshold is parameterized in the merge code (per
   `REVIEW(merge-iou-threshold-v1)`) so the threshold-boundary
   fixtures can re-run as the threshold is tuned.
5. Adding a future merge case requires extending this document with a
   new fixture entry; ad-hoc test additions are not allowed in v0.
6. CI runs the full fixture corpus; failures block merge.

## Non-goals for this corpus

Intentionally deferred:

- JSON round-trip fixtures. v0 stays in Rust to avoid encoding /
  decoding noise during early algorithm work.
- Performance fixtures. v0 cares about correctness, not throughput.
- Fuzz / property-based generation. The corpus is a known-shape
  acceptance contract; fuzzing comes after the algorithm is stable.
- Cross-domain fixtures (non-NetEase examples). Add when a second
  view parser exists.
- Anchor / landmark merge fixtures. Anchors and landmarks attach to
  nodes; their merge semantics fall out of node merge. A separate
  fixture set may be added if anchor-specific edge cases surface.

## Forbidden in v0

- Modifying these fixtures to make a failing implementation pass.
  The fixtures define the contract; the implementation conforms.
- Adding a "skip" annotation on a fixture without an owner-approved
  revision of this document explaining why and tracking the unskip
  condition.
- Sharing mutable state between fixtures. Each fixture is independent.
- Asserting only structural equality without checking the listed
  per-fixture invariants. Structural equality alone misses important
  derivation bugs.

## How to use this corpus

When implementing or modifying the merge algorithm:

- Run the full corpus before and after the change. Any new failure is
  a regression, even if the change "seems unrelated".
- When tuning the IoU threshold, re-run
  `merge_partial_overlap` and `merge_partial_overlap_below_threshold`
  at the new threshold and confirm the boundary behavior matches the
  intended contract.
- When adding a new merge edge case found in the wild, add a fixture
  entry here before adding the test. Revising this document first
  keeps the corpus the authoritative contract.

This document is part of the convergence phase. Revisions are explicit,
dated, and owner-approved.
