# SceneBridge B1: Inspect viewer view-parser consumption

Date: 2026-06-30
Status: **B1a landed** (run detail proof panel)

## Summary

B1 consumes the existing `GET /runs/{id}.view_parser` field in the inspect
viewer HTML. Each `ViewResolutionSummary` in `resolution_summaries` renders as a
human-readable proof card answering the six A8 owner inspect questions without
client-side recomputation.

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

`run_finished` WebSocket frames carry `frame.run` **without** `view_parser`.
Calling full `loadRunDetail()` on finish would overwrite `state.activeRun`,
reset `activeArtifactKey`, and re-render spans/events/artifacts — destroying
the user's current span / artifact / surface-node selection.

**B1a rule:** when `frame.run.run_id === state.activeRunId`, call
`refreshViewParserProofFromRunDetail(runId)` only:

1. `fetch("/runs/" + runId)` — run metadata + `view_parser` only
2. `mergeRunDetail(state.activeRun, json)`
3. Sync sidebar row + `setMainHeader`
4. `renderViewParserProof(state.activeRun)`
5. **Do not touch** `state.spans`, `state.events`, `state.artifacts`,
   `activeSpanId`, `activeArtifactKey`, or `activeSurfaceNode*`

If proof is still empty after finish, the panel stays hidden.

## B1b follow-on (not in B1a)

Artifact / lineage drill-down must **not** join `select_results` to
`resolution_summaries` by `query` — duplicate queries in one run are ambiguous
and summaries carry no stable artifact key.

**Recommended (viewer-only):** during a single render pass, before any
sort/filter, pair `resolution_summaries[i]` with `select_results[i]` by array
index for `known_limits` and artifact shortcuts. Do not sort summaries
independently without re-pairing.

Alternate path (separate read slice): add a stable key on the wire — requires
owner approval beyond viewer-only scope.

## B1c deferred — needs API decision

`GET /runs` list responses do **not** include `view_parser`; only
`GET /runs/{id}` detail does. Run-list badges are **not** a natural B1a/b
follow-on.

| Option | Trade-off |
|--------|-----------|
| Extend `GET /runs` with lightweight badge/summary fields | Server read-model change |
| Client N+1 detail fetch per row | Latency / load on large stores |
| Cache badge only for runs already opened in session | Partial UX, no list-at-a-glance |

Pick one in a separate slice before implementing B1c.

## Key files

- `src/inspect_server_viewer.html` — panel, render helpers, narrow refresh
- `src/inspect_server/mod.rs` — `viewer_renders_view_parser_proof_hooks` contract test
- [A8 proof graduation](2026-06-30-auv-scenebridge-a8-proof-graduation.md)

## Validation

```sh
cargo fmt --check
cargo check
cargo test inspect_server --lib -- viewer_renders_view_parser
cargo test inspect_server --lib
git diff --check
```
