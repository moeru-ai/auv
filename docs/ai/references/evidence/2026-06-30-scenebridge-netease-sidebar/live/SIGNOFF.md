# SceneBridge A6 live sign-off

`proof_class: live`

**Date:** 2026-07-01 (A6c-3 live re-probe)
**live_binary_rev:** `dbb7f1e192baef76304a87737743a7d3b204c32a`
**evidence_docs_rev:** `71a286448a4efd6e4186b30f1c3c0cfa91c62ca6`
**Environment:** macOS 27.0 (arm64); NetEase foreground; logged-in account; default window `1057×752`, resized probe `1200×820`
**Closure:** [A6 live evidence closure](../../2026-06-30-auv-scenebridge-a6-live-evidence-closure.md)

## Hermetic pre-gate

| Check | Result |
| --- | --- |
| `cargo fmt --check` | PASS |
| `cargo check -p auv-view -p auv-netease-music` | PASS |
| `cargo test -p auv-view memory` | PASS (16 tests) |
| `cargo test -p auv-netease-music playlist_select` | PASS (7 tests) |
| `cargo test -p auv-netease-music --lib view_parsers::sidebar::region` | PASS (23 tests) |
| `cargo test -p auv-netease-music --lib write_from_scan_when_enabled` | PASS (3 tests) |
| `git diff --check` | PASS |

## Pre/post probe对照（geometry / write blockers）

| Probe | Window | `sidebar_region.height` | `item_count` | `match_count` | ViewMemory write | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| A6b `case-ls-probe.json` | 1057×752 | 283 | 0 | 0 | yes (empty projection) | guest / 创建的歌单0 |
| A6c pre-fix `case-ls-window-resized-probe.json` | 1200×820 | 202 | 2 | 1 (VIP) | **no** (pre-A6c-1 write skip) | dedup-only blocker |
| **A6c-3 default** `case-ls-a6c3-default-probe.json` | 1057×752 | **285.76** | 4 | 1 (`最近播放`) | **yes** | A6c-2 expand + A6c-1 dedup write |
| **A6c-3 resized** `case-ls-a6c3-resized-probe.json` | 1200×820 | 311.6 | 4 | 1 (VIP) | **yes** | A6c-1 dedup-only live confirmed |

## A6c-3 baseline checklist (default window)

| # | Gate | Result |
| --- | --- | --- |
| 1 | `item_count ≥ 1` | PASS (`4`) |
| 2 | window ≈ 1057×752 | PASS |
| 3 | `sidebar_region.height` (soft) | PASS (`285.76` ≥ `0.38×752`) |
| 4 | expand: `sidebar_region.y` < section header y (soft) | PASS (`384.24`) |
| 5 | `match_count ≥ 1` + `candidate_id` | PASS (`最近播放`) |
| 6 | `view-memory-playlist_sidebar.json` | PASS |
| 7 | no `view memory write skipped` | PASS |
| 8 | dedup-only scan diagnostics; VM `diagnostics: []` | PASS |
| 9 | match item in viewport | PASS |

**Default baseline:** PASS @ A6c-3.

## Live acceptance matrix

| Case | Status | Notes |
| --- | --- | --- |
| **A Hit** | **PASS** | `reacquire.outcome=reacquired`, `skipped_rescan_replay=true`, no `scroll-sidebar-top-*` — [`case-a-hit-select.json`](case-a-hit-select.json) |
| **B Miss** | **FAIL** | UI scroll + memory tamper attempts still yielded `reacquired`; `not_found` + rescan replay not observed — [`case-b-miss-select.json`](case-b-miss-select.json) |
| **C Stale** | **PASS** | `stale` + `stale_reason=memory_rejected_at_freshness`, rescan replay — [`case-c-stale-select.json`](case-c-stale-select.json) |
| **D Memory missing** | **PASS** | `reacquire=null`, missing-memory limit, rescan replay — [`case-d-missing-select.json`](case-d-missing-select.json) |
| **E Gate off** | **PASS** | `reacquire=null`, legacy scroll replay — [`case-e-gate-off-select.json`](case-e-gate-off-select.json) |

## Conclusion

**PARTIAL** — A6c-1/A6c-2 confirmed on live `playlist ls` @ `dbb7f1e` (default geometry unblocked; dedup-only ViewMemory write on default + resized). Cases **A, C, D, E PASS**; **Case B FAIL** (miss / `not_found` path not reproduced in this session). Full A6 PASS deferred until Case B is re-run with a verified manual miss recipe.

Gate remains default-off; NOTICE removal deferred.
