# AUV View Parser v0 Overview

Date: 2026-05-29

Status: v0 corpus overview. Reading guide and cross-reference map for
the 11 documents that together define view parser v0.

Audience: anyone (owner, reviewer, Codex, Claude, future maintainer)
opening the view parser docs for the first time.

## What this is

The view parser v0 is defined by **17 documents** in `docs/ai/references/`,
written between 2026-05-28 and 2026-05-29. This overview is the entry
point. It does **not** restate the specs; it tells you which to read
in which order for the task at hand.

> This overview was revised on 2026-05-29 — first after the three
> algorithm specs (region detection, item parsing, scroll loop)
> landed, then again after ViewMemory persistence and anchor
> reacquisition landed, then again after inspect viewer integration
> landed.

## The corpus

| # | Doc | Owns |
|---|---|---|
| 1 | `2026-05-28-view-parser-ir-netease-playlist-example-design.md` | The original design rationale, principles, terminology, NetEase example framing, non-goals |
| 2 | `2026-05-28-surface-analyze-v0.md` | Surface candidate model, promotion gate, kind table (consumed by view parser) |
| 3 | `2026-05-29-view-parser-contract-bridge-v0.md` | Rule that view parser reuses `ArtifactRef`, `SurfaceNode`, `RecognitionResult`, and the surface-analyze promotion gate — no parallel schemas |
| 4 | `2026-05-29-view-parser-ir-shapes-v0.md` | Concrete IR Rust type shapes + ID derivation + cross-viewport merge rule. 8 inline `TODO` / `NOTICE` / `REVIEW` markers anchor intentional deferrals to type sites |
| 5 | `2026-05-29-view-parser-diagnostic-policy-v0.md` | The 10 `ParserDiagnosticKind` variants' firing matrix; severity is kind-implied; `Ok+Fatal` vs `Err` discipline |
| 6 | `2026-05-29-view-parser-merge-fixtures-v0.md` | 9 canonical merge test fixtures (5 positive + 4 negative) with per-fixture asserted invariants |
| 7 | `2026-05-29-view-parser-trace-layout-v0.md` | Span tree shape, artifact role placement, ArtifactRef chain rules, 4 root-span signals |
| 8 | `2026-05-29-view-parser-example-placement-v0.md` | The two workspace crates (`auv-view`, `auv-example-netease-playlist`) + import rules |
| 9 | `2026-05-29-view-parser-layer-contracts-v0.md` | Trait signatures for the four parser layers + adapter rules (driver outputs → `ViewEvidenceNode`) |
| 10 | `2026-05-29-view-parser-cli-rendering-v0.md` | `netease-playlist-ls` rendering modes, exit codes, stream discipline |
| 11 | `2026-05-29-view-parser-v0-overview.md` (this doc) | Reading order + cross-references |
| 12 | `2026-05-29-netease-sidebar-region-detection-v0.md` | NetEase sidebar detection cascade (AX → OCR anchor → geometry) with `REVIEW(...)` markers on every threshold |
| 13 | `2026-05-29-netease-playlist-item-parsing-v0.md` | NetEase playlist row grouping, section taxonomy, section assignment, clip detection, confidence mapping |
| 14 | `2026-05-29-view-parser-scroll-loop-v0.md` | Observation loop control flow, scroll step policy, hard / soft / repeat boundary detection, 5 stop conditions |
| 15 | `2026-05-29-view-parser-view-memory-v0.md` | `ViewMemory` persistence shape (memory_id keying, freshness rules, eviction, owning span `view.parse.memory_write`) |
| 16 | `2026-05-29-view-parser-anchor-reacquisition-v0.md` | 6-stage cascade for re-finding a node from `ViewMemory` with bounded scroll / wall-clock budgets and `view.reacquire.*` trace namespace |
| 17 | `2026-05-29-view-parser-inspect-viewer-v0.md` | `Runtime::list_view_*` methods, HTTP envelope `view_parser` field, viewer HTML tab, color / severity mapping (no severity on wire) |

## Dependency map

The reading order is partial — some specs depend on earlier ones for
their definitions:

```text
                       1. design (rationale)
                              │
                              ▼
                       2. surface-analyze (consumed model)
                              │
                              ▼
                       3. bridge  ──────────┐
                              │             │
                              ▼             │
                       4. IR shapes  ───────┤
                              │             │
       ┌──────────────────────┼─────────────┤
       ▼                      ▼             ▼
  5. diagnostic         6. merge       7. trace layout
     policy                fixtures
                                            │
                                            ▼
                                    8. example placement
                                            │
                                            ▼
                                    9. layer contracts
                                            │
                                            ▼
                                    10. CLI rendering
                                            │
                              ┌─────────────┼─────────────┐
                              ▼             ▼             ▼
                       12. region    13. item    14. scroll
                        detection    parsing       loop
                       (NetEase)    (NetEase)    (algorithm)
                                            │
                                            ▼
                                    15. ViewMemory
                                        persistence
                                            │
                                            ▼
                                    16. anchor
                                       reacquisition
                                            │
                                            ▼
                                    17. inspect
                                        viewer
                                       integration
```

Doc 1 is the rationale; 2 is the consumed surface model. 3 sets the
no-parallel-schemas rule. 4 defines the IR types every later doc
references. 5, 6, 7 are siblings that pin behavior, tests, and trace
respectively. 8 → 9 → 10 are the implementation-near specs that
require everything above. 12, 13, 14 are the algorithm specs that
fill in `RegionParser`, `ItemParser`, and the `ViewParser` loop
with v0 defaults whose every threshold is marked `REVIEW(...)`.
15 → 16 close the persistence + read-side reacquisition loop that
turns single-parse `playlist ls` into follow-up-able `playlist get`.
17 surfaces all of the above through `Runtime::list_view_*`, the
HTTP envelope, and the viewer HTML so reviewers can open a run and
see what the parser saw.

## Reading order by task

### "I'm reviewing the architecture"

Read in this order: 1, 2, 3, 4. Stop. The other specs are
implementation-bounding; they do not change the architecture story.

### "I'm implementing the IR crate (`auv-view`)"

Read in this order:

1. `view-parser-ir-shapes-v0.md` (4) — type shapes you implement
2. `view-parser-contract-bridge-v0.md` (3) — what you must consume,
   not reinvent
3. `view-parser-diagnostic-policy-v0.md` (5) — firing-rule helpers
4. `view-parser-merge-fixtures-v0.md` (6) — your test corpus
5. `view-parser-trace-layout-v0.md` (7) — span / artifact helper
   builders
6. `view-parser-example-placement-v0.md` (8) — crate layout
7. `view-parser-layer-contracts-v0.md` (9) — trait signatures you
   own + adapter helpers

You do not need to read 1, 2, or 10 to implement `auv-view`.

### "I'm implementing the NetEase example crate"

Read in this order:

1. `view-parser-example-placement-v0.md` (8) — where files go +
   import rules
2. `view-parser-layer-contracts-v0.md` (9) — traits you implement +
   adapter pattern
3. `view-parser-ir-shapes-v0.md` (4) — types you produce
4. `view-parser-diagnostic-policy-v0.md` (5) — when to fire what
5. `view-parser-trace-layout-v0.md` (7) — spans you emit + signals
   you set
6. `netease-sidebar-region-detection-v0.md` (12) — region cascade
   you implement for `SidebarRegionParser`
7. `netease-playlist-item-parsing-v0.md` (13) — row grouping +
   section taxonomy + clip detection for `PlaylistSidebarItemParser`
8. `view-parser-scroll-loop-v0.md` (14) — loop control flow you
   implement inside `NeteaseSidebarViewParser::parse_view`
9. `view-parser-cli-rendering-v0.md` (10) — what the binary prints
10. `view-parser-ir-netease-playlist-example-design.md` (1) — NetEase-
    specific framing, sidebar / scroll requirements

You can skim 2, 3, 9 unless you change framework-side code. Docs 12,
13, 14 are the meat of the example's parser implementation work; their
`REVIEW(...)` markers are the v0 tuning surface.

### "I'm extending inspect read-side / viewer for view parser data"

Read in this order:

1. `view-parser-inspect-viewer-v0.md` (17) — the new
   `Runtime::list_view_*`, HTTP envelope additions, viewer tab and
   forbidden surface changes
2. `view-parser-trace-layout-v0.md` (7) — span attributes the
   viewer filters on
3. `view-parser-diagnostic-policy-v0.md` (5) — severity table the
   viewer reads (severity stays kind-implied, never on the wire)
4. `view-parser-anchor-reacquisition-v0.md` (16) — span tree for
   the span-derived reacquisitions sub-view

Skip 4, 6, 8, 12–14 unless you change the underlying data.

### "I'm implementing `playlist get` or any follow-up command"

Read in this order:

1. `view-parser-view-memory-v0.md` (15) — what the previous parse
   stored and how freshness is checked
2. `view-parser-anchor-reacquisition-v0.md` (16) — the 6-stage
   cascade you call, its bounded budget, and the
   `view.reacquire.*` trace namespace
3. `view-parser-layer-contracts-v0.md` (9) — region / item parser
   traits the cascade reuses
4. `view-parser-diagnostic-policy-v0.md` (5) — diagnostic kinds the
   cascade emits (no new kinds invented for reacquisition)
5. `view-parser-cli-rendering-v0.md` (10) — for the read-side
   command's own CLI surface

Skip 1, 4, 6–8 unless you are also changing parse-side behavior.

### "I'm writing tests"

Read in this order:

1. `view-parser-merge-fixtures-v0.md` (6) — the 9 fixtures you must
   pass
2. `view-parser-diagnostic-policy-v0.md` (5) — when each diagnostic
   fires (positive + negative cases)
3. `view-parser-ir-shapes-v0.md` (4) — JSON round-trip, ID
   derivation determinism, schema_version checks
4. `view-parser-trace-layout-v0.md` (7) — required attributes + the
   4 root-span signals
5. `view-parser-cli-rendering-v0.md` (10) — exit codes + stream
   discipline tests
6. `netease-sidebar-region-detection-v0.md` (12) — recorded-fixture
   coverage for AX / OCR-anchor / geometry / absent / collapsed /
   resized branches
7. `netease-playlist-item-parsing-v0.md` (13) — at least 9 distinct
   scenarios (full section / item before header / past carry-forward
   limit / clipped top / clipped bottom / unknown header / pseudo-
   section / OCR-only / OCR + icon)
8. `view-parser-scroll-loop-v0.md` (14) — boundary detection
   coverage (hard / likely with grace / repeat / stuck / modal /
   max_steps ceiling)

### "I'm reviewing a view parser PR"

Open the relevant spec(s) for the layer the PR touches. Use the
"Forbidden in v0" section of each as a quick lint. If the PR adds a
new variant, kind, or schema field, the matching v0 doc must be
revised in the same PR — silent additions fail review.

## What "v0 done" means across the corpus

Each spec has its own "v0 done criteria" section. The aggregate v0 is
done when **every** doc's criteria are satisfied, not when each is
individually green. Two near-failure modes:

- All 7 mine-only docs pass, but the NetEase example never runs.
  v0 is not done — the corpus expects the example to exercise the
  diagnostic policy paths.
- The example runs but the IR crate has not implemented JSON
  round-trip. v0 is not done — readers depend on the wire shape.

## Cross-references summary

The most-cited specs (specs that other specs reference repeatedly):

- `view-parser-ir-shapes-v0.md` — referenced by 5, 6, 7, 9 for type
  shapes
- `view-parser-contract-bridge-v0.md` — referenced by 4, 7, 9 for
  no-parallel-schemas
- `view-parser-diagnostic-policy-v0.md` — referenced by 6, 7, 9, 10
  for outcome / exit code mapping

The least-cited spec is 1 (the original design). It is the source for
non-goals and principles, but later specs lift those into their own
forbidden lists.

## What is NOT in v0 (consolidated)

The single source of truth for each deferral is the spec where it is
listed. Consolidated below for one-glance review:

- (`ViewMemory` persistence — moved into v0 via doc 15)
- (Anchor reacquisition algorithm — moved into v0 via doc 16)
- DOM / CDP / CV / YOLO backends (2, 4)
- (Inspect viewer integration — moved into v0 via doc 17)
- Cross-app reconstruction (3, 9)
- Promotion of `ViewNode` to `contract::Candidate` without
  `AppSurfaceCandidate` (3, 9)
- Async parser traits (9)
- A default `compose_layers` helper (9)
- A `ParserSeverity` enum on the wire (5)
- Schema migration policy (deferred; v0 is the first version)
- Mixed CLI modes / subcommands / progress on stdout (10)
- Cross-platform variants of the example (8)
- Cross-version baseline registry for region detection (12)
- Localized section header lists beyond Simplified Chinese (12, 13)
- AX-direct item parsing (13)
- Bidirectional / reverse / mid-loop direction changes in the scroll
  loop (14)
- Adaptive `should_stop` based on item count or section count (14)
- Interruptible / async parse runs (14)

When implementing, treat these as wall, not gap. The spec where each
is listed names the condition under which it may be lifted.

## Revising any spec in this corpus

Revisions are explicit, dated, and owner-approved. When revising:

1. Update the spec(s) touched.
2. Update this overview if a doc's purpose, dependency edge, or
   reading-order placement changes.
3. Bump `view-ir-v0` to a new version if a wire-shape field changes
   (per IR shapes spec rules).
4. Do not silently extend a `Vec<Variant>` listed as closed without
   matching the spec's "Forbidden in v0" guard rail.

The corpus is part of the convergence phase. Adding a twelfth doc is
allowed if it pins something new the existing 11 do not; do not add
one just to restate.
