# 2026-06-24 Minecraft MC-6 dual-gate closure reference

Date: 2026-06-24

Classification label: `docs-only`.

Purpose: pin the accepted MC-6 closure shape after the 2026-06-24 semantics,
geometry, and full 9-run live completeness closure. This note is the narrow
handoff for resuming MC-6 without reopening old debates, reusing stale
`/tmp/auv-mc67-live*` paths, or regressing back to the old 3024x1964 staging
screenshots as geometry proof.

## Dual-gate split

MC-6 no longer asks one report to prove two different things.

- **Gate 1 — geometry gate**
  - Goal: prove that one canonical projected target lands on the correct block
    face in the screenshot.
  - Entry: `auv-cli minecraft calibrate-projection`
  - Output: `minecraft-projection`, `minecraft-overlay`,
    `minecraft-projection-calibration`
  - Review rule: visual/auditable overlay review, not a fabricated numeric pass
    bit.
- **Gate 2 — completeness gate**
  - Goal: prove `sample_count`, `duration_seconds`, and
    `noise_refusal_exercised` from fresh real-source live evidence.
  - Entry chain:
    `export-spatial-bundle -> build-texture-sweep-samples -> eval-texture-sweep --require-real-source`
  - Review rule: the final report is a coverage proof, not a geometry proof.

Do not read a single-frame calibration artifact as coverage closure.
Do not read a sweep report as geometry validation.

## Canonical calibration lineage

Use the fresh frontmost-window lineage captured on 2026-06-24 for Gate 1:

- `rich`
  - capture run: `run_1782249745960_13265_0`
  - screenshot:
    `.auv/runs/run_1782249745960_13265_0/artifacts/artifact_0001_window-capture-window-capture.png`
  - bridge run: `run_1782249819524_13379_0`
  - spatial frame:
    `.auv/runs/run_1782249819524_13379_0/artifacts/artifact_0002_minecraft-spatial-frame.json`
  - `telemetry_session_id`: `0bf5f5b3-6a34-4a15-b373-f641447c75ff`
- `flat_color`
  - capture run: `run_1782250118864_14927_0`
  - screenshot:
    `.auv/runs/run_1782250118864_14927_0/artifacts/artifact_0001_window-capture-window-capture.png`
  - bridge run: `run_1782250218139_15240_0`
  - spatial frame:
    `.auv/runs/run_1782250218139_15240_0/artifacts/artifact_0002_minecraft-spatial-frame.json`
  - `telemetry_session_id`: `feb47bfc-8e27-44e0-af1a-1f1d270c59fa`
- `repetitive`
  - capture run: `run_1782250420931_16392_0`
  - screenshot:
    `.auv/runs/run_1782250420931_16392_0/artifacts/artifact_0001_window-capture-window-capture.png`
  - bridge run: `run_1782250477803_16666_0`
  - spatial frame:
    `.auv/runs/run_1782250477803_16666_0/artifacts/artifact_0002_minecraft-spatial-frame.json`
  - `telemetry_session_id`: `9b9af73c-1c2e-459b-a55f-b840f1c31723`

Historical boundary:

- The older `run_178188...` screenshots remain valid as historical staging /
  fail-rebuild provenance only.
- Those PNGs are `3024x1964` desktop screenshots, not the accepted reopened
  Gate 1 geometry evidence.
- Gate 1 geometry proof now standardizes on frontmost `window.capture`
  screenshots plus the corresponding bridge run.

Current canonical target evidence:

- block position: `511,73,728`
- block id: `minecraft:oak_button`

Current geometry rule for MC-6:

- default target semantics for calibration and bridge closure is
  `hit_face_center`
- hit-face-center only applies when
  `raycast_hit.block_pos == --target-block`
- otherwise the target falls back to plain `block_center`

That is intentional local MC-6 behavior. It does not change the public default
semantics of `MinecraftBlockTarget::new()`.

## Fresh mini-sweep naming

Each resource-pack profile should produce exactly three source runs:

- `accepted-early`
- `accepted-late`
- `refusal-menu`

Expected profile set:

- `rich`
- `flat_color`
- `repetitive`

That yields nine closure runs total:

- `rich-accepted-early`
- `rich-accepted-late`
- `rich-refusal-menu`
- `flat_color-accepted-early`
- `flat_color-accepted-late`
- `flat_color-refusal-menu`
- `repetitive-accepted-early`
- `repetitive-accepted-late`
- `repetitive-refusal-menu`

Closed live completeness set:

- `repetitive`
  - `accepted-early`
    - capture run: `run_1782281165555_53632_0`
    - bridge run: `run_1782281195384_53866_0`
  - `accepted-late`
    - capture run: `run_1782281366295_54733_0`
    - bridge run: `run_1782281378426_54930_0`
  - `refusal-menu`
    - capture run: `run_1782282071489_57893_0`
    - bridge run: `run_1782282101783_58126_0`
    - refusal reason: `MenuLoadingScreen`
  - `telemetry_session_id = 9b9af73c-1c2e-459b-a55f-b840f1c31723`
- `rich`
  - `accepted-early`
    - capture run: `run_1782283188235_69082_0`
    - bridge run: `run_1782283214391_69376_0`
  - `accepted-late`
    - capture run: `run_1782283386128_70571_0`
    - bridge run: `run_1782283416142_70808_0`
  - `refusal-menu`
    - capture run: `run_1782283457672_71083_0`
    - bridge run: `run_1782283473776_71280_0`
    - refusal reason: `MenuLoadingScreen`
  - `telemetry_session_id = 246a3105-1417-470c-943f-5e48abd09224`
- `flat_color`
  - `accepted-early`
    - capture run: `run_1782283814387_75282_0`
    - bridge run: `run_1782283850920_75557_0`
  - `accepted-late`
    - capture run: `run_1782283980548_76079_0`
    - bridge run: `run_1782284010123_76280_0`
  - `refusal-menu`
    - capture run: `run_1782284054773_76561_0`
    - bridge run: `run_1782284093349_76766_0`
    - refusal reason: `MenuLoadingScreen`
  - `telemetry_session_id = 8d5bf3fc-36e0-4d61-b5cd-163d9e775990`

Live runbook rules:

- switch profiles through `devtools/auv-game-minecraft/run/options.txt`
- restart the Minecraft client between profiles so each profile gets a fresh
  `telemetry_session_id`
- keep `accepted-early` and `accepted-late` in the same
  `telemetry_session_id`
- keep at least `30.0 s` between accepted observations inside the same session
- produce a real refusal with `menu_loading_screen` rather than relying on
  `screenshot_unavailable` or other missing-data fallbacks

## Current Gate 2 closure artifact

The current completeness proof comes from the reopened 9-run set above:

- local bundle/sample/eval workspace: `.tmp/mc6-live-a-20260624/`
- sample-build run:
  `.auv/runs/run_1782284483150_79654_0/artifacts/artifact_0001_texture_sweep_samples.json`
- eval run:
  `.auv/runs/run_1782284485217_79709_0/artifacts/artifact_0002_texture_sweep_report.json`
- local eval copy:
  `.tmp/mc6-live-a-20260624/eval/texture_sweep_report.json`

Final report reading:

- `actual_resource_pack_count = 3`
- `noise_refusal_exercised = true`
- `passed = true`
- `flat_color`
  - `sample_count = 2`
  - `refused_noise_count = 1`
  - `duration_seconds = 159.244`
- `repetitive`
  - `sample_count = 2`
  - `refused_noise_count = 1`
  - `duration_seconds = 183.035`
- `rich`
  - `sample_count = 2`
  - `refused_noise_count = 1`
  - `duration_seconds = 201.762`

Read boundary:

- Gate 2 is now closed by this report.
- The report is a coverage proof only; it does not replace Gate 1 geometry
  review.
- `pose_error_p95_px = 0.0` remains intentional bridge-only semantics here,
  not a claim that MC-6 has a richer pose metric.

## Accepted read-side semantics

The current closure read-side intentionally uses these stricter rules:

- sample dedupe key: `(spatial_frame_id, refused_noise)`
- duration bucket: accepted frames only, grouped by
  `resource_pack + telemetry_session_id`
- historical fallback: if `telemetry_session_id` is missing, bucket by
  `source_run_id`
- refusal source of truth: prefer `minecraft-projection.mismatch_refusal_reason`
  over frame-only fallback classification
- bridge-only visible samples emit `pose_error_px = 0.0` by design; MC-6 does
  not treat center-distance as a real pose metric

## Resume order

When resuming MC-6 from the current slice, do it in this order:

1. keep Gate 1 pinned to the fresh `window.capture` lineage above; do not
   regress to the old `run_178188...` 3024x1964 PNGs for geometry proof
2. treat Gate 1 as already passed unless projection code changes again
3. treat the 9-run `eval-texture-sweep --require-real-source` report above as
   the current Gate 2 closure artifact
4. rerun Gate 2 only if projection code, sample-builder semantics, thresholds,
   or the required profile set changes

As of
`.auv/runs/run_1782284485217_79709_0/artifacts/artifact_0002_texture_sweep_report.json`,
MC-6 is closed under the current dual-gate contract.
