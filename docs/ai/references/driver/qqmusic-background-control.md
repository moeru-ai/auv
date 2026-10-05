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


