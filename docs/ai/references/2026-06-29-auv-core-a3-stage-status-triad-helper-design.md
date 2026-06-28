# 2026-06-29 AUV Core-A3 stage status triad helper extraction

Date: 2026-06-29

Status: implemented helper-only extraction. This note records a narrow code move.
It does **not** graduate stage status triad to Core-B manifest enum, query
status triad, quality verdict, backend label discipline, or Core-C2 admission
alignment.

## Why this slice exists

Core-A2 graduation review upgraded proof-matrix row 65 to
`candidate, helper-only admissible` (review language) after osu full-chain closure
added semantic + witness + quality persisted stages sharing the same
`ready / blocked / failed` triad as Minecraft MC-10/MC-16/MC-17:

- [`2026-06-28-auv-core-a2-stage-quality-graduation-review.md`](2026-06-28-auv-core-a2-stage-quality-graduation-review.md)
- [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md) (row 65)

That repetition justified moving only the duplicate enum glue into a shared
helper without touching manifest structs, domain reasons, inspect surfaces, or
readiness/query/quality verdict contracts.

## What changed

Added a new crate:

- `crates/auv-stage-status`

Current scope of that crate is intentionally narrow:

- `StageStatus` — `Ready | Blocked | Failed`
- `StageStatus::as_str()` — wire labels `ready`, `blocked`, `failed`
- `Display` for `StageStatus`
- `Serialize` / `Deserialize` with `#[serde(rename_all = "snake_case")]`

The helper owns only the shared persisted-stage status triad labels. It does
**not** own manifest parsing, domain reasons, witness/quality verdict enums,
query status (`answered/blocked/failed`), or action readiness.

**NOTICE:** `crates/auv-query-readiness` owns the derived-action eligibility
triad (unrelated). `StageStatus` must not be read as query status or dispatch
readiness.

## Dependency diagram

```text
auv-stage-status
  ├── StageStatus (ready | blocked | failed)
  │
  ├── auv-game-minecraft
  │     training_result_semantic.rs
  │       type alias TrainingResultSemanticStatus
  │     training_result_holdout_preview.rs
  │       type alias HoldoutPreviewStatus
  │     training_result_holdout_render_quality.rs
  │       type alias HoldoutRenderQualityStatus
  │
  └── auv-game-osu
        visual_truth_semantic.rs
          type alias VisualTruthSemanticStatus
        detection_eval_witness.rs
          type alias DetectionEvalWitnessStatus
        detection_eval_quality.rs
          type alias DetectionEvalQualityStatus

auv-cli (unchanged)
  src/inspect.rs
    still uses donor status types and `.as_str()`; no direct helper dependency
```

## Why this helper is admissible now

This extraction satisfies the helper-only bar from Core-A2:

- repeated in more than one owned vertical (Minecraft + osu semantic/witness/quality stages)
- extraction removes enum duplication without creating a new manifest contract
- helper name is donor-neutral (`StageStatus`), not `Holdout*` or `DetectionEval*`
- wire shape preserved via type aliases to the shared serde enum

Helper-only admissible (row 65) justified the slice; it does **not** approve
Core-B enum graduation into shared manifests.

## Deliberate non-goals

This slice intentionally does **not**:

- extract query status triad (`answered/blocked/failed`)
- extract action readiness (`auv-query-readiness` territory)
- extract quality measurement verdict (`measured_only/metric_partial/blocked/failed`)
- extract persisted backend label discipline
- add a generic runtime trait or shared manifest schema
- wire `auv-cli` dependency on `auv-stage-status`
- add a read-side inspect overhaul (donor `.as_str()` remains sufficient)
- touch controller/planner/MC-20 or a third vertical

Those remain blocked by the proof matrix and Core-A2 defer list.

## Touched files

- `Cargo.toml` — workspace member
- `crates/auv-stage-status/` — new helper crate
- `crates/auv-game-minecraft/Cargo.toml` — dependency
- `crates/auv-game-minecraft/src/training_result_semantic.rs` — type alias + wire test
- `crates/auv-game-minecraft/src/training_result_holdout_preview.rs` — type alias
- `crates/auv-game-minecraft/src/training_result_holdout_render_quality.rs` — type alias
- `crates/auv-game-osu/Cargo.toml` — dependency
- `crates/auv-game-osu/src/visual_truth_semantic.rs` — type alias + wire test
- `crates/auv-game-osu/src/detection_eval_witness.rs` — type alias
- `crates/auv-game-osu/src/detection_eval_quality.rs` — type alias

## Behavior preserved on purpose

- All stage gate branching, lineage checks, and domain reasons stay donor-local.
- Donor-facing type aliases preserve existing public symbol names.
- JSON manifest `status` / `semantic_status` fields still serialize as
  `"ready"`, `"blocked"`, `"failed"`.
- Inspect/read status labels remain identical via `.as_str()` on aliased types.

## Validation

```bash
cargo fmt --check
cargo check -p auv-stage-status -p auv-game-minecraft -p auv-game-osu
cargo test -p auv-stage-status
cargo test -p auv-game-minecraft semantic_status_type_alias training_result_semantic training_result_holdout_preview training_result_holdout_render_quality
cargo test -p auv-game-osu semantic_status_type_alias visual_truth_semantic detection_eval_witness detection_eval_quality
git diff --check
```

## Related references

- Core-A4 quality/backend falsifier gate (rows 69/70 unchanged; defers A5):
  [`2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md`](2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md)
- Core-A2 graduation (row 65 helper-only admissible):
  [`2026-06-28-auv-core-a2-stage-quality-graduation-review.md`](2026-06-28-auv-core-a2-stage-quality-graduation-review.md)
- Prior helper extraction pattern:
  [`2026-06-27-auv-core-query-readiness-helper-extraction.md`](2026-06-27-auv-core-query-readiness-helper-extraction.md)
- Proof matrix row 65:
  [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)

## One-sentence summary

Core-A3 extracts only the shared `ready | blocked | failed` persisted-stage
status enum into `auv-stage-status`, rewires six Minecraft/osu donor type
aliases, preserves JSON wire shapes, and defers all other Core-A rows.
