# P7 Detector-Backed Offline Eval Evidence

Date: 2026-06-13

Status: local fixture-stage evidence for the P7 offline detection-eval wiring slice

## Scope

This note records local evidence for the first P7 stage only: wiring offline detector output into the existing osu visual-eval path using recorded run artifacts. It does not claim real-model smoke closure.

Note: benchmark-only capture verification no longer emits `visual_eval_report.json` by itself. P7 eval output now appears only through the explicit `osu eval-detections` path, which keeps detector provenance honest.

## Code change under test

Local code changes add:

- `auv-cli osu eval-detections <run-artifact-dir> --detections <dir-or-json> [--output-dir <dir>]`
- read-side detector fixture loading into `DetectionSet`
- `FrameKey`-exact expansion into `FrameDetections`
- `visual_eval_report.json` output with detector provenance
- `detection_eval_manifest.json` output with source run and detector input provenance

The slice stays offline and artifact-driven:

- reads `visual_truth_manifest.json` and `projection.json` from a recorded run dir
- reads checked-in detector fixture JSON only
- reuses the existing `evaluate_visual_truth(...)` scoring logic
- does not run any model, capture, or app interaction path

## Fixture-stage integration test

Command:

```text
cargo test -p auv-game-osu detection_fixture_eval_writes_report_with_provenance
```

Observed result:

- test passed
- emitted report summary asserted:
  - `total_frames = 3`
  - `label_matched_frames = 1`
  - `label_missing_frames = 2`
  - `spatial_matched_frames = 1`
  - `spatial_missing_frames = 2`
  - `spurious_detection_count = 0`
- detector provenance asserted:
  - `model_id = "test-osu-fixture-detector"`
  - `label_map_source = "inline_fixture_dir"`

## CLI parser coverage

Commands:

```text
cargo test -p auv-cli parse_osu_eval_detections_command
cargo test -p auv-cli parse_osu_eval_detections_requires_detections
cargo test -p auv-cli parse_osu_eval_detections_accepts_default_output_dir
```

Observed result:

- all three parser tests passed
- command shape accepts required run dir + `--detections`
- command rejects missing `--detections`
- `--output-dir` remains optional for the first offline fixture slice

## Local CLI smoke

Command shape:

```text
cargo run --quiet -- osu eval-detections /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-dispatch-p4ab-closeout --detections /Users/liuziheng/https-github-com-moeru-ai-auv/crates/auv-game-osu/tests/fixtures/osu_eval_detection --output-dir /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-eval-detections-p7
```

Observed stdout summary:

```text
runId: run_1781347858406_22548_0
status: completed
totalFrames: 3
labelMatchedFrames: 1
spatialMatchedFrames: 1
output: /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-eval-detections-p7
```

Observed output artifacts:

- `.tmp-osu-eval-detections-p7/visual_eval_report.json`
- `.tmp-osu-eval-detections-p7/detection_eval_manifest.json`

## Interpretation

This is enough to close the roadmap's fixture stage locally:

- detector fixture input is consumed offline as `DetectionSet`
- detections are expanded onto exact `FrameKey(object_index, phase, capture_file_name)` semantics
- the existing visual-eval scoring contract remains intact
- the resulting report preserves detector provenance instead of silently dropping model identity

## Stage status vs roadmap

Roadmap requirement status:

- fixture stage end-to-end subcommand on recorded smoke run + checked-in fixture detections: yes
- emitted `visual_eval_report.json` summary asserted by integration test: yes
- real-model smoke with an actual detector trained or acquired against the P6 dataset: not yet done

This means P7 is locally closed only at stage 1. Stage 2 remains open until a real detector smoke is run and recorded.
