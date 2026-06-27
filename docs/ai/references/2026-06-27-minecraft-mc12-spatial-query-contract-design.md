# MC-12: 3DGS training result spatial query contract

## Verdict boundary

MC-12 v1 is a **block-only spatial query contract** over MC-10 semantic manifests. It
answers whether a semantic-ready training result can produce an auditable screen-point /
visibility answer for one block target.

MC-12 is **not**:

- a model-quality or splat-usefulness gate
- a checkpoint-native or Gaussian-native query core
- entity / anchor / label query
- render preview, action integration, or dedicated read-side viewer consumption

`projection_reference` is an in-repo **reference fallback** using
`scene_packet + MinecraftProjector + mc6_projection_target_for_frame`. It closes the
query contract honestly but does **not** claim real Gaussian inference.

Checkpoint-native / Gaussian-native query cores remain deferred until owner approval and
until source contracts expose stable query nouns.

## Command

```bash
auv-cli minecraft query-3dgs-training-result \
  --training-result-semantic-manifest <minecraft-3dgs-training-result-semantic.json> \
  --target-block <x,y,z> \
  [--target-face <up|down|north|south|east|west>] \
  [--target-semantics hit_face_center|block_center] \
  [--query-command <command>] \
  --output-dir <dir>
```

Input boundary:

- MC-12 consumes **MC-10 only** (`minecraft-3dgs-training-result-semantic.json`).
- Lineage continues through `source_scene_packet_manifest_path`; MC-12 does not accept
  D11/D7/D6 CLI inputs directly.

## Backends

1. `projection_reference` — always attempted when MC-10 `semantic_status = ready`.
2. `command_provider` — only when `--query-command` is present.

Selection:

- provider `answered` > reference `answered` > most specific `blocked` / `failed`

Comparison when both backends answer:

- `match | divergent | provider_only | reference_only | not_comparable`

`visibility = behind_camera | out_of_frustum | outside_window` may still be `answered`.
`failed` means no answer or backend execution/contract failure. `blocked` means upstream
preconditions such as semantic source not ready.

v1 inspect convention: when `--query-command` is omitted, `provider_status=blocked` in the
inspect report means the command provider was not configured (intentional), not a runtime
failure.

## Artifacts

- role `minecraft-3dgs-training-result-query`
- role `minecraft-3dgs-training-result-query-inspect`
- files `minecraft-3dgs-training-result-query.json` and
  `minecraft-3dgs-training-result-query-inspect.json`

Implementation module: `crates/auv-game-minecraft/src/training_result_spatial_query.rs`.

## Collabi

No automated Collabi check-in entrypoint was found in this repository during MC-12
implementation. Manual writer flow:
`https://collabi-airi-cu-free-01.koreacentral.cloudapp.azure.com/writer.html`.

## Related slices

- MC-10 semantic gate: `docs/ai/references/2026-06-27-minecraft-mc10-result-semantic-validation-design.md`
- MC-11 read-side semantic inspect consumer:
  `docs/ai/references/2026-06-27-minecraft-mc11-semantic-read-side-inspect-consumer-design.md`
