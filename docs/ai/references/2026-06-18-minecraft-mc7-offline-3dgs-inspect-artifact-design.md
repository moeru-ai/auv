# 2026-06-18 Minecraft MC-7 offline 3DGS inspect artifact design

Date: 2026-06-18

Status: owner-opened design note for MC-7. This starts the 3DGS lane while MC-6
is deliberately held as unlive / not numerically closed. It does not promote
3DGS into the action path.

## Owner override

Earlier plans parked MC-7 behind the MC-6 numerical texture-sweep gate. The
current owner instruction is different:

```text
mc-6现在先到文件里面显示保留,保留未live状态,直接开mc-7
```

This document records that override. The practical meaning is:

- MC-6 remains visibly retained as preparation-only and not live-run.
- Missing MC-6 numbers are not reinterpreted as a 2.5D failure.
- MC-7 may start, but only as an offline inspect-artifact lane.
- The action path, verification seam, and refusal taxonomy remain unchanged.

## Scope

`docs-first design`, Minecraft vertical / read-side artifact lane.

MC-7 is not AUV core, not an agent, and not a replacement for the MC-2/3/4
live-proof seam. The first implementation should make a 3DGS candidate
inspectable from recorded spatial bundles. It should not train or trust a splat
inside the click path.

## Local tooling inventory

Checked on 2026-06-18 in the local checkout:

- `python3`: `/opt/anaconda3/bin/python3`, Python 3.13.5.
- Python packages present: `numpy`, `PIL`, `cv2`.
- Python packages absent: `torch`, `open3d`, `gsplat`, `nerfstudio`.
- Commands not found in the local PATH during this pass: `uv`, `nvidia-smi`,
  `colmap`.

Conclusion: the immediate local slice should not assume GPU 3DGS training or a
COLMAP/nerfstudio pipeline is available. Start with schema, manifest, conversion,
and inspect-read artifact plumbing. A later environment-prep slice can choose a
trainer/runtime after the dataset shape is stable.

## What MC-7 is for

The P0 document is still the useful north star: Minecraft is an answer-key gym
for spatial memory in future 3D surfaces that do not expose truth. Modded
Minecraft already has stronger raycast/matrix truth, so 3DGS is not needed to
make MC clicks safe. Its value is to produce and inspect a learned spatial
representation against known truth.

Therefore MC-7's first artifact is an offline comparison object:

```text
spatial bundle(s)
  -> scene packet
  -> 3DGS candidate artifact or placeholder manifest
  -> inspect report: coverage, camera count, expected-view renders, truth diff
```

Only the first two arrows are mandatory for D1 if the local machine still lacks
the training stack.

## Non-goals

- No MC-7 action delivery.
- No `InputActionResult` replacement or third action-result schema.
- No dense photometric mismatch refusal reason.
- No core graduation of Minecraft terms.
- No Mineflayer/MCP/mod action path.
- No claim that MC-6 is closed.
- No fake trained splat that pretends to be real 3DGS output.

## Data contract direction

Use existing MC-6 spatial bundles as the input boundary:

```text
<bundle>/run.json
<bundle>/screenshots/*
<bundle>/spatial_frames/* minecraft-spatial-frame JSON
<bundle>/spatial_frames/* minecraft-projection JSON
<bundle>/overlays/*
```

The first MC-7 converter should emit a scene-packet manifest, not a trained
model:

```text
<scene-packet>/
  run.json
  frames/
    frame_000001.json
    frame_000001.png
  cameras.json
  known_limits.json
```

`run.json` should record:

- schema version
- source bundle manifest paths
- source run ids
- frame count
- camera/frame ids
- screenshot artifact lineage
- matrix/viewport provenance
- known limits

The frame JSON should carry the already-recorded vertical truth:

- `spatial_frame_id`
- `monotonic_timestamp_ms`
- `viewport`
- `view_matrix`
- `projection_matrix`
- `player_pose`
- `raycast_hit`
- `screen_state`
- `resource_pack_ids`
- screenshot file path inside the scene packet

This stays Minecraft-vertical for now because `block_pos`, `raycast_hit`, and
resource-pack labels are not core concepts.

## First implementation slice: D1 scene packet exporter

Implement a narrow command after this note:

```bash
auv-cli minecraft export-3dgs-scene-packet \
  --bundle-manifest <bundle/run.json> \
  --output-dir <dir> \
  --store-root .auv \
  --inspect-server-write false
```

Allowed behavior:

- Read one or more real MC spatial bundle manifests.
- Copy source screenshots and frame JSON into an MC-7 scene packet.
- Reject bundles without `minecraft-spatial-frame` artifacts.
- Record a scene-packet manifest as an AUV run artifact.
- Mark the artifact as "3DGS input scene packet", not trained 3DGS.

Deliberate deferrals:

- Training: deferred until a selected local or remote stack exists.
- COLMAP: deferred; Minecraft already provides camera matrices, so the first
  converter should not require SfM just to reconstruct known camera poses.
- Expected-view rendering: deferred until there is either a trained splat or a
  tiny deterministic placeholder renderer that is clearly labeled as not 3DGS.
- Dense mismatch refusal: deferred until an owner explicitly changes the
  refusal contract.

## D1 acceptance gate

- `2026-06-18-auv-mc5-onward-execution-plan.md` marks MC-6 as held unlive and
  MC-7 as owner-opened.
- This MC-7 note lands before code.
- The new command, if implemented, emits only an offline inspect artifact.
- The artifact cites source bundle manifests and source run ids.
- Missing spatial frames fail with a clear error.
- `cargo fmt --check && cargo check && cargo test && git diff --check`.

## Open decisions after D1

- Choose training backend: local Python package, external CLI, remote GPU job,
  or no training until a real scene packet exists.
- Decide whether the inspect server needs a specialized 3D scene viewer or
  whether manifest/render artifacts are enough for D2.
- Decide whether MC-7 should consume only MC-6 bundles or also direct AUV runs
  before export.
