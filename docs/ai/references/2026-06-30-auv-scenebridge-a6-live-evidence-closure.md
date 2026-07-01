# SceneBridge A6: NetEase ViewMemory Live Evidence Closure

**Date:** 2026-06-30 (updated 2026-07-01 @ A6c-3)
**Status:** **PARTIAL** — live baseline unblocked @ A6c-3; Cases A/C/D/E PASS; Case B miss not reproduced.
**Prior work:** [A3 handoff](2026-06-30-auv-scenebridge-a3-implementation-handoff.md) →
[A4 closure](2026-06-30-auv-scenebridge-a4-closure.md) →
[A5 inspect identity charter](2026-06-30-auv-scenebridge-a5-inspect-identity-proof-charter.md)

## One-line summary

**PARTIAL** — @ `dbb7f1e`, logged-in live probes confirm A6c-2 default-window sidebar expansion (`height≈286`, `item_count≥1`) and A6c-1 dedup-only ViewMemory write (default + resized). Cases A/C/D/E PASS on owner Mac; Case B **OPEN** — A6c-8b closed `ls '3'` @ 1725; `select` hit ViewMemory write gate (A6c-9).

Question: does `AUV_NETEASE_VIEW_MEMORY=1` make real `playlist ls → select` use ViewMemory reacquire and honestly fall back on stale/miss/missing/gate-off?

Answer: **PARTIAL** (reacquire hit + stale/missing/gate-off fallbacks demonstrated; miss fallback not demonstrated).

## Owner freeze block

```text
hermetic：fmt/check + memory (16) + playlist_select (7) + region (23) + write_from_scan (3) — PASS @ dbb7f1e
live A6c-3：default 1057×752 sidebar height 285.76, item_count=4, ViewMemory written; resized 1200×820 VIP match + ViewMemory written (dedup-only)
hit signal：Case A reacquire.outcome=reacquired + skipped_rescan_replay=true + no scroll-sidebar-top-*
fallback：Case C stale + Case D missing + Case E gate-off → rescan replay; Case B miss not reproduced
wire：reacquired / stale / not_found (not hit)
gate：remains default-off; A3e NOTICE removal deferred
```

## Acceptance matrix results

| Case | Expected | Result (A6c-3) |
| --- | --- | --- |
| **A Hit** | `reacquired`, skip top-scroll replay | **PASS** |
| **B Miss** | `not_found`, rescan replay | **FAIL** (still `reacquired` after scroll/tamper attempts) |
| **C Stale** | `stale` + wire `stale_reason` | **PASS** (`memory_rejected_at_freshness`) |
| **D Memory missing** | `reacquire=null`, missing limit | **PASS** |
| **E Gate off** | `reacquire=null`, legacy replay | **PASS** |

### Blocker status after A6c-3

1. **Closed @ A6c-2 (live)** — default `1057×752` sidebar region captures playlist rows (`height≈286`, was ~136 in 2026-07-01 SIGNOFF narrative).
2. **Closed @ A6c-1 (live)** — dedup-only scans write `view-memory-playlist_sidebar.json` on default + resized probes.
3. **Open** — Case B miss recipe not validated live (`not_found` + honest rescan replay).

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
| [`live/case-b-miss-select.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-b-miss-select.json) | Case B (FAIL) |
| [`live/case-c-stale-select.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-c-stale-select.json) | Case C |
| [`live/case-d-missing-select.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-d-missing-select.json) | Case D |
| [`live/case-e-gate-off-select.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-e-gate-off-select.json) | Case E |
| [`live/case-ls-probe.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-ls-probe.json) | A6b historical (retained) |
| [`live/case-ls-window-resized-probe.json`](evidence/2026-06-30-scenebridge-netease-sidebar/live/case-ls-window-resized-probe.json) | A6c pre-fix (retained) |

**live_binary_rev:** `dbb7f1e192baef76304a87737743a7d3b204c32a`

## Open items (PARTIAL)

- Re-run **Case B** with owner-verified manual scroll-off miss recipe → expect `not_found` + `scroll-sidebar-top-*`.
- Gate default-on / NOTICE removal — future owner slice (out of A6 scope).

## Done checklist (A6)

- [x] Hermetic pre-gate PASS @ A6c-3
- [x] A6c-2 live default-window baseline PASS
- [x] A6c-1 live dedup write PASS (default + resized)
- [x] Cases A, C, D, E live PASS
- [ ] Case B live PASS
- [ ] Full A6 PASS

## Explicit non-goals

- Proto / MCP changes
- Default-on `AUV_NETEASE_VIEW_MEMORY` or NOTICE removal
- Run-storage `view-memory` role + real `source_run_id`
- `view.reacquire.*` trace spans

## Related

- [A3 implementation handoff](2026-06-30-auv-scenebridge-a3-implementation-handoff.md)
- [live/README.md](evidence/2026-06-30-scenebridge-netease-sidebar/live/README.md)
