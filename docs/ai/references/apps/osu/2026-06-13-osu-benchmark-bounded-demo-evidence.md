# P8 First Bounded Vision Demo Slice Evidence

Date: 2026-06-13

Status: code-complete, locally validated, and now backed by a successful real-app closeout smoke for the bounded demo command

## Scope

This note records only the first bounded P8 slice: a dedicated local demo command that reuses the existing osu typed-dispatch benchmark/runtime/artifact path.

It does **not** claim:

- detector-driven live control
- beatmap-truth removal from execution
- online or ranked automation
- model training or acquisition
- a new AUV core contract

## Coordination

The user asked to go through Collabi before file edits.

Observed environment state:

- local Collabi endpoint on `localhost:3000` was unavailable
- login/check-in succeeded against the remote writer API using the configured shared account
- active Collabi session id: `auv-game-osu-p8`

## Code change under test

Local code changes add:

- `auv-cli osu vision-demo <beatmap.osu> --target-app <name> [--output-dir <dir>] [--dispatch-limit <n>] [--capture-verify]`
- a new `CliCommand::OsuVisionDemo` parser surface
- a new `src/main.rs` match arm that routes the command through the normal runtime
- a new `src/osu.rs` wrapper `run_osu_vision_demo(...)`
- bounded demo defaults that reuse `BenchmarkInputs::typed_dispatch(...)`
- demo-specific recorded-operation metadata:
  - run type id: `auv.osu.vision_demo`
  - input event: `osu.vision_demo.inputs`
- a smoke-oriented `evidence_summary.json` artifact plus matching stdout summary fields; this is an evidence summary, not a formal pass/fail acceptance contract

The slice reuses the existing artifact family:

- `parsed_map_summary.json`
- `action_schedule.json`
- `dispatch_trace.json`
- `latency_report.json`
- `evidence_summary.json`
- optional capture/projection artifacts when `--capture-verify` is enabled

## Parser coverage

Command:

```text
cargo test -p auv-cli parse_osu_vision_demo
```

Observed result:

- all four parser tests passed
- command accepts required beatmap + `--target-app`
- command rejects missing `--target-app`
- `--output-dir` remains optional
- large explicit `--dispatch-limit` values still parse at the CLI layer; the bounded cap remains a runtime wrapper policy

## Build/test validation

Commands:

```text
cargo fmt --check
cargo check
cargo test
```

Observed result:

- all passed locally after the P8 command wiring landed
- follow-up evidence-summary rename and wiring also passed local `cargo check`, `cargo test -p auv-cli parse_osu_vision_demo`, `cargo test -p auv-game-osu`, and `git diff --check`
- focused regression test for the non-capture evidence path also passed:
  - `cargo test -p auv-game-osu benchmark_writes_smoke_evidence_summary_without_capture_verify`

## Runtime summary refinement

A second narrow P8 follow-up landed locally without widening the command surface:

- `osu vision-demo` stdout now reports bounded-demo evidence fields including:
  - `dispatchSamples`
  - `captureArtifacts`
  - `evidenceNotes`
  - `hasEvidenceArtifact`
  - `hasProjectionArtifact`
  - `hasVisualTruthManifest`
- `evidence_summary.json` records smoke-oriented evidence counts and notes instead of pretending to be a formal acceptance verdict
- no new command, detector path, or contract was introduced

## Local runtime smoke

Successful local bounded smoke already recorded earlier:

- recorded run id: `run_1781353335250_32172_0`
- output dir: `.tmp-osu-vision-demo-p8-success`
- summary stdout:
  - `objects = 12`
  - `latencyP95Ms = 1060`
  - `jitterMs = 0`
  - `verificationCapturedActions = 1`
  - `verificationMissingFrames = 0`
- the command produced the expected bounded demo artifact path on top of the existing benchmark/capture/projection chain

Additional successful smoke after stdout refinement:

- recorded run id: `run_1781354449040_32632_0`
- output dir: `.tmp-osu-vision-demo-p8-success-2`
- additional stdout evidence:
  - `dispatchSamples = 1`
  - `captureArtifacts = 1`
  - `hasProjectionArtifact = true`
  - `hasVisualTruthManifest = true`

Real-app closeout smoke now succeeded in the current session:

- recorded run id: `run_1781359194811_47312_0`
- output dir: `.tmp-osu-vision-demo-p8-closeout`
- closeout stdout evidence:
  - `dispatchSamples = 2`
  - `captureArtifacts = 2`
  - `evidenceNotes = 2 scheduled actions missed their target time`
  - `hasEvidenceArtifact = true`
  - `hasProjectionArtifact = true`
  - `hasVisualTruthManifest = true`
  - `verificationCapturedActions = 2`
  - `verificationMissingFrames = 0`
- this closes the environment blocker that previously prevented an honest real-app P8 closeout smoke in the current session

Environment-blocked retries retained as honest earlier evidence:

- `run_1781355823652_45345_0`
- `run_1781355891032_45495_0`
- `run_1781357172070_46455_0`
- each failed because selector `"osu!"` did not resolve a visible app window in the session at that time

## Current interpretation

What is proven now:

- P8 has an explicit local command surface
- the command stays inside the existing osu runtime/recorded-operation path
- parser/build/test validation is green
- bounded local smoke succeeded with capture verification enabled
- one real-app closeout smoke succeeded in the current environment with `--dispatch-limit 2 --capture-verify`
- the command reuses existing benchmark/capture/projection artifact behavior instead of creating a new architecture path
- the code path emits smoke-oriented evidence summaries for later closeout runs

What is still not proven:

- any detector-backed live behavior
- any beatmap-truth-free execution path
- any claim that the current latency figures are acceptable for a stronger control lane than this bounded demo
- closure of any broader post-P8 detector/live-control roadmap beyond this bounded local demo slice

## Current status label

P8 can now be treated as:

- `completed locally for the bounded demo command`

## Next evidence needed

Any later work beyond this bounded closeout should be treated as a separate approved follow-up, especially:

- detector-backed live-demo behavior
- beatmap-truth-free execution
- stronger latency/health targets than the current smoke-oriented evidence summary
