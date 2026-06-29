# 2026-06-18 Minecraft MC-6 run preparation exploration

Date: 2026-06-18

Status: exploration ledger plus implemented preparation-slice handoff. This is
not a closure report. As of 2026-06-18 owner override, MC-6 was intentionally
held in an unlive / not-numerically-closed state while MC-7 started as a
separate offline inspect-artifact lane. As of 2026-06-23 owner reopen, that
prepare-only hold is lifted for the next slice: MC-6 may re-enter the live
chain, but it is still not numerically closed and must first clear the constant
~119 px projection/convention bug signature recorded in
`2026-06-19-minecraft-mc6-texture-sweep-gate-verdict.md`.

## Current repo state checked

- Branch: `main`.
- Latest local commit observed: `2b0a6b7 feat(minecraft): gate mc6 real sweep sources`.
- `main` was ahead of `origin/main` by 10 commits at the time of this pass.
- Only untracked path observed: `.codex-worktrees/realtime-session-substrate/`.
  Do not enter, delete, stage, or commit that worktree unless the owner says so.

## What is already implemented

From `2026-06-18-auv-mc5-onward-execution-plan.md` and
`2026-06-18-minecraft-mc6-spatial-dataset-measurement-design.md`:

- `auv-cli minecraft export-spatial-bundle <run-id> --output-dir <dir>` exists.
- `auv-cli minecraft eval-texture-sweep --samples <json> --output-dir <dir>`
  exists.
- MC-6 closure invocations must add `--require-real-source`.
- The real-source gate rejects missing source blocks and generator names
  containing `fixture`, `smoke`, or `test`.
- The Fabric sidecar code has fields for `screen_state` and
  `resource_pack_ids`.
- The Fabric sidecar telemetry shape now also carries `telemetry_session_id`
  so accepted duration does not silently span multiple client sessions.
- The evaluator is intentionally offline and consumes precomputed
  `TextureSweepSampleSet` JSON.
- The sample builder now dedupes repeated observations by
  `(spatial_frame_id, refused_noise)`, computes duration from accepted frames
  only, and reads `minecraft-projection.mismatch_refusal_reason` before falling
  back to conservative refusal classification.

## What is not closed

MC-6 is not numerically closed. The missing evidence is still:

```text
resource_pack_count       = 3
profiles                  = rich, flat_color, repetitive
per_pack_duration_seconds = 30.0
pose_error_p95_max_px     = 8.0
occlusion_iou_min         = 0.85
noise refusal             = at least one exercised refusal
```

The required final command shape is:

```bash
auv-cli minecraft eval-texture-sweep \
  --samples <real-samples.json> \
  --output-dir <dir> \
  --require-real-source \
  --store-root .auv \
  --inspect-server-write false
```

Do not claim MC-6 closed until the report table comes from real sample
provenance citing source run ids and bundle manifest paths. Also do not claim
closure from tables whose `sample_count` came from duplicate frame copies,
whose `duration_seconds` crosses telemetry sessions, or whose
`noise_refusal_exercised` was satisfied only by `screenshot_unavailable` /
`telemetry_unreliable`.

## Evidence inventory checked

Checked local `.auv` runs for:

- `minecraft-spatial-frame`
- `minecraft-projection`
- `minecraft-screenshot`
- `resource_pack_ids`
- texture sweep samples/reports
- spatial bundle files

Findings:

- Existing `.auv` contains many old Minecraft MC-2/MC-3/MC-4 runs with
  `minecraft-screenshot`, `minecraft-projection`, and `operation-result`.
- No existing `.auv` run was found with `minecraft-spatial-frame` artifacts.
- No existing `.auv` run was found with `resource_pack_ids` in recorded run
  artifacts.
- No existing texture sweep sample/report or spatial bundle output was found in
  committed or ignored `.auv` state.

Conclusion: old MC-2/MC-3/MC-4 evidence cannot close MC-6. It is useful as
historical live-click/refusal evidence only. MC-6 remains explicitly unclosed
until a future owner decision reopens its live K-pack sweep.

## Sidecar state checked

Checked `devtools/auv-game-minecraft/run/auv/telemetry.jsonl`:

- It exists and had 1333 lines at exploration time.
- The tail was menu-state frames with `screen_state: "menu"`.
- The observed current JSONL did not include `resource_pack_ids`.

Checked sidecar run config:

- `devtools/auv-game-minecraft/run/resourcepacks/` existed but was empty.
- `devtools/auv-game-minecraft/run/options.txt` contained
  `resourcePacks:["fabric"]`.
- No reusable resource-pack zips were found under the sidecar run directory.

Checked local Minecraft/Fabric cache:

- Minecraft/Fabric 1.21.1 jars exist under the Gradle/Fabric Loom cache.
- Yarn mappings expose `SharedConstants.getResourceVersion(ResourceType)`, so
  if the pack format is needed precisely, derive it from local 1.21.1 sources
  or a tiny Java/Gradle probe rather than guessing.

Conclusion: current sidecar output proves the telemetry writer is alive enough
to emit `screen_state`, but the historical snapshot checked in this exploration
is not sufficient MC-6 resource-pack provenance. New closure runs should expect
`resource_pack_ids` plus `telemetry_session_id` on fresh telemetry.

## Existing code shape

Relevant files:

- `crates/auv-game-minecraft/src/measurement.rs`
- `crates/auv-game-minecraft/src/dataset.rs`
- `crates/auv-game-minecraft/src/types.rs`
- `crates/auv-game-minecraft/src/projection.rs`
- `src/cli.rs`
- `src/main.rs`
- `src/minecraft.rs`
- `devtools/auv-game-minecraft/src/main/java/ai/moeru/auv/minecraft/telemetry/TelemetryRecorder.java`
- `devtools/auv-game-minecraft/src/main/java/ai/moeru/auv/minecraft/telemetry/TelemetrySample.java`

Important shape:

- `TextureSweepSampleSet` is a precomputed JSON input shape.
- `TextureSweepSampleSource` is the provenance block copied into reports.
- `SpatialBundleManifest` records source run metadata and copied artifact
  records.
- `MinecraftSpatialFrame` already carries `raycast_hit`, matrices, viewport,
  `screen_state`, and `resource_pack_ids`.
- `MinecraftProjector` can project block targets through existing camera
  matrices. Do not add a second projection math implementation for MC-6.

## Scope decision for the next slice

Owner said on 2026-06-18: "先不跑真实链路,先准备跑".

That statement explains why this note originally stopped at a preparation-only
substrate. It is now historical context, not the current execution boundary.
The owner has since explicitly reopened MC-6 live work on 2026-06-23, so the
next slice may launch Minecraft/Fabric and continue the real chain again — but
only after a single-frame projection/overlay check clears the constant ~119 px
offset signature from the 2026-06-19 verdict.

Therefore the next slice should reopen MC-6 execution in this order:

1. Reconfirm the projection basis on one real frame / overlay pair through the
   dedicated `minecraft calibrate-projection` path.
2. If needed, fix the constant-offset convention bug before widening the live
    sweep.
3. Run the real client and collect new live source runs through the MC-6 bridge
   path, preferably using direct `window.capture` selection instead of ad-hoc
   external screenshots.
4. Export spatial bundles, build `TextureSweepSampleSet`, and keep
    `--require-real-source` as the closure gate.
5. Update docs/report state to "reopened, still not numerically closed until the
    rebuilt table passes".

## Implemented preparation substrate

Implemented command surfaces:

```text
auv-cli minecraft prepare-texture-sweep \
  --sidecar-run-dir devtools/auv-game-minecraft/run \
  --output-dir .tmp-mc6-prep

auv-cli minecraft build-texture-sweep-samples \
  --bundle-manifest <rich-bundle>/run.json \
  --bundle-manifest <flat-bundle>/run.json \
  --bundle-manifest <repetitive-bundle>/run.json \
  --output <real-samples.json>
```

The first command creates/refreshes ignored local packs under
`devtools/auv-game-minecraft/run/resourcepacks/` and writes an auditable manifest
plus runbook under the requested output directory. The second command reads real
bundle manifests and copied `minecraft-spatial-frame` artifacts, then writes
`TextureSweepSampleSet` with generator `mc6.bundle-texture-sweep` and source
run/bundle provenance.

Preparation smoke run:

```text
run_1781776132841_15186_0
```

Negative smoke:

```text
run_1781776150086_15555_0
```

The negative smoke intentionally passed a missing bundle manifest and failed
with `failed to read MC-6 spatial bundle manifest ... No such file or
directory`, proving the builder does not fabricate sample JSON without real
bundle provenance.

## Red lines

- Do not treat MC-6 as live-run or numerically closed **until new post-reopen
  live evidence exists**.
- Do not use MC-7 work as a substitute for the missing MC-6 K-pack table.
- MC-7 may proceed only under the separate owner-opened offline
  inspect-artifact lane; it does not change this MC-6 status.
- Do not jump straight to a widened K-pack sweep while the constant ~119 px
  projection/convention bug signature remains unvalidated.
- Do not fabricate real-source sample JSON by hand.
- Do not commit generated resource pack zips or sidecar `run/` output.
- Do not put Minecraft nouns into AUV core.
- Do not add a third action-result schema.
- Sidecar remains read-only truth/verifier; actions still go through AUV
  driver paths.

## Validation target for the reopened closure substrate

Minimum before committing the reopened MC-6 closure slice:

```bash
cargo fmt --check
cargo check
cargo test -p auv-game-minecraft
cargo test --bin auv-cli minecraft
git diff --check
```

Optional smoke after the code slice is stable:

```bash
auv-cli minecraft calibrate-projection \
  --frame <canonical-frame.json> \
  --screenshot <canonical-frame.png> \
  --target-block 511,73,728 \
  --target-semantics hit_face_center \
  --screenshot-is-minecraft-window true
```

This smoke may write ignored local artifacts and recorded run evidence. It is
still narrower than the fresh live mini-sweep and should be used to clear the
geometry gate before relaunching the real client.
