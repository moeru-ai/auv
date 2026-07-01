# SceneBridge A6: NetEase ViewMemory Live Evidence Closure

**Date:** 2026-06-30 (updated 2026-07-01 @ A6 Case B closeout)
**Status:** **PASS (scoped)** — Cases A–E live pass with `AUV_NETEASE_VIEW_MEMORY=1`; gate remains default-off by explicit non-goal.
**Prior work:** [A3 handoff](2026-06-30-auv-scenebridge-a3-implementation-handoff.md) →
[A4 closure](2026-06-30-auv-scenebridge-a4-closure.md) →
[A5 inspect identity charter](2026-06-30-auv-scenebridge-a5-inspect-identity-proof-charter.md)

## One-line summary

**PASS (scoped)** — Cases A–E all live pass with `AUV_NETEASE_VIEW_MEMORY=1`; gate remains default-off by explicit non-goal.

Question: does `AUV_NETEASE_VIEW_MEMORY=1` make real `playlist ls → select` use ViewMemory reacquire and honestly fall back on stale/miss/missing/gate-off?

Answer: **YES** (reacquire hit + stale/miss/missing/gate-off fallbacks all demonstrated live @ gate=1).

## Owner freeze block

```text
hermetic：fmt/check + memory (16) + playlist_select (37) + sidebar (84) + write_from_scan (3) — PASS @ fc4977b
live A6c-3：default 1057×752 sidebar height 285.76, item_count=4, ViewMemory written; resized 1200×820 VIP match + ViewMemory written (dedup-only)
hit signal：Case A reacquire.outcome=reacquired + skipped_rescan_replay=true + no scroll-sidebar-top-*
fallback：Case B not_found + rescan replay @ 2338; Case C stale + D missing + E gate-off → rescan replay
wire：reacquired / stale / not_found (not hit)
gate：remains default-off; A3e NOTICE removal deferred
```

## Acceptance matrix results

| Case | Expected | Result (A6c-3) |
| --- | --- | --- |
| **A Hit** | `reacquired`, skip top-scroll replay | **PASS** |
| **B Miss** | `not_found`, rescan replay | **PASS** @ 2338 (`observation_count=6`, scan-cache resolve) |
| **C Stale** | `stale` + wire `stale_reason` | **PASS** (`memory_rejected_at_freshness`) |
| **D Memory missing** | `reacquire=null`, missing limit | **PASS** |
| **E Gate off** | `reacquire=null`, legacy replay | **PASS** |

### Blocker status after A6c-3

1. **Closed @ A6c-2 (live)** — default `1057×752` sidebar region captures playlist rows (`height≈286`, was ~136 in 2026-07-01 SIGNOFF narrative).
2. **Closed @ A6c-1 (live)** — dedup-only scans write `view-memory-playlist_sidebar.json` on default + resized probes.
3. **Closed @ 2338 (live)** — Case B `not_found` + honest rescan replay + verification passed (`case-b-miss-select.json`). A6c-10b ls `unique_exact` for `"3"`; A6c-13 scan-cache target resolve.

## Slice classification

| Item | Value |
| --- | --- |
| A6c-3 (this refresh) | **docs-only** + owner-labeled live artifacts |
| A6c-1 / A6c-2 | **bug fix** (merged @ `dbb7f1e`) |
| Live execution | `proof_class: live`, not CI |

## Evidence attachments

| Path | Status |
| --- | --- |
| [`live/SIGNOFF.md`](evidence/2026-06-30-scenebridge-netease-sidebar/live/SIGNOFF.md) | A6c-3 matrix + probe table |
| [`live/case-ls-a6c3-default-probe.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-ls-a6c3-default-probe.json) | Default-window post-fix probe |
| [`live/case-ls-a6c3-resized-probe.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-ls-a6c3-resized-probe.json) | Resized dedup write confirmation |
| [`live/case-a-hit-select.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-a-hit-select.json) | Case A |
| [`live/case-b-miss-select.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-b-miss-select.json) | Case B (PASS @ 2338) |
| [`live/case-c-stale-select.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-c-stale-select.json) | Case C |
| [`live/case-d-missing-select.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-d-missing-select.json) | Case D |
| [`live/case-e-gate-off-select.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-e-gate-off-select.json) | Case E |
| [`live/case-ls-probe.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-ls-probe.json) | A6b historical (retained) |
| [`live/case-ls-window-resized-probe.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-ls-window-resized-probe.json) | A6c pre-fix (retained) |

**live_binary_rev:** `fc4977bf64e3fe1cfd0c6dcfc1c647205279b04a`

## Open items (post-A6 scoped PASS)

- Gate default-on / NOTICE removal — future owner-approved slice (explicit non-goals).

## Done checklist (A6)

- [x] Hermetic pre-gate PASS @ A6c-3
- [x] A6c-2 live default-window baseline PASS
- [x] A6c-1 live dedup write PASS (default + resized)
- [x] Cases A, C, D, E live PASS
- [x] Case B live PASS @ 2338
- [x] Full A6 PASS (scoped)

## Explicit non-goals

- Proto / MCP changes
- Default-on `AUV_NETEASE_VIEW_MEMORY` or NOTICE removal
- Run-storage `view-memory` role + real `source_run_id`
- `view.reacquire.*` trace spans

## Related

- [A3 implementation handoff](2026-06-30-auv-scenebridge-a3-implementation-handoff.md)
- [live/README.md](evidence/2026-06-30-scenebridge-netease-sidebar/live/README.md)
