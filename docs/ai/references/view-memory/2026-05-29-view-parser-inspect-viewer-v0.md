# AUV View Parser Inspect Viewer Integration v0

Date: 2026-05-29

Status: v0 integration spec. Pins how view parser artifacts and
spans surface through the existing inspect read-side
(`src/inspect.rs`, `src/inspect_server/`) and the viewer HTML so a
reviewer can open a run and see what the parser saw.

Audience: owner, reviewers, and any agent (Codex, Claude, others)
extending the inspect read-side or the viewer to render view parser
artifacts.

## Purpose

The trace layout spec pinned span names, artifact roles, and the four
root-span signals. The current `Runtime::list_*` / `inspect_server`
GET endpoints surface `verifications` and `observation_snapshots` but
do not know about view parser artifacts. Without an integration spec:

- The viewer renders view parser runs as a flat span list without
  surfacing reconstruction structure.
- HTTP consumers cannot query "show me the reconstruction for this
  run" without parsing artifacts client-side.
- Reacquisition runs (under `view.reacquire.*` spans) mix with parse
  runs (`view.parse.*`) because the readers do not branch on the
  namespace.

This spec pins v0. It is the **smallest viewer-side change** that
makes view parser data first-class without breaking existing
verification / observation viewer behavior.

## Relationship to other specs

```text
view-parser-trace-layout-v0.md          span tree + 4 root-span signals (parse)
view-parser-anchor-reacquisition-v0.md  view.reacquire.* trace namespace
view-parser-view-memory-v0.md           view.parse.memory_write span
view-parser-ir-shapes-v0.md             JSON shapes the viewer renders
view-parser-diagnostic-policy-v0.md     diagnostic severity table the viewer applies
```

## Read-side data model

`Runtime` gains four list methods, one per view parser artifact role:

```rust
impl Runtime {
    pub fn list_view_observations(&self, run_id: &RunId) -> AuvResult<Vec<ViewObservation>>;
    pub fn list_view_reconstructions(&self, run_id: &RunId) -> AuvResult<Vec<ViewReconstruction>>;
    pub fn list_view_projections(&self, run_id: &RunId) -> AuvResult<Vec<RawViewProjection>>;
    pub fn list_view_memory_writes(&self, run_id: &RunId) -> AuvResult<Vec<ViewMemory>>;
}

pub struct RawViewProjection {
    pub projection_id: String,
    pub reconstruction_ref: ArtifactRef,
    pub domain: String,
    pub records: serde_json::Value,        // domain-typed; viewer reads raw
    pub diagnostics: Vec<ParserDiagnostic>,
}
```

`RawViewProjection` keeps the records as `serde_json::Value` because
the viewer is platform- and domain-agnostic — it cannot statically
name `PlaylistSidebarProjection`. Per the bridge spec, this is **not**
a third candidate / projection schema; it is a viewer-only record
mirror.

Reading rules:

- Each method extracts artifacts by role (`view-observation` etc.)
  from the stored run.
- Results are ordered by `observation_index` (for observations) or
  by artifact creation time (others).
- Empty `Vec` is returned when the run produced no view parser
  artifacts; this is not an error.

> **REVIEW(raw-projection-as-value):** v0 uses `serde_json::Value` for
> `records` because the viewer crate cannot depend on every domain
> example crate. If real usage shows readers always pre-deserialize
> to a known projection type, consider exposing a typed reader as a
> generic. Until then, raw values keep the boundary clean.

## HTTP endpoints

The `inspect_server` GET `/runs/{run_id}` response gets an additional
field that includes view parser data alongside the existing
`verifications` and `observation_snapshots`:

```json
{
  "run": { ... },
  "verifications": [ ... ],
  "observation_snapshots": [ ... ],
  "view_parser": {
    "observations": [ ... ],
    "reconstructions": [ ... ],
    "projections": [ ... ],
    "memory_writes": [ ... ],
    "reacquisitions": [ ... ]
  }
}
```

When a run has no view parser artifacts, the `view_parser` field is
present but empty:

```json
"view_parser": {
  "observations": [],
  "reconstructions": [],
  "projections": [],
  "memory_writes": [],
  "reacquisitions": []
}
```

The viewer relies on the field always being present; consumers do not
need `if obj.has("view_parser")` branches.

The new sub-field `reacquisitions` is derived from the
`view.reacquire.*` span subtree (not artifacts). Per the
reacquisition spec, reacquisition does not produce a new artifact
role; it leaves a span subtree on the run. The viewer summarizes that
subtree into one `ReacquisitionRecord` per `view.reacquire.<scope_id>`
root:

```json
{
  "scope_id": "...",
  "target_kind": "node_id" | "anchor" | "label",
  "outcome": "reacquired" | "stale" | "not-found",
  "stage_used": "<strategy name or 'none'>",
  "observation_count": 3,
  "fatal_diagnostic_kind": null,
  "diagnostics": [ ... ]
}
```

These fields read directly from the root span's signals plus
diagnostics aggregated from child spans.

> **REVIEW(reacquisition-as-span-derived):** v0 derives reacquisition
> records from spans, not artifacts, because reacquisition does not
> produce a `view-reacquire` artifact role. If real usage pushes for
> a typed artifact (e.g. for replay / debugging), introduce one
> behind a revision of the reacquisition spec; the viewer would then
> read the artifact instead of the span tree.

## Viewer HTML rendering

The viewer (`src/inspect_server_viewer.html`) gains one new tab and
two new sub-views.

### New tab: "View Parser"

Visible only when the `view_parser` field has any non-empty array.

| Sub-view | Contents |
|---|---|
| Reconstructions | One card per `ViewReconstruction`. Header: `scope_id`, observation count, fatal diagnostic kind if any. Body: scrollable tree of nodes by `node_id`. Clicking a node highlights its evidence refs in the Observations sub-view. |
| Observations | One row per observation. Columns: index, viewport fingerprint (short hash), bounds, candidate count, evidence count. Clicking opens the underlying capture / OCR / AX artifacts in their existing panels. |
| Projections | One card per projection. Domain label + records summary (record count). Raw JSON expandable. |
| Memory writes | One row per `ViewMemory` write. Columns: memory_id (short), last_reconstructed_at, anchor count, landmark count, eviction count. |
| Reacquisitions | One row per reacquisition record. Columns: scope, target, outcome (colored badge), stage_used, observation_count. |

> **REVIEW(viewer-default-tab):** v0 places the View Parser tab after
> existing tabs. If view parser runs become the dominant usage, swap
> ordering or auto-select. v0 keeps the existing default tab so the
> change is additive.

### Color coding for outcomes

| Outcome | Color | Used in |
|---|---|---|
| `clean` / `reacquired` | green | reconstruction header, reacquisition badge |
| `observed-failure` / `stale` | amber | reconstruction header, reacquisition badge |
| `infra-failure` / `not-found` | red | reconstruction header, reacquisition badge |

Diagnostic severity badges use the kind-implied severity table from
`view-parser-diagnostic-policy-v0.md`:

| Severity | Color |
|---|---|
| Info | gray |
| Warn | amber |
| Error | orange |
| Fatal | red |

Color values follow the existing viewer palette; v0 does not
introduce new CSS variables.

## Filters and span attributes

The viewer's span list (existing) uses span attributes to filter. Per
trace layout, view parser spans carry typed attributes already
suitable as filter keys. The viewer adds three new filter
chip categories:

| Chip | Key |
|---|---|
| Scope | `view.scope_id` (on the root parse / reacquire span) |
| Outcome | `view.parse.outcome` and `view.reacquire.outcome` (unified UI) |
| Region detection | `view.region.outcome` (`normal` / `resized` / `collapsed` / `absent`) |

Chips are visible only when at least one span carries the attribute.
Empty chips are not rendered.

## Backward compatibility

Existing `verifications` and `observation_snapshots` fields in the
GET response are unchanged. Existing tabs in the viewer are
unchanged. v0 is purely additive on the response and viewer surface.

Schema version on the HTTP response: the existing endpoint does not
emit a version field; v0 does not introduce one. If a future change
breaks the additive guarantee, introduce versioning then.

> **REVIEW(http-schema-versioning):** v0 deliberately does not add a
> schema_version field to the GET response. The viewer is shipped
> with the server; they evolve together. If third-party HTTP
> consumers emerge, introduce versioning.

## v0 done criteria

The integration is v0-complete when:

1. `Runtime::list_view_*` methods exist and are exercised by tests
   that build a stored run, write each artifact role, and read it
   back.
2. `inspect_server` GET `/runs/{run_id}` returns the `view_parser`
   field on every response, empty when no artifacts exist.
3. The HTML viewer has a View Parser tab that renders only when the
   field is non-empty.
4. Diagnostic severity badges use the kind-implied table; severity
   is not stored on the wire.
5. Reacquisition records are derived from spans (per the v0 rule),
   not from a `view-reacquire` artifact role.
6. Existing `/runs/{run_id}` schema for `verifications` and
   `observation_snapshots` is byte-for-byte unchanged when the run
   has no view parser artifacts.
7. Filter chips render only when at least one matching span exists.

## Forbidden in v0

- Introducing a `view-reacquire` artifact role. Reacquisition is
  span-derived in v0.
- Adding a typed projection reader path in `Runtime` that depends
  on a specific domain crate. Records stay as `serde_json::Value`.
- Storing diagnostic severity in the HTTP response. The viewer
  derives it from kind.
- Versioning the GET response schema. Additive evolution only in v0.
- Replacing existing tabs or panels with view parser content. The
  tab is additive.
- Reading artifacts from outside the run being requested. v0
  endpoints are scoped to one run.
- Adding new CSS variables. Reuse the existing palette.

## Non-goals for this spec

Intentionally deferred:

- Cross-run aggregation (e.g. "show me all reacquisition failures
  this week"). v0 is per-run.
- Streaming / WebSocket updates of in-progress parses. v0 reads
  completed runs only.
- Search / full-text over view parser records.
- Export of view parser data to external formats.
- Inline editing of parser results in the viewer.
- A separate `/runs/{run_id}/view-parser` endpoint. v0 reuses the
  existing `/runs/{run_id}` envelope.
- Localized viewer chrome. Existing viewer locale rules apply.

## How to use this spec

When implementing or reviewing the integration:

- Start with `Runtime::list_view_*`; the HTTP layer is a thin
  wrapper. Tests at the runtime level catch the most regressions
  cheaply.
- The HTTP envelope addition is single-field-additive; reviewers
  should diff response schemas for existing-run-without-view-parser
  cases to confirm zero changes there.
- The viewer tab is opt-in by data presence, not by config. Empty
  arrays hide the tab.
- Color and severity mappings come from the diagnostic policy; do
  not duplicate the table in viewer code beyond a single shared
  constant module.

This document is part of the convergence phase. Revisions are explicit,
dated, and owner-approved.
