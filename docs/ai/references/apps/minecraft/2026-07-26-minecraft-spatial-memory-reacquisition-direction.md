# Minecraft spatial-memory reacquisition direction (2026-07-26)

Direction note for the restored 3DGS lane. Records why the lane's product
question is **agent spatial memory**, not a trained splat, and what the current
code and captured evidence can and cannot answer about that question.

Status: **direction note — docs-only.** Opens a planning lane. Does not approve
implementation, contract changes, or CLI surface.

Evidence level: **read of committed code plus measurement of one local capture.**
No trainer was installed, no training run was executed, and no live Minecraft
session was recorded for this note.

## Why this note exists

The lane's own charter already scoped 3DGS as a means, not an end.
[`2026-06-14-3d-minecraft-spatial-skill.md`](2026-06-14-3d-minecraft-spatial-skill.md)
§8 states it directly:

> With a Fabric mod you have raycast + depth from the truth source — a stronger,
> cheaper, exact occlusion/visibility signal than 3DGS. So in modded MC you do
> not need 3DGS for occlusion. Its real role: **MC is the answer-key gym to
> incubate a vision-only spatial-memory capability you will deploy later on 3D
> apps that do NOT expose truth.**

[`scan/2026-07-05-surface-slam-direction.md`](../../scan/2026-07-05-surface-slam-direction.md)
supplies the matching product question under "Spatial grounding boundary":

> Can it be reacquired after viewpoint motion?

That question, not splat fidelity, is the falsifiable core of spatial memory.
This note exists because the restored code and the only available capture were
both measured against it, and neither currently reaches it. Recording that now
matters because the capture is untracked local data (see Finding 3) and these
numbers are otherwise unrecoverable.

## Finding 1: the query contract cannot express a reacquisition query

`TrainingResultSpatialQueryInputs` (`training_result_spatial_query.rs`) accepts a
semantic manifest path, a target block, an optional target face, target
semantics, backend switches, and an output directory. It accepts **no observer
viewpoint** — no pose, no current frame, no camera basis.

`select_reference_frame` consequently resolves the answer by searching recorded
history for a frame that *already observed* the target: newest-first over
`in_game` frames, matching `raycast_hit.block_pos` first, then falling back to
`nearby_blocks`. With no match it returns
`TrainingResultSpatialQueryReason::TargetBlockAbsentFromScenePacket`.

So the contract answers:

```text
In which recorded frame did I see this block, and where was it on that frame?
```

It cannot answer:

```text
Given my current viewpoint, where is that remembered block on screen now?
```

The first is observation retrieval. The second is spatial memory. The gap is at
the **contract** level, so it survives any improvement to capture quality or
trainer backend: a perfect splat behind `checkpoint_native` still has nowhere to
receive a current viewpoint.

This also explains why 3DGS previously looked load-bearing for reacquisition.
With no viewpoint parameter there was no place to reproject world-anchored
memory into the present view, so a learned scene representation appeared to be
the only route to an answer.

## Finding 2: addressable memory is one block per frame

`select_reference_frame` matches `raycast_hit` first and `nearby_blocks` second.
`TelemetryRecorder.java` never populates the nearby-block list: `recordTick`
calls `populatePlayerPose`, `populateRaycast`, `populateInventory`,
`populateScreenState`, and `populateResourcePacks`, and no other method writes
`TelemetrySample.nearbyBlocks`. The field and its serializer
(`appendNearbyBlocks`) exist, so the shape is reserved but unfilled.

For any real capture the second branch is therefore unreachable, and the
addressable set of query targets equals **the set of blocks the crosshair
actually pointed at** — one candidate per frame.

That bounds what "memory" can currently mean: it retains the attention target of
each frame, not the scene around it. It also means the existing
`TODO(opensplat-seed-cloud-densification)` unlock condition ("once telemetry
populates nearby_blocks") cannot become true without a mod change first.

## Finding 3: the only available capture has a single viewpoint

`sidecar/minecraft-telemetry/run/auv/telemetry.jsonl` — 33982 frames, 117 MB,
**not tracked by git**. Full scan of every line, parsed as JSON:

| Property | Value |
| --- | --- |
| Distinct camera poses | **1** (eye x/y/z, yaw, pitch all zero-span) |
| Distinct `view_matrix` values | **1** |
| Unique `raycast_hit` blocks | **1**, face `north` on all 33982 frames |
| Frames with non-empty `nearby_blocks` | **0** |
| Frames with `screenshot_artifact_ref` | **0** |
| Telemetry sessions | 1 |
| Viewports observed | `(854, 480)`, `(1708, 960)` |

Method: read every line, count distinct `player_pose` and `view_matrix` tuples,
count distinct raycast block positions and faces, count non-empty
`nearby_blocks` and non-null `screenshot_artifact_ref`. Unparsed lines: 0.

Reacquisition requires at least two viewpoints that both relate to the same
target. This capture has one. The capture is therefore unusable for the lane's
core question **independently of 3DGS** — the limit is viewpoint count, not
representation quality.

Two of these gaps are capture-process problems rather than mod limitations. The
mod samples on `WorldRenderEvents.LAST`, so pose varies whenever the player
moves; screenshots are never produced by the mod at all, they are bound on the
AUV side. A new recording protocol addresses both. `nearby_blocks` is different
and needs the mod change from Finding 2.

## Finding 4: no current path answers without recorded truth

None of the query backends derive an answer from a learned representation:

- `ProjectionReference` — pure projective geometry over a recorded scene-packet
  frame via `MinecraftProjector`.
- `CheckpointNative` — validates normalized-result paths, scans
  `nerfstudio_models/*.ckpt`, records the latest checkpoint's relative path as
  `basis_frame_id`, then delegates the actual projection to
  `run_projection_reference_backend`. Its own known limit says so: "Gaussian
  render inference is deferred".
- `ClosedSceneToy` — bounded fixture/label lookup, explicitly not projection and
  not inference.
- `CommandProvider` — defers to an external command not present in this
  repository.

So the "provider seam is not model truth" invariant from
[`runtime/2026-06-27-core-spatial-result-consumption-pattern.md`](../../runtime/2026-06-27-core-spatial-result-consumption-pattern.md)
currently holds in its strongest form: no backend in-tree answers a spatial
query from anything except recorded geometry or a fixture.

## What the paradigm needs

Stated so it can be falsified rather than demonstrated:

```text
spatial memory = world-anchored target
               + viewpoint-conditioned projection
               + verification against an independent answer key
```

3DGS is one way to supply the middle term when an application refuses to expose
camera matrices. It is not the memory itself. This maps onto the grounding
ladder in the surface-SLAM note by truth availability rather than by app genre:

| Consumer | Viewpoint source | Grounding tier |
| --- | --- | --- |
| Engine-instrumented game | Engine matrices (this lane's Fabric mod) | 2.5D / 3D with answer key |
| Camera / passthrough video | None; must be inferred | 3D scene map, no answer key |
| Spatial headset | Device pose from on-device sensing | 3D scene map with device truth |

The engineering value of keeping one query contract across all three is that the
agent-facing question stays identical while the viewpoint backend changes. That
is the reusable artifact — not the trainer chain.

## Proposed falsifiable gate (not approved)

Smallest experiment that measures reacquisition and requires no training:

```text
frame A: target observed, answer key known
  -> derive memory from A alone
frame B: different camera pose
  -> ask the query for the target using B's viewpoint only
  -> score against B's own raycast/projection answer key
  -> report pixel error, visibility-class accuracy, backend provenance
```

This is the `Quality Measurement` stage of the core consumption pattern applied
to reacquisition instead of render fidelity. It is hermetic and fixture-first as
the surface-SLAM note requires: `MinecraftProjector` exists, scene-packet frames
already carry matrices, and two synthetic camera matrices plus one known block
yield deterministic expected pixels with no live client.

A useful secondary outcome: if recorded geometry alone reacquires well, 3DGS is
only required for the no-truth consumers, which is itself a result worth having
before investing in a trainer.

Prerequisite: Finding 1. The gate cannot be built without a viewpoint input, and
that is a contract change needing owner approval.

## Non-goals

- Not training 3DGS, and not completing the trainer chain.
- Not changing the spatial query contract in this note.
- Not reconnecting CLI, `run_read`, or inspect surfaces for the restored lane.
- Not modifying the Fabric mod.
- Not declaring the checkpoint-native provider broken; it is honest about its
  own deferral.
- Not proposing core extraction. The consumption pattern note still governs that
  and still requires a second vertical.

## Deferred, with triggers

- **Viewpoint-conditioned query input.** Blocked on owner approval of a contract
  slice. Unlocks the gate above.
- **`nearby_blocks` population in the mod.** Unlocks scene-scale memory instead
  of crosshair-only memory, and unlocks the existing seed-cloud densification
  TODO.
- **Multi-viewpoint capture with bound screenshots.** Needs a live recording
  session; unlocks any real-data reacquisition measurement.
- **Gaussian-native inference behind `checkpoint_native`.** Only becomes the
  critical path for consumers with no viewpoint source. Ordering it before the
  gate would measure the trainer instead of the paradigm.

## Related

- [`2026-06-14-3d-minecraft-spatial-skill.md`](2026-06-14-3d-minecraft-spatial-skill.md) — lane charter, 3DGS as act 3
- [`2026-07-26-minecraft-3dgs-trainer-backend-evidence.md`](2026-07-26-minecraft-3dgs-trainer-backend-evidence.md) — trainer backend reachability on Apple Silicon
- [`2026-06-27-minecraft-spatial-query-contract-design.md`](2026-06-27-minecraft-spatial-query-contract-design.md) — the contract measured in Finding 1
- [`runtime/2026-06-27-core-spatial-result-consumption-pattern.md`](../../runtime/2026-06-27-core-spatial-result-consumption-pattern.md) — stage vocabulary and provider-seam invariant
- [`scan/2026-07-05-surface-slam-direction.md`](../../scan/2026-07-05-surface-slam-direction.md) — grounding boundary and reacquisition question
