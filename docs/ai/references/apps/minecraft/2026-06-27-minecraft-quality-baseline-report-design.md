# 2026-06-27 Minecraft MC-17 D2 quality baseline report design

Date: 2026-06-27

Status: D2 derived read-side quality evidence line (approved slice).

## Scope

MC-17 D2 fuses existing MC-12 spatial query correctness, MC-16 holdout witness, and
MC-17 photometric metrics into a **fixed-profile baseline report** that answers:

```text
On a pinned lineage, is the MC-12 / MC-16 / MC-17 stack trustworthy enough to
inform downstream behavior — as comparable evidence only?
```

This slice is **derived read-side only**:

- no new persisted artifact role
- no pass/fail gates or usefulness verdicts
- no MC-20 controller / planner / multi-action orchestration
- no MC-19 action-chain extensions

## Fixed baseline profile v1

Committed fixture:
`crates/auv-game-minecraft/tests/fixtures/mc17-d2/baseline-profile-v1.json`

| Pin | Value |
|-----|-------|
| `profile_id` | `mc17-d2-primary-v1` |
| Semantic manifest | `.tmp/mc10-smoke-review/semantic/minecraft-3dgs-training-result-semantic.json` |
| Query target | block `511,73,728`, face `north`, semantics `hit_face_center` |
| Holdout frame | index `6`, checkpoint suffix `step-000001.ckpt` |
| Render probe | screenshot-copy external command (doc-only; not persisted) |

Render command text follows the MC-17 D1 rule: runtime input only, never stored in
manifests or run-store artifact bodies. See
`crates/auv-game-minecraft/tests/training_result_holdout.rs` (`HOLDOUT_RENDER_QUALITY_COMMAND`).

## Derived report shape

Rust types live in `src/run_read.rs`:

- `QualityBaselineProfile`
- `QualityBaselineEvidenceBundle`
- `MinecraftTrainingResultQualityBaselineReportSummary`
- `derive_minecraft_training_result_quality_baseline_report`
- `collect_quality_baseline_evidence_for_run`

Per-stage evidence is read from existing summary types (not re-computed):

- **MC-12:** status, visibility, screen_point, selected_backend, comparison_verdict,
  basis_frame_id, target_block/face/semantics
- **MC-16:** status, holdout_frame_index, basis_checkpoint_path, holdout_screenshot_path,
  spatial_frame_id (from holdout frame witness)
- **MC-17:** status, verdict, image_size_match, l1_mean, mse, psnr, known_limits

## Synthesis rules (evidence-only)

- `evidence_coverage = complete` only when all three stages are present **and** profile
  pins match (semantic path, query target, holdout frame index, checkpoint suffix).
- Pin mismatch → `issue` + `evidence_coverage = partial` (never silently merge wrong lineage).
- `missing_stage` when no stage evidence is available.
- `trust_notes` always includes:
  - MC-12 `projection_reference` is not Gaussian inference
  - MC-17 screenshot-copy probe is pipeline comparability only, not trained-splat usefulness
  - MC-17 manifest `known_limits` when render quality evidence is present
- **No** `pass`/`fail`, threshold fields, or action eligibility reuse.

## Evidence assembly (`collect_quality_baseline_evidence_for_run`)

Resolution order:

1. Artifacts in the **current run** (MC-12 / MC-16 / MC-17 roles via existing `list_*` helpers).
2. MC-16 from `holdout_preview_manifest_path` on the selected MC-17 manifest (filesystem JSON read).
3. MC-12 from store scan: match `training_result_semantic_manifest_path` + profile query target.

No new persisted artifact role; no back-write to run store.

## Read-side surfacing

### Text inspect (`src/inspect.rs`)

Section after `MC-17 Holdout Render Quality:`:

```text
MC-17 Quality Baseline Report:
- profile_id=... evidence_coverage=... spatial_query_status=... holdout_status=... render_quality_status=... trust_notes=[...]
```

Wired through `inspect_run` using default profile v1 (`quality_baseline_profile_v1()`).

### Viewer (`src/inspect_server_viewer.html`)

One combined baseline card (MC-17 D1 viewer deferral fulfilled in D2):

- shown when MC-17 manifest exists on the run (or derived baseline is non-empty)
- JS mirror of Rust derive logic (same approach as MC-19 `refusal_reason` parsing)

## Relationship

- MC-17 D1: producer + per-stage inspect section
- MC-17 D2: derived baseline synthesis on fixed profile
- MC-17 D3: derived threshold verdict on D2 evidence (pass | partial | fail | blocked); see docs/ai/references/2026-06-27-minecraft-mc17-d3-quality-verdict-design.md
- MC-19: wiring honesty (`query → click`); orthogonal to D2 trust evidence
- Live closure: `docs/ai/references/2026-06-27-minecraft-mc17-d2-quality-baseline-live-closure.md`

## Non-goals

- MC-20 controller / planner
- New query providers or click paths
- Pass/fail quality gates
- Core-B runtime extraction
- New persisted artifact role for the derived report
- Real trained-splat quality claims from copy-probe baseline
