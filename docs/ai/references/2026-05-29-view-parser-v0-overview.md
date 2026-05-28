# AUV View Parser v0 Overview

Date: 2026-05-29

Status: v0 corpus overview. Reading guide and cross-reference map for
the 11 documents that together define view parser v0.

Audience: anyone (owner, reviewer, Codex, Claude, future maintainer)
opening the view parser docs for the first time.

## What this is

The view parser v0 is defined by **11 documents** in `docs/ai/references/`,
written between 2026-05-28 and 2026-05-29. This overview is the entry
point. It does **not** restate the specs; it tells you which to read
in which order for the task at hand.

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
```

Doc 1 is the rationale; 2 is the consumed surface model. 3 sets the
no-parallel-schemas rule. 4 defines the IR types every later doc
references. 5, 6, 7 are siblings that pin behavior, tests, and trace
respectively. 8 → 9 → 10 are the implementation-near specs that
require everything above.

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
6. `view-parser-cli-rendering-v0.md` (10) — what the binary prints
7. `view-parser-ir-netease-playlist-example-design.md` (1) — NetEase-
   specific framing, sidebar / scroll requirements

You can skim 2, 3, 6 unless you change framework-side code.

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

- `ViewMemory` persistence (4, 7)
- Anchor reacquisition algorithm (4)
- DOM / CDP / CV / YOLO backends (2, 4)
- Inspect viewer panels (3, 7, 9)
- Cross-app reconstruction (3, 9)
- Promotion of `ViewNode` to `contract::Candidate` without
  `AppSurfaceCandidate` (3, 9)
- Async parser traits (9)
- A default `compose_layers` helper (9)
- A `ParserSeverity` enum on the wire (5)
- Schema migration policy (deferred; v0 is the first version)
- Mixed CLI modes / subcommands / progress on stdout (10)
- Cross-platform variants of the example (8)

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
