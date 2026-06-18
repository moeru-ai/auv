# 2026-06-18 Minecraft MC-6 run preparation exploration

Date: 2026-06-18

Status: exploration ledger for the next MC-6 preparation slice. This is not a
closure report.

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
- The evaluator is intentionally offline and consumes precomputed
  `TextureSweepSampleSet` JSON.

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
provenance citing source run ids and bundle manifest paths.

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
historical live-click/refusal evidence only.

## Sidecar state checked

Checked `sidecar/minecraft-telemetry/run/auv/telemetry.jsonl`:

- It exists and had 1333 lines at exploration time.
- The tail was menu-state frames with `screen_state: "menu"`.
- The observed current JSONL did not include `resource_pack_ids`.

Checked sidecar run config:

- `sidecar/minecraft-telemetry/run/resourcepacks/` existed but was empty.
- `sidecar/minecraft-telemetry/run/options.txt` contained
  `resourcePacks:["fabric"]`.
- No reusable resource-pack zips were found under the sidecar run directory.

Checked local Minecraft/Fabric cache:

- Minecraft/Fabric 1.21.1 jars exist under the Gradle/Fabric Loom cache.
- Yarn mappings expose `SharedConstants.getResourceVersion(ResourceType)`, so
  if the pack format is needed precisely, derive it from local 1.21.1 sources
  or a tiny Java/Gradle probe rather than guessing.

Conclusion: current sidecar output proves the telemetry writer is alive enough
to emit `screen_state`, but it is not sufficient MC-6 resource-pack provenance.

## Existing code shape

Relevant files:

- `crates/auv-game-minecraft/src/measurement.rs`
- `crates/auv-game-minecraft/src/dataset.rs`
- `crates/auv-game-minecraft/src/types.rs`
- `crates/auv-game-minecraft/src/projection.rs`
- `src/cli.rs`
- `src/main.rs`
- `src/minecraft.rs`
- `sidecar/minecraft-telemetry/src/main/java/ai/moeru/auv/minecraft/telemetry/TelemetryRecorder.java`
- `sidecar/minecraft-telemetry/src/main/java/ai/moeru/auv/minecraft/telemetry/TelemetrySample.java`

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

Owner said: "先不跑真实链路,先准备跑".

Therefore the next slice should prepare MC-6 execution without launching
Minecraft or claiming closure:

1. Generate the K=3 local resource-pack inputs into ignored sidecar run state.
2. Produce a runbook/manifest that fixes the pack ids, profiles, expected
   options entry, and final commands.
3. Add or expose a narrow conversion path from real spatial bundles to
   `TextureSweepSampleSet` so later live runs do not require hand-written JSON.
4. Keep `--require-real-source` as the closure gate.
5. Update docs to say "ready to run", not "numerically closed".

## Recommended implementation

Recommended command surfaces:

```text
auv-cli minecraft prepare-texture-sweep \
  --sidecar-run-dir sidecar/minecraft-telemetry/run \
  --output-dir .tmp-mc6-prep

auv-cli minecraft build-texture-sweep-samples \
  --bundle-manifest <rich-bundle>/run.json \
  --bundle-manifest <flat-bundle>/run.json \
  --bundle-manifest <repetitive-bundle>/run.json \
  --output <real-samples.json>
```

The first command should create/refresh ignored local packs under
`sidecar/minecraft-telemetry/run/resourcepacks/` and write an auditable manifest
under the requested output directory. The second command should read real
bundle manifests and spatial frame artifacts, then write `TextureSweepSampleSet`
with a non-fixture generator name and source run/bundle provenance.

If command-surface growth feels too much for one slice, a committed runbook plus
a private library function covered by tests is acceptable, but the later live
operator must not have to hand-write sample JSON.

## Red lines

- Do not start MC-7 or 3DGS.
- Do not run Minecraft/Fabric while this slice is "prepare only".
- Do not fabricate real-source sample JSON by hand.
- Do not commit generated resource pack zips or sidecar `run/` output.
- Do not put Minecraft nouns into AUV core.
- Do not add a third action-result schema.
- Sidecar remains read-only truth/verifier; actions still go through AUV
  driver paths.

## Validation target for preparation only

Minimum before committing the prep slice:

```bash
cargo fmt --check
cargo check
cargo test -p auv-game-minecraft
cargo test --bin auv-cli minecraft
git diff --check
```

Optional smoke for prep commands only:

```bash
auv-cli minecraft prepare-texture-sweep \
  --sidecar-run-dir sidecar/minecraft-telemetry/run \
  --output-dir .tmp-mc6-prep
```

This smoke may write ignored local artifacts. It must not launch the real
client.
