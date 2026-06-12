# Codex Handoff: osu! Benchmark Mainline

Date: 2026-06-12

Status: current handoff for the next coding agent before session compaction

Current HEAD when written: `WORKTREE_DIRTY_P2_PENDING_COMMIT`

## Start Here

Read these files first, in this order:

1. `AGENTS.md`
2. `CLAUDE.md`
3. `docs/TERMS_AND_CONCEPTS.md`
4. `crates/auv-game-osu/src/benchmark.rs`
5. `src/osu.rs`
6. `src/cli.rs`
7. `src/main.rs`
8. `crates/auv-driver/src/input.rs`
9. `crates/auv-driver-macos/src/session.rs`

## Current Goal

Active goal is still:

```text
AUV realtime benchmark lane for osu!, benchmark-first rather than YOLO-first
```

There is no new goal beyond this lane.

Current shape of the lane:

- `P0`: beatmap-driven offline scheduler benchmark — **done as merged skeleton**
- `P1`: typed macOS window dispatch benchmark mode — **done as merged slice**
- `P2`: capture / visual verification — **implemented locally and smoke-verified, pending commit**
- `P3`: YOLO/CV as independent validation channel — **not started**

## Current Repo State

Current branch state when written:

```text
main...origin/main
M crates/auv-game-osu/src/benchmark.rs
M crates/auv-game-osu/src/lib.rs
M docs/ai/references/2026-05-24-codex-handoff.md
M src/cli.rs
M src/main.rs
M src/osu.rs
```

Recent commits that matter:

```text
4d7f06a feat(osu): add typed dispatch benchmark mode
54394b4 feat(osu): add beatmap benchmark skeleton
d394430 fix(auv-game-balatro): accept main menu play restart button
a8dd2c5 docs: move superpowers specs to references
```

Before coding again, verify live state:

```bash
git status --short --branch
git log --oneline --decorate -5
```

When this handoff was written, the working tree was clean.

## What Was Completed In This Session

### P0 merged

Commit:

```text
54394b4 feat(osu): add beatmap benchmark skeleton
```

What it did:

- added `crates/auv-game-osu`
- added `rosu-map` based local `.osu` parsing
- generated deterministic action schedules from beatmap truth
- added dry-run timing benchmark output
- emitted artifacts:
  - `parsed_map_summary.json`
  - `action_schedule.json`
  - `dispatch_trace.json`
  - `latency_report.json`
- added CLI entry:

```text
auv-cli osu benchmark <beatmap.osu> [--output-dir <dir>]
```

### P1 merged

Commit:

```text
4d7f06a feat(osu): add typed dispatch benchmark mode
```

What it did:

- extended `RunMode` beyond `DryRun` to include typed dispatch
- extended benchmark inputs to carry:
  - target app
  - dispatch limit
- added typed macOS dispatch path through:

```text
MacosDriver::new()
  -> open_local()
  -> session.window().resolve(...)
  -> session.window().click(...)
```

- extended dispatch trace records with:
  - `delivery_path`
  - `fallback_reason`
- added CLI entry:

```text
auv-cli osu dispatch <beatmap.osu> --target-app <name> [--output-dir <dir>] [--dispatch-limit <n>]
```

### P2 local and smoke-verified

Commit:

```text
pending local commit
```

What it does:

- extends typed dispatch benchmark inputs with `capture_verify`
- captures window evidence around each dispatched action
- emits new artifacts:
  - `capture_trace.json`
  - `verification_summary.json`
  - staged `capture-object-*.png` frame evidence
- stages both JSON and PNG evidence into the normal recorded run artifact layout
- extends CLI entry:

```text
auv-cli osu dispatch <beatmap.osu> --target-app <name> [--output-dir <dir>] [--dispatch-limit <n>] [--capture-verify]
```

Smoke verification completed locally against installed `osu!.app` and a real local beatmap file.

- `src/osu.rs` wraps benchmark execution through `Runtime::run_recorded_operation(...)`
- artifacts are staged into the normal `.auv/runs/<run_id>/` layout
- inspect/read surfaces remain reusable without a special osu persistence path

This preserves the active AUV core lane instead of forking a private benchmark recorder.

## Verification Already Run

The following checks passed for the local P2 state:

```bash
cargo fmt --check
cargo check
cargo test
cargo build
git diff --check
cargo run -- help | rg "osu benchmark|osu dispatch"
cargo run -- osu benchmark <beatmap.osu> [--output-dir <dir>]
cargo run -- osu dispatch <local beatmap> --target-app "osu!" --dispatch-limit 1 --capture-verify --output-dir .tmp-osu-dispatch-p2
auv-cli inspect run_1781276425182_80682_0
```

## Collabi State

Collabi was used during this lane.

Active session:

```text
auv-game-osu-p0
```

Claim used for the owned path set:

```text
auv-game-osu-p0-impl
```

The session was updated after both merged slices.

## Current Boundaries

Still true:

- benchmark-first, not YOLO-first
- strongest available signal wins
- `.osu` beatmap truth remains the primary source for scheduling
- no online or ranked automation
- no memory reader dependency in the merged state
- capture verification now exists as a separate evidence channel around typed dispatch
- YOLO/CV control path still does not exist
- osu-specific logic remains in `crates/auv-game-osu`, not in generic core runtime modules

## Next Single Best Step

Do not open a new goal.

Next step is one of these, with `P2` currently the most direct forward move:

1. `P2`: add timestamped capture / visual verification around typed dispatch
2. alternatively thicken `P0` reporting/schema without changing the truth model

If continuing with `P2`, preserve these rules:

- capture is a separate evidence channel, not the first truth source
- keep typed dispatch as the execution path
- do not turn this into YOLO-first control

## Useful Mental Model

The lane now proves two different things in sequence:

```text
P0: can AUV derive a deterministic action timeline from structured beatmap truth?
P1: can AUV send real typed macOS window clicks on that timeline and record delivery facts?
```

What is still unproven:

```text
can AUV capture and correlate visual feedback against those actions?
```

That is the natural P2 question.
