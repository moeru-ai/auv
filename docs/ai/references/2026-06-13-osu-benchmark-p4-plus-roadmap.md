# osu! Benchmark Lane: P4+ Roadmap

Date: 2026-06-13

Status: owner-approved forward plan for the osu! benchmark lane after P3

Predecessor: `docs/ai/references/2026-05-24-codex-handoff.md` (P0–P3 ladder,
all merged and pushed at `ca96775`)

## What This Document Is

The owner approved continuing the lane past P3 and asked for the remaining
slices to be written down once, with enough precision that each slice can be
started without a new approval round. This document is that approval record.

Rules for using it:

- Each slice below is individually approved as written. Starting one does not
  approve the next; finish, report, stop, and let the owner pick again.
- Deviating from a slice's scope or acceptance section requires going back to
  the owner first.
- After each merged slice, update the phase ladder in the codex handoff doc
  and this doc's status lines.

## Standing Boundaries (apply to every slice)

- benchmark-first, not YOLO-first; `.osu` beatmap truth stays the primary
  scheduling source
- strongest available signal wins: never re-derive from pixels what structured
  truth or window geometry already provides; vision is a verification and
  evaluation channel
- local offline beatmaps only; no online, ranked, or multiplayer automation
- no LLM and no detector inference inside the dispatch loop
- osu-specific logic stays in `crates/auv-game-osu`; core crates only gain
  changes through the graduation gate at the end of this doc
- no new core-wide contract or third action-result schema; reuse
  `auv-inference-common` detection types and the existing recorded-run
  artifact layout
- every slice ends with the standard validation block plus a real-app smoke
  where the slice touches runtime behavior

## The Code Facts Driving the Order

1. `run_typed_dispatch` currently clicks raw beatmap osupixels as window
   coordinates: `WindowPoint::new(f64::from(action.x), f64::from(action.y))`.
   The 512x384 playfield space is never projected into the rendered window,
   so clicks do not land on rendered circles. The lane's latency evidence is
   valid; its spatial behavior is not.
2. `visual_eval.rs` already defines the consumer for the missing piece:
   `EvalProjection::PlayfieldToPixels { scale_x, scale_y, offset_x, offset_y,
   match_radius_px }`. Without a producer, spatial scoring stays `NotScored`.
3. `auv-inference-common::DetectionCoordinateSpace` v0 documents the same gap
   from the other side: only `SourceImagePixels` exists "because no runtime
   capture/window/display projection is available". Capture-image pixel space
   is therefore the shared evaluation space; no enum change is needed.
4. `auv-inference-ort` and `auv-inference-ultralytics` already exist. The
   detector lane reuses them; this lane never grows its own inference stack.

One projection therefore feeds three consumers: correct dispatch clicks, real
spatial scoring, and beatmap-truth auto-labels for detector training.

## Phase Ladder

```text
P4a  playfield-to-window projection + projected dispatch   (next)
P4b  projection into visual eval, spatial scoring goes real
P5   dispatch latency hardening (multi-object, honest numbers)
P6   auto-label dataset exporter from truth manifest + projection
P7   detector-backed offline eval wiring (fixtures first, real model joint)
P8   vision-only low-difficulty demo                        (parked)
```

Default order is top to bottom. Dependency notes: P4b needs P4a. P5 needs
only P0/P1 and may be pulled ahead of P4 if the owner wants latency numbers
first. P6 needs P4a. P7's AUV-side wiring needs P6 only for the real-model
smoke, not for the fixture-driven part.

---

## P4a: Playfield-to-Window Projection + Projected Dispatch

Classification: approved feature (this document is the approval).

Goal: a deterministic projection from beatmap osupixel space (512x384) to
window/capture pixel space, used by typed dispatch and recorded as an
artifact.

Touches:

- `crates/auv-game-osu/src/projection.rs` (new)
- `crates/auv-game-osu/src/benchmark.rs` (dispatch call site, artifact write)
- `crates/auv-game-osu/src/lib.rs` (exports)
- `src/osu.rs` (artifact staging only)

Design constraints:

- The installed `osu!.app` on macOS is lazer. Do not copy osu!stable's
  0.8-of-height playfield formula from folklore; treat the lazer layout rule
  as a hypothesis, derive constants from resolved window content bounds, and
  verify against a real capture before trusting them.
- Verification channel stays separate from derivation: derivation uses window
  geometry (structured signal); verification uses a before-dispatch capture
  frame where a known object's approach/hit circle is visible at projected
  coordinates.
- If the layout hypothesis fails on lazer, fall back to a one-time empirical
  calibration recorded as constants. Vision may verify the calibration; it
  must not continuously estimate it.
- `match_radius_px` is not a magic number: derive object radius from CS
  (already carried in `ExpectedObjectTruth`) and scale it through the
  projection.
- Projection math lives behind a small pure function with unit tests at fixed
  window sizes; no AX or capture calls inside it.

Deliverables:

- `projection.json` run artifact recording: source window bounds, capture
  dimensions, derived `scale_x/scale_y/offset_x/offset_y`, derived
  `match_radius_px`, derivation method (`layout_rule` vs
  `empirical_calibration`), and the verification evidence reference
- typed dispatch clicks at projected coordinates
- unit tests: known window sizes map known osupixel corners/centers to
  expected pixels; degenerate window sizes are rejected loudly

Acceptance:

- on a real-app smoke (`osu dispatch ... --dispatch-limit 1
  --capture-verify`), the after-dispatch capture shows hit feedback at the
  projected location (manual visual check of the staged PNGs is acceptable
  evidence; record the run id and the conclusion)
- `projection.json` is staged in the recorded run with finite values and a
  stated derivation method
- projected click coordinates stay within window bounds for every scheduled
  object of the smoke beatmap
- all unit tests pass

Out of scope: visual_eval wiring (P4b), any detector work, slider paths or
follow trajectories (clicks at object head only, as today).

Gate: report smoke run id + projection.json contents, stop.

## P4b: Projection Into Visual Eval

Classification: approved feature (this document is the approval).

Goal: spatial scoring in `evaluate_visual_truth(...)` moves from `NotScored`
to real outcomes by feeding the P4a projection.

Touches:

- `crates/auv-game-osu/src/visual_eval.rs` (only if a loader/adapter is
  needed; scoring logic itself should not change)
- `crates/auv-game-osu/src/benchmark.rs` or a small new module: build
  `EvalProjection::PlayfieldToPixels` from `projection.json`
- `src/osu.rs` (staging if a new artifact appears)

Deliverables:

- a documented path from a capture-verified run to an `EvalProjection`
  loaded from that run's `projection.json`
- regression test: a manifest frame plus a synthetic detection at the
  projected truth point scores `FrameSpatialOutcome::Matched`; the same
  detection displaced beyond `match_radius_px` scores `Missing`
- `VisualEvalReport.known_limits` keeps the honesty note about linear
  projection quality

Acceptance:

- on the P4a smoke artifacts plus synthetic fixture detections,
  `spatial_unscored_frames == 0` and spatial outcomes are real
  (`Matched`/`Missing`), with the report serialized next to the run
- unit/regression tests pass

Out of scope: real detector inference; dataset export.

Gate: report the eval report summary counts, stop.

## P5: Dispatch Latency Hardening

Classification: bug fix + narrow refactor (the P2.5 smoke recorded
`scheduled_time_ms = 151`, `actual_dispatch_time_ms = 1283`; first-object
lateness of ~1.1s is a defect in clock semantics, not jitter).

Goal: honest multi-object latency numbers, with setup cost moved out of the
measured window and known per-click overheads reduced.

Touches:

- `crates/auv-game-osu/src/benchmark.rs` (schedule clock start, warm-up,
  per-click path)
- `crates/auv-driver-macos` only if a measured overhead points there, and
  then only via the graduation gate below

Steps, in order:

1. Baseline first: real-app run with `--dispatch-limit 20` or more on a
   simple local beatmap, capture-verify off. Record the full
   `dispatch_error_ms` distribution (`latency_report.json` already
   aggregates percentiles).
2. Move window resolve and any first-click setup before the schedule clock
   starts (lead-in semantics: the clock must begin only when the dispatch
   path is warm).
3. Identify and reduce per-click overhead (e.g. repeated AX queries inside
   `session.window().click`); measure again.
4. If a floor remains, record the floor and its cause as evidence. Do not
   tune the benchmark to flatter the number.

Deliverables:

- before/after `latency_report.json` from real-app runs, run ids recorded
- a short evidence note in `docs/ai/references/` with the distributions and
  the explanation of any remaining floor

Acceptance:

- first-object dispatch error is no longer an outlier class of its own
  (setup excluded from the measured window)
- post-warm-up p95 of `|dispatch_error_ms|` is at or under 16 ms on the
  local machine, or the documented floor explains why not and where the time
  goes
- `cargo test` keeps passing; no public contract changes

Out of scope: capture-path latency (separate question), input device
simulation fidelity, slider trajectories.

Gate: report both distributions, stop.

## P6: Auto-Label Dataset Exporter

Classification: approved feature (this document is the approval).

Goal: turn capture-verified runs into a labeled detection dataset: beatmap
truth + projection => pixel bounding boxes on staged capture PNGs. This is
the "free annotation machine" the detector lane consumes.

Touches:

- `crates/auv-game-osu/src/dataset.rs` (new)
- `src/osu.rs` / `src/cli.rs` (new subcommand, e.g.
  `auv-cli osu export-dataset <run-or-output-dir> --format yolo`)

Design constraints:

- inputs are existing artifacts only: `visual_truth_manifest.json`,
  `projection.json`, staged capture PNGs. No new capture path, no game
  interaction.
- boxes are written in capture-image pixel space and declared as
  `SourceImagePixels`; label names reuse the `LabelMap` defaults
  (`hit_circle`, `slider`, `spinner`, `hold`)
- box size derives from CS through the projection (same radius rule as
  P4a's `match_radius_px`), not from hand-tuned constants
- frames whose truth says the object is not visually present (e.g. long
  after dispatch) must not get a positive label; use the frame phase and
  timing fields to decide, and record the decision rule in the dataset
  manifest
- export both machine labels (YOLO txt per image) and a human-auditable
  overlay (reuse `auv_inference_common::render_annotated_image`) so the
  labels can be eyeballed without tooling

Deliverables:

- dataset directory: images, YOLO-format labels, overlay images, and a
  `dataset_manifest.json` recording source run id, projection, label map,
  visibility rule, and per-frame provenance
- unit tests for box derivation and the visibility rule

Acceptance:

- exporter runs on the P4a/P4b smoke run and produces a dataset where every
  box lies within image bounds
- spot-check of overlay images confirms boxes sit on rendered objects
  (record which frames were checked)
- a run with `capture_verify` off fails loudly with a clear message instead
  of exporting an empty dataset silently

Out of scope: training any model, dataset balancing/augmentation, multi-run
aggregation (single-run export is enough for the first slice).

Gate: hand the dataset format to the detector owner (Neko), stop.

## P7: Detector-Backed Offline Eval Wiring

Classification: approved feature (this document is the approval).

Goal: close the loop `capture frames -> external detector -> DetectionSet
-> FrameDetections -> evaluate_visual_truth(...) -> visual_eval_report.json`,
entirely offline.

Touches:

- `crates/auv-game-osu/src/visual_eval.rs` consumers only; scoring logic
  stays as merged in P3
- `src/osu.rs` / `src/cli.rs` (new subcommand, e.g.
  `auv-cli osu eval-detections <run-or-output-dir> --detections <dir-or-json>`)

Design constraints:

- the detector runs outside this repo's dispatch path: input is per-frame
  detection JSON (serialized `DetectionSet` from `auv-inference-ort` /
  `auv-inference-ultralytics`, or any tool emitting the same shape), keyed by
  capture file name
- mapping into `FrameDetections` must use the P3 `FrameKey` semantics
  (`object_index`, `phase`, `capture_file_name`); never collapse before/after
  frames onto one detection set
- model identity (`model_id`) and label mapping land in the eval report for
  provenance

Two-stage acceptance, both required before the slice closes:

1. fixture stage (AUV-side done): an integration test drives the new
   subcommand end to end on a recorded smoke run plus checked-in fixture
   detection JSON, and the emitted `visual_eval_report.json` matches expected
   counts
2. real-model smoke (joint with the detector owner): run once with a real
   detector trained or acquired against the P6 dataset; record run id,
   model id, and the report summary in an evidence note. If no real model
   exists yet, the slice stays open at stage 1 and says so.

Out of scope: detector training, online/in-loop inference, promoting
detections into any control path.

Gate: report both stages' status, stop.

## P8: Vision-Only Low-Difficulty Demo (Parked)

Not approved by this document. It stays parked until the owner explicitly
reopens it. If reopened, it is a demo lane: beatmap truth removed, detector
plus capture drive a low-difficulty local map, with the explicit framing
that this demonstrates the stack and does not define the architecture.

Recorded here only so nobody mistakes its absence for an oversight.

---

## Graduation Gate (osu lane -> core crates)

May graduate into core (`auv-driver*`, `auv-inference-common`,
runtime/tracing), each as its own owner-approved slice with a named design
note:

```text
timestamped capture semantics hardening
input scheduler / clock-start semantics (if P5 finds driver-side cost)
latency histogram / percentile reporting as a reusable shape
frame/action correlation keys
a future DetectionCoordinateSpace variant for projected spaces, only if a
  second consumer outside osu appears
```

Never graduates:

```text
beatmap parsing, playfield projection constants, CS/AR/OD rules
osu label map, dataset export format, play policy of any kind
```

## Per-Slice Process (same for every slice)

```bash
git status --short --branch   # verify clean start
cargo fmt --check
cargo check
cargo test
cargo build
git diff --check
```

Plus the slice's real-app smoke where runtime behavior changed, with run ids
recorded in the slice report. Docs-only follow-ups may skip Cargo validation
and must say so.

After each slice: state what changed and what was validated, list follow-up
observations without starting them, stop, and wait for the owner to choose
the next slice from this ladder.
