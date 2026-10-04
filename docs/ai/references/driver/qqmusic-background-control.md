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

## Phase 3 — Auto double-loop v0.1: Optimistic compilation + Pessimistic execution (2026-10-04)

Built the compiler and scheduler architecture (`crates/auv-auto-loop`) for the full dual-loop auto mode. Separated completely from Windows hot-path optimizations to ensure isolated verification and measurable performance comparison.

### 1. Design & Core Principles
- **Optimistic compilation + pessimistic execution**: The machine does not require human per-operation approval; human engineers design guardrails and inspect the exception backlog. Problems that machines cannot solve reliably (implicit mutations, single-trajectory generalization, async side-effects) are bypassed via guardrails.
- **Zero silent errors invariant**: Every automated decision (approval, rejection, exact hit, candidate interception, gate failure, auto-isolation, escalation) emits a structured `DecisionLog` entry with a typed `ReasonCode`.

### 2. Verified Subsystems & Empirical Test Suite

Verified across 8 automated acceptance tests (`crates/auv-auto-loop/tests/acceptance_test.rs`):

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

### 3. Decisions & Handoff
- Clean trajectory (`2026-10-04-qqmusic-vlm-record.json`) achieves 100% automated compilation into an operation semantically consistent with manual YAML.
- Operations with unverified steps run in strict mode, preventing unverified mutations from degrading the production loop.
- Manual review queue operates out-of-band: backlog does not block fast-loop execution of active operations.


