# SceneBridge A8: View-parser inspect read graduation

Date: 2026-06-30
Status: **landed (read-side proof surface)**

## Summary

A8 wires durable NetEase `playlist select` proof artifacts and controlled
`view.reacquire.*` spans into shared read extractors. CLI `inspect`, library
`inspect_run`, and `inspect_server` `GET /runs/{id}` now expose the same
`ViewParserInspect` / `ViewResolutionSummary` answers for the six owner inspect
questions (identity, memory, resolution, replay, verification, geometry).

## Shipped

| Area | Detail |
|------|--------|
| A8a trace subset | `persist_playlist_select_proof` records root + memory_load + winning stage |
| A8b extractors | `src/view_parser_read.rs` — view-memory, select-result wire, reacquire spans |
| A8c summaries | `auv-view::memory::ViewParserInspect` + `ViewResolutionSummary` tiers |
| Read API | `list_view_memory_writes`, `view_parser_inspect`, `inspect_run` proof appendix |
| HTTP | `inspect_server` always returns `view_parser` on `GET /runs/{id}` |
| Viewer (B1a) | `inspect_server_viewer.html` renders `resolution_summaries` proof panel — see [B1 handoff](2026-06-30-auv-scenebridge-b1-inspect-viewer-consumption.md) |
| Re-exports | `run_read` pub-use for shared scan/extract entrypoints |

## Not shipped

- Full six-stage reacquire span tree on every path
- Default-on implicit CLI recording beyond existing `--store-root` consumer
- Cross-app donors beyond `com.netease.163music` + `playlist_sidebar`

## Closure sentence

Stored select proof runs are inspectable without replay: identity keys, memory
lineage, reacquire outcome, replay steps, verification method, and tier-IV
geometry notes are machine-readable on run read and in the inspect server JSON.

## Key files

- `src/view_parser_read.rs`, `src/inspect_view_parser.rs`, `src/inspect_server/mod.rs`
- `crates/auv-view/src/memory/inspect.rs`
- `crates/auv-netease-music/src/recording.rs`
- [gap card](evidence/2026-06-30-scenebridge-netease-sidebar/gap-run-storage-bridge.txt)

## Viewer consumption

B1a inspect viewer proof panel: [B1 handoff](2026-06-30-auv-scenebridge-b1-inspect-viewer-consumption.md).
