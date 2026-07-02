# SceneBridge B2a: Inspect viewer diagnostic links

Date: 2026-06-30
Status: **landed** (in-run proof card navigation)

## Summary

B2a makes existing view-parser proof **actionable** inside a single run detail page.
Proof cards gain diagnostic link chips that jump to evidence already loaded in the
viewer (known limits section, artifacts, reacquire/replay spans). No new inspect
data is produced.

## Primary pain

B1 made proof **readable**; users still paid a high cost moving between the proof
panel, span tree, and artifact list. B2a reduces that navigation cost.

## Iron rule

**Only link when uniquely resolvable; otherwise hide/disable the chip.**

No first-match guessing, no `reacquisitions[0]` fallback, no view-memory
pair-index alignment with `resolution_summaries`.

## B2a scope (shipped)

| Link kind | Resolution | Jump target |
|-----------|------------|-------------|
| Known limits | `selectResult.known_limits.length > 0` | Same card section (`data-proof-section="known_limits"`) scroll + brief highlight |
| Select-result artifact | `pairIndex` → Nth `netease-playlist-select-result` artifact (1:1 with `select_results[i]`) | Artifact list + preview |
| View-memory artifact | Unique via `memory_id` + `span_scope_id` on `memory_writes`, or sole `view-memory` artifact on run | Artifact list + preview |
| Reacquire span | Composite key on `reacquisitions`: `scope_id`, `outcome`, `observation_count`, `strategy_used`, `stale_reason` — must be unique; then unique `span.name` match | Span tree + span detail |
| Replay step | Each `replay.step_names[i]` with **unique** `span.name` on run | Span tree + span detail |

Card **Artifacts** shortcut chips use the same resolution rules as the diagnostic bar.

## Non-goals

- No cross-run compare
- No list filter / sort — **list filter shipped in [B2b](2026-06-30-auv-scenebridge-b2b-inspect-list-filter.md)**
- **No further `/runs` API changes** (B1c already ships `view_parser_summary` on list rows)
- No A-line producer expansion
- No new inspect HTTP endpoints
- No `view_parser` / `resolution_summaries` schema changes
- No `source_run_id` navigation to another run
- No inspect-viewer-v0 full tab reopen

## No server API

B2a is **viewer-only**. It consumes `GET /runs/{id}` payload already loaded
(`view_parser`, `spans`, `artifacts`). If a link cannot be uniquely resolved from
existing payload, the chip is hidden — that is not grounds to extend the API in
this slice.

## Key files

- [`src/inspect_server_viewer.html`](../../../src/inspect_server_viewer.html) — diagnostic links, resolve-then-jump helpers
- [`src/inspect_server/mod.rs`](../../../src/inspect_server/mod.rs) — `viewer_renders_view_parser_diagnostic_links_hooks`
- [B1 handoff](2026-06-30-auv-scenebridge-b1-inspect-viewer-consumption.md) (proof panel + list badges)

## Validation

```sh
cargo fmt --check
cargo check
cargo test viewer_renders_view_parser_diagnostic
cargo test viewer_renders_view_parser_proof
git diff --check
```
