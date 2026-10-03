# QQ Music background control (living doc)

Goal: control QQ Music without stealing foreground focus (parity with macOS background input). Append new phases here.

## Phase 1 — SMTC + CoreAudio driver (2026-10-03)

Recon: QQ Music registers an SMTC session (`QQMusic.exe`). Probe of background search-and-play paths: (1) HWND `WM_SETTEXT`: `TXGuiFoundation` has 0 child Edit HWNDs; sending `WM_SETTEXT` only mutates the window caption bar, unable to target the DirectComposition search box. (2) UIA `ValuePattern::SetValue`: returns `0x80004001` (`E_NOTIMPL`) and actively steals foreground focus (`SetForegroundWindow`). (3) UIA `LegacyIAccessible::SetValue`: returns `0x80004001` (`E_NOTIMPL`). WGC captures occluded windows; minimized windows are not capturable (DWM suspends composition).

Shipped `media.rs`: `SmtcMediaManager` / `SmtcSession` (play, pause, toggle, next, previous, status, track metadata) + `AudioVolumeController` (per-process volume via CoreAudio; global volume untouched).

Eval: 100 ops, `GetForegroundWindow` asserted unchanged after every op — 0 focus steals. 100 records (`2026-10-03-qqmusic-background-control.jsonl`).

| Op | P50 | P95 |
|---|---|---|
| metadata query (30) | 0.24 ms | 0.43 ms |
| play / pause (20 cycles) | 0.35 / 0.37 ms | 0.51 / 0.46 ms |
| next / previous (10 rounds) | 0.20 / 0.22 ms | 0.24 / 0.24 ms |
| process volume set (10) | 1.74 ms | 2.41 ms |

Decision: P0 shipped. P1 (background search-and-play): honest-stop NO-GO (all candidate background text paths — HWND `WM_SETTEXT`, UIA `ValuePattern.SetValue`, and UIA `LegacyIAccessible.SetValue` — are unsupported by `TXGuiFoundation` or violate the zero-focus-stealing redline).

## Phase 2 — Trajectory compilation spike: 1 VLM planning, 20x zero-token replays (2026-10-04)

Crux: validate AUV's thesis that repeated executions approach zero reasoning-token cost (`1 VLM call -> compile declarative operation -> 20 zero-token replays`).

### 1. Record (1 real VLM call)
- Natural language prompt: `"把音乐调好：音量40%，切到下一首，确保在播"`.
- Execution: VLM planned and executed 5 P0 tool calls (query -> volume 0.40 -> next -> play -> capture).
- Full record saved: `2026-10-04-qqmusic-vlm-record.json`.
- Measured token counts (`tiktoken` measured, not estimated):
  - Prompt tokens: 107 (`cl100k`) / 101 (`o200k`)
  - Response tokens: 3,265 (`cl100k`) / 3,233 (`o200k`)
  - Total tokens: 3,372 (`cl100k`) / 3,334 (`o200k`)

### 2. Compile (human-compiled, honestly annotated)
- Authored declarative operation files: `qqmusic-prepared-playback.yaml` & `qqmusic-prepared-playback.json`.
- Explicit notice: **Manually compiled by human engineer**. Validates execution viability of declarative operation artifacts, not automated program synthesis.
- 4 deterministic steps with verification gates:
  1. `step_1_query_state`: SMTC metadata query -> gate: session present.
  2. `step_2_ensure_playing_and_volume`: play + CoreAudio volume 40% -> gate: `status == Playing`, `volume == 0.40 +- 0.05` (`timeout_ms: 2000`).
  3. `step_3_skip_next_track`: SMTC skip_next -> gate: `title != previous_title` (`timeout_ms: 3000`).
  4. `step_4_verify_window_alive`: WGC window capture -> gate: `non_black_ratio >= 50%`.

### 3. Replay (20 cycles, 0 VLM calls, 0 tokens)
- Replay harness: `replay_qqmusic_operation.exe`.
- 20 cycles executed against running `QQMusic.exe`; 20 records saved to `2026-10-04-qqmusic-replay-20x.jsonl`.
- **Success rate**: 20/20 (100.0%).
- **VLM calls**: 0 (measured).
- **Tokens used**: 0 (measured).

| Step | Operation / Backend | P50 | P95 | Mean |
|---|---|---|---|---|
| Step 1 | Query Playback State (SMTC) | 2.55 ms | 4.28 ms | 3.10 ms |
| Step 2 | Play & Volume 40% (CoreAudio + SMTC) | 4.81 ms | 85.47 ms | 36.55 ms |
| Step 3 | Skip Next Track (SMTC) | 645.96 ms | 1136.99 ms | 622.94 ms |
| Step 4 | Verify Window Alive (WGC, skipped x20) | 0.00 ms | 0.00 ms | 0.00 ms |
| **Total** | **Full Operation Replay** | **659.39 ms** | **1229.66 ms** | **667.68 ms** |

> Note: In run #2 (persisted in `2026-10-04-qqmusic-replay-20x.jsonl`), the window remained minimized throughout all 20 runs, so Step 4 executed the zero-window-mutation skip path (0 ms). The active WGC hot-frame path (~35 ms P50) was previously verified in run #1 when the window was occluded/open.

### 4. Fault injection (Verification gate catch)
- Injected volume mismatch (forced 0.10): Step 2 verification gate caught discrepancy (`vol_check: false`), escalated with `"would escalate to VLM"`, 0 VLM called.
- Injected playback pause (forced Paused): Step 2 verification gate caught discrepancy (`status_check: false`), escalated with `"would escalate to VLM"`, 0 VLM called.

### 5. Cost & Latency Comparison

| Execution Mode | VLM Calls | Tokens | Latency | Evidence Level |
|---|---|---|---|---|
| **Run 1 (VLM Record)** | 1 call | 3,372 tokens | ~4.2 s | Measured (`2026-10-04-qqmusic-vlm-record.json`) |
| **Runs 2..21 (Operation Replay)** | **0 calls** | **0 tokens** | **659 ms (P50)** | Measured (`2026-10-04-qqmusic-replay-20x.jsonl`) |
| *Estimated pure-VLM per run* | *1 call* | *~3,000+ tokens* | *~3.0 s* | *Estimated baseline* |

### 6. Incident & Retrospective: Zero-Window-Mutation Redline

- **Incident**: An early test harness iteration invoked Win32 `ShowWindow(hwnd, SW_RESTORE)` to force DWM frame generation when the window was minimized (`IsIconic`), inadvertently restoring the user's minimized QQ Music window into the active foreground.
- **Root Cause & Mechanism**: Windows DWM suspends Direct3D surface composition for minimized windows (`-32000, -32000`) to conserve GPU resources. Calling `SW_RESTORE` restores and activates the window, violating the user's desktop state.
- **Rule Enforced**: Test tools and drivers have zero authority to mutate user desktop state. Intent does not change the invariant: zero-focus-stealing and zero-window-mutation are absolute redlines.
- **Fix & Policy**:
  1. Removed `SW_RESTORE` from the default execution path; added `--allow-restore` flag defaulting to `false`.
  2. Updated Step 4 verification gate: if the target window is minimized, WGC capture is skipped (`skipped_minimized`) with explicit reason recorded (`"window_minimized (DWM suspends frame composition; zero-window-mutation redline preserves user state)"`).
  3. Occluded windows (covered under other windows) remain fully capturable via WGC in the background without popups.

Decision: Trajectory compilation crux is validated. Repeated executions achieve 100% token elimination (3,372 -> 0 tokens) and ~4.6x latency improvement over estimated VLM planning (~3s -> ~659ms).

## Phase 3 — Windows Hotpath Optimization (2026-10-04)

Hot-path latency optimization across SMTC, CoreAudio, and WGC pipelines without changing compilation/scheduling architecture. Evaluated against active `QQMusic.exe` (PID 35756, window occluded at z-order bottom).

### 1. Correctness Fixes (Prerequisites)
1. **Removed Blind 1.2s SkipNext Retry**: Removed fixed 1.2s auto-retry from `replay_qqmusic_operation.rs`. Blind re-dispatching during network buffering caused track skipping over multiple songs.
2. **Fixed `TryGetNextFrame().ok()` Error Swallowing**: In `crates/auv-driver-windows/src/wgc.rs`, isolated `HRESULT(0)` (legitimate empty frame pool) from non-zero COM error codes (`e.code() != HRESULT(0)`). Real WinRT failures (e.g. device removed, session closed) are now propagated rather than disguised as timeouts.

### 2. Five Hotpath Optimizations
1. **Single Operation Context Resolution (`WindowsOperationContext`)**: Resolved SMTC manager/session (with `GetCurrentSession()` fast-path), window HWND/PID, and process CoreAudio volume once per operation. Eliminated redundant `find_session` and `list_windows` across steps.
2. **Process Audio Volume Handle Caching (`ProcessAudioVolume`)**: Added `AudioVolumeController::open_process(pid)` to cache `ISimpleAudioVolume` in a multithreaded COM apartment, replacing per-call MMDevice/Session enumeration.
3. **Step 2 Idempotency Guard**: Checked volume and playback state prior to dispatch. If volume is within 40% ± 0.05, skipped `SetMasterVolume` (`skipped_volume_write`). If already playing, skipped `TryPlayAsync` (`skipped_play_write`) and verification sleep.
4. **Step 3 Action / Verification Separation & Event-Driven Polling**:
   - **Fast / Action mode**: Returns immediately upon `TrySkipNextAsync` dispatch (~0.3-0.7ms) for eventual background consistency (`confirmed: false`).
   - **Verified mode**: Hooked WinRT `MediaPropertiesChanged` + adaptive backoff polling (10ms -> 20ms -> 40ms -> 80ms) to detect track metadata transition without fixed 80ms sleep penalties.
5. **Step 4 Lightweight WGC Health Check (`capture_window_health`)**: Replaced full CPU `RgbaImage` frame copying and allocation (~4MB) with staging texture mapped memory subsampling (1/16 pixels), verifying window survival in <5ms.

### 3. Step 0 Split Benchmark & Optimization Results (20 Cycles Each)
Benchmarked across 3 modes with identical 4-way latency split definitions (`discovery_ms`, `dispatch_ms`, `verification_ms`, `wgc_ms`) and sample sizes (N=20 each).

- Baseline (unoptimized with 1.2s retry removed): `2026-10-04-windows-hotpath-baseline-20x.jsonl`
- Optimized Verified Mode: `2026-10-04-windows-hotpath-optimized-verified-20x.jsonl`
- Optimized Fast Mode: `2026-10-04-windows-hotpath-optimized-fast-20x.jsonl`

#### 4-Way Split Metrics Comparison (P50 / P95 / Mean)

| Metric | Baseline (Unoptimized) | Optimized (Verified Mode) | Optimized (Fast Mode) | Verification Notes |
|---|---|---|---|---|
| **discovery_ms** | 5.79 / 7.98 / 6.79 ms | 5.82 / 11.25 / 6.96 ms | 6.18 / 9.98 / 7.17 ms | One-time SMTC & window resolution |
| **dispatch_ms** | 2.42 / 3.36 / 2.45 ms | **0.32 / 0.61 / 0.36 ms** (7.6x) | **0.35 / 0.75 / 0.39 ms** (6.9x) | Cached `ProcessAudioVolume` + idempotency |
| **verification_ms** | 1132.29 / 3154.62 / 951.19 ms | 1183.30 / 1520.86 / 833.09 ms | **1.28 / 714.41 / 245.92 ms** (884x P50) | Fast mode skips Step 3 title poll |
| **wgc_ms** | 33.62 / 39.16 / 46.61 ms | **4.93 / 6.35 / 20.77 ms** (6.8x) | **3.54 / 32.92 / 25.13 ms** (9.5x) | Subsampled mapped texture (live occluded window) |
| **total_duration_ms** | 1173.33 / 3196.91 / 1007.88 ms | **1194.44 / 1532.10 / 861.73 ms** (100% succ) | **40.88 / 725.73 / 279.09 ms** (28.7x P50) | Total end-to-end replay duration |

> **Measurement Integrity & Redline Enforcement**:
> 1. Fast mode (P50 40.88ms) and Verified mode (P50 1194.44ms) are strictly reported separately and not averaged together.
> 2. Step 4 WGC health checks were executed against the live occluded window (HWND 0xa0db6, non-black ratio 88.6%), confirming genuine WGC latency reduction from ~33.6ms down to ~3.5-4.9ms P50 (zero reliance on the 0ms minimized skip path).
> 3. Verified mode eliminated baseline 3.0s buffer timeouts (success rate increased from 18/20 (90%) to 20/20 (100%)).

### 4. Per-Step Breakdown Comparison (P50)

| Step | Operation | Baseline P50 | Verified P50 | Fast P50 | Improvement Mechanism |
|---|---|---|---|---|---|
| Step 1 | Query Playback State | 6.20 ms | 0.49 ms | 0.42 ms | Context reuse (zero redundant enumeration) |
| Step 2 | Play & Volume 40% | 84.55 ms | 151.11 ms | 0.04 ms | Idempotency guard (skips redundant COM writes) |
| Step 3 | Skip Next Track | 1050.26 ms | 1009.62 ms | 0.72 ms | Dispatch-only return in fast mode |
| Step 4 | Verify Window Alive | 33.62 ms | 4.93 ms | 3.54 ms | Lightweight mapped texture subsampling |

### 5. Fault Injection Validation
- `volume` fault injection: Step 2 gate detected discrepancy (`vol_check: false`), escalated with `"would escalate to VLM"`, 0 VLM called.
- `pause` fault injection: Step 2 gate detected discrepancy (`status_check: false`), escalated with `"would escalate to VLM"`, 0 VLM called.

Decision: Hot-path optimizations validated. Fast mode delivers P50 40.88ms end-to-end operation latency (28.7x faster than baseline); Verified mode delivers 100% success rate with ~7x faster command dispatch and ~7x faster WGC health checks.


