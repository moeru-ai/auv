# 2026-06-27 Minecraft MC-16 holdout preview render inspect design

Date: 2026-06-27

Status: implemented producer + read-side consumer slice.

## Scope

MC-16 closes **holdout preview manifest/inspect evidence** on top of MC-10
semantic-ready lineage. It answers:

```text
Which scene-packet holdout frame and checkpoint basis witness the trained result lineage?
```

MC-16 does **not**:

- grade trainer quality or trained splat usefulness
- run photometric holdout quality gates (PSNR/SSIM pass/fail)
- perform true Nerfstudio / Gaussian holdout render (deferred MC-16+ / MC-17)
- dispatch actions or upgrade MC-15 provider inference
- replace MC-12 spatial query read-side surfaces

## Producer seam (v1 honest boundary)

Input: MC-10 `minecraft-3dgs-training-result-semantic.json` only.

Default in-repo seam (mutually exclusive with `--holdout-render-command`):

- select last `in_game` scene-packet frame (or `--holdout-frame-index` override)
- witness holdout frame screenshot + frame json paths
- record latest `*.ckpt` under normalized `nerfstudio_models/` as `basis_checkpoint_path`
- optional deterministic reference overlay PNG when screenshot + raycast witness allow

Status model: `ready` / `blocked` / `failed` aligned with MC-10/12 honest gating.

## Read-side consumer

Symmetric MC-11 pairing on business lineage keys (semantic manifest path +
source artifact/result/job/launch/scene-packet/run ids). Surfaces:

- `run_read` lineage extract/list helpers
- `auv inspect` `MC-16 Training Result Holdout Preview:` section (after MC-10, before MC-12)
- inspect server viewer summary cards for both artifact roles

## Command surface

```text
auv-cli minecraft inspect-3dgs-training-result-holdout   --training-result-semantic-manifest <semantic.json>   [--holdout-frame-index <n>]   [--holdout-render-command <command>]   --output-dir <dir>
```

Artifact roles:

- `minecraft-3dgs-training-result-holdout-preview`
- `minecraft-3dgs-training-result-holdout-preview-inspect`

## Known limits

- `MC-16 v1 holdout preview records scene-packet holdout witness and checkpoint basis; trained splat holdout render and photometric quality judgment are deferred`
- MC-10 forward pointer: `docs/ai/references/2026-06-27-minecraft-mc10-result-semantic-validation-design.md`

## Relationship

- MC-10 semantic gate is the sole CLI input boundary (same as MC-12)
- MC-12/13/14/15 producer schemas are unchanged
- Live closure: `docs/ai/references/2026-06-27-minecraft-mc16-holdout-preview-render-inspect-live-closure.md`
