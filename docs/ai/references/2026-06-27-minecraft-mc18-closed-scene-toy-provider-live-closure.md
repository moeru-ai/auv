# MC-18: Closed-scene toy query provider live closure

Date: 2026-06-27

## Summary

First fresh local pass where MC-12 spatial query used the in-repo `closed_scene_toy`
provider with a committed closed-scene fixture to produce dual-backend compare
evidence against `projection_reference`.

This is a **provider seam closure** only. It does **not** claim:

- Gaussian inference or splat-quality judgment
- action dispatch or MC-14 readiness changes
- holdout / render quality mixing (MC-16 / MC-17)

## Input lineage

Local MC-18 live setup (minimal MC-10 semantic + scene packet):

- semantic: `.tmp/mc18-live/setup/semantic.json`
- scene packet: `.tmp/mc18-live/setup/scene-packet/scene-packet.json`
- fixtures (committed):
  - `crates/auv-game-minecraft/tests/fixtures/mc18/visible.json`
  - `crates/auv-game-minecraft/tests/fixtures/mc18/outside_window.json`

## Commands

Visible target (answered + visible):

```sh
cargo run --quiet -- minecraft query-3dgs-training-result \
  --training-result-semantic-manifest .tmp/mc18-live/setup/semantic.json \
  --target-block 511,73,728 \
  --target-face north \
  --target-semantics hit_face_center \
  --query-provider closed-scene-toy \
  --closed-scene-fixture crates/auv-game-minecraft/tests/fixtures/mc18/visible.json \
  --output-dir .tmp/mc18-live/query-visible
```

Non-clickable visibility (answered + outside_window):

```sh
cargo run --quiet -- minecraft query-3dgs-training-result \
  --training-result-semantic-manifest .tmp/mc18-live/setup/semantic.json \
  --target-block 511,73,728 \
  --target-face north \
  --target-semantics hit_face_center \
  --query-provider closed-scene-toy \
  --closed-scene-fixture crates/auv-game-minecraft/tests/fixtures/mc18/outside_window.json \
  --output-dir .tmp/mc18-live/query-outside
```

Negative control (absent closed label):

```sh
cargo run --quiet -- minecraft query-3dgs-training-result \
  --training-result-semantic-manifest .tmp/mc18-live/setup/semantic.json \
  --target-block 9,9,9 \
  --query-provider closed-scene-toy \
  --closed-scene-fixture crates/auv-game-minecraft/tests/fixtures/mc18/visible.json \
  --output-dir .tmp/mc18-live/query-absent
```

## Recorded runs

### Visible (`.tmp/mc18-live/query-visible`)

- run: `run_1782578029036_39675_0`
- `status = answered`
- `selectedBackend = closed_scene_toy`
- `visibility = Visible`
- `screenPoint = 640,360`
- `basisFrameId = closed_scene_toy:mc18-smoke-v1:frame-0003`
- `comparisonVerdict = divergent` (acceptable; provider vs reference compare only)

Artifacts:

- `.tmp/mc18-live/query-visible/minecraft-3dgs-training-result-query.json`
- `.tmp/mc18-live/query-visible/minecraft-3dgs-training-result-query-inspect.json`

### Outside window (`.tmp/mc18-live/query-outside`)

- run: `run_1782578030217_39914_0`
- `status = answered`
- `selectedBackend = closed_scene_toy`
- `visibility = OutsideWindow`
- `screenPoint = 1200,360`
- `basisFrameId = closed_scene_toy:mc18-smoke-v1:frame-0003`
- `comparisonVerdict = divergent`

Artifacts:

- `.tmp/mc18-live/query-outside/minecraft-3dgs-training-result-query.json`
- `.tmp/mc18-live/query-outside/minecraft-3dgs-training-result-query-inspect.json`

### Absent label (`.tmp/mc18-live/query-absent`)

- run: `run_1782578030890_39672_0`
- `status = blocked`
- `selectedBackend = none`
- `comparisonVerdict = not_comparable`
- provider blocked honestly for target outside closed label set

Artifacts:

- `.tmp/mc18-live/query-absent/minecraft-3dgs-training-result-query.json`
- `.tmp/mc18-live/query-absent/minecraft-3dgs-training-result-query-inspect.json`

## Honest limits

- MC-18 answers from bounded fixture closed-label lookup only; not Gaussian inference
- Provider does not use `MinecraftProjector`; `basis_frame_id` uses `closed_scene_toy:` prefix
- MC-13/14 read-side consumers accept `selected_backend = closed_scene_toy` without schema changes
