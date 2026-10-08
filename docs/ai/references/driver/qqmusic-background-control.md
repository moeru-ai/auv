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

#### Percentile Methodology Disclosure
With $N=20$ samples, percentiles in the table below are computed using standard linear interpolation (`numpy.percentile(vals, p, method='linear')`), where median $P_{50} = \frac{\text{sorted}[9] + \text{sorted}[10]}{2}$.
- Under standard linear interpolation: Baseline Total P50 is **891.32 ms**; Fast Mode Total P50 is **36.82 ms** (speedup: **~24.2x**).
- Under nearest-rank / upper-median (`sorted[10]`): Baseline Total P50 is 1173.33 ms; Fast Mode Total P50 is 40.88 ms (speedup: **~28.7x**).
- Under nearest-rank lower (`sorted[9]`): Baseline Total P50 is 609.30 ms; Fast Mode Total P50 is 32.77 ms (speedup: **~18.6x**).
All three methods confirm order-of-magnitude acceleration (~19x to ~29x). The table below adopts the standard linear interpolation convention.

#### 4-Way Split Metrics Comparison (Linear P50 / Linear P95 / Mean)

| Metric | Baseline (Unoptimized) | Optimized (Verified Mode) | Optimized (Fast Mode) | Verification Notes |
|---|---|---|---|---|
| **discovery_ms** | 5.76 / 8.71 / 6.79 ms | 5.76 / 11.86 / 6.96 ms | 6.06 / 10.68 / 7.17 ms | One-time SMTC & window resolution |
| **dispatch_ms** | 2.42 / 3.37 / 2.45 ms | **0.31 / 0.63 / 0.36 ms** (7.8x) | **0.30 / 0.76 / 0.39 ms** (8.1x) | Cached `ProcessAudioVolume` + idempotency |
| **verification_ms** | 848.25 / 3154.69 / 951.19 ms | 1139.32 / 1527.20 / 833.09 ms | **1.00 / 746.55 / 245.92 ms** (848x P50) | Aggregate S1+S2+S3 verification (see note below) |
| **wgc_ms** | 33.37 / 51.95 / 46.61 ms | **4.89 / 22.43 / 20.77 ms** (6.8x) | **3.54 / 47.81 / 25.13 ms** (9.4x) | Subsampled mapped texture (live occluded window) |
| **total_duration_ms** | 891.32 / 3197.07 / 1007.88 ms | **1149.45 / 1538.37 / 861.73 ms** (100% succ) | **36.82 / 757.87 / 279.09 ms** (24.2x P50) | Total end-to-end replay duration |

> **Measurement Integrity & Metric Disclosures**:
> 1. **Fast vs Verified separation**: Fast mode (P50 36.82ms) and Verified mode (P50 1149.45ms) are strictly reported separately and not averaged together.
> 2. **Step 4 live window proof**: WGC health checks were executed against the live occluded window (HWND 0xa0db6, non-black ratio 88.6%), confirming genuine WGC latency reduction from ~33.4ms down to ~3.5-4.9ms P50 (zero reliance on the 0ms minimized skip path).
> 3. **Success rate**: Verified mode eliminated baseline 3.0s buffer timeouts (success rate increased from 18/20 (90%) to 20/20 (100%)).
> 4. **`verification_ms` aggregate definition & Fast P95 source**: `verification_ms` is the sum of verification durations across Step 1, Step 2, and Step 3. In Fast mode, Step 3 title change polling is bypassed entirely (P50 0.68ms, max 1.61ms). The Fast mode P95 of 746.55ms (upper-median 714.41ms) comes **entirely from Step 2's play state wait** (`TryPlayAsync` stream transition) when QQ Music happened to be paused between iterations. Skipping Step 3 title polling explains the 848x reduction at P50, but does not eliminate Step 2 playback waits at P95 when the player was not already playing.

### 4. Per-Step Breakdown Comparison (Linear P50)

| Step | Operation | Baseline P50 | Verified P50 | Fast P50 | Improvement Mechanism & Behavioral Analysis |
|---|---|---|---|---|---|
| Step 1 | Query Playback State | 6.11 ms | 0.48 ms | 0.42 ms | Context reuse (zero redundant enumeration) |
| Step 2 | Play & Volume 40% | 84.49 ms | 151.09 ms | 0.04 ms | Idempotency guard for volume (0ms writes). Verified P50 regression explained below. |
| Step 3 | Skip Next Track | 566.31 ms | 544.46 ms | 0.68 ms | Dispatch-only return in fast mode; adaptive backoff polling in verified mode |
| Step 4 | Verify Window Alive | 33.37 ms | 4.89 ms | 3.54 ms | Lightweight mapped texture subsampling |

#### Step 2 Verified P50 Regression Analysis (84.49 ms -> 151.09 ms)
In Verified mode, Step 2 duration increased at P50 due to an explicit test condition and verification gate rigor difference:
- **Volume**: Idempotency was 100% effective (`skipped_volume_write: true` in 20/20 runs, 0ms volume write).
- **Play status**: After Step 3's track skip in the preceding iteration, QQ Music frequently transitioned to a buffering/paused inter-track state. Verified mode strictly required `PlaybackStatus::Playing` before proceeding, looping through adaptive polling intervals (71ms, 151ms, 231ms, 312ms, 472ms, 633ms).
- **Baseline difference**: Baseline used a loose single-check check that exited earlier (sometimes accepting intermediate states), whereas Verified mode waited for confirmed playback state, trading ~66ms median wait time for 100% confirmed end-to-end success (vs 90% in baseline).
- **Fast mode contrast**: In Fast mode, iterations where the track was already playing took **0.02–0.04 ms** (both volume and play write skipped), while iterations requiring play wait took 151–714 ms, exhibiting a clear bimodal distribution.

### 5. Fault Injection Validation
- `volume` fault injection: Step 2 gate detected discrepancy (`vol_check: false`), escalated with `"would escalate to VLM"`, 0 VLM called.
- `pause` fault injection: Step 2 gate detected discrepancy (`status_check: false`), escalated with `"would escalate to VLM"`, 0 VLM called.

Decision: Hot-path optimizations validated. Fast mode delivers P50 36.82ms end-to-end operation latency (24.2x faster than baseline under standard linear interpolation, ~19x nearest-rank, ~28.7x upper-median); Verified mode delivers 100% success rate with ~7.8x faster command dispatch and ~6.8x faster WGC health checks.

## Phase 4 — Auto double-loop v0.1: Optimistic compilation + Pessimistic execution (2026-10-04)

Built the compiler and scheduler architecture (`crates/auv-auto-loop`) for the full dual-loop auto mode. Separated completely from Windows hot-path optimizations to ensure isolated verification and measurable performance comparison.

### 1. Design & Core Principles
- **Optimistic compilation + pessimistic execution**: The machine does not require human per-operation approval; human engineers design guardrails and inspect the exception backlog. Problems that machines cannot solve reliably (implicit mutations, single-trajectory generalization, async side-effects) are bypassed via guardrails.
- **Zero silent errors invariant**: Every automated decision (approval, rejection, exact hit, candidate interception, gate failure, auto-isolation, escalation) emits a structured `DecisionLog` entry with a typed `ReasonCode`.

### 2. Verified Subsystems & Empirical Test Suite

Verified across 9 automated acceptance tests (`crates/auv-auto-loop/tests/acceptance_test.rs`):

| Subsystem | Mechanism | Verification & Decision Code | Status |
|---|---|---|---|
| **Clean Compilation** | Forward diff + backward slice + 3 compile gates | Compiled `record.json` into `qqmusic.prepare_playback` without human intervention (`COMPILATION_APPROVED`) | **PASS** |
| **Dirty Trajectory** | Ambiguous drop detection | Ambiguous background mutation rejected into manual review queue (`REJECT_AMBIGUOUS_DROPS`) | **PASS** |
| **Parameter Gate** | Anti-unification & atomic guards | `Next()` parameterization rejected (`REJECT_FORBIDDEN_PARAMETERIZATION`); `SetVolume(40/60)` lifted to `{{volume}}` (`PARAMETER_GATE_APPROVED`) | **PASS** |
| **Blast Radius Gate** | Action whitelist & destructive blacklist | Deletion action rejected into manual review queue (`REJECT_BLAST_RADIUS_VIOLATION`) | **PASS** |
| **Scheduler (Exact)** | Canonical operation key matching | `QQMusic.exe:prepare_playback` matched directly with 0 embedding calls (`EXACT_KEY_MATCH`) | **PASS** |
| **Scheduler (Fallback)** | Embedding top-3 + strict preconditions | Unknown long-tail query falls back to embedding top-3; false candidate intercepted by `App.ProcessName` (`PRECONDITION_MISMATCH`) | **PASS** |
| **Runtime Isolation** | Consecutive failure threshold (>= 2) | Injected faults trigger gate catch; 2 consecutive failures auto-isolate operation (`AUTO_ISOLATED_CONSECUTIVE_FAILURES`); routes task to VLM (`ESCALATE_TO_VLM`) | **PASS** |
| **Strict Runtime Mode** | `unverified-step` fallback | Custom/uncovered step tagged `unverified-step`; 1st failure triggers instant escalation and isolation (`STRICT_STEP_FAILED`) | **PASS** |
| **Zero Silent Errors** | Structured decision logging | 100% of decisions across lifecycle logged with non-empty timestamps, task names, messages, and `ReasonCode` | **PASS** |
| **Isolation Persistence** | Durable JSONL records & reboot reload | Auto-isolated operations persisted to disk; reloaded on catalog restart; active registration rejected to prevent bad operation revival | **PASS** |

### 3. Decisions & Handoff
- Clean trajectory (`2026-10-04-qqmusic-vlm-record.json`) achieves 100% automated compilation into an operation semantically consistent with manual YAML.
- Operations with unverified steps run in strict mode, preventing unverified mutations from degrading the production loop.
- Manual review queue operates out-of-band: backlog does not block fast-loop execution of active operations.
- Isolation state persists across restarts to prevent revived faulty operations.

## Phase 5 — Verified-path profiling & conditional optimization (2026-10-06)

Granular profiling across 10 non-overlapping execution metrics and 4 operational dimensions (Cache: Cold/Warm, Mode: Fast/Verified/Baseline, Window: Active/Minimized, Frame: Fresh/Stale/Error), validating driver-side hot-path optimizations and Step 2 idempotency command counts.

### 1. Granular Timing Benchmarks (Standard Linear Interpolation)

Empirical measurements gathered across 80 real operational executions against QQ Music on Windows 11:
- **Warm Suites** ($N=20$ each): Baseline, Fast, Verified (`2026-10-06-windows-verify-warm-*.jsonl`)
- **Cold Suites** ($N=10$ independent processes each): Cold Fast, Cold Verified (`2026-10-06-windows-verify-cold-*.jsonl`)

All percentiles computed using standard linear interpolation ($R$ type 7 / numpy default `method='linear'` without outlier trimming).

#### Table 1.1: Warm Runs ($N=20$) Benchmark: Baseline vs Verified vs Fast

| Metric (ms) | Baseline (P50 / P95 / Mean) | Verified (P50 / P95 / Mean) | Fast (P50 / P95 / Mean) | Fast vs Baseline (P50 / Mean) | Description |
|---|---|---|---|---|---|
| **Total Duration** | **1189.18 / 3273.47 / 1025.83** | **1260.19 / 3096.35 / 1036.34** | **25.23 / 1455.49 / 288.41** | **47.1x / 3.56x** | End-to-end operation execution latency |
| `discovery_ms` (sum) | 5.99 / 9.52 / 7.13 | 4.73 / 34.45 / 7.98 | 4.99 / 14.19 / 6.35 | 1.2x / 1.12x | Total resource discovery duration |
| `manager_discovery_ms` | 3.32 / 4.89 / 3.86 | 3.30 / 7.81 / 4.35 | 3.29 / 8.66 / 4.14 | 1.0x / 0.93x | WinRT SMTC session manager acquisition |
| `session_discovery_ms` | 0.00 / 0.01 / 0.01 | 0.00 / 0.01 / 0.01 | 0.00 / 0.01 / 0.01 | 1.1x / 1.00x | `GetCurrentSession()` fast-path vs `GetSessions()` |
| `window_discovery_ms` | 0.61 / 0.91 / 0.64 | 0.54 / 1.34 / 0.69 | 0.53 / 1.04 / 0.66 | 1.2x / 0.97x | QQ Music HWND enumeration and resolution |
| `audio_lookup_ms` | 2.33 / 5.08 / 2.62 | 0.70 / 15.87 / 2.93 | **0.53 / 8.23 / 1.55** | **4.4x / 1.69x** | CoreAudio endpoint + session lookup (cached) |
| `volume_rw_ms` | 2.18 / 3.97 / 2.72 | 151.50 / 237.90 / 152.00 | **0.00 / 1444.75 / 258.33** | — | Step 2 volume read/write duration |
| `dispatch_ms` | 2.52 / 4.43 / 3.07 | 151.89 / 238.46 / 152.36 | **0.37 / 1445.12 / 258.64** | **6.8x / 0.01x** | WinRT / SMTC command dispatch latency |
| `verification_ms` | 1174.07 / 3259.42 / 992.04 | 1112.18 / 3007.56 / 855.04 | **0.44 / 2.63 / 0.69** | **2672.3x / 1437.7x** | Semantic title / identity verification |
| `wgc_ms` | 7.13 / 25.82 / 22.87 | 4.14 / 22.50 / 19.01 | **3.17 / 40.46 / 22.00** | **2.2x / 1.04x** | WGC window health verification |
| `serialization_ms` | 0.01 / 0.02 / 0.01 | 0.01 / 0.02 / 0.01 | 0.01 / 0.02 / 0.01 | 1.0x / 1.00x | In-memory JSON serialization |
| `pacing_ms` (isolated) | 50.00 / 50.00 / 50.00 | 50.00 / 50.00 / 50.00 | 50.00 / 50.00 / 50.00 | — | Inter-iteration pacing (strictly excluded) |

#### Table 1.2: Cold Runs ($N=10$, Independent Processes) Benchmark

| Metric (ms) | Cold Fast (P50 / P95 / Mean) | Cold Verified (P50 / P95 / Mean) | Description |
|---|---|---|---|
| **Total Duration** | **321.25 / 466.00 / 344.46** | **391.88 / 419.11 / 395.74** | End-to-end operation execution latency |
| `discovery_ms` (sum) | 23.77 / 26.46 / 23.83 | 25.89 / 31.10 / 26.20 | Total cold discovery duration |
| `manager_discovery_ms` | 14.79 / 18.01 / 15.20 | 15.17 / 16.75 / 15.09 | WinRT SMTC session manager acquisition |
| `session_discovery_ms` | 0.01 / 0.02 / 0.01 | 0.01 / 0.02 / 0.01 | `GetCurrentSession()` fast-path |
| `window_discovery_ms` | 0.76 / 1.02 / 0.81 | 0.80 / 1.07 / 0.86 | QQ Music HWND enumeration and resolution |
| `audio_lookup_ms` | 7.77 / 8.53 / 7.82 | 9.70 / 14.50 / 10.22 | Cold CoreAudio endpoint enumeration (10/10 miss) |
| `volume_rw_ms` | 0.00 / 127.26 / 23.14 | 0.00 / 0.00 / 0.00 | Step 2 volume read/write duration |
| `dispatch_ms` | 0.24 / 127.59 / 23.39 | 0.27 / 0.46 / 0.31 | WinRT / SMTC command dispatch latency |
| `verification_ms` | 2.87 / 6.38 / 3.50 | **75.29 / 92.39 / 77.07** | Step 3 verification (10/10 confirmed on fast path) |
| `wgc_ms` | **286.25 / 332.18 / 292.86** | **292.12 / 301.07 / 291.30** | Cold first-frame D3D11 / WGC initialization |
| `serialization_ms` | 0.02 / 0.03 / 0.02 | 0.02 / 0.03 / 0.02 | In-memory JSON serialization |
| `pacing_ms` (isolated) | 0.00 / 0.00 / 0.00 | 0.00 / 0.00 / 0.00 | Independent process runs (0 pacing) |

#### 2. Key Findings & Empirical Reality Disclosures

1. **Verification Latency is Bimodal, NOT a "Fixed ~1.1s Ceiling"**:
   - In Verified Warm mode ($N=20$), `verification_ms` exhibits a clear **bimodal distribution**:
     - **Fast Path Peak (45%, 9/20 runs)**: Confirmed in **65.6ms ~ 92.8ms** (P50 72.0ms). When QQ Music's playback state is primed, SMTC change events fire promptly, verifying identity in <100ms.
     - **Slow Path Peak (45%, 9/20 runs)**: Confirmed in **1105.7ms ~ 1185.9ms** (P50 1145.8ms). QQ Music internal audio buffering / SMTC event delay produces ~1.15s latency.
     - **Timeout Tail (10%, 2/20 runs)**: Timed out at **3003.5ms and 3084.0ms** waiting for metadata changes.
   - **Cold Verified Proof**: In Cold Verified mode ($N=10$, independent runs), **100% (10/10) of executions confirmed on the Fast Path** (P50 75.29ms, range 65.85ms ~ 98.28ms, zero occurrences of ~1.1s).
   - **Correction**: We retract the assertion of a "fixed ~1.1s physical hardware/engine ceiling". The latency depends strictly on QQ Music's internal playback/buffering state and SMTC event scheduling. Future optimization can investigate the exact condition that triggers the ~70ms fast path.

2. **Fast Mode 40% Tail Latency Disclosure**:
   - Fast mode achieves a P50 of **25.23ms** (12/20 runs finished in 7.14ms ~ 30.8ms).
   - However, **P95 is 1455.49ms, and Mean is 288.41ms**. 8 out of 20 runs (40%) exceeded 100ms (max 1606.66ms).
   - **Root Cause**: In those 8 runs, QQ Music was not in `Playing` state, triggering `play_calls = 1`. Calling WinRT `session.play()` and waiting for playback status caused `volume_rw_ms` / `dispatch_ms` to block for ~1.4s–1.6s.
   - **Narrative Clarification**: "Instantaneous command-like response (~25ms)" holds true when playback is already active and within volume tolerance; when player state recovery (`Play()`) is needed, tail latency is bounded by WinRT playback state transition.

3. **CoreAudio Cached Lookup Hit Rate**:
   - In Cold runs: 10/10 were `miss` (P50 7.77ms), requiring full device enumeration.
   - In Warm runs: Run 1 was `miss` (initial resolution), runs 2–20 achieved **19/19 hits (100% cache hit rate)** with P50 **0.53ms** (14.7x speedup vs cold).
   - `endpoint_count` was consistently 1. Both `audio_lookup_status` and `audio_endpoint_count` are now surfaced as top-level fields in `ReplayRecord`.

4. **Track Identity Defensive Line Status**:
   - Across all 80 empirical runs against live QQ Music, metadata was always complete (non-empty title, artist, and album), resulting in `identity_level = "full"` for all optimized runs (baseline only sampled title, so `"title_only"`).
   - `indeterminate` (handling identical tracks in single-track loops, missing position reset, or empty metadata) was not triggered in live runs and is currently verified by 7 dedicated unit tests in `crates/auv-driver-windows/src/track_identity.rs`.

---

### 3. Decision Record & Trigger Evaluations

#### Phase 1: Real Profiling & Granular Separation (Implemented)
- Established a 10-field granular timing schema and captured 4 profiling dimensions across 80 execution records.
- Isolated `pacing_ms` strictly from `total_duration_ms`.
- Standardized linear interpolation percentiles across all benchmarks with zero outlier filtering.

#### Phase 2: Verified Wait Refinement (Implemented Driver Components; Bimodal Disclosed)
- **Phase 2A (Track Identity)**:
  - Implemented `TrackIdentity` tuple `(title, artist, album_title, album_artist)` with normalization (whitespace collapsing, full-width ASCII conversion, unicode case-folding).
  - Implemented explicit degradation ladder: `full` $\to$ `partial` $\to$ `title_only` $\to$ `indeterminate`.
  - Enforced strict repeated-track invariant: Identical title and artist without external disambiguation (position reset) degrades to `indeterminate` and returns `confirmed: false` (never forced success).
  - Validated via 7 unit tests. Live runs observed 100% `full` identity.
- **Phase 2B (Two-Stage Verification)**:
  - Added `PlaybackInfoChanged` auxiliary wakeup alongside `MediaPropertiesChanged`.
  - Step 3 waits for event signals before pulling full metadata, eliminating blind polling.
  - Bimodal latency documented: fast path (~70ms) vs slow path (~1150ms).

#### Phase 3: CoreAudio Cold Path (Implemented & Verified)
- Implemented context-level endpoint caching (`AudioVolumeController::open_process_cached`) prioritizing the previously resolved endpoint.
- Validates the endpoint's validity before reusing; falls back to default multimedia, communications, and active endpoints on invalidation.
- Emits structured `AudioLookupStats` with `status: hit | miss | invalidated`, `endpoint_count`, and `session_count`.
- Surfaced `audio_lookup_status` and `audio_endpoint_count` in top-level JSONL records.
- **Zero Global COM Leaks**: Scoped strictly within `WindowsOperationContext`; zero process-global static COM state.
- **Result**: Reduced warm audio resolution latency from $7.8\,\text{ms}$ cold to **$0.53\,\text{ms}$ warm** ($14.7\times$ speedup, 19/19 hits).

#### Phase 4: Step 2 Idempotency Command Counting Proof (Implemented & Formally Proven)
- Built `crates/auv-driver-windows/src/playback_guard.rs` with `Step2Executor`, `PlaybackActionSink`, and `RealPlaybackSink`.
- Evaluates pre-state: checks volume against target within $\pm 0.05$ tolerance and checks playback status against `Playing`.
- **Proof of Zero Commands**:
  - `SetMasterVolume` calls: **0 calls across 20 warm fast runs (20/20 skipped, 100%)**, **0 calls across 20 warm verified runs (20/20 skipped, 100%)**, **0 calls across 10 cold fast runs (10/10 skipped, 100%)**, **0 calls across 10 cold verified runs (10/10 skipped, 100%)**. Total: **60/60 runs (100%) with 0 volume writes**.
  - 9 automated unit tests in `playback_guard::tests` assert exact invocation counts under tolerance boundary conditions ($0.43 \to 0$ calls, $0.46 \to 1$ call).

#### Phase 5: WGC Three Paths (Evaluated & Active Window Verified)
- Active window WGC health check latency is **$3.17\,\text{ms}$ P50** under lightweight memory subsampling (`capture_window_health`), well within the 5ms budget.
- Minimized window paths cleanly emit `skipped_minimized` without window restores (`SW_RESTORE` redline strictly enforced).

#### Phase 6: Batch Logging & Serialization (Implemented & Verified)
- In-memory collection of `ReplayRecord` during execution loops; single batch flush to disk at run completion ($1.59\,\text{ms}$ for 20 records).
- Serialization latency measured at **$0.01\,\text{ms}$ P50** ($0.04\%$ of Fast total).

---

### 4. Archived Artifacts
- `docs/ai/references/driver/2026-10-06-windows-verify-warm-fast-20x.jsonl`: 20x warm Fast mode records (P50 25.23ms, P95 1455.49ms, Mean 288.41ms, 100% success, 0 VLM).
- `docs/ai/references/driver/2026-10-06-windows-verify-warm-verified-20x.jsonl`: 20x warm Verified mode records (P50 1260.19ms, P95 3096.35ms, Mean 1036.34ms, 90% success, 100% full track identity).
- `docs/ai/references/driver/2026-10-06-windows-verify-warm-baseline-20x.jsonl`: 20x warm Baseline mode records (P50 1189.18ms, P95 3273.47ms, Mean 1025.83ms, 0/20 writes skipped).
- `docs/ai/references/driver/2026-10-06-windows-verify-cold-fast-10x.jsonl`: 10x independent process cold Fast runs (P50 321.25ms, P95 466.00ms, Mean 344.46ms).
- `docs/ai/references/driver/2026-10-06-windows-verify-cold-verified-10x.jsonl`: 10x independent process cold Verified runs (P50 391.88ms, P95 419.11ms, Mean 395.74ms).

---

## Phase 6 — Latency Tail Elimination, WGC Prewarming & Verified Path Spike (2026-10-07)

### 1. Objective & Scope

Branch: `perf/windows-latency-tail` (built upon `perf/windows-verify-path`).
Eliminate latency tails and cold startup bottlenecks across three distinct tracks:
1. **Item 1: Fast Mode Play Fire-and-Forget**: Eliminate the 40% tail where `Play()` blocked for ~1.5s waiting for playback status transitions in Fast mode.
2. **Item 2: Verified Fast-Path Spike**: Investigate the bimodal distribution of Step 3 verification (~70ms fast path vs ~1150ms slow path) across 4 dimensions and evaluate engineering feasibility.
3. **Item 3: Cold WGC Eager Prewarming & Device-Lost Resilience**: Move ~270ms of D3D11 device creation and initial capture negotiation out of operation execution into process startup, dropping Cold Fast P50 from ~321ms to <40ms.

---

### 2. Empirical Benchmark Data

All percentiles computed using **Standard Linear Interpolation** with zero outlier filtering.

#### Table 6.1: Warm Fast Latency Tail Elimination ($N=20$)

| Metric (ms) | Verify-Path Fast (P50 / P95 / Mean) | Tail-Eliminated Fast (P50 / P95 / Mean) | Speedup / Tail Reduction | Description |
|---|---|---|---|---|
| **Total Duration** | **25.23 / 1455.49 / 288.41** | **8.68 / 29.70 / 15.71** | **2.9x / 49.0x tail reduction** | End-to-end operation execution latency |
| `discovery_ms` (sum) | 4.99 / 14.19 / 6.35 | 4.22 / 19.26 / 6.05 | 1.2x | Total discovery phase |
| `manager_discovery_ms` | 3.29 / 8.66 / 4.14 | 2.93 / 15.95 / 4.27 | 1.1x | SMTC manager acquisition |
| `session_discovery_ms` | 0.00 / 0.01 / 0.01 | 0.01 / 0.03 / 0.01 | — | `GetCurrentSession()` fast path |
| `window_discovery_ms` | 0.53 / 1.04 / 0.66 | 0.66 / 1.33 / 0.84 | — | QQ Music HWND resolution |
| `audio_lookup_ms` | 0.53 / 8.23 / 1.55 | 0.55 / 1.09 / 0.93 | 1.0x / 7.6x | CoreAudio cached endpoint resolution |
| `volume_rw_ms` | **0.00 / 1444.75 / 258.33** | **0.26 / 0.42 / 0.24** | **3440x tail reduction** | Step 2 volume read/write & play dispatch |
| `dispatch_ms` | **0.37 / 1445.12 / 258.64** | **0.53 / 0.67 / 0.51** | **2157x tail reduction** | SMTC command dispatch latency |
| `verification_ms` | 0.44 / 2.63 / 0.69 | 0.47 / 0.83 / 0.66 | 0.9x | Fast mode verification (no wait) |
| `wgc_ms` | 3.17 / 40.46 / 22.00 | 2.71 / 9.12 / 7.85 | 1.2x / 4.4x | WGC window health check |
| `wgc_init_ms` | — | 256.25 / 256.25 / 256.25 | — | Process startup prewarm duration |
| `serialization_ms` | 0.01 / 0.02 / 0.01 | 0.07 / 0.11 / 0.08 | — | In-memory JSON serialization |
| `pacing_ms` (isolated) | 50.00 / 50.00 / 50.00 | 50.00 / 50.00 / 50.00 | — | Inter-iteration pacing (strictly excluded) |

**Command Counting & Idempotency Proof**:
- `SetMasterVolume` calls: **0 total across 20 runs (skipped: 20/20, 100%)**.
- `Play` calls: **16 calls dispatched across 20 runs (skipped: 4/20)**; `play_dispatched = true` in 16/16.
- **Zero Blocking**: In all 16 `Play()` calls, fire-and-forget returned immediately after WinRT async dispatch without awaiting `MediaPlaybackStatus::Playing`, completely eliminating the 1.5s blocking tail.

---

#### Table 6.2: Cold Fast Process Startup & Execution ($N=10$, Independent Processes)

| Metric (ms) | Cold Fast Baseline (P50 / P95 / Mean) | Cold Fast with Prewarm (P50 / P95 / Mean) | Speedup | Description |
|---|---|---|---|---|
| **Total Duration** | **321.25 / 466.00 / 344.46** | **24.44 / 41.59 / 27.52** | **13.1x faster** | End-to-end operation execution latency |
| `discovery_ms` (sum) | 23.77 / 26.46 / 23.83 | 17.22 / 20.11 / 17.59 | 1.4x | Total cold discovery duration |
| `manager_discovery_ms` | 14.79 / 18.01 / 15.20 | 6.99 / 8.98 / 7.30 | 2.1x | SMTC manager acquisition |
| `session_discovery_ms` | 0.01 / 0.02 / 0.01 | 0.01 / 0.02 / 0.01 | — | `GetCurrentSession()` fast path |
| `window_discovery_ms` | 0.76 / 1.02 / 0.81 | 0.99 / 1.41 / 1.03 | — | QQ Music HWND resolution |
| `audio_lookup_ms` | 7.77 / 8.53 / 7.82 | 9.38 / 10.26 / 9.25 | — | Cold CoreAudio endpoint enumeration |
| `volume_rw_ms` | 0.00 / 127.26 / 23.14 | 0.00 / 0.00 / 0.00 | — | Step 2 volume read/write |
| `dispatch_ms` | 0.24 / 127.59 / 23.39 | 0.26 / 0.59 / 0.32 | — | Command dispatch latency |
| `verification_ms` | 2.87 / 6.38 / 3.50 | 2.89 / 5.28 / 3.35 | — | Step 3 dispatch-only verification |
| `wgc_ms` (in-operation) | **286.25 / 332.18 / 292.86** | **2.51 / 16.83 / 5.19** | **114.0x faster** | WGC window health check |
| `wgc_init_ms` (startup) | — | **269.60 / 300.63 / 274.18** | — | Process startup D3D11 & session prewarm |
| `serialization_ms` | 0.02 / 0.03 / 0.02 | 0.02 / 0.03 / 0.02 | — | In-memory JSON serialization |
| `pacing_ms` (isolated) | 0.00 / 0.00 / 0.00 | 0.00 / 0.00 / 0.00 | — | Independent process runs (0 pacing) |

**Prewarm Result**:
- D3D11 context creation (~200ms) + initial frame pool and capture session negotiation (~70ms) are completely moved to process startup via `prewarm_wgc()` and `prewarm_wgc_window()`.
- Cold Fast operation execution drops from 321.25ms to **24.44ms P50**, significantly outperforming the ~40ms brief target.

---

### 3. Item 2 Verified Fast-Path Trigger Spike Summary

Full Spike Report: [`2026-10-07-verified-fast-path-spike.md`](2026-10-07-verified-fast-path-spike.md)  
Data File: [`2026-10-07-spike-verify-experiments.jsonl`](2026-10-07-spike-verify-experiments.jsonl) ($N=96$ live single-variable runs).

1. **Physical Cause of Bimodal Distribution**:
   - The primary controlling variable is **Skip Interval** (time delta between consecutive track changes):
     - **$<200\text{ms}$ interval** (e.g. 50ms test pacing): QQ Music enters an alternating limit-cycle oscillator: `Fast (70ms) -> Slow (1150ms) -> Fast (70ms) -> Slow (1150ms)`, yielding exactly 50% fast path rate.
     - **$\ge 1.0\text{s}$ interval** (1s, 5s, 30s): Audio demuxing and decoding pipeline stabilizes, achieving **91.7% to 100% fast path rate (P50 77.1ms ~ 79.8ms)**.
2. **Cold Launch & Position Hypotheses Refuted**:
   - Fresh `QQMusic.exe` process launch: First skip achieved 80% (4/5) fast path (<99ms).
   - Playback position: Skipping at 2s into song achieved **100% fast path** ($N=10$, P50 47.8ms); skipping at 30s and 60s also achieved **100% fast path** (P50 53.9ms and 64.8ms). Track change speed is independent of prebuffering at song end.
3. **Event Timing Deltas**:
   - `PlaybackInfoChanged` arrives consistently **15–50ms before** `MediaPropertiesChanged` on both fast and slow paths. Slow path delay is caused entirely by QQ Music's internal audio engine thread reset (~1050ms) before emitting events.
4. **Honest Engineering Verdict: 【NO-GO】**:
   - Waiting $\ge 800\text{--}1000\text{ms}$ in the driver to trigger the 70ms fast path yields total latency of $1000 + 75 = 1075\text{ms}$, which offers zero net gain over the native 1150ms slow path while penalizing throughput.
   - For real-world user / agent operations (interval $\ge 1\text{s}$), the ~75ms fast path occurs naturally. The adaptive event-driven early exit (`on_media_properties_changed` + `on_playback_info_changed` with exponential backoff) is architecture-optimal.

---

### 4. Item 3 Device-Lost Resilience Implementation

In `crates/auv-driver-windows/src/wgc.rs`:
- Replaced static immutable `OnceLock<D3dContext>` with thread-safe recoverable `RwLock<Option<Arc<D3dContext>>>`.
- Added detection for Win32/DXGI device-lost errors:
  - `DXGI_ERROR_DEVICE_REMOVED` (`0x887A0005`)
  - `DXGI_ERROR_DEVICE_RESET` (`0x887A0007`)
- Implemented automatic reset: on device-lost error, `reset_d3d_context()` clears the cached D3D11 device and active WGC session. Subsequent captures automatically recreate the device and session without panic, crash, or freeze.
- Unit tested via `test_device_lost_recovery_simulation` and `test_prewarm_wgc`.

---

### 5. Archived Artifacts (Phase 6)

- `docs/ai/references/driver/2026-10-07-windows-tail-warm-fast-20x.jsonl`: 20x warm Fast records with play fire-and-forget (P50 8.68ms, P95 29.70ms, Mean 15.71ms, 16 play calls dispatched without blocking).
- `docs/ai/references/driver/2026-10-07-windows-tail-cold-fast-10x.jsonl`: 10x independent cold process Fast runs with eager prewarm (P50 24.44ms, P95 41.59ms, Mean 27.52ms, `wgc_ms` P50 2.51ms).
- `docs/ai/references/driver/2026-10-07-spike-verify-experiments.jsonl`: 96x live single-variable runs across skip intervals, process lifecycles, playback positions, and event arrival deltas.
---

## Phase 7 — Compiled-Operation Execution-Mode Schema & Routing (2026-10-08)

### 1. Objective & Decision (Locked 2026-10-08)

- **Goal**: Introduce explicit execution mode semantics (`Fast` vs `Verified`) to `OperationDef` schema, compilation gates, catalog admission, scheduler matching, and runtime execution.
- **Fail-Closed Decision**: Operations lacking `execution_mode` fail immediately during compilation or catalog admission. Missing modes are **never defaulted to Verified** and **never silently filled**.
- **Schema Version**: Bumped to `auv.operation.v2`. Operations with legacy `auv.operation.v1` or missing versions are rejected fail-closed with `REJECT_SCHEMA_VERSION_MISMATCH`.

---

### 2. Architecture & Rules

#### 2.1 Schema Definition (`models.rs`)
- `ExecutionMode`: Enum `Fast` | `Verified`. Required on `OperationDef` without serde default.
- Rejection codes added:
  - `REJECT_MODE_UNDECLARED`: Operation definition lacks `execution_mode`.
  - `REJECT_MODE_CONFLICT`: Incompatible mode requested or derived for operation gates.
  - `REJECT_SCHEMA_VERSION_MISMATCH`: Operation schema version is not `auv.operation.v2`.
  - `MODE_MISMATCH_ESCALATE_VLM`: Fast mode requested for a Verified operation; intercepted to escalate to VLM with zero side-effects.

#### 2.2 Compilation Gates & Mode Derivation (`compiler/`)
- `derive_mode_from_steps`: Automatically infers operation mode from step gates. Steps containing `TitleChangeGate(require_title_change=true)`, `StatusAndVolumeGate`, or `UnverifiedFallback` require `Verified` mode.
- `evaluate_execution_mode_gate`:
  - Rejects `unverified-step` tag or `is_unverified: true` with `Fast` mode (`REJECT_MODE_CONFLICT`).
  - Rejects destructive high-risk actions (delete, format, drop) with `Fast` mode (`REJECT_BLAST_RADIUS_VIOLATION` / `REJECT_MODE_CONFLICT`).
  - Rejects `TitleChangeGate(require_title_change=true)` with `Fast` mode.

#### 2.3 Scheduler Matching & Zero Side-Effects Invariant (`scheduler/`)
- `TaskRequest` carries optional `requested_mode: Option<ExecutionMode>`.
- In `FastLoopScheduler::schedule`:
  - When exact key or candidate matches an operation requiring `Verified` mode while `requested_mode == Some(ExecutionMode::Fast)`, scheduler intercepts execution, logs `MODE_MISMATCH_ESCALATE_VLM`, and returns `selected_operation: None`.
  - **Zero Side-Effects Guarantee**: No commands or driver actions are dispatched.

#### 2.4 Catalog Admission (`catalog.rs`)
- `admit_operation_json` and `register_active` validate schema version `auv.operation.v2` and require `execution_mode`.
- Missing fields reject the operation into `ManualReviewItem` with `REJECT_MODE_UNDECLARED` and location tracking.

#### 2.5 Runtime Enforcement (`runtime/executor.rs`)
- `ExecutionResult::Success`: In `Fast` mode, `confirmed` is constructively clamped to `false`.
- `execute_fast`: Attempting to execute a `Verified` operation via the fast path fails immediately with `ModeConflictError` before calling action sinks.

---

### 3. Verification & Acceptance Criteria

All 8 acceptance criteria verified via 15 acceptance tests in `crates/auv-auto-loop/tests/acceptance_test.rs`:
1. **Missing `execution_mode`**: Serde deserialization fails, and catalog admission rejects with `REJECT_MODE_UNDECLARED` (`test_10`).
2. **Mode mismatch interception**: Requesting Fast for a Verified operation returns `None`, logs `MODE_MISMATCH_ESCALATE_VLM`, and dispatches zero commands (`test_11`).
3. **Fast mode confirmation invariant**: `ExecutionResult::confirmed()` is strictly `false` under Fast mode (`test_12`).
4. **High-risk actions barred from Fast**: Destructive actions fail compilation when targeted for Fast mode (`test_13`).
5. **Unverified step barred from Fast**: `unverified-step` tags with Fast mode are rejected during compilation (`test_14`).
6. **Schema v1 migration rejected**: Legacy `auv.operation.v1` operations are rejected with `REJECT_SCHEMA_VERSION_MISMATCH` (`test_15`).
7. **Existing regression coverage**: All 9 prior acceptance tests updated to `auv.operation.v2` and pass cleanly (`tests 1–9`).
