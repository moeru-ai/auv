# 2026-06-27 Minecraft MC-17 D3 quality verdict design

Date: 2026-06-27

Status: D3 derived read-side threshold verdict layer (approved slice).

## Scope

MC-17 D3 derives a **reviewable quality verdict** (`pass | partial | fail | blocked`) from
existing MC-12/16/17 evidence and pre-committed threshold profiles. It is separate from D2
`evidence_coverage` and does not change the D2 baseline report shape.

This slice is **derived read-side only**:

- no new persisted artifact role
- no MC-19 click admission or MC-14 readiness wiring
- no MC-20 controller / planner
- no new MC-12/16/17 producers

## Relationship to D2

| Layer | Question |
|-------|----------|
| D2 baseline report | What evidence exists, and does it match the pinned profile? |
| D3 quality verdict | Given that evidence and committed thresholds, is it good enough to trust for downstream behavior review? |

**Invariant:** `evidence_coverage=complete` does **not** imply `quality_verdict=pass`.

## Threshold profiles (v1)

Committed fixtures under `crates/auv-game-minecraft/tests/fixtures/mc17-d3/`:

| File | `render_evidence_mode` | Purpose |
|------|------------------------|---------|
| `baseline-verdict-thresholds-v1-probe.json` | `screenshot_copy_probe` | Pipeline comparability gate for copy-probe render |
| `baseline-verdict-thresholds-v1-trained-render.json` | `trained_render` | Real trained-render photometric gate |

Both reference `profile_id: mc17-d2-primary-v1` (same lineage pins as D2
`baseline-profile-v1.json`).

### Probe thresholds

- spatial: `answered` + `visible`
- holdout: `ready`
- render: `ready` + `measured_only` + `image_size_match=true`
- metrics: `l1_mean_max=0.001`, `mse_max=0.001`, `psnr_min=null`

### Trained-render thresholds (provisional v1)

NOTICE: numeric bounds are **provisional v1** — tune only via fixture revision plus new
closure, following MC-6 pre-commit precedent
(`docs/ai/references/2026-06-19-minecraft-mc6-texture-sweep-gate-verdict.md`).

- spatial/holdout gates: same as probe
- render: `ready` + `measured_only` + `image_size_match=true`
- metrics: `l1_mean_max=0.05`, `mse_max=0.01`, `psnr_min=20.0`

## Derived verdict shape

Rust types in `src/run_read.rs`:

- `QualityBaselineVerdictThresholds`
- `QualityBaselineStageCheck`
- `MinecraftTrainingResultQualityVerdictSummary`
- `derive_minecraft_training_result_quality_verdict`
- `quality_baseline_verdict_for_run`
- `quality_baseline_report_with_verdicts_for_run`

## Verdict policy (test-locked)

| Condition | `quality_verdict` |
|-----------|-------------------|
| `evidence_coverage=missing_stage` or collection issue blocks evaluation | `blocked` |
| `evidence_coverage=partial` | `blocked` |
| upstream stage `status=blocked` | `blocked` |
| `evidence_coverage=complete`, all stage checks pass | `pass` |
| `evidence_coverage=complete`, render `metric_partial` or mixed pass/fail across stages | `partial` |
| `evidence_coverage=complete`, deterministic metric threshold miss | `fail` |

Spatial query `visibility=outside_window` when `visible` is required → stage `fail`;
aggregate mixed pass/fail → `partial` unless a metric threshold miss forces `fail`.

`trust_notes` reuse D2 `build_quality_baseline_trust_notes` plus mode-specific notes from
each threshold fixture.

## Read-side surfacing

- `src/inspect.rs`: `MC-17 Quality Verdict:` section (probe first, trained_render second)
- `GET /runs/{run_id}/minecraft-quality-baseline-report`: baseline report fields +
  `verdicts: { probe, trained_render }`
- `src/inspect_server_viewer.html`: baseline card shows `quality_verdict` + stage rows
- `examples/mc17_quality_baseline_report.rs`: `--verdict-mode probe|trained_render|both`

## Non-goals

- MC-19 / MC-14 gate wiring
- MC-20 planner
- Persisted verdict artifact role
- Claim copy-probe pass equals trained-splat usefulness
- Core crate extraction of threshold enums

## Live closure

See `docs/ai/references/2026-06-27-minecraft-mc17-d3-quality-verdict-live-closure.md`.
