# SceneBridge A3: Implementation Handoff

**Date:** 2026-06-30  
**Status:** implementation charter for Package A3-min prototype.

**Boundary:** [A3 prototype boundary review](2026-06-30-auv-scenebridge-a3-prototype-boundary-review.md)
(**Owner: Package A3-min accepted**)

## Scope

This handoff sequences Rust work after the A3 boundary doc lands. Each sub-slice
is independently reviewable and should land as its own commit when possible.

```text
A3a  ViewMemory types + writer (auv-view)
A3b  Reader + freshness + artifact-dir store (auv-view)
A3c  Reacquire stages 1/3/5 + fake adapter tests (auv-view)
A3d  NetEase SidebarReacquireAdapter + playlist wire (auv-netease-music)
A3e  Optional live evidence + gate sign-off
```

## Feature gate

| Env var | Value | Behavior |
| --- | --- | --- |
| `AUV_NETEASE_VIEW_MEMORY` | unset or `0` | Legacy rescan-replay path (NOTICE behavior) |
| `AUV_NETEASE_VIEW_MEMORY` | `1` | Try ViewMemory load + reacquire; fallback to rescan on failure |

Default **off** until A3e owner sign-off. Do not remove NOTICE or flip default
without hermetic matrix green + optional live evidence.

## A3a — ViewMemory type + writer

**Crate:** `auv-view`  
**Modules:**

| Path | Responsibility |
| --- | --- |
| `src/memory/mod.rs` | `ViewMemory`, snapshots, `VIEW_MEMORY_SCHEMA_VERSION` |
| `src/memory/write.rs` | `memory_from_reconstruction`, write guards |
| `src/lib.rs` | `pub mod memory;` re-exports |

**Write guards (A3-min):**

1. Reconstruction has ≥1 anchor or ≥1 non-Unknown item node.
2. Caller asserts clean parse (no observed-failure gate in framework — netease passes `clean: true`).

**Synthetic fields:**

- `source_run_id`: `artifact-dir-bridge-a3` placeholder with `NOTICE` at netease write site.
- `source_reconstruction_ref`: optional path string under artifact dir.

**Tests:**

- `memory_roundtrip_serde`
- `memory_write_skips_empty_reconstruction`
- `memory_id_stable_for_app_scope_pair`

**Spec mapping:** view-memory-v0 done criteria #1, #6 (partial #2).

## A3b — Reader + freshness + store

**Modules:**

| Path | Responsibility |
| --- | --- |
| `src/memory/read.rs` | `read_memory`, TTL, schema check |
| `src/memory/store.rs` | `view-memory-{scope_id}.json` under artifact dir |

**Freshness (A3-min defaults):**

| Rule | Default |
| --- | --- |
| Hard TTL | 24 hours |
| Schema | reject if `schema_version != "view-memory-v0"` |
| Baseline width drift | warn in diagnostics; still return memory in A3-min |

**Tests:**

- `read_rejects_expired_memory`
- `read_rejects_schema_mismatch`
- `store_roundtrip_load_latest`

**Spec mapping:** view-memory-v0 done criteria #3, #7 (staleness subset).

## A3c — Reacquire core

**Modules:**

| Path | Responsibility |
| --- | --- |
| `src/memory/reacquire.rs` | `ReacquireTarget`, `ReacquireOutcome`, cascade |
| `src/memory/reacquire_adapter.rs` | `ReacquireDriverAdapter` trait |

**Stages implemented (A3-min):**

| Stage | Name | Hermetic |
| --- | --- | --- |
| 1 | ViewNodeId / node id direct match | yes |
| 3 | Label in current viewport | yes |
| 5 | Label + section context with scroll | yes |
| 4 | Viewport fingerprint neighborhood | live adapter only |
| 2, 6 | AX / Mixed | deferred A4 |

**Tests** (fixtures: [`reacquire-target-fixtures.json`](evidence/2026-06-30-scenebridge-netease-sidebar/reacquire-target-fixtures.json)):

- `reacquire_stage1_direct_id_on_screen`
- `reacquire_stage3_unique_label`
- `reacquire_stage3_ambiguous_falls_through`
- `reacquire_stage5_label_section_after_scroll`
- `reacquire_not_found_lists_attempted_strategies`

**Spec mapping:** anchor-reacquisition-v0 done criteria #1–#3, #6 (partial #7).

## A3d — NetEase wire

**Crate:** `auv-netease-music`

| Path | Change |
| --- | --- |
| `src/view_parsers/sidebar/reacquire_adapter.rs` | Live `ReacquireDriverAdapter` (macOS) |
| `src/view_parsers/sidebar/mod.rs` | `mod reacquire_adapter;` |
| `src/commands/playlist.rs` | Reacquire branch in `run_playlist_select_resolved` |
| `src/cli.rs` | `write_view_memory` after `write_playlist_scan_cache` when gate on |
| `src/lib.rs` | `PlaylistSelectResult` reacquire fields |

**Flow when gate on:**

```text
playlist ls --json
  → scan → playlist-scan-cache.json + view-memory-playlist_sidebar.json

playlist select <label>  (or play --candidate-id → select path)
  → load ViewMemory from artifact-dir
  → reacquire(label, section_hint)
  → on Reacquired: use fresh bounds, skip scroll-replay loop
  → on Stale/NotFound: fallback to existing rescan loop + known_limits note
```

**Tests:**

- Existing sidebar tests unchanged
- `playlist_select_uses_reacquire_when_memory_hit` (injected fake adapter, no driver)

**Doc fix when touching A2 strings nearby:** `playlist ls --json` not `playlist --json`.

## A3e — Sign-off (optional)

- Add `evidence/.../live/README.md` transcript template (`proof_class: live`)
- Checklist below before NOTICE removal / default-on gate

## Done checklist (A3-min)

- [ ] `cargo test -p auv-view memory` green
- [ ] `cargo test -p auv-netease-music view_parsers::sidebar` green
- [ ] `cargo fmt --check` / `cargo check -p auv-view -p auv-netease-music`
- [ ] Gate off: playlist select behavior matches pre-A3 rescan path
- [ ] Gate on + memory hit: reacquire path skips top-scroll replay (hermetic test)
- [ ] `git diff --check` on docs
- [ ] A2 `playlist ls --json` doc drift fixed if touched
- [ ] Optional live evidence attached (owner request)

## Rollback

1. Unset `AUV_NETEASE_VIEW_MEMORY` — immediate legacy behavior.
2. Revert A3d commit — netease wire only; memory crate can remain unused.
3. Delete `view-memory-*.json` from artifact dir — no schema migration needed.

## Related

- [A3 boundary review](2026-06-30-auv-scenebridge-a3-prototype-boundary-review.md)
- [A2 evidence pack](2026-06-30-auv-scenebridge-a2-netease-sidebar-evidence-pack.md)
- [view-memory-v0](2026-05-29-view-parser-view-memory-v0.md)
- [anchor-reacquisition-v0](2026-05-29-view-parser-anchor-reacquisition-v0.md)
