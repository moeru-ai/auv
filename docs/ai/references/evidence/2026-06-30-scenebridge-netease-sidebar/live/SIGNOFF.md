# SceneBridge A6 live sign-off

`proof_class: live`

**Date:** 2026-07-01 (A6b/A6c computer-use refresh)
**Git rev (hermetic gate):** `10857ccfa33ec337c82349acfafa4be5786e73e2`
**Environment:** macOS 27.0 (arm64); NetEase foreground; logged-in account; visible sidebar playlist `VIP黑胶专属歌单`; default window `1057x752`, resized probe window `1200x820`
**Closure:** [A6 live evidence closure](../../2026-06-30-auv-scenebridge-a6-live-evidence-closure.md)

## Hermetic pre-gate

| Check | Result |
| --- | --- |
| `cargo fmt --check` | PASS |
| `cargo check -p auv-view -p auv-netease-music` | PASS |
| `cargo test -p auv-view memory` | PASS (16 tests) |
| `cargo test -p auv-netease-music playlist_select` | PASS (7 tests) |
| `git diff --check` | PASS |

## Live acceptance matrix

| Case | Status | Notes |
| --- | --- | --- |
| **A Hit** | **blocked** | default window: `item_count=0`; resized window: label matched; dedup write blocker fixed @ A6c-1 (live re-probe pending) |
| **B Miss** | **blocked** | Depends on Case A baseline |
| **C Stale** | **blocked** | Depends on Case A baseline |
| **D Memory missing** | **blocked** | Depends on Case A baseline |
| **E Gate off** | **blocked** | Depends on Case A baseline |

## A6b/A6c live probes

Commands: `playlist ls "VIP"` with `AUV_NETEASE_VIEW_MEMORY=1`, first at default window
size, then after resizing the front window.

| Signal | Observed |
| --- | --- |
| default window `1057x752` | detected `sidebar_region.height=136`; headers only; `item_count=0`, `match_count=0` |
| resized window `1200x820` | detected `sidebar_region.height=202`; `item_count=2`, `match_count=1` for `VIP黑胶专属歌单` |
| `view-memory-playlist_sidebar.json` | **not written** on 2026-07-01 resized probe (pre-A6c-1); A6c-1 Rust fix expects dedup-only scans to write — live re-probe pending |
| write blocker (historical) | `view memory write skipped: scan did not produce writable ViewMemory` |
| scan diagnostics | repeated `deduplicated_item` for `VIP黑胶专属歌单` (non-blocking @ A6c-1 when dedup-only) |

Probe attachments (blocker evidence, not Case PASS):

- [`case-ls-probe.json`](case-ls-probe.json)
- [`case-ls-window-resized-probe.json`](case-ls-window-resized-probe.json)
- [`view-memory-playlist_sidebar-probe.json`](view-memory-playlist_sidebar-probe.json)

## Conclusion

**PARTIAL** — AUV computer use ran; hermetic gate green. Cases A–E **not executed**
because live probes still cannot establish the required `playlist ls` →
`view-memory-playlist_sidebar.json` baseline at **default** window geometry (resized +
dedup-only path unblocked in code @ A6c-1; live confirmation pending).

**Next unblock:** fix the live scan boundary so default geometry captures playlist rows.
Re-run resized-window `playlist ls` after A6c-1 merge to confirm dedup-only scans write
`view-memory-playlist_sidebar.json`, then execute Cases A–E.

Gate remains default-off; NOTICE removal deferred.
