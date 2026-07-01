# SceneBridge A6 live sign-off

`proof_class: live`

**Date:** 2026-07-01 (A6 Case B closeout @ 2338)
**live_binary_rev:** `fc4977bf64e3fe1cfd0c6dcfc1c647205279b04a`
**evidence_docs_rev:** `71a286448a4efd6e4186b30f1c3c0cfa91c62ca6`
**Environment:** macOS 27.0 (arm64); NetEase foreground; logged-in account; default window `1057×752`, resized probe `1200×820`
**Closure:** [A6 live evidence closure](../../2026-06-30-auv-scenebridge-a6-live-evidence-closure.md)

## Hermetic pre-gate

| Check | Result |
| --- | --- |
| `cargo fmt --check` | PASS |
| `cargo check -p auv-view -p auv-netease-music` | PASS |
| `cargo test -p auv-view memory` | PASS (16 tests) |
| `cargo test -p auv-netease-music playlist_select` | PASS (37 tests) |
| `cargo test -p auv-netease-music view_parsers::sidebar` | PASS (84 tests) |
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
| **B Miss** | **PASS** | `not_found` + rescan replay @ `/tmp/auv-a6c13-case-b-20260701-2338`; query `"3"`; `observation_count=6`; verification `sidebar_row_echo_detail_chrome_v1` — [`case-b-miss-select.json`](case-b-miss-select.json) |
| **C Stale** | **PASS** | `stale` + `stale_reason=memory_rejected_at_freshness`, rescan replay — [`case-c-stale-select.json`](case-c-stale-select.json) |
| **D Memory missing** | **PASS** | `reacquire=null`, missing-memory limit, rescan replay — [`case-d-missing-select.json`](case-d-missing-select.json) |
| **E Gate off** | **PASS** | `reacquire=null`, legacy scroll replay — [`case-e-gate-off-select.json`](case-e-gate-off-select.json) |




## A6c-13 Case B PASS (2338 live)

| Run | `/tmp/auv-a6c13-case-b-20260701-2338` | Meaning |
| --- | --- | --- |
| `case-ls.json` | `query_resolution=unique_exact`, `view_memory.written=true` | A6c-10b ls gate satisfied |
| `reacquire.outcome` | `not_found` | True miss — not pre-scan relocation |
| `observation_count` | `6` | Reacquire seek exhausted before rescan |
| `skipped_rescan_replay` | `false` | Honest rescan replay |
| `known_limits` | `playlist_select_target_from_scan_cache_v1` | A6c-13 scan-cache target resolve |
| `steps` | `scroll-sidebar-top-*` + `reobserve-playlist-after-rescan-replay` | Full miss fallback path |
| `verification.status` | `passed` | A6c-12 sidebar echo on replay path |

**Case B jq audit** (committed artifact):

```bash
jq -e '
  .reacquire.outcome == "not_found" and
  .reacquire.skipped_rescan_replay == false and
  (.known_limits | index("playlist_select_target_from_scan_cache_v1")) and
  (.known_limits[] | select(test("reacquire missed target"))) and
  ([.steps[].name] | any(test("^scroll-sidebar-top-"))) and
  ([.steps[].name] | index("reobserve-playlist-after-rescan-replay")) and
  .verification.status == "passed" and
  (([.diagnostics[].code] | index("playlist_select_rescan_reobserve_missed_target")) | not)
' case-b-miss-select.json
```

## A6c-13 Case B blocker (2309 live negative)

| Run | `/tmp/auv-a6c12-live-20260701-2309` | Meaning |
| --- | --- | --- |
| Manual scroll | visible `41..24`, `"3"` off-screen | Owner believed miss precondition met |
| `reacquire.outcome` | `reacquired` | Pre-select live scan relocated target before reacquire |
| `verification.status` | `passed` | A6c-12 path OK — not a verification regression |

**Root cause:** `resolve_playlist_target_for_query` → `run_live_scan_until_query` erases
manual scroll. **A6c-13 (closed @ 2338):** gate=1 + unique cache `select_target` → skip pre-resolve live scan.
2309 negative bisect retained above; Case B PASS artifact replaces pre-A6c-13 attempts.

## A6c-12 verification false-negative (1812 bisect)

| Signal | 1812 live (`/tmp/auv-a6c11-live-20260701-1812`) | Meaning |
| --- | --- | --- |
| `reacquire.outcome` | `reacquired` | Hit path — **not** Case B |
| `verification.method` | `main_title_ocr_full_window_v1` | A6c-11 ladder exhausted |
| `verification.note` | `region_count=13` | OCR produced regions |
| `playlist-select-post-click-recognition.json` | no `"3"` region | Root cause: **main-pane OCR miss**, not guard/match |
| `obs-0007-recognition.json` (ls) | `"3"` @ (70,657) | Sidebar digit readable; hero title not |

**A6c-12 (closed @ 1812 fix, confirmed @ 2338):** hero-header crop tier; sidebar row echo at
`click_bounds` with strict detail chrome. 2338 Case B verification passed via
`sidebar_row_echo_detail_chrome_v1` after rescan replay.

## Conclusion

**PASS (scoped)** — Cases **A–E PASS** @ `AUV_NETEASE_VIEW_MEMORY=1` on owner Mac.
A6c-10b ls `unique_exact` for `"3"`; A6c-13 scan-cache resolve; Case B `not_found` +
rescan replay @ 2338. Gate remains default-off; NOTICE removal deferred (explicit non-goals).
