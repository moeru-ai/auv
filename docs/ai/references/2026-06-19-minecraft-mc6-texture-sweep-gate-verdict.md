# 2026-06-19 Minecraft MC-6 texture-sweep gate verdict

Date: 2026-06-19

Scope: `docs-only`. Reads the measured MC-6 texture-sweep report against the
pre-committed numerical gate and records the verdict. Adds no code, contract, or
core surface. This note does **not** reopen, close, or numerically pass MC-6 by
itself; it reads off the table that already exists from real-source samples.

## What this verdict is for

The execution plan (`2026-06-18-auv-mc5-onward-execution-plan.md`, Slice C) and
the measurement design (`2026-06-18-minecraft-mc6-spatial-dataset-measurement-design.md`)
both say the texture-sweep table is the **only** technical-forcing input for the
session-floor vs 2.5D vs 3DGS decision — "by number, not argument." A K-pack
sweep has now been run, so this note converts its numbers into a pass/fail
verdict against the thresholds that were fixed *before* the run.

## Source (real, auditable)

- Report: `.auv/runs/run_1781881912382_19513_0/artifacts/artifact_0002_texture_sweep_report.json`
- Samples: `.auv/runs/run_1781881912382_19513_0/artifacts/artifact_0001_texture_sweep_samples.json`
- Generator: `mc6.bundle-texture-sweep` (real-source), source runs
  `run_1781881896971_19131_0` / `run_1781881897582_19207_0` / `run_1781881898175_19213_0`,
  bundles `/tmp/auv-mc67-live/mc6-bundle-{rich,flat,repetitive}-fixed/run.json`.
- `report.passed = false`, `noise_refusal_exercised = false`.

Pre-committed thresholds (fixed before the run):

```text
pose_error_p95_max_px     = 8.0
occlusion_iou_min         = 0.85
resource_pack_count       = 3        (rich, flat_color, repetitive)
per_pack_duration_seconds = 30.0
refuse_on_noise_rule      = exercised at least once
```

## Verdict table

| profile     | samples | pose p95 (px) | < 8.0 px      | min IoU | > 0.85 | dur (s) | = 30 s | refusal | pack     |
|-------------|---------|---------------|---------------|---------|--------|---------|--------|---------|----------|
| rich        | 1       | 119.80        | ✗ (~15×)      | 1.00    | ✓      | 0.0     | ✗      | none    | **FAIL** |
| flat_color  | 1       | 119.80        | ✗ (~15×)      | 1.00    | ✓      | 0.0     | ✗      | none    | **FAIL** |
| repetitive  | 1       | 118.79        | ✗ (~15×)      | 1.00    | ✓      | 0.0     | ✗      | none    | **FAIL** |

Overall: **FAIL.** Every pack fails pose and duration; the noise-refusal rule was
never exercised; expected K=3 profiles are present but each has a single sample.

## Reading — the part that matters

The naive read — "119 ≫ 8, so 2.5D fails on texture, therefore 3DGS is forced" —
is exactly the mouth-decision this gate exists to prevent. The table does **not**
support it, for three independent reasons:

1. **It is a single shot, not a sweep.** 1 sample per pack, `duration = 0.0 s`
   against a 30 s/pack budget, and the refusal rule never fired. By the gate's own
   overall-pass rule and the plan's explicit wording ("a failed or missing report
   does not imply 'start 3DGS'; it means MC-6 is incomplete"), this is an
   **incomplete** MC-6, not a technical-forcing result. Note the real-source gate
   only certifies *provenance* (the samples cite real run ids / bundle manifests);
   it does not certify *sufficiency* (one frame per pack).

2. **No degradation curve — the opposite of what the experiment measures.** The
   sweep exists to watch pose error climb as texture goes rich → flat → repetitive.
   Here it is essentially flat: 119.80 / 119.80 / 118.79. Even the **rich** pack —
   the easy, most-textured case — sits at ~119 px. A texture-robustness failure
   shows a curve; a constant does not.

3. **Convention-bug signature, not a 2.5D-localization signature.** `rich` and
   `flat_color` pose error are byte-identical to 14 significant figures
   (`119.79580120693693`). Two different texture profiles cannot produce identical
   floating-point localization error if the pose were estimated from image content.
   That points at a **fixed offset** — a projection / coordinate-convention error
   (column-major vs row-major, window-vs-screen pixels, Retina/framebuffer scale) —
   i.e. the exact KG1 / unvalidated-matrix seam flagged in the MC-1b review, not a
   property of 2.5D under sparse texture. `min IoU = 1.00` across all three is the
   same degeneracy on the occlusion side (one fully-visible block, nothing to
   occlude).

So ~119 px most plausibly measures a **constant transform bug**, not 2.5D's
texture limit. It cannot be cited as evidence for or against 3DGS in either
direction.

## What this authorizes (per the plan's own rules)

- MC-6 stays **not numerically closed / not live-passed** — unchanged status.
- 3DGS gets **no technical-forcing credit** from this table. The three judges are
  untouched: technical forcing = not satisfied (no valid curve; likely a units
  bug); refusal-seam = still **0 of MC-4's 7 classes** need dense photometric
  comparison; market forcing = unchanged owner question.
- MC-7 may continue **only** as the owner-opened offline inspect-artifact lane
  (read-side, never action/verification path). It must not cite this report as
  closure or forcing evidence.

## Authorized next slice (one, then stop)

Root-cause the constant ~119 px **before** collecting any more packs. Smallest
step: take one real bundle frame, render the projected block box as
overlay-on-frame, and check whether the box lands on the block.

- If it is off by a roughly constant amount, fix the projection convention
  (matrix order / pixel space / framebuffer scale) and re-measure that one frame.
- Only once a single frame lands correctly does a K-pack sweep (≥1 real refusal,
  real 30 s/pack, many frames) become meaningful.

Do not stack more resource-pack runs on top of an unvalidated projection; that
just multiplies the same constant error across three columns, which is precisely
what this table shows.
