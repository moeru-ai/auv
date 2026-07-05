# NetEase Music ACP-1 — Playlist Select Proof Handoff

**Date:** 2026-07-05  
**Prerequisite:** [ACP gate](2026-07-05-auv-core-app-command-pack-gate.md), [L8 closeout](2026-07-05-auv-core-l8-closeout-review.md), [L9 inspect surface](2026-07-05-auv-core-l9-inspect-surface-handoff.md)

> **Orthogonality:** ACP-1 packaging success does not re-validate or close the L8b candidate-action seam. See [ACP gate orthogonality callout](2026-07-05-auv-core-app-command-pack-gate.md#orthogonality-callout-mandatory-in-every-acp-handoff).

## Pack identity

| Field | Value |
|-------|-------|
| App | `com.netease.163music` / crate `auv-netease-music` |
| Pack | **ACP-1 playlist select proof** (single command group) |
| Proof class | `hermetic` (fixture-driven; no live scan in default CI) |

## Command surface

| Surface | ID | Recording path |
|---------|-----|----------------|
| Product CLI (existing) | `auv-netease-music playlist select --store-root` | [`persist_playlist_select_proof`](../../../crates/auv-netease-music/src/recording.rs) |
| App invoke (new) | `netease.playlist.selectProof` | Same persist seam (hermetic fixture; no live scan) |

**Invoke vs recording seam:** `invoke` is the **CLI dispatch surface** only. Recording uses the app persist helper — **not** `invoke_recorded` (which would produce run-level `auv.command`).

## RunSpec / artifact contract

| Item | Value |
|------|-------|
| Run root span / RunSpec name | `auv.netease.playlist.select` |
| Invoke command id | `netease.playlist.selectProof` (CLI output / handler summary only; **not** run-level name) |
| Merge gate artifact | `netease-playlist-select-result` (**only** required artifact for ACP-1 acceptance) |
| Hermetic defaults | `evidence=None`, `memory=None` — no sibling `view-memory` / sidebar-scan unless explicitly passed later |

**L8b / ATL:** deferred in ACP-1. Do not treat `action_transition_lineage` as merge gate.

## Fixture layout

| Path | Role |
|------|------|
| `crates/auv-netease-music/tests/fixtures/select-proof/hermetic_v0/select-result.json` | Hermetic CI input (`proof_class: hermetic`) |
| [`evidence/.../case-a-hit-select.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-a-hit-select.json) | Live structure reference only — **not** executed from docs tree |

## `store-root` contract

- `--store-root` is **required** for `netease.playlist.selectProof` (aligned with product CLI: no `store_root` → no durable proof).
- No silent temp default (avoids ghost runs invisible to inspect).
- CLI success output **must echo** `store_root` and `run_id`.

## InvokeNamespace boundary

App-local registry may borrow `InvokeNamespace::Fixture` when building `InvokeCommand` via `command::spec`.

- **Internal placeholder only** — not ACP-1 user-facing taxonomy.
- **Must not** appear in help headings, JSON output, or viewer copy as "fixture command" / NetEase classification.

## Non-goals (hard)

- Do not extend `InvokeNamespace` enum or `invoke_command` macro.
- Do not register commands in root `default_registry()` / MCP driver catalog.
- Do not use `invoke_recorded` in ACP-1b.
- Do not change `GET /runs` list schema.
- Do not expand view-memory / reacquire / L8b / S-line behavior.
- ACP-1c viewer hint must **not** say `selectProof` (invoke and product CLI share the same persist seam).

## Acceptance

```sh
cargo fmt --check && cargo check
cargo test -p auv-netease-music select_proof
cargo test -p auv-netease-music recording::
cargo run -p auv-netease-music -- invoke --help
git diff --check
```

`invoke --help` must show `auv-netease-music invoke` usage and list `netease.playlist.selectProof`.

## ACP-1c landed (viewer)

Superseded by ACP-2c unified panel — see [ACP-2 handoff](2026-07-05-auv-netease-music-acp-2-handoff.md#acp-2c-landed-viewer).

- Run-detail panel `#netease-proof-hint` (unified) shows **NetEase playlist select proof** when root span is `auv.netease.playlist.select` and artifacts include `netease-playlist-select-result`.
- Generic labels only (no `selectProof` wording).

## Next slices (not ACP-1)

| Slice | Gate |
|-------|------|
| ACP-1c | Run-detail hint `NetEase playlist select proof` (generic; landed) |
| ACP-2 | Hermetic `netease.playlist.sidebarScanProof` (see [ACP-2 handoff](2026-07-05-auv-netease-music-acp-2-handoff.md)) |
| L8b reconnect | Owner-approved; failing evidence required |
