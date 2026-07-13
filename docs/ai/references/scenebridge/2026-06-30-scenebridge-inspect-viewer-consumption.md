# SceneBridge B1: Inspect viewer view-parser consumption

Date: 2026-06-30
Status: **B1a + B1b + B1c landed** (run detail proof panel + drill-down + list badges)

## Summary

B1 consumes the existing `GET /runs/{id}.view_parser` field and the B1c
`view_parser_summary` list read-model in the inspect viewer HTML. Each
`ViewResolutionSummary` in `resolution_summaries` renders as a human-readable
proof card answering the six A8 owner inspect questions without client-side
recomputation. Run-list rows show lightweight proof badges without fetching full
`view_parser` per row in the browser.

## B1a scope (shipped)

| Area | Detail |
|------|--------|
| Panel | `#view-parser-proof` sibling before `#main-body` |
| Data | Read-only `run.view_parser.resolution_summaries` |
| Cards | Identity / Memory / Resolution / Replay / Verification / geometry note |
| Outcomes | Reuse existing `status-pill` tokens only (`s-validated`, `s-candidate`, `s-frozen`, `s-failed`) |

### Clear rules

| Event | Behavior |
|-------|----------|
| `selectRun` start | `clearViewParserProof()` — no stale proof from prior run |
| `loadRunDetail` success | `renderViewParserProof(state.activeRun)` |
| `loadRunDetail` failure | `clearViewParserProof()` |
| No summaries | Panel `hidden` + empty `innerHTML` |
| `mergeRunDetail` | Preserve `view_parser` when incoming payload omits it (same as `verifications`) |

### Narrow freshness on `run_finished`

`run_finished` WebSocket frames carry `frame.run` **without** `view_parser` or
`view_parser_summary`. Calling full `loadRunDetail()` on finish would overwrite
`state.activeRun`, reset `activeArtifactKey`, and re-render spans/events/artifacts
— destroying the user's current span / artifact / surface-node selection.

**B1a rule:** when `frame.run.run_id === state.activeRunId`, call
`refreshViewParserProofFromRunDetail(runId)` only:

1. `fetch("/runs/" + runId)` — run metadata + `view_parser` + `view_parser_summary`
2. `mergeRunDetail(state.activeRun, json)`
3. Sync sidebar row + `setMainHeader`
4. `renderViewParserProof(state.activeRun)` + `renderRunList()` (B1c)
5. **Do not touch** `state.spans`, `state.events`, `state.artifacts`,
   `activeSpanId`, `activeArtifactKey`, or `activeSurfaceNode*`

If proof is still empty after finish, the panel stays hidden.

## B1b scope (shipped)

| Area | Detail |
|------|--------|
| Lineage | Each card shows `memory_id · source_run_id · run_id` (current run) |
| Known limits | `pairViewParserProofCards` index-pairs `resolution_summaries[i]` with `select_results[i]` in one render pass — **never** join by `query` |
| Artifact drill-down | Chip shortcuts call `jumpToViewParserArtifactRole` for `view-memory` and `netease-playlist-select-result`; sets `activeArtifactRoleFilter` and selects matching artifact via existing `renderArtifactList` |

### Index-pairing constraint

Duplicate queries in one run are ambiguous and summaries carry no stable
artifact key. B1b pairs by array index only, before any sort/filter. Do not
sort `resolution_summaries` independently without re-pairing.

Alternate path (separate read slice): add a stable key on the wire — requires
owner approval beyond viewer-only scope.

## B1c scope (shipped)

| Area | Detail |
|------|--------|
| List API | `GET /runs` returns `RunListEntry` with flattened run metadata + `view_parser_summary` |
| Detail API | `GET /runs/{id}` always includes required `view_parser_summary` (derived from same `view_parser` build) |
| Viewer list | `renderViewParserListBadges(run.view_parser_summary)` on each sidebar row |
| Freshness | `mergeRunDetail` preserves `view_parser_summary`; narrow refetch writes summary back to `state.runs` |

### `view_parser_summary` fields

| Field | Type | Meaning |
|-------|------|---------|
| `has_proof` | `bool` | `true` when `resolution_summaries` is non-empty |
| `resolution_count` | `usize` | `resolution_summaries.len()` |
| `latest_outcome` | `string?` | Last summary's `resolution.outcome` when `reacquired` / `not_found` / `stale` |
| `latest_verification_status` | `string?` | Last summary's `verification.status` when `passed` / `failed` |
| `has_known_limits` | `bool` | **Any** `select_results[].known_limits` is non-empty |

Empty summary (`has_proof: false`, count 0, optional fields omitted, `has_known_limits: false`)
is always present on every list row and on detail responses.

### Aggregation semantics

- `latest_outcome` and `latest_verification_status` read from the **last**
  `resolution_summaries` entry only.
- `has_known_limits` scans **all** `select_results` entries (not limited to the
  last resolution).

Implemented in `auv_view::memory::summarize_view_parser_inspect` — no pass-through
builder wrapper.

### Row-level failure policy (`GET /runs`)

| Failure | Behavior |
|---------|----------|
| `read_run` fails | Row kept with run metadata + `ViewParserListSummary::default()`; `tracing::warn!` with `stage=read_run` |
| `build_view_parser_inspect_for_run` fails | Row kept with default summary; `tracing::warn!` with `stage=build_view_parser_inspect` |
| Never | Single bad run causes `GET /runs` to return 500 |

### `mergeRunDetail` preserve (list badge anti-flicker)

```javascript
if (!merged.view_parser_summary && previous && previous.view_parser_summary) {
  merged.view_parser_summary = previous.view_parser_summary;
}
```

`run_finished` → `mergeRunDetail` (preserve summary) →
`refreshViewParserProofFromRunDetail` (detail refetch overwrites with final values)
→ `renderRunList()`.

## Key files

- `crates/auv-view/src/memory/inspect.rs` — `ViewParserListSummary`, `summarize_view_parser_inspect`
- `src/inspect_server/mod.rs` — `GET /runs` / `GET /runs/{id}` wiring + route tests
- `src/inspect_server_viewer.html` — panel, list badges, narrow refresh
- [A8 proof graduation](2026-06-30-scenebridge-closure.md)
- [B2a diagnostic links](2026-06-30-scenebridge-inspect-diagnostic-links.md) — in-run proof card navigation (viewer-only)
- [B2b list filter](2026-06-30-scenebridge-inspect-list-filter.md) — client-side run list filters (viewer-only)
- [B2c cross-run compare](2026-06-30-scenebridge-inspect-cross-run-compare-deferred.md) — **deferred** (evidence gate)

## Validation

```sh
cargo fmt --check
cargo check
cargo test summarize_view_parser
cargo test list_runs_includes_view_parser_summary
cargo test viewer_renders_view_parser_list
cargo test inspect_server --lib
git diff --check
```
