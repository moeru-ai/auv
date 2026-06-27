# 2026-06-27 Minecraft MC-10 result semantic validation design

Date: 2026-06-27

Status: implemented semantic-only slice. Render preview and dedicated read-side
summary consumption are deferred to later slices.

## Scope

MC-10 closes **normalized training-result semantic inspect evidence** after MC-9
D11 artifact fetch. It answers:

```text
Are the normalized artifacts a checkable 3DGS training result package?
```

MC-10 does **not**:

- grade trainer quality or trained splat usefulness
- inspect checkpoint internal semantics
- run render preview / holdout view generation
- replace MC truth / verifier
- add dedicated read-side summary consumption (MC-11)
- enter the action path

## Input boundary

Single entry:

```text
minecraft-3dgs-training-result-artifact-manifest.json
```

MC-10 reads D11 lineage from that manifest only. It does not reopen D6/D7/D5
command inputs.

Required manifest fields:

- `normalized_result_dir`
- `trainer_backend`
- `job_backend`
- `source_training_result_manifest_path`
- `source_training_job_manifest_path`
- `source_training_launch_plan_path`
- `source_training_package_manifest_path`
- `source_scene_packet_manifest_path`
- `source_bundle_manifest_paths`
- `source_run_ids`

## Command surface

```text
auv-cli minecraft validate-3dgs-training-result \
  --training-result-artifact-manifest <d11-manifest.json> \
  --output-dir <dir>
```

Artifact roles:

- `minecraft-3dgs-training-result-semantic`
- `minecraft-3dgs-training-result-semantic-inspect`

Output files:

- `minecraft-3dgs-training-result-semantic.json`
- `minecraft-3dgs-training-result-semantic-inspect.json`

## Semantic gate

Reads under `normalized_result_dir`:

- `config.yml` must be a real file (not symlink)
- `nerfstudio_models/` must be a real directory (not symlink)
- `config.yml` must parse as YAML mapping
- top-level `trainer` scalar must exist and equal `trainer_backend`
- recursive `*.ckpt` scan must find at least one checkpoint

`job_status.json` is observed only (`present/readable`) and does not gate status.

Status model:

- `ready`: all semantic checks pass
- `blocked`: missing config/models dir or symlink/invalid path
- `failed`: files exist but semantic checks fail

## Known limits

- MC-10 v1 supports `nerfstudio.splatfacto` only
- backend hint uses top-level `trainer` scalar only
- no render preview
- no checkpoint semantic validation
- read-side inspect/viewer summary consumption is MC-11; see `docs/ai/references/2026-06-27-minecraft-mc11-semantic-read-side-inspect-consumer-design.md`

## Relationship to MC-9 / MC-11

MC-9 closes real-provider submit/status/fetch. MC-10 is the next slice and
does not retroactively change the MC-9 verdict.

MC-11 will consume semantic manifest/inspect artifacts on the read side.
MC-12 block-only spatial query over semantic manifests is documented in `docs/ai/references/2026-06-27-minecraft-mc12-spatial-query-contract-design.md`.

Fresh local live evidence:
`docs/ai/references/2026-06-27-minecraft-mc10-semantic-validation-live-closure.md`
