# 2026-06-27 Minecraft MC-16 holdout preview live closure

Date: 2026-06-27

Status: live closure on MC-10 semantic + MC-7 scene packet + normalized result lineage.

## Primary gate

```sh
cargo run --quiet -- minecraft inspect-3dgs-training-result-holdout \
  --training-result-semantic-manifest .tmp/mc10-smoke-review/semantic/minecraft-3dgs-training-result-semantic.json \
  --output-dir .tmp/mc16-live/holdout-preview-primary
```

Observed (2026-06-27):

- `status=ready`
- `holdoutFrameIndex=6` (last `in_game` frame in MC-7 6-frame scene packet)
- `spatialFrameId=frame-355416-47699343801916`
- `basisCheckpointPath` ends with `step-000001.ckpt`
- `holdoutScreenshotPath` points at scene-packet `frame_000006.png`
- `reference_overlay_path` records deterministic overlay witness when raycast + screenshot allow

## Inspect smoke

```sh
cargo run --quiet -- inspect <run_id>
```

Expect `MC-16 Training Result Holdout Preview:` with paired manifest/inspect artifacts on business lineage keys.

Example run: `run_1782551796653_60906_0`

## Honest boundary

MC-16 v1 closes holdout frame + checkpoint basis + scene-packet witness evidence only.
It does **not** close trained splat holdout render or photometric quality judgment.
