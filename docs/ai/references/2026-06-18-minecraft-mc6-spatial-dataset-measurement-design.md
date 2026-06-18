# 2026-06-18 Minecraft MC-6 spatial dataset recorder and 2.5D measurement design

Date: 2026-06-18

Status: accepted local slice design for MC-6 implementation. This note narrows
Slice C from `2026-06-18-auv-mc5-onward-execution-plan.md` into an offline
recorder and measurement contract. It does not open MC-7.

## Scope

`approved feature`, Minecraft vertical only.

MC-6 is an offline evidence lane. It reads completed AUV runs and emits a
spatial dataset bundle that later measurement tools can consume. It does not
need the realtime session substrate, does not add an agent loop, and does not
put Minecraft nouns into core.

## Recorder bundle v0

Each exported source run produces one bundle:

```text
<bundle>/
  screenshots/
  spatial_frames/
  actions/
  verification/
  overlays/
  run.json
```

`run.json` is the bundle manifest, not the canonical AUV run record. The
canonical run remains the source of truth in `.auv/runs/<run_id>/`; the bundle
records copied files plus source artifact lineage.

Manifest v0 fields:

- `schema_version = 1`
- `source_run_id`, source operation name, source run type, source status
- `generated_at_millis`
- `auv_git_commit` and `exporter_git_commit` when known
- per-directory artifact counts
- artifact records: source artifact id, source role, source path, bundle path,
  and optional source summary
- `known_limits`

The exporter copies only existing run artifacts. It must not synthesize labels
that the source run did not record. Empty directories are valid but visible in
the manifest counts.

Current role mapping:

```text
minecraft-screenshot       -> screenshots/
minecraft-spatial-frame    -> spatial_frames/
minecraft-projection       -> spatial_frames/
candidate-action-*         -> actions/
operation-result           -> verification/
minecraft-overlay          -> overlays/
```

`minecraft-spatial-frame` is the new source artifact role added by this slice.
It stores the full `MinecraftSpatialFrame` JSON for the observed frame so the
bundle does not have to reconstruct matrices, pose, raycast, or nearby witness
state from a projection artifact.

For MC-6 sweep provenance, the Fabric sidecar also records `resource_pack_ids`
on every telemetry sample and therefore every persisted `minecraft-spatial-frame`
artifact. This is read-only client state from the same Minecraft client AUV is
capturing and driving; it is not an action path and it does not decide whether a
pack satisfies the rich / flat-color / repetitive profile labels.

NOTICE(mc6-action-artifact-gap): current MC live-click evidence does not yet
persist a first-class `InputActionResult` artifact inside the Minecraft source
run. Until that seam is approved, `actions/` may be empty for MC bridge runs and
the exporter must not invent a third action-result schema.

## Measurement contract v0

The first MC-6 consumer is a 2.5D-baseline texture sweep evaluator. It consumes
precomputed measurement samples; it does not run 3DGS, does not choose a
representation, and does not collect live screenshots by itself.

Thresholds for the first sweep are fixed before running:

```text
pose_error_p95_max_px       = 8.0
occlusion_iou_min           = 0.85
resource_pack_count         = 3
required texture profiles   = rich, flat_color, repetitive
per_pack_duration_seconds   = 30.0
refuse_on_noise_rule        = "exclude refused noisy frames from metrics, but require at least one exercised refusal"
```

The evaluator computes per-pack:

- sample count
- refused-noise count
- pose error p95 from non-refused samples
- minimum occlusion IoU from non-refused samples
- duration pass/fail
- pose pass/fail
- occlusion pass/fail

The input sample set may include a `source` block:

```text
source.generated_at_millis
source.generator
source.source_run_ids[]
source.bundle_manifest_paths[]
source.known_limits[]
```

The evaluator copies this source block into `texture_sweep_report.json` and the
CLI records both the sample JSON and the report as run artifacts. A real MC-6
sweep must cite the source run ids / bundle manifests in this block; a fixture or
synthetic sample file may exercise the evaluator but cannot close the numerical
gate. Closure runs use `auv-cli minecraft eval-texture-sweep --require-real-source`,
which rejects missing source blocks and fixture/smoke/test generators unless the
source cites source run ids plus bundle manifest paths.

The overall report passes only when every pack passes pose, occlusion, and
duration gates, the expected K packs are present, the required texture profiles
are covered, and the noise refusal rule is exercised at least once.

The report is the only technical forcing input for the session-floor vs 2.5D vs
3DGS decision. A failed or missing report does not imply "start 3DGS"; it means
MC-6 is incomplete.

## Acceptance

- Minecraft projection/live-click runs persist `minecraft-spatial-frame`
  artifacts in addition to existing screenshot/projection/overlay/verification
  artifacts.
- `auv-cli minecraft export-spatial-bundle <run-id> --output-dir <dir>` writes
  the v0 bundle shape and records the manifest as a run artifact.
- Inspect/read-side can list recorded MC-6 bundle manifests.
- `auv-cli minecraft eval-texture-sweep --samples <json> --output-dir <dir>`
  emits the p95/IoU table from pre-set thresholds and refuses reports that did
  not exercise the noise rule. The run records the input sample file and report
  file as separate artifacts so the table is auditable after the CLI exits.
  MC-6 closure invocations add `--require-real-source`; fixture invocations may
  omit it only for plumbing tests.
- `cargo fmt --check && cargo check && cargo test && git diff --check`.

## Explicit non-goals

- No MC-7 or 3DGS artifact.
- No dense photometric mismatch refusal class.
- No new action-result schema.
- No Mineflayer/MCP/mod action path.
- No realtime daemon transport work.
