# NetEase Music ACP-2 — Playlist Sidebar Scan Proof Handoff

**Date:** 2026-07-05  
**Prerequisite:** [ACP-1 handoff](2026-07-05-auv-netease-music-acp-1-handoff.md), [ACP gate](2026-07-05-auv-core-app-command-pack-gate.md)

## Pack identity

| Field | Value |
|-------|-------|
| App | `com.netease.163music` / crate `auv-netease-music` |
| Pack | **ACP-1 + ACP-2** observe + act hermetic proof pair |
| Proof class | `hermetic` (fixture-driven; no live scan in default CI) |

## Command surface

| Surface | ID | Recording path |
|---------|-----|----------------|
| Product CLI (existing) | `auv-netease-music playlist ls --store-root` | [`persist_playlist_ls_artifacts`](../../../crates/auv-netease-music/src/recording.rs) after **live** `run_live_scan` |
| App invoke (new) | `netease.playlist.sidebarScanProof` | Same persist helper; **hermetic fixture only; no live scan** |

**Invoke vs recording seam:** `invoke` is the **CLI dispatch surface** only. Recording uses [`persist_playlist_ls_artifacts`](../../../crates/auv-netease-music/src/recording.rs) — **not** `invoke_recorded` (which would produce run-level `auv.command`).

## RunSpec / artifact contract

| Item | Value |
|------|-------|
| Run root span / RunSpec name | `auv.netease.playlist.ls` |
| Invoke command id | `netease.playlist.sidebarScanProof` (CLI output / handler summary only; **not** run-level name) |
| Merge gate artifact | `netease-playlist-sidebar-scan` (**only** required artifact for ACP-2 acceptance) |
| Default persist flags | `memory_enabled=false` — no `view-memory` artifact, no reacquire spans |

**L8b / ATL / S-line:** deferred. ACP-2 does not write lineage manifest to artifact-dir or enable view-memory.

## Fixture layout

| Path | Role |
|------|------|
| `crates/auv-netease-music/tests/fixtures/sidebar-scan-proof/hermetic_v0/playlist-sidebar-scan.json` | Hermetic CI input (`proof_class:hermetic`) |
| [`evidence/.../hermetic-reconstruct-sidebar-synthetic.json`](evidence/2026-06-30-scenebridge-netease-sidebar/hermetic-reconstruct-sidebar-synthetic.json) | Structure reference only — **not** executed from docs tree |

Fixture must decode through the crate's existing scan wire path (`VIEW_IR_SCHEMA_VERSION`). Read strategy: prefer [`decode_playlist_sidebar_scan_json`](../../../crates/auv-netease-music/src/lib.rs); if unsuitable, add a narrow IO helper that delegates to the same decode — **no** second schema.

## `store-root` contract

- `--store-root` is **required** for `netease.playlist.sidebarScanProof`.
- No silent temp default.
- CLI success output **must echo** `store_root` and `run_id`.

## InvokeNamespace boundary

Same as ACP-1: `InvokeNamespace::Fixture` is an internal placeholder when building `InvokeCommand` via `command::spec`. Must not appear in user-facing taxonomy.

## Non-goals (hard)

- Live `run_live_scan` / live `playlist ls` invoke
- View-memory / reacquire / lineage manifest to artifact-dir
- L8b, S-line expansion, transport/playback commands
- `GET /runs` list schema changes
- **ACP-2c viewer hint** (defer to unified proof-pack inspect design)

## Persist-only `Inputs`

ACP-2 invoke constructs **minimal** [`Inputs`](../../../crates/auv-netease-music/src/lib.rs) for `persist_playlist_ls_artifacts(..., memory_enabled=false)` only. Do **not** use `Inputs::with_defaults()` — product scroll/OCR defaults are not part of the hermetic proof contract.

## Acceptance

```sh
cargo fmt --check && cargo check
cargo test -p auv-netease-music sidebar_scan_proof
cargo test -p auv-netease-music select_proof
cargo test -p auv-netease-music recording::
cargo run -p auv-netease-music -- invoke --help
git diff --check
```

`invoke --help` must list **both** `netease.playlist.selectProof` and `netease.playlist.sidebarScanProof`.

## Pack graduation (ACP-1 + ACP-2)

Minimum observe + act hermetic pack:

| Slice | Invoke id | RunSpec | Artifact role |
|-------|-----------|---------|---------------|
| ACP-1 | `netease.playlist.selectProof` | `auv.netease.playlist.select` | `netease-playlist-select-result` |
| ACP-2 | `netease.playlist.sidebarScanProof` | `auv.netease.playlist.ls` | `netease-playlist-sidebar-scan` |

## Next slices (not ACP-2)

| Slice | Gate |
|-------|------|
| ACP-2c | Unified proof-pack viewer / inspect panel (defer) |
| Live `netease.playlist.ls` invoke | Owner-approved |
| Scan→select chained invoke | Owner-approved |
| L8b reconnect | Failing evidence required |
