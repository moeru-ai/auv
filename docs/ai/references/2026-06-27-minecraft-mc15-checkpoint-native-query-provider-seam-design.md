# MC-15: Checkpoint-native query provider seam

Date: 2026-06-27

## Verdict boundary

MC-15 v1 closes the **checkpoint-native / Gaussian-native query provider seam** for MC-12.
It adds an in-repo provider adapter that **reads normalized training-result inputs**
(`config.yml`, `nerfstudio_models/` checkpoints) and still emits the existing MC-12
`TrainingResultSpatialQueryAnswer` wire shape for dual-backend compare.

MC-15 is **not**:

- Gaussian render inference or splat-quality judgment
- action dispatch / MC-14+ live-click wiring
- render preview / holdout / usefulness gate (**MC-16**)
- a new artifact role or persisted JSON file type

v1 honest behavior: validate normalized-result inputs, record checkpoint basis witness,
project with `scene_packet + MinecraftProjector` (same math as `projection_reference`),
and populate MC-12 manifest + inspect dual-backend fields.

## Adapter contract

Module: `crates/auv-game-minecraft/src/training_result_spatial_query_provider.rs`

- `CheckpointNativeProviderInputs` — derived from `TrainingResultSpatialQueryRequest` +
  MC-10 semantic manifest paths
- `CheckpointNativeProviderOutcome` — maps to `TrainingResultSpatialQueryAnswer`
- `run_checkpoint_native_provider_backend` — in-repo provider entrypoint

Backend enum extension in `training_result_spatial_query.rs`:

- `checkpoint_native` (`TrainingResultSpatialQueryBackend::CheckpointNative`)

External `command_provider` (`--query-command`) remains the escape hatch for contract tests.

## Normalized input read (D2)

| Path | Purpose |
| --- | --- |
| `normalized_result_dir/config.yml` | trainer / backend witness |
| `normalized_result_dir/nerfstudio_models/` | checkpoint scan via `collect_checkpoint_files` |
| MC-10 semantic manifest | `semantic_status` gate + authoritative normalized paths |

v1 policy:

1. `semantic_status != ready` → `blocked`
2. invalid normalized paths / missing config / models dir / checkpoints → `blocked` or `failed`
3. checkpoint readable → provider may `answer`; projection math uses scene packet reference path;
   `basis_frame_id` records latest checkpoint witness (`checkpoint:<relative_path>`);
   `known_limits` / provider `message` state Gaussian render inference is deferred

## CLI wiring (D3)

Existing command only:

```sh
auv-cli minecraft query-3dgs-training-result \
  --training-result-semantic-manifest <semantic.json> \
  --target-block <x,y,z> \
  [--target-face <face>] \
  [--target-semantics hit_face_center|block_center] \
  [--query-provider checkpoint-native] \
  [--query-provider closed-scene-toy] \
  [--closed-scene-fixture <fixture.json>] \
  [--query-command <command>] \
  --output-dir <dir>
```

- `--query-provider checkpoint-native` and `--query-command` are mutually exclusive
- default unchanged: reference-only when no provider flag

Selection / inspect fields remain MC-12 (`provider_status`, `reference_status`,
`comparison_verdict`, `selected_backend`).

## Related slices

- MC-12 spatial query contract:
  `docs/ai/references/2026-06-27-minecraft-mc12-spatial-query-contract-design.md`
- MC-10 semantic gate:
  `docs/ai/references/2026-06-27-minecraft-mc10-result-semantic-validation-design.md`
- MC-15 live closure:
  `docs/ai/references/2026-06-27-minecraft-mc15-checkpoint-native-query-provider-live-closure.md`

## Sibling provider seam

MC-18 closed-scene toy provider (parallel seam, no reference projector):
`docs/ai/references/2026-06-27-minecraft-mc18-closed-scene-toy-provider-design.md`.
Live closure: `docs/ai/references/2026-06-27-minecraft-mc18-closed-scene-toy-provider-live-closure.md`.

## Deferred

- MC-15+ / MC-17: true Gaussian render inference inside query providers (MC-16/17
  holdout witness/quality evidence does not close this gap)


## Closed related slices

- MC-14 action-readiness consumer:
  `docs/ai/references/2026-06-27-minecraft-mc14-spatial-query-action-facing-consumer-design.md`
- MC-16 holdout preview:
  `docs/ai/references/2026-06-27-minecraft-mc16-holdout-preview-render-inspect-design.md`
- MC-17 holdout render quality:
  `docs/ai/references/2026-06-27-minecraft-mc17-holdout-render-quality-design.md`
