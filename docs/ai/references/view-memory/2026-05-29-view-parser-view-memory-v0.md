# AUV View Parser ViewMemory v0

Date: 2026-05-29

Status: v0 persistence spec. Pins what `ViewMemory` stores, how it
is keyed and evicted, and how it links to existing run storage.
**Per the design doc, memory scope "should be derived from the
matching and reacquisition algorithm once the first scan result
exists." v0 defines a starting shape; every threshold and policy
choice below carries a `REVIEW(...)` marker for tuning after real
runs land.**

Audience: owner, reviewers, and any agent (Codex, Claude, others)
implementing the `ViewMemory` type, the writer that persists it
after a parse run, or the reader that loads it before a follow-up
command.

## Purpose

The IR shapes spec reserved `ViewMemory` as a type slot and the
`view-memory` artifact role without defining either. The trace
layout left the owning span TBD. The design doc lists
`ViewMemory` as the persistence layer for anchors and landmarks but
explicitly defers the scope decision.

Without this spec, follow-up commands like `playlist get <anchor>`
cannot read prior parse results, and implementations of ViewMemory
will diverge on:

- What gets persisted from a `ViewReconstruction`.
- How `ViewMemory` is keyed across runs.
- When stored memory is considered stale.
- How it serializes vs the live IR types.

This spec pins v0. It deliberately stays a starting shape; the design
doc's warning about scope derivation is preserved as a top-level
`REVIEW(...)`.

> **REVIEW(view-memory-scope-derivation):** The design doc states
> ViewMemory scope should be derived from the matching/reacquisition
> algorithm once a first scan result exists. v0 picks a scope shape
> by hand. After the first NetEase parse + reacquisition runs land,
> measure which fields actually drive successful reacquisition and
> revise this spec to drop unused fields and add ones that were
> missing.

## Relationship to other specs

```text
view-parser-ir-shapes-v0.md             ViewMemory type slot, view-memory artifact role
view-parser-trace-layout-v0.md          view-memory owning span (was TBD; pinned here)
view-parser-contract-bridge-v0.md       ArtifactRef + reuse-existing-types discipline
view-parser-layer-contracts-v0.md       parsers consume / produce reconstruction; memory consumes reconstruction
view-parser-ir-netease-playlist-example-design.md   anchor / landmark concept origins
view-parser-anchor-reacquisition-v0.md  algorithm that consumes ViewMemory (sibling of this spec)
```

## Concrete shape

`ViewMemory` lives in `auv-view::memory`. Per the placement spec it
is platform-agnostic; only the NetEase example writes one out, but
the shape is generic.

```rust
pub struct ViewMemory {
    pub schema_version: String,                  // "view-memory-v0"
    pub memory_id: String,                       // content-hash of (app_bundle_id, scope_id)
    pub app_bundle_id: String,
    pub scope_id: String,
    pub last_reconstructed_at_millis: u64,
    pub source_run_id: RunId,                    // run that produced this memory
    pub source_reconstruction_ref: ArtifactRef,  // ref to the view-reconstruction artifact
    pub anchors: Vec<ViewAnchor>,                // reuses ViewAnchor from ir-shapes
    pub landmarks: Vec<ViewLandmark>,            // reuses ViewLandmark from ir-shapes
    pub node_snapshots: BTreeMap<ViewNodeId, ViewNodeSnapshot>,
    pub scope_snapshot: ViewMemoryScopeSnapshot,
    pub diagnostics: Vec<ParserDiagnostic>,      // forward those that affect reacquisition
}

pub struct ViewNodeSnapshot {
    pub node_id: ViewNodeId,
    pub kind: ViewNodeKind,
    pub domain_kind: Option<String>,
    pub label: Option<String>,
    pub parent: Option<ViewNodeId>,
    pub section_hint: Option<String>,            // resolved at parse time
    pub bounds_window_local: Option<ViewBounds>,
    pub viewport_fingerprint_hint: Option<ViewportFingerprint>,
    pub last_seen_observation_index: ObservationIndex,
    pub confidence: Confidence,
}

pub struct ViewMemoryScopeSnapshot {
    pub region_id: String,
    pub region_bounds_window_local: ViewBounds,
    pub baseline_width: u32,                     // from region detection config at write time
    pub schema_version_view_ir: String,          // copy of "view-ir-v0" for compatibility checking
}
```

Notes on the shape:

- `memory_id` is content-derived from `(app_bundle_id, scope_id)` so
  the same scope on the same app always maps to the same memory
  slot. Cross-run reuse keys on this.
- `node_snapshots` is a flat dictionary keyed by `ViewNodeId`. v0
  stores **every** non-`Unknown` node from the reconstruction; the
  reacquisition algorithm decides what to use. Unknowns are dropped
  because their IDs are not stable (per IR shapes
  `NOTICE(unknown-node-id)`).
- `bounds_window_local` is `Option<...>` because containers may have
  no stable bounds. The reacquisition algorithm tolerates `None`.
- `scope_snapshot.baseline_width` is a copy of the value used at
  write time. Reacquisition compares against the current run's
  baseline to detect regressions (e.g. user resized the sidebar
  between runs).

> **REVIEW(snapshot-every-non-unknown-node):** v0 stores every
> non-Unknown node. If reacquisition only ever uses anchors and
> landmarks, snapshotting all nodes is wasteful. After the first
> reacquisition runs land, measure which snapshots are read and
> drop the dead fields.

## Lifecycle

```text
parse run completes (ViewReconstruction emitted)
       │
       ▼
View parser writes ViewMemory (artifact role: view-memory)
       │
       ▼ (later, possibly in a different run)
follow-up command reads memory_id
       │
       ▼
loads the latest ViewMemory for memory_id (per eviction policy below)
       │
       ▼
anchor-reacquisition-v0 algorithm consumes it
```

### Write path

A view parser writes `ViewMemory` if **all** of:

1. The parse run's outcome is `clean` (per trace layout). Observed
   failures do not produce memory; they would memorize a failure
   state.
2. The reconstruction has at least one anchor or one item-kind node.
3. The implementation is configured to persist memory. v0 default:
   write on every clean run; reading is opt-in via the follow-up
   command.

> **REVIEW(write-on-observed-failure):** v0 skips writes on observed
> failure to avoid memorizing a degraded state. If a `RegionResized`
> Warn outcome still produces useful anchors, allow writing under
> that specific outcome.

### Read path

A follow-up command reads `ViewMemory` by:

1. Computing `memory_id` from `(app_bundle_id, scope_id)`.
2. Looking up the most recent `ViewMemory` artifact with role
   `view-memory` whose `memory_id` matches.
3. Checking freshness rules (below).
4. Returning the memory (or `None` if stale).

### Freshness / staleness

| Rule | v0 default | REVIEW key |
|---|---|---|
| Hard TTL | 24 hours from `last_reconstructed_at_millis` | `REVIEW(memory-hard-ttl)` |
| Schema version | reject if `schema_version != "view-memory-v0"` or `scope_snapshot.schema_version_view_ir != "view-ir-v0"` | (spec-bound, no review) |
| Baseline mismatch | warn (do not reject) if `scope_snapshot.baseline_width` differs from current config by > 25 % | `REVIEW(baseline-mismatch-tolerance)` |
| App version change | (not tracked in v0; see non-goals) | — |

> **REVIEW(memory-hard-ttl):** 24 hours is a starting guess. The
> real signal is "did the user open the app and reorganize their
> playlists". If reorganizations are rare, a longer TTL is fine
> (e.g. 7 days). If frequent, shorter (1 hour). Measure cache hit
> rate after a few weeks of usage.

A memory that fails any rejection rule returns `None` from the read
path; the caller falls back to a full parse run.

### Eviction policy

`ViewMemory` artifacts accumulate over runs. v0 keeps:

- The most recent `ViewMemory` per `memory_id`.
- The last `keep-history-count` non-most-recent memories per
  `memory_id` (for debugging / replay), oldest first to evict.

| Constant | v0 default | REVIEW key |
|---|---|---|
| `keep-history-count` | 3 | `REVIEW(memory-history-depth)` |

Eviction runs lazily — on write of a new `ViewMemory`, the writer
deletes older ones beyond the history depth. v0 does not provide a
background cleanup.

> **REVIEW(memory-history-depth):** 3 is enough to compare current
> vs previous vs older without exhausting disk. If memory artifacts
> are large in practice, drop to 1. If reviewers frequently compare
> across more history, raise.

## Span and artifact placement

The `view-memory` artifact role attaches to a new span:

| Span name | Owner | Cardinality |
|---|---|---|
| `view.parse.memory_write` | View parser, after `view.parse.project.<domain>` (or after `view.parse.reconstruct` for projection-less runs) | one per clean parse run |

The span name extends the canonical tree from
`view-parser-trace-layout-v0.md`. **This spec extends the trace
layout: implementations of the trace layout spec must add this span
when ViewMemory is enabled.**

Required attributes on `view.parse.memory_write`:

| Attribute | Value |
|---|---|
| `view.memory.memory_id` | the content-derived id |
| `view.memory.node_snapshot_count` | count as string |
| `view.memory.anchor_count` | count as string |
| `view.memory.landmark_count` | count as string |
| `view.memory.eviction_count` | number of older memories deleted |
| `view.memory.last_reconstructed_at_millis` | epoch ms |

## Serialization

`ViewMemory` serializes via `serde_json` with `serde(rename_all =
"snake_case")`, matching the IR shapes convention.

- `ArtifactRef` serializes as the canonical contract form (per
  bridge spec).
- Newtype IDs serialize as plain strings.
- `BTreeMap` serializes as JSON objects with sorted keys (matches
  IR shapes rule for `ViewReconstruction.nodes`).
- The on-disk file is one JSON document per `ViewMemory` instance;
  no NDJSON / streaming variant in v0.

## Storage location

`ViewMemory` artifacts live in run storage like any other artifact.
v0 does **not** introduce a separate `view-memory/` directory or
sidecar database. Reuse the existing run artifact directory layout.

The follow-up command reads memory by scanning recent runs for
artifacts with role `view-memory` whose `memory_id` matches the
target. v0 implementations may build an in-memory index over recent
runs on read; persistent indexing is non-goal.

> **REVIEW(memory-index-strategy):** v0 reads memory by scanning
> recent run artifacts at the time of the read. If the number of
> runs is small (< 1000), this is fast. If it grows, introduce a
> persistent index. Until then, no index.

## What ViewMemory does NOT carry

These are intentionally excluded:

- Raw observation artifacts (capture, OCR result, AX dump). The
  source run's artifacts remain authoritative; `ViewMemory` is a
  compact, derivable summary.
- Per-observation `ViewObservation` records. Those are reachable
  via `source_reconstruction_ref → ViewReconstruction.observations`.
- `ViewProjection<P>` records. Projections are domain-specific; the
  follow-up command projects from the reconstruction when needed.
- Cross-app correlation (e.g. "the same playlist in two apps").
- User-identifying data beyond the OS-provided bundle id.

The single source of truth for raw evidence remains the parse run's
artifacts. `ViewMemory` is a compact projection of those for
reacquisition.

## v0 done criteria

ViewMemory is v0-complete when:

1. The `ViewMemory` type is implemented in `auv-view::memory` with
   the exact shape above.
2. Writes happen exactly under the conditions in the Write path
   section.
3. Reads enforce the freshness rules and return `None` on any
   rejection.
4. Eviction keeps at most `keep-history-count + 1` memories per
   `memory_id`.
5. `view.parse.memory_write` span exists with all 6 required
   attributes when memory is written.
6. JSON round-trip works against the IR shapes' serde rules.
7. Reacquisition tests (per anchor-reacquisition-v0) confirm read
   returns the expected memory; explicit staleness tests cover TTL
   expiry, schema mismatch, and baseline mismatch.

## Forbidden in v0

- Storing raw evidence (capture, OCR result, AX dump) inside
  `ViewMemory`. References only, via `source_reconstruction_ref`.
- Writing memory on observed-failure outcomes.
- Adding fields beyond the shape above without a dated revision.
- Mutating an existing `ViewMemory` artifact after write. Writes
  are append-only; updates replace via eviction.
- Inventing a sidecar database or non-artifact persistence path.
  v0 reuses run storage exclusively.
- Reading memory across `app_bundle_id` boundaries.
- Promoting `Unknown` node snapshots into `node_snapshots`. Their
  IDs are not stable; including them would produce false matches
  on reacquisition.

## Non-goals for this spec

Intentionally deferred:

- App-version-aware staleness. v0 does not track NetEase version;
  staleness is TTL-based only.
- Cross-platform memory portability (e.g. moving from macOS to
  Linux). v0 keys on bundle id, which is platform-specific.
- Compressed / binary serialization. JSON only in v0.
- Multi-user memory partitioning beyond the run's existing user
  context.
- Background memory cleanup / GC. Eviction is on-write only.
- Persistent memory index. Reads scan recent runs.

## How to use this spec

When implementing or reviewing ViewMemory:

- All thresholds in a single `ViewMemoryConfig` struct.
- The Write path runs only after a clean parse outcome; observed
  failures do not call the writer.
- Freshness rules are AND-ed. Any single failure rejects.
- When extending the shape (a real need surfaces), bump
  `view-memory-v0` to `view-memory-v1` per the schema versioning
  rule in IR shapes. v0 readers reject v1; this is intentional.

This document is part of the convergence phase. Revisions are explicit,
dated, and owner-approved.
