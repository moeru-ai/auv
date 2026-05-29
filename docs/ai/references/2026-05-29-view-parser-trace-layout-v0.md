# AUV View Parser Trace Layout v0

Date: 2026-05-29

Status: v0 trace layout spec. Pins the span hierarchy, artifact role
placement, and `ArtifactRef` chain conventions for a view parser run.

Audience: owner, reviewers, and any agent (Codex, Claude, others)
implementing view parser layers, the NetEase example, or the inspect
viewer side of view parser artifacts.

## Purpose

The IR shapes spec defines artifact types and uses `ArtifactRef` for
evidence linkage. The bridge spec defines artifact roles. Neither
specifies **where** those artifacts live in the trace tree:

- Does each observation get its own child span, or do they share one?
- Does the scroll loop have its own span, or are scroll actions
  attached to observation spans?
- Does the reconstruction live under a sibling span to the observation
  loop, or as a child?
- Does a view parser run reuse `RunType::Command`, or introduce a new
  type?
- How does `ViewReconstruction.source_artifacts` chain through the
  observation artifacts?

Without those answers, two parser implementations land at different
span shapes, the inspect viewer cannot render them uniformly, and
`list_*` read-side APIs return inconsistent slices.

This document closes those questions for v0.

## Relationship to other specs

```text
view-parser-ir-netease-playlist-example-design.md   what & why
surface-analyze-v0.md                                surface candidates & gate
view-parser-contract-bridge-v0.md                    must-use existing contracts
view-parser-ir-shapes-v0.md                          IR types + merge rules
view-parser-diagnostic-policy-v0.md                  diagnostic firing rules
view-parser-merge-fixtures-v0.md                     canonical merge test cases
view-parser-trace-layout-v0.md     (this doc)        span tree + artifact placement
```

## RunType for a view parse

A view parser run reuses the existing `RunType::Command`. v0 does
**not** introduce a new run type. Reasons:

- View parses are user-triggered (CLI / agent calls a command); they
  share the lifecycle of any other command.
- Read-side APIs (`list_*`, inspect server endpoints) iterate by
  `RunType`; introducing a new type forks every reader.
- A new type would imply a separate `Runtime` entry point.

If a future host (REPL, persistent agent loop) needs a different run
type, that revision is owner-approved and lives in a future spec.

## Span hierarchy

A view parser run's span tree has one canonical shape:

```text
root span: view.parse.<scope_id>
├── span: view.parse.region_detect
│   ├── child capture / OCR / AX spans (per existing driver crate)
│   └── artifacts: existing driver evidence artifacts
├── span: view.parse.observe.loop
│   ├── span: view.parse.observe[0]
│   │   ├── child capture / OCR / AX spans (existing driver)
│   │   └── artifact: view-observation
│   ├── span: view.parse.scroll[0]   // optional, only if scroll happened
│   ├── span: view.parse.observe[1]
│   │   ├── child capture / OCR / AX spans
│   │   └── artifact: view-observation
│   ├── span: view.parse.scroll[1]
│   └── ... (continues until boundary)
├── span: view.parse.reconstruct
│   └── artifact: view-reconstruction
└── span: view.parse.project.<domain>
    └── artifact: view-projection-<domain>
```

The shape is fixed for v0. Implementations must not reorder, collapse,
or invent additional top-level child spans without a revision of this
document.

### Span naming convention

Span name strings use dot-separated segments. v0 spans are named
exactly as in the diagram above. Indexed spans (`observe[N]`,
`scroll[N]`) use `view.parse.observe.<index>` and
`view.parse.scroll.<index>` in the actual `span.name` field (bracket
notation is documentation-only).

Reserved span name prefix: `view.parse.*`. Implementations must not
emit any other span under the root for a parse run.

### When to omit a span

Some spans are conditional:

- `view.parse.region_detect` — required if the parser performs region
  detection (NetEase example does). May be omitted only if the scope
  itself already names the region with no detection step.
- `view.parse.scroll[N]` — emitted only if a scroll action was taken
  between observation N and observation N+1. If no scroll happened
  (e.g. single-viewport content), omit the scroll spans.
- `view.parse.project.<domain>` — emitted only if a domain projection
  was produced. Pure-reconstruction runs may stop after
  `view.parse.reconstruct`.

## Artifact role placement

Each view parser artifact role attaches to exactly one span kind:

| Role | Owning span | Cardinality per run |
|---|---|---|
| `view-observation` | `view.parse.observe.<index>` | one per observation span |
| `view-reconstruction` | `view.parse.reconstruct` | exactly one per parse run |
| `view-projection-<domain>` | `view.parse.project.<domain>` | one per projection span; multiple projections = multiple spans |
| `view-memory` | `view.parse.memory_write` (see view-memory-v0) | one per clean parse run when memory is enabled |

Existing driver-produced artifacts (capture, OCR result, AX dump) stay
under whichever span produced them, per the driver's existing
conventions. The view parser does not relocate or wrap them.

## ArtifactRef chain conventions

Refs propagate up the layer hierarchy without modification:

```text
driver capture / OCR / AX  →  ArtifactRef A
  └─ embedded in  ViewEvidenceNode.source_ref               (one ref per evidence)
       └─ collected into  ViewObservation.source_artifacts  (deduped union of A across the observation)
            └─ collected into  ViewReconstruction.observations as ObservationIndex
                 (the observation's own artifact ref is reachable via
                  the observation span's artifact entry, not duplicated
                  inside the reconstruction)
                 └─ ViewProjection.reconstruction_ref points to the
                    ViewReconstruction's ArtifactRef
```

Two non-obvious rules:

1. `ViewReconstruction.source_artifacts` (when emitted) lists the
   `ArtifactRef` of each contributing `ViewObservation` artifact, not
   the underlying capture/OCR/AX artifacts. Underlying refs are
   reachable transitively through the observation's own
   `source_artifacts`. This keeps the reconstruction's ref list to a
   bounded count (`O(observations)`) instead of unbounded
   (`O(evidence per observation × observations)`).
2. `ViewProjection.reconstruction_ref` points to the reconstruction
   artifact, never directly to an observation. Projections never
   bypass the reconstruction layer in their ref chain, even when the
   projection conceptually references a single observation.

## Span attributes

v0 standardizes a small set of attributes per span. Implementations may
add more; the listed attributes are the contract that readers can
depend on.

### `view.parse.<scope_id>` (root)

| Attribute | Value | Required |
|---|---|---|
| `view.scope_id` | the `ViewScope.scope_id` | yes |
| `view.app_bundle_id` | the bundle id | yes |
| `view.schema_version` | `"view-ir-v0"` | yes |

### `view.parse.region_detect`

| Attribute | Value | Required |
|---|---|---|
| `view.region_id` | the target region id | yes |
| `view.region.detected` | `"true"` / `"false"` | yes |

### `view.parse.observe.<index>`

| Attribute | Value | Required |
|---|---|---|
| `view.observation_index` | the `ObservationIndex` as a string | yes |
| `view.viewport_fingerprint` | the `ViewportFingerprint` | yes |
| `view.observation.candidate_count` | candidate count as string | yes |

### `view.parse.scroll.<index>`

| Attribute | Value | Required |
|---|---|---|
| `view.scroll.axis` | `"vertical"` / `"horizontal"` / `"both"` | yes |
| `view.scroll.from_observation` | the `ObservationIndex` before scroll | yes |
| `view.scroll.to_observation` | the `ObservationIndex` after scroll | yes |

### `view.parse.reconstruct`

| Attribute | Value | Required |
|---|---|---|
| `view.reconstruction.node_count` | node count as string | yes |
| `view.reconstruction.observation_count` | contributing observation count | yes |
| `view.reconstruction.diagnostic_count` | total diagnostics as string | yes |

### `view.parse.project.<domain>`

| Attribute | Value | Required |
|---|---|---|
| `view.projection.domain` | the domain name | yes |
| `view.projection.record_count` | record count as string | yes |

Reader-side APIs (`list_*`, inspect server endpoints) may use these
attributes as filter keys without parsing the artifact bodies.

## Signals on the root span

A view parser run emits a small fixed set of signals on the **root**
span's enclosing `DriverResponse` (or the equivalent runtime envelope),
not on individual child spans:

| Signal | Value | When |
|---|---|---|
| `view.parse.scope_id` | the scope id | always |
| `view.parse.outcome` | `"clean"` / `"observed-failure"` / `"infra-failure"` | always; mirrors the diagnostic policy outcome |
| `view.parse.observation_count` | count as string | always |
| `view.parse.fatal_diagnostic_kind` | the `ParserDiagnosticKind` name | only on `observed-failure` |

`view.parse.outcome` is the signal a caller checks first to decide
"did this run produce a usable reconstruction". Callers must not need
to parse the reconstruction artifact to make that decision.

## Failure propagation

Per the diagnostic policy, view parser runs distinguish three outcomes.
Their trace shape:

- **Clean success**: every required span emitted, every required
  artifact present, `view.parse.outcome = "clean"`.
- **Observed failure**: every required span emitted up to the failure
  point. The failure span (e.g. `view.parse.region_detect` for a
  `RegionNotFound`) carries `status_code = error` (or platform
  equivalent), the parent root span carries `status_code = ok`
  because the run completed observation, and
  `view.parse.outcome = "observed-failure"`.
- **Infra failure**: the run bubbles `Err(...)` from the runtime
  layer. The trace shape is whatever the runtime layer emits for any
  command-level error; no view-parser-specific span guarantees.

Implementations must not mark the root span as `error` when the
outcome is `observed-failure`. The run completed; only the
observation reported failure. `list_*` callers depend on this
distinction.

## v0 done criteria

The trace layout is v0-complete when:

1. Every view parser run emits exactly the span hierarchy in the
   diagram above; child spans of `view.parse.*` are not emitted
   outside the conditional rules.
2. Every span carries the required attributes from the attribute
   tables.
3. Every artifact role attaches to its specified owning span; no role
   is emitted under an alien parent.
4. `ViewReconstruction.source_artifacts` lists observation artifacts
   only, not transitive driver evidence.
5. The root span's enclosing response carries the four
   `view.parse.*` signals on every parse run.
6. `RunType::Command` is reused; no new `RunType` is introduced.
7. `view.parse.outcome` distinguishes the three outcomes per the
   failure-propagation rules.

## Forbidden in v0

- Adding a span under the root that is not in the canonical hierarchy.
- Marking the root span as `error` for an observed failure. Use
  `view.parse.outcome = "observed-failure"` instead.
- Embedding view parser-specific data inside an existing driver
  artifact (e.g. annotating an OCR result with view candidate IDs).
  View parser data lives in view parser artifacts.
- Skipping required attributes "because the artifact carries them".
  Attributes are the reader-side filter contract; readers must not
  parse artifact bodies to filter spans.
- Emitting `view-reconstruction` without `view-observation` artifacts
  to back its observation index list. The chain must be
  reconstructible without external state.

## Non-goals for this spec

Intentionally deferred:

- (Inspect viewer panel layout for view parser runs — covered by
  `2026-05-29-view-parser-inspect-viewer-v0.md`. The viewer reads
  span attributes and artifacts per this spec; the panel UI lives
  there.)
- Cross-run trace aggregation (e.g. "what NetEase parses ran today").
- Performance budgets per span.
- Span sampling / truncation policy for long scroll loops. v0 emits
  every span; sampling is a future concern.
- (`view-memory` artifact owning span — pinned by view-memory-v0 as
  `view.parse.memory_write`; the table at the top of this spec was
  refreshed accordingly.)

## How to use this spec

When implementing or reviewing view parser runtime code:

- Set up the span tree skeleton first, then attach artifacts. Code
  that creates an artifact before its owning span is set up will
  produce a misplaced artifact silently.
- Use the attribute tables as field validation in tests — the test
  runner can assert every required attribute is present without
  parsing artifact bodies.
- When unsure where an artifact should live, default to the smallest
  enclosing span that owns its scope. If still unsure, file a gap in
  this document.
- The four root-span signals are the only stable surface for "did
  this run produce a usable result". Callers depend on them; do not
  add new signals at the root without a revision of this document.

This document is part of the convergence phase. Revisions are explicit,
dated, and owner-approved.
