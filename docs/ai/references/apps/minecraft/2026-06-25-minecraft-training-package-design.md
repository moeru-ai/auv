# 2026-06-25 Minecraft MC-7 D3 training-package design

Date: 2026-06-25

Status: implemented code slice; synthetic validation closed. Historical
accepted-only lineage is no longer restorable, so any future real accepted-run
reference validation requires a fresh accepted-only capture lineage.

## Scope

MC-7 D3 is defined as:

- consume one accepted-only D2 scene packet manifest
- export one canonical training-prep package
- export one Nerfstudio-compatible view
- do not start training
- do not add action-path behavior

D3 stays an offline artifact lane.

The next downstream consumer is MC-7 D5:

- D5 consumes this package as the first trainer-side launch/readiness surface
- D5 still does not mean real training has run
- trainer execution remains a later explicit slice

## Input boundary

D3 no longer reads MC-6 bundles directly.

Its only code-level input is:

- `scene-packet/run.json`

and the sibling D2 files that belong to that scene packet:

- `cameras.json`
- `known_limits.json`
- `frames/frame_*.json`
- copied frame screenshots

This preserves the D2 accepted-only packet as the first real-source boundary.

## Output shape

The canonical output directory is:

```text
run.json
frames/frame_000001.json
images/frame_000001.png
cameras.json
known_limits.json
inspect_report.json
compat/nerfstudio/export_report.json
compat/nerfstudio/transforms.json   # only when at least one frame is compatible
compat/nerfstudio/images/*
```

Two artifact roles are staged:

- `minecraft-3dgs-training-package`
- `minecraft-3dgs-training-package-inspect`

## Canonical vs compatibility truth

The canonical package is the authority.

That means:

- every D2 frame is preserved in canonical output
- canonical output keeps source lineage
- canonical output does not silently drop unusable frames just because an
  external format cannot accept them

The Nerfstudio view is only an attached compatibility export.

Compatibility status is:

- `ready`: every canonical frame exported into the compatibility view
- `partial`: at least one frame exported, but not all
- `blocked`: zero frames exported

`partial` and `blocked` do not fail the canonical package.

## Compatibility skip semantics

The compatibility view skips frames for these reasons:

- `missing_screenshot`
- `non_ingame_screen_state`
- `no_file_resource_pack`
- `multiple_file_resource_packs`
- `invalid_view_matrix`
- `invalid_projection_matrix`
- `noninvertible_camera_transform`
- `invalid_intrinsics`

These reasons are reported in:

- canonical frame records
- `inspect_report.json`
- `compat/nerfstudio/export_report.json`

## Matrix policy

The compatibility export deliberately reuses the existing MC projection
compatibility stance instead of inventing a second matrix policy.

That means:

- `view_matrix` is treated as world-to-camera
- the exporter inverts it to produce Nerfstudio `transform_matrix`
- if the matrix is a legacy rotation-only matrix with zero translation, the
  exporter uses `player_pose.eye_position` to synthesize translation before
  inversion
- use of that fallback is explicitly reported as a known limit

No extra axis-system “beautification” is added in D3.

## Failure policy

Hard failures are reserved for canonical input corruption, including:

- missing or unparsable `scene-packet/run.json`
- missing or unparsable `cameras.json`
- missing or unparsable referenced frame JSON
- a frame claims a screenshot path, but the underlying file is physically missing
- camera/frame sets do not line up

Compatibility-only failures do not invalidate canonical export.

## Non-goals

- no trainer/backend selection
- no remote GPU job
- no viewer-specific command
- no rewrite of D2 history
- no real-run completion claim without a later fresh accepted-only smoke pass

D5 may add a fixed trainer launch-prep contract on top of this package, but D3
itself still stops at package export plus compatibility truth.

## Reference-validation reality

The earlier plan to reuse the old local accepted lineage is no longer valid.

- the historical accepted-only `.auv` lineage is not recoverable from current
  local state
- D3 therefore remains code-closed and synthetic-validated
- if future work wants a real-source reference package, it must first fresh
  capture a new accepted-only lineage and rerun D2 -> D3 on that lineage
