# SceneBridge A6: NetEase ViewMemory Live Evidence Closure

**Date:** 2026-06-30
**Status:** **owner-approved A6 closure (docs + linked sub-slices)** — live evidence protocol +
sign-off template. **A6c-1** (separate Rust slice) fixed dedup-only ViewMemory write;
does **not** flip `AUV_NETEASE_VIEW_MEMORY` default-on, remove NOTICE, or change proto/MCP.

**Prior work:** [A3 handoff](2026-06-30-auv-scenebridge-a3-implementation-handoff.md) →
[A4 closure](2026-06-30-auv-scenebridge-a4-closure.md) →
[A5 inspect identity charter](2026-06-30-auv-scenebridge-a5-inspect-identity-proof-charter.md)

## One-line summary

**PARTIAL** — hermetic gate green; refreshed live probes on 2026-07-01 found two
real blockers before Case A: default window geometry cropped the detected sidebar
region to headers only (**still open**), and the enlarged-window scan that matched
`VIP黑胶专属歌单` produced `deduplicated_item` diagnostics that blocked ViewMemory
write (**resolved in Rust @ A6c-1**; live re-probe pending).

Question: does `AUV_NETEASE_VIEW_MEMORY=1` make real `playlist ls → select` use
ViewMemory reacquire and honestly fall back on stale/miss/missing/gate-off?

Answer: **PARTIAL** (computer use OK; live probes exposed scan / clean-memory blockers).

## Owner freeze block

```text
hermetic：fmt/check + auv-view memory (16) + playlist_select (7) — PASS @ 10857cc
live A6b/A6c：computer use ran; default window saw headers-only item_count=0 (still open); enlarged window matched VIP黑胶专属歌单 but dedup-only scan write was blocked pre-A6c-1 (fixed hermetically; live re-probe pending)
hit signal：reacquire.outcome=reacquired + skipped_rescan_replay=true + no scroll-sidebar-top-*
fallback：stale/not_found/missing/gate-off → known_limits + rescan replay steps
wire：reacquired / stale / not_found (not hit)
gate：remains default-off; A3e NOTICE removal deferred
```

## Acceptance matrix results

| Case | Expected | Result (2026-07-01 refresh) |
| --- | --- | --- |
| **A Hit** | `reacquired`, skip top-scroll replay | **blocked** (default window: `item_count=0`; resized + dedup-only path unblocked in code @ A6c-1, live matrix not re-run) |
| **B Miss** | `not_found`, rescan replay | **blocked** (depends on clean Case A baseline) |
| **C Stale** | `stale` + wire `stale_reason` | **blocked** (depends on clean Case A baseline) |
| **D Memory missing** | `reacquire=null`, missing limit | **blocked** (depends on clean Case A baseline) |
| **E Gate off** | `reacquire=null`, legacy replay | **blocked** (depends on clean Case A baseline) |

A6 blocker status (2026-07-01 probes + A6c-1 fix):

1. **Open — default-sized window:** detected `sidebar_region` was only `136px` high and captured
   section headers but no playlist rows, yielding `item_count=0` / `match_count=0`.
2. **Resolved @ A6c-1 (hermetic) — dedup-only dirty scan:** enlarged-window probes matched
   `VIP黑胶专属歌单` but `deduplicated_item` diagnostics caused `write_from_scan` to skip
   memory write. Rust fix: dedup-only scans are writable; mixed diagnostics still block.
   Live re-probe after merge is still pending.

## Slice classification

| Item | Value |
| --- | --- |
| This note (A6 closure) | **docs-only** |
| [A6c-1](../../../crates/auv-netease-music/src/view_memory.rs) dedup write fix | **bug fix** (hermetic; live re-probe pending) |
| Live execution | **owner-labeled** (`proof_class: live`), not CI |
| Hermetic gate | Required pre-condition — **PASS** |
| Not | proto/MCP, gate default-on, NOTICE removal, run-storage, trace spans, Q5 |

## Evidence attachments

| Path | Status |
| --- | --- |
| [`live/README.md`](evidence/2026-06-30-scenebridge-netease-sidebar/live/README.md) | Protocol + matrix + recipes |
| [`live/SIGNOFF.md`](evidence/2026-06-30-scenebridge-netease-sidebar/live/SIGNOFF.md) | Matrix checkboxes + env |
| [`live/transcript.txt`](evidence/2026-06-30-scenebridge-netease-sidebar/live/transcript.txt) | Redacted hermetic + partial probe |
| `live/case-*.json` | **Not attached** — Cases A–E blocked |
| [`live/case-ls-probe.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-ls-probe.json) | A6b blocker probe |
| [`live/case-ls-window-resized-probe.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-ls-window-resized-probe.json) | A6c probe: resized window matches playlist; dedup dirty-scan blocker (pre-A6c-1). **Machine-parseable JSON** (stderr stripped from attachment). |
| [`live/view-memory-playlist_sidebar-probe.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/view-memory-playlist_sidebar-probe.json) | A6b probe snapshot |
| [`live/examples/`](evidence/2026-06-30-scenebridge-netease-sidebar/live/examples/) | Structure exemplars only (`structure_exemplar`) |

**Git rev (hermetic gate refresh):** `10857ccfa33ec337c82349acfafa4be5786e73e2`

## Anti-misread rules (A6)

1. **Reacquire does not skip the live scan before select** — `resolve_playlist_target_for_query`
   always runs; reacquire only optimizes scroll replay when memory loads
   ([`playlist.rs`](../../../crates/auv-netease-music/src/commands/playlist.rs) L362–418).
2. **`reacquire.outcome` wire values** are `reacquired` / `stale` / `not_found` —
   [`outcome_label`](../../../crates/auv-view/src/memory/reacquire_adapter.rs) — not `hit`.
3. **Hermetic FakeAdapter tests ≠ live proof** (A5 #6); `examples/` JSON is
   `structure_exemplar` only.
4. **Live evidence surface** is CLI JSON + artifact-dir files — no `view.reacquire.*`
   spans, no run-storage `view-memory` role (A5 Tier II–III).
5. **`known_limits` strings** supplement human-readable fallback; pair with structured
   `reacquire` fields for resolution proof (A5 #4).
6. **Scan diagnostics ≠ memory diagnostics** — `playlist ls` scan JSON may carry
   `deduplicated_item` while persisted `view-memory-*.json` keeps `diagnostics: []`
   when write used `clean: true` (A6c-1). Forensics: use `playlist-scan-cache.json`.

## Sign-off template

```text
Question: AUV_NETEASE_VIEW_MEMORY=1 时 ls→select 是否真走 reacquire 并诚实回退？
Answer: PARTIAL
Hit: reacquire.outcome=reacquired + skipped_rescan_replay=true + no top-scroll replay
Fallback: stale/miss/missing/gate-off → known_limits + rescan replay steps
Gate: remains default-off; A3e NOTICE removal deferred to future owner slice
```

## Open items (PARTIAL only)

- Fix the live scan so default window geometry captures playlist rows, not only section headers.
- Re-run Cases A–E per [`live/README.md`](evidence/2026-06-30-scenebridge-netease-sidebar/live/README.md) after A6c-1 merge (confirm resized + dedup-only writes `view-memory-playlist_sidebar.json` on owner Mac).
- Attach `case-a-hit-select.json` (and B–E) after successful matrix.
- ~~Fix dirty-scan `deduplicated_item` write path~~ — **done @ A6c-1** (hermetic regression; live confirmation pending).

## Done checklist (A6 docs-only)

- [x] Extended live README (Cases A–E, recipes, redaction, bash protocol)
- [x] A6b computer-use probe + blocker artifacts (`case-ls-probe.json`)
- [x] Structure exemplars under `live/examples/` (labeled, not live proof)
- [x] Hermetic pre-gate PASS
- [ ] Cases A–E live PASS on owner Mac
- [x] `git diff --check` before commit

## Explicit non-goals (A6 closure note)

- Proto / MCP changes
- Default-on `AUV_NETEASE_VIEW_MEMORY` or NOTICE removal
- Run-storage `view-memory` role + real `source_run_id`
- `view.reacquire.*` trace spans
- Q5 cross-app comparison
- Select skipping live scan (undesigned; out of scope)

## Related

- [A3 implementation handoff](2026-06-30-auv-scenebridge-a3-implementation-handoff.md)
- [A4 closure](2026-06-30-auv-scenebridge-a4-closure.md)
- [A5 inspect identity charter](2026-06-30-auv-scenebridge-a5-inspect-identity-proof-charter.md)
- [Evidence folder](evidence/2026-06-30-scenebridge-netease-sidebar/)
- [anchor-reacquisition-v0](2026-05-29-view-parser-anchor-reacquisition-v0.md)
