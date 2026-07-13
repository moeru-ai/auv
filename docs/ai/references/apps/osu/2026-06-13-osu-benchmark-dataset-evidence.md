# P6 Auto-Label Dataset Evidence

Date: 2026-06-13

Status: local evidence note for the P6 auto-label dataset exporter slice

## Scope

This note records the local evidence for P6: exporting a single capture-verified
osu benchmark run into an offline labeled dataset using existing artifacts only.
The slice goal is to turn run evidence into reusable detector-training inputs
without reopening capture, dispatch, or detector execution.

## Code change under test

Local code changes add:

- `crates/auv-game-osu/src/dataset.rs`
- `auv-cli osu export-dataset <run-artifact-dir> --output-dir <dir>`

The exporter:

- reads `visual_truth_manifest.json`, `projection.json`, and staged capture PNGs
- maps `ObjectKind` through `LabelMap::default()`
- projects beatmap truth into capture-image pixel-space boxes using the recorded
  projection artifact
- writes copied source images, YOLO labels, overlay images, and a
  `dataset_manifest.json`
- refuses to export from a source directory that lacks capture-verified truth
  artifacts

## Successful smoke run

Command shape:

```text
cargo run --quiet -- osu export-dataset /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-dispatch-p4ab-closeout --output-dir /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-dataset-p6
```

Export run id:

```text
run_1781341861941_13652_0
```

Source run dir:

```text
.tmp-osu-dispatch-p4ab-closeout
```

Output dir:

```text
.tmp-osu-dataset-p6
```

Observed command summary:

```text
status: completed
exportedFrames: 1
skippedFrames: 2
```

Observed `dataset_manifest.json` highlights:

- `coordinate_space = "source_image_pixels"`
- `visibility_rule = "label frames captured before dispatch or within 128ms after dispatch; skip later frames"`
- label map entries:
  - `0 => hit_circle`
  - `1 => slider`
  - `2 => spinner`
  - `3 => hold`
- one exported frame:
  - `capture-object-0000-before-16ms.png`
- two skipped frames:
  - `capture-object-0000-after-16ms.png`
  - `capture-object-0000-after-48ms.png`

Observed YOLO label output:

```text
labels/capture-object-0000-before-16ms.txt
0 0.241750 0.179688 0.104608 0.166667
```

Observed overlay/image spot-check:

- copied image: `.tmp-osu-dataset-p6/images/capture-object-0000-before-16ms.png`
- overlay image: `.tmp-osu-dataset-p6/overlays/capture-object-0000-before-16ms.png`
- both decoded at `1512x949`
- overlay box aligns with the projected truth region for the sampled frame by
  local visual spot-check

## Interpretation

The successful smoke confirms the exporter stays inside the approved P6 boundary:

- it consumes existing run artifacts only
- it emits machine labels and human-auditable overlays in capture-image pixel
  space
- it records its conservative visibility rule and per-frame provenance in a
  durable dataset manifest
- it exports only frames allowed by the rule instead of guessing prolonged
  post-dispatch visibility

## Failure smoke

Command shape:

```text
cargo run --quiet -- osu export-dataset /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-dispatch-p5-pid-targeted --output-dir /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-dataset-p6-fail
```

Failure run id:

```text
run_1781341877086_13676_0
```

Observed failure:

```text
failed to read /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-dispatch-p5-pid-targeted/visual_truth_manifest.json: No such file or directory (os error 2)
```

## Interpretation of failure path

This is the intended first-slice failure mode: a non-capture-verified source run
lacks `visual_truth_manifest.json`, so the exporter stops immediately instead of
silently creating an empty or partial dataset.

## Acceptance status vs roadmap

Roadmap requirement status:

- exporter runs on a capture-verified smoke run and produces a dataset: yes
- every emitted box lies within image bounds: yes
- overlay spot-check confirms boxes sit on rendered objects: yes, for the
  sampled exported frame above
- a run with `capture_verify` off fails loudly with a clear message: yes

This means the current P6 slice is acceptable to close locally on the tested
single-run smoke inputs.
