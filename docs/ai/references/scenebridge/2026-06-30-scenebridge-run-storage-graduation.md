# SceneBridge A7-min: ViewMemory run-storage graduation

Date: 2026-06-30
Status: **landed (one consumer path)**

## Summary

`view-memory` is a durable run-storage artifact role for NetEase `playlist ls` when
`--store-root` is set. `select` / `play` / `play --candidate-id` read store-first via
`view-memory-run-lineage.json` manifest anchoring, but query-based cache resolve only
trusts a cached scan on `unique_exact` query resolution. Without `--store-root`, A6
artifact-dir bridge behavior is unchanged.

## Shipped

| Area | Detail |
|------|--------|
| A7a contract | `VIEW_MEMORY_ARTIFACT_ROLE`, `serialize_memory_bytes`, `view_memory_lineage_ref_wire` in `auv-view` |
| A7a test | `auv-tracing-driver` hermetic `stage_artifact_bytes` + `read_run` role check |
| A7b write | Post-hoc `persist_playlist_ls_artifacts` after live scan; scan stays outside recording lifecycle |
| A7b manifest | `view-memory-run-lineage.json` (required when `--store-root`) |
| A7b read | `NOTICE(store_root_read_bias_v1)` manifest → store → artifact-dir fallback |
| A7b CLI | `--store-root` on `playlist ls`, `select`, `play` |
| Degraded success | exit 0 + `known_limits` in `playlist ls --json` when mirror/manifest/store partial fail |

## Not shipped (A8)

- `view.parse.memory_write` trace spans
- `Runtime::list_view_memory_writes` / inspect read API
- Tier II inspect capabilities
- Default-on `--store-root`, catalog/invoke registration, second donor

## Closure sentence

`view-memory` under `--store-root` is a real run artifact; `select`/`play` prefer store reads; without `--store-root` A6 is unchanged; inspect/trace remain A8.

## Key files

- `crates/auv-view/src/memory/mod.rs`, `store.rs`
- `crates/auv-netease-music/src/recording.rs`
- `crates/auv-netease-music/src/cli.rs` (`run_playlist`)
- [gap card](../evidence/2026-06-30-scenebridge-netease-sidebar/gap-run-storage-bridge.txt)
