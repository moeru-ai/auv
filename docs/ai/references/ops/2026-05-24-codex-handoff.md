# Codex Handoff: osu! Benchmark Mainline

Date: 2026-06-12

Status: current handoff for the next coding agent before session compaction

Current HEAD when written: `eb04605`

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

Active goal is now:

```text
AUV core lane mainline: runtime surfaces + osu graduation, with C1 next
```

The osu bounded benchmark lane is closed locally for its approved mission and the
owner lane has returned to core graduation / runtime-surface work. The active
roadmap now lives in `docs/ai/references/runtime/2026-06-13-core-roadmap.md`.

Current shape of the lane:

- `P0`: beatmap-driven offline scheduler benchmark — **done as merged skeleton**
- `P1`: typed macOS window dispatch benchmark mode — **done as merged slice**
- `P2`: capture / visual verification — **done as merged slice with local real-app smoke verification**
- `P2.5`: capture timestamp semantics — **done as merged slice with local real-app smoke verification and pushed to main**
- `P3a`: visual dataset / evaluation harness from beatmap truth + corrected timestamped captures — **done as merged slice with local build/test verification and real-app smoke evidence**
- `P3`: offline visual eval harness for the vision validation lane — **done as merged slice with review fixups and real-app upstream evidence chain**

## Current Repo State

Current branch state when written:

```text
main...origin/main
```

Recent commits that matter:

```text
ca96775 docs(osu): refresh handoff after local P3 validation
eb04605 fix(osu): score visual eval per capture frame
582e8c2 feat(osu): add offline visual eval harness for vision validation lane
979b162 docs(osu): record P3a visual truth manifest slice in handoff
04ef4de feat(osu): add visual truth manifest from beatmap and capture traces
```

Before coding again, verify live state:

```bash
git status --short --branch
git log --oneline --decorate -5
```

When this handoff was written, `main` matched `origin/main`; the handoff file itself was then refreshed to record that pushed state.

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

### P2 merged

Commit:

```text
33bdabf feat(osu): add capture verification evidence to dispatch benchmark
```

What it did:

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

### P2.5 merged

Commit:

```text
7c63900 fix(osu): correct capture timestamp semantics
```

What it did:

- adds `pre_capture_offset_ms` with default `16`
- moves before capture to `scheduled_time_ms - pre_capture_offset_ms`
- treats `post_capture_offsets_ms` as absolute offsets relative to `actual_dispatch_time_ms`
- splits capture timing into both:
  - `relative_to_scheduled_ms`
  - `relative_to_dispatch_ms`
- keeps `VerificationSummary` aligned with dispatch-time semantics instead of mixing scheduled and dispatch references

Local real-app smoke re-run passed against installed `osu!.app` and a real local beatmap file before the commit was pushed.

- run id: `run_1781278300171_81552_0`
- output dir: `.tmp-osu-dispatch-p25`
- `verificationCapturedActions: 1`
- `verificationMissingFrames: 0`
- produced files:
  - `capture-object-0000-before-16ms.png`
  - `capture-object-0000-after-16ms.png`
  - `capture-object-0000-after-48ms.png`
- `capture_trace.json` now records both scheduled-relative and dispatch-relative offsets

Observed timing on that smoke confirms why P2.5 matters: dispatch itself was late (`scheduled_time_ms = 151`, `actual_dispatch_time_ms = 1283`), so scheduled-relative and dispatch-relative capture semantics must stay separate and explicit.

### P3a merged locally with real-app smoke evidence

Commits:

```text
04ef4de feat(osu): add visual truth manifest from beatmap and capture traces
979b162 docs(osu): record P3a visual truth manifest slice in handoff
```

What it did:

- adds `crates/auv-game-osu/src/visual_truth.rs` with `VisualTruthManifest`,
  `VisualTruthFrame`, `ExpectedObjectTruth`, and `build_visual_truth_manifest`
- joins schedule + dispatch + capture traces by `object_index`, cross-checks
  kind / scheduled-time / dispatch-time / dispatch-error consistency, and
  fails loudly on any mismatch instead of silently dropping frames
- expands every capture sample into a `VisualTruthFrame` carrying both
  scheduled-relative and dispatch-relative timing plus the beatmap-truth
  expected object (playfield x/y, CS/AR/OD)
- builds the manifest only when `capture_verify` is on, writes
  `visual_truth_manifest.json` next to the other run artifacts, and stages it
  through the existing recorded-operation artifact path in `src/osu.rs`
- exports the new types from `crates/auv-game-osu/src/lib.rs`

Real-app smoke rerun passed against installed `osu!.app`:

- run id: `run_1781290429209_99048_0`
- output dir: `.tmp-osu-dispatch-p3-real`
- `verificationCapturedActions: 1`
- `verificationMissingFrames: 0`
- produced artifacts include:
  - `visual_truth_manifest.json`
  - `capture_trace.json`
  - `verification_summary.json`
  - `capture-object-0000-before-16ms.png`
  - `capture-object-0000-after-16ms.png`
  - `capture-object-0000-after-48ms.png`

Observed manifest shape on that real run confirms why frame-granular eval matters:

- `frames.len() = 3`
- all 3 frames belong to `object_index = 0`
- phases were one `before_dispatch` frame plus two `after_dispatch` frames

Boundaries held:

- beatmap truth stays the primary source; the manifest is offline evidence only
- reuses the existing P2/P2.5 capture artifacts, no parallel evidence path
- no YOLO control, training, LLM in hit loop, or new core-wide contract

### P3 merged with review fixups

Commits:

```text
582e8c2 feat(osu): add offline visual eval harness for vision validation lane
eb04605 fix(osu): score visual eval per capture frame
ca96775 docs(osu): refresh handoff after local P3 validation
```

What it did:

- adds `crates/auv-game-osu/src/visual_eval.rs`
- introduces `evaluate_visual_truth(...)` to score `VisualTruthManifest` frames
  against per-frame `DetectionSet` inputs using reused
  `auv-inference-common` detection types
- splits evaluation into:
  - label-presence scoring that always runs
  - spatial scoring that stays `NotScored` when no playfield-to-pixel
    projection is available
- after review, fixes the scoring key from bare `object_index` to
  `FrameKey { object_index, phase, capture_file_name }`
- introduces `FrameDetections` so before/after frames do not silently share one
  detection set
- counts repeated same-label detections as spurious after consuming only one
  expected match per frame
- keeps all evaluator logic inside `crates/auv-game-osu`

Validation for the P3 slice passed locally before push:

- `cargo fmt --check`
- `cargo check -p auv-game-osu`
- `cargo test -p auv-game-osu`
- `cargo build`
- `git diff --check`
- unit tests now include frame-separation and repeated-label-spurious regression coverage

## Verification Already Run

The following checks passed for the merged P2 state:

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

Additional verification for the merged P2.5 state:

```bash
cargo fmt --check
cargo check
cargo test
cargo build
git diff --check
cargo run --quiet -- osu benchmark <local beatmap> --output-dir .tmp-osu-benchmark-p25
cargo run --quiet -- osu dispatch <local beatmap> --target-app "osu!" --dispatch-limit 1 --capture-verify --output-dir .tmp-osu-dispatch-p25
```

Additional verification for the local P3a/P3 state:

```bash
cargo fmt --check
cargo check -p auv-game-osu
cargo test -p auv-game-osu
cargo build
git diff --check
cargo run --quiet -- osu dispatch "/Users/liuziheng/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rosu-map-0.2.1/resources/sample-beatmap-osu.osu" --target-app "osu!" --dispatch-limit 1 --capture-verify --output-dir .tmp-osu-dispatch-p3-real
```

Notes:

- one intermediate dispatch smoke failed only because the `osu!` window was not visible/resolvable at the time of launch
- the successful rerun produced `run_1781290429209_99048_0`
- that rerun confirmed `visual_truth_manifest.json` and all three capture PNG artifacts landed on a real app window
- the P3 eval harness itself remains library-only in this slice; no detector/control path is wired yet

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

Do not reopen the osu ladder as the main owner lane.

The active forward plan is now `docs/ai/references/runtime/2026-06-13-core-roadmap.md`.
The next owner-approved slice is `C1: auv-cli-invoke Metadata/Registry Boundary`.
When work resumes, start there, use that document's slice boundary/gate, then stop
for owner selection after C1 completes.

### P4a completed locally with real-app smoke evidence

What landed in the local slice:

- adds `crates/auv-game-osu/src/projection.rs`
- projects typed dispatch clicks through `PlayfieldProjection` instead of
  sending raw 512x384 playfield coordinates directly as window coordinates
- records `projection.json` as a benchmark artifact and stages it through the
  existing recorded-run path in `src/osu.rs`
- corrects the P4a artifact semantics so `projection.json` stores capture pixel
  space parameters when capture evidence exists, instead of incorrectly leaving
  window-space values in the artifact

Validation passed locally for the landed P4a code:

- `cargo fmt --check`
- `cargo check -p auv-game-osu --manifest-path /Users/liuziheng/https-github-com-moeru-ai-auv/Cargo.toml`
- `cargo test -p auv-game-osu --manifest-path /Users/liuziheng/https-github-com-moeru-ai-auv/Cargo.toml`
- `cargo build --manifest-path /Users/liuziheng/https-github-com-moeru-ai-auv/Cargo.toml`
- `git -C /Users/liuziheng/https-github-com-moeru-ai-auv diff --check`

Real-app smoke rerun passed against visible local `osu!`:

- run id: `run_1781296969595_4256_0`
- output dir: `.tmp-osu-dispatch-p4ab-closeout`
- `verificationCapturedActions: 1`
- `verificationMissingFrames: 0`
- staged projection artifact values:
  - `capture_width = 1512`
  - `capture_height = 949`
  - `scale_x = scale_y = 2.4713541666666665`
  - `offset_x = 123.33333333333337`
  - `offset_y = 0.0`
  - `match_radius_px = 79.083336`
  - `derivation_method = "layout_rule"`
  - `verification_reference = "before_dispatch capture smoke"`

Manual visual check of `capture-object-0000-after-16ms.png` on that run showed
visible hit feedback at the projected location, which is accepted evidence for
this slice per the roadmap gate.

### P8 bounded demo command completed locally

What landed in the local P8 slice:

- adds `auv-cli osu vision-demo <beatmap.osu> --target-app <name> [--output-dir <dir>] [--dispatch-limit <n>] [--capture-verify]`
- routes the command through the existing osu runtime path in `src/main.rs` and `src/osu.rs`
- reuses `BenchmarkInputs::typed_dispatch(...)` and the existing benchmark/capture/projection artifact path instead of adding a new executor or core contract
- caps the default demo dispatch scope at a bounded local limit so this slice stays a low-difficulty demo command rather than pretending to be general gameplay automation
- records demo-specific run metadata under `auv.osu.vision_demo` / `osu.vision_demo.inputs`
- follow-up refinement keeps the same command surface but now emits a smoke-oriented `evidence_summary.json` artifact and matching stdout evidence fields instead of pretending to be a formal acceptance contract

Validation completed locally so far:

- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `cargo test -p auv-cli parse_osu_vision_demo`
- `cargo test -p auv-game-osu`
- `cargo test -p auv-game-osu benchmark_writes_smoke_evidence_summary_without_capture_verify`
- `git diff --check`

Coordination note:

- Collabi localhost startup was unavailable on `localhost:3000`, so login/check-in was completed against the remote writer API using the configured shared account before code edits continued.
- Active session id: `auv-game-osu-p8`

Current slice status:

- code path and artifact wiring are complete locally
- parser/build/test validation passed locally
- one earlier local runtime smoke failed honestly because selector `"osu!"` could not resolve a visible app window; recorded run id: `run_1781353198422_32066_0`
- one local runtime smoke then succeeded after the local `osu!` window was visible; recorded run id: `run_1781353335250_32172_0`
- a second successful bounded smoke also succeeded after stdout refinement; recorded run id: `run_1781354449040_32632_0`
- the real-app closeout smoke now also succeeded in the current session; recorded run id: `run_1781359194811_47312_0`
- closeout smoke summary reported:
  - `dispatchSamples = 2`
  - `captureArtifacts = 2`
  - `evidenceNotes = 2 scheduled actions missed their target time`
  - `hasEvidenceArtifact = true`
  - `hasProjectionArtifact = true`
  - `hasVisualTruthManifest = true`
  - `verificationCapturedActions = 2`
  - `verificationMissingFrames = 0`
- this slice still does **not** add detector inference, training, ranked automation, or a new architecture path

Interpretation:

- P8 is completed locally for the bounded demo command
- this does **not** mean detector-backed live control exists, and it does **not** mean beatmap-truth-free execution exists
- any broader post-P8 detector/live-control slice still needs separate approval and evidence

Open follow-up observations, not started:

- The closeout smoke still reports `evidenceNotes = 2 scheduled actions missed their target time`, so stronger latency/health goals would need a separate follow-up slice rather than being folded into this bounded demo closeout.
- If future P8 work removes beatmap-truth scheduling entirely, that must be an explicitly approved follow-up slice rather than being implied by this command wrapper.

### P7 fixture stage completed locally with offline detection-eval evidence

What landed in the local P7 fixture-stage slice:

- adds `auv-cli osu eval-detections <run-artifact-dir> --detections <dir-or-json> [--output-dir <dir>]`
- consumes recorded `visual_truth_manifest.json` and `projection.json` from an existing capture-verified run
- consumes offline detector fixture JSON in `DetectionSet` shape only; no live detector execution, no capture, no dispatch
- expands detections onto exact `FrameKey { object_index, phase, capture_file_name }` semantics instead of collapsing before/after frames
- writes `visual_eval_report.json` plus `detection_eval_manifest.json`
- preserves detector provenance in the eval report via `model_id` and label-map source metadata

Validation passed locally for the landed P7 fixture-stage code:

- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `git diff --check`
- `cargo test -p auv-game-osu detection_fixture_eval_writes_report_with_provenance`
- `cargo test -p auv-cli parse_osu_eval_detections_command`
- `cargo test -p auv-cli parse_osu_eval_detections_requires_detections`
- `cargo test -p auv-cli parse_osu_eval_detections_accepts_default_output_dir`
- `cargo run --quiet -- osu eval-detections /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-dispatch-p4ab-closeout --detections /Users/liuziheng/https-github-com-moeru-ai-auv/crates/auv-game-osu/tests/fixtures/osu_eval_detection --output-dir /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-eval-detections-p7`

Real offline detection-eval smoke evidence:

- eval run id: `run_1781347858406_22548_0`
- source run dir: `.tmp-osu-dispatch-p4ab-closeout`
- detections fixture dir: `crates/auv-game-osu/tests/fixtures/osu_eval_detection`
- output dir: `.tmp-osu-eval-detections-p7`
- report summary:
  - `total_frames = 3`
  - `label_matched_frames = 1`
  - `spatial_matched_frames = 1`
- detector provenance:
  - `model_id = "test-osu-fixture-detector"`
  - `label_map_source = "inline_fixture_dir"`

Fixture-stage interpretation:

- P7 stage 1 is now closed locally: the AUV-side offline eval wiring works end to end on a recorded smoke run plus checked-in detector fixtures
- P7 stage 2 is still open: no real detector smoke has been run yet against the P6 dataset, so the full roadmap slice is not honestly closed beyond fixture stage

Evidence note for P7 lives in:

- `docs/ai/references/apps/osu/2026-06-13-osu-benchmark-detection-eval-evidence.md`

Open follow-up observations, not started:

- P7 still lacks negative fixture coverage for bad detection fixture inputs such as missing frames, duplicate frame-key semantics, or malformed detector fixture JSON.
- `visual_eval` still scores the first same-label detection rather than the nearest same-label detection, which keeps instance-level spatial honesty weaker than it could be.

### P6 completed locally with dataset export evidence

What landed in the local P6 slice:

- adds `crates/auv-game-osu/src/dataset.rs`
- adds `auv-cli osu export-dataset <run-artifact-dir> --output-dir <dir>` as an
  artifact-driven exporter for a single capture-verified run
- consumes existing `visual_truth_manifest.json`, `projection.json`, and staged
  capture PNG artifacts only; no new capture or detector path
- exports a dataset directory with:
  - `images/`
  - `labels/` (YOLO txt)
  - `overlays/` (human-auditable rendered boxes)
  - `dataset_manifest.json`
- reuses `LabelMap::default()` label names and emits boxes in
  `source_image_pixels` space
- applies a conservative visibility rule and records it in the dataset manifest
- fails loudly when required capture-verified source artifacts are missing

Validation passed locally for the landed P6 code:

- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `cargo build`
- `git diff --check`
- `cargo run --quiet -- osu export-dataset /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-dispatch-p4ab-closeout --output-dir /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-dataset-p6`
- `cargo run --quiet -- osu export-dataset /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-dispatch-p5-pid-targeted --output-dir /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-dataset-p6-fail`

Real dataset export smoke evidence:

- export run id: `run_1781341861941_13652_0`
- source run dir: `.tmp-osu-dispatch-p4ab-closeout`
- output dir: `.tmp-osu-dataset-p6`
- exported frames: `1`
- skipped frames: `2`
- overlay/image spot-check:
  - `capture-object-0000-before-16ms.png` exported in both `images/` and `overlays/`
  - overlay and copied image both stayed `1512x949`
- generated YOLO label:
  - `labels/capture-object-0000-before-16ms.txt` => `0 0.241750 0.179688 0.104608 0.166667`

Failure smoke evidence:

- failure run id: `run_1781341877086_13676_0`
- exporting from `.tmp-osu-dispatch-p5-pid-targeted` fails immediately because
  `visual_truth_manifest.json` is missing, so the exporter does not silently
  produce an empty dataset from a non-capture-verified source

Evidence note for P6 lives in:

- `docs/ai/references/apps/osu/2026-06-13-osu-benchmark-dataset-evidence.md`

### P5 completed locally with app-local `PidTargeted` evidence

What landed across the local P5 slices:

- starts the measured typed-dispatch clock only after the window-targeted click
  path is warmed, so first-object latency no longer includes one-time driver
  setup in the benchmark timing window
- keeps the existing `dispatch_error_ms` / `LatencyReport` contract unchanged so
  before/after distributions remain directly comparable
- switches the osu benchmark typed dispatch path from
  `WindowClickStrategy::ChromiumCompatible` to the existing lighter
  `WindowClickStrategy::PidTargeted` strategy, while keeping
  `InputPolicy::ForegroundPreferred` and `WindowTargetedMouse` delivery intact
- adds narrow regression coverage around latency-report missed-schedule counting

Validation passed locally for the landed P5 code:

- `cargo fmt --check --manifest-path /Users/liuziheng/https-github-com-moeru-ai-auv/Cargo.toml`
- `cargo check --manifest-path /Users/liuziheng/https-github-com-moeru-ai-auv/Cargo.toml`
- `cargo test --manifest-path /Users/liuziheng/https-github-com-moeru-ai-auv/Cargo.toml`
- `git -C /Users/liuziheng/https-github-com-moeru-ai-auv diff --check`

Real-app evidence chain for P5:

1. Benchmark timing-boundary fix evidence:
   - run id: `run_1781298128793_4766_0`
   - `WindowTargetedMouse` on all 12 objects
   - no fallback reasons recorded
   - first-object outlier removed, but a steady `119ms-126ms` floor remained
2. App-local strategy-switch evidence:
   - run id: `run_1781299108760_5250_0`
   - output dir: `.tmp-osu-dispatch-p5-pid-targeted`
   - `WindowTargetedMouse` on all 12 objects
   - no fallback reasons recorded
   - `latency_report.json` summary:
     - `mean_error_ms = 0.0`
     - `p50_error_ms = 0`
     - `p95_error_ms = 0`
     - `p99_error_ms = 0`
     - `max_error_ms = 0`
     - `jitter_ms = 0`
     - `missed_schedule_count = 0`

Interpretation of the final P5 state:

- the benchmark timing-boundary fix removed the setup-only first-object anomaly
- the remaining floor came from the `ChromiumCompatible` strategy itself, not
  from scheduler semantics
- switching the osu benchmark lane to the existing app-local `PidTargeted`
  strategy collapses that steady floor on the tested local `osu!` setup without
  introducing fallback or delivery-path regression
- P5 now satisfies the roadmap acceptance target of post-warm-up
  `|dispatch_error_ms| p95 <= 16ms` on this local machine

Evidence note for P5 lives in:

- `docs/ai/references/apps/osu/2026-06-13-osu-benchmark-latency-evidence.md`

After push, the likely next slices are:

1. acquire or train a real osu detector model that emits `DetectionSet` labels
   compatible with the P3 eval harness
2. add honest playfield-to-pixel calibration so spatial scoring can move from
   `NotScored` to real matching on captured frames
3. only after those land, wire a detector-backed offline eval path that feeds
   `FrameDetections` into `evaluate_visual_truth(...)`

If continuing past this point, preserve these rules:

- beatmap truth remains the primary source; vision stays a validation lane
- reuse the P3a manifest + P2/P2.5 capture artifacts instead of inventing a parallel evidence path
- do not put LLM or detector inference into the hit loop
- do not introduce a new core-wide contract without owner-approved design

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
