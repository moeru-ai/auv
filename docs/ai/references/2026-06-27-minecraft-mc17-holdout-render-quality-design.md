# 2026-06-27 Minecraft MC-17 holdout render quality design

Date: 2026-06-27

Status: D1 producer + minimal read-side inspect section.

## Scope

MC-17 closes **holdout render quality evidence** on top of MC-16 holdout preview
witness. It answers:

```text
Given MC-16-selected holdout frame and checkpoint basis, what photometric metrics
does an external trained render produce against the scene-packet holdout screenshot?
```

MC-17 does **not**:

- grade trainer quality or trained splat usefulness
- apply pass/fail thresholds or usefulness verdicts
- re-select holdout frames (MC-16 manifest is authoritative)
- resize, crop, or auto-align mismatched images in D1
- dispatch actions or wire CandidatePromotion / ActionResolver
- rewrite MC-12 schema or upgrade MC-15 Gaussian inference
- graduate patterns to core (Core-A/B deferred)

## Input boundary

```text
auv-cli minecraft measure-3dgs-holdout-render-quality \
  --training-result-semantic-manifest <semantic.json> \
  --holdout-preview-manifest <mc16-holdout-preview.json> \
  --render-command <command> \
  --output-dir <dir>
```

- MC-16 holdout preview manifest is **authoritative** for holdout frame index,
  screenshot path, checkpoint basis, and frame JSON path.
- MC-10 semantic manifest is used for lineage cross-check and normalized
  `config_path` witness only.
- `--render-command` is **required** (distinct from MC-16 `--holdout-render-command`).
- Persisted `render_backend` is the stable label `external_command` only; raw
  `--render-command` text is runtime input and must not appear in manifest,
  inspect, or run-store artifact bodies.

## External render command contract

Execution: `sh -lc <command>`, stdin JSON + newline, stdout single-line JSON.

**stdin `HoldoutRenderQualityRequest`:** `normalized_result_dir`, `config_path`,
`basis_checkpoint_path`, `holdout_frame_index`, `holdout_frame_json_path`,
`holdout_screenshot_path`, `viewport`, `view_matrix`, `projection_matrix`,
`player_pose`, `requested_rendered_image_path`.

**stdout `HoldoutRenderQualityAnswer`:** `status` (`ready` | `blocked` | `failed`),
optional `rendered_image_path`, `message`, `known_limits`.

## Metric policy (evidence only)

| Condition | `image_size_match` | `verdict` | Metrics |
|-----------|-------------------|-----------|---------|
| Pre-render gates fail | n/a | `blocked` | absent |
| Render command fails | n/a | `failed` | absent |
| Sizes match | `true` | `measured_only` | `l1_mean`, `mse`, `psnr` |
| Sizes mismatch | `false` | `metric_partial` | absent (no resize) |

- `ssim` is always `null` in D1 (deferred).
- When `mse == 0`, `psnr` is omitted and noted in `known_limits`.
- Metrics are **evidence only**; no threshold pass/fail.

## Artifact roles

- `minecraft-3dgs-holdout-render-quality`
- `minecraft-3dgs-holdout-render-quality-inspect`

Files: `minecraft-3dgs-holdout-render-quality.json`,
`minecraft-3dgs-holdout-render-quality-inspect.json`.

**Persisted manifest fields include:** lineage copies, holdout witness paths,
`render_backend` (`external_command`), `rendered_image_path`, metric evidence,
`status`, `reason`, `verdict`, `known_limits`. No command-string persistence.

## Read-side (D1)

- `run_read` extract/list helpers for both roles
- `auv inspect` `MC-17 Holdout Render Quality:` section (after MC-16, before MC-12)
- inspect server viewer cards deferred to D2

## Known limits

- `MC-17 v1 records photometric metrics as evidence only; pass/fail thresholds and trained splat usefulness verdicts are deferred`
- `MC-17 does not resize, crop, or auto-align mismatched holdout images in D1`
- SSIM deferred
- MC-16 forward pointer: `docs/ai/references/2026-06-27-minecraft-mc16-holdout-preview-render-inspect-design.md`

## Relationship

- MC-16 closes holdout witness; MC-17 closes render + metric evidence on that witness
- MC-12/13/14/15 schemas unchanged
- MC-18+ Gaussian-native provider and MC-19+ action wiring remain deferred
