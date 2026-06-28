# 2026-06-30 AUV Core-X3 third-donor graduation review

Date: 2026-06-30

Status: design-only graduation review after **Core-X2 Balatro consumption probe**
(`feat/core-x2-balatro-consumption-probe` @ `39577ff`, including inline-eval lineage
fix). Re-adjudicates proof-matrix rows **69 (quality measurement verdict)** and
**70 (persisted backend label discipline)** with a **third vertical probe** present.
**No code extraction approved.**

## Scope boundary

**In scope:**

- Re-grade rows 69 and 70 after Balatro X2 probe evidence
- Same-contract vs similar-shape vs **third semantic datapoint** analysis
- Coincidence-risk (A4 F5) re-test
- Honest proof-matrix footnote / appendix update only

**Out of scope:**

- Core-A5a/A5b helper extraction
- Core-B, Core-C2, MC-20, controller/planner
- Row 65 stage triad re-litigation (Core-A2/A3 closed)
- Rows 66–68, 67 provider compare (prior reviews stand)
- Balatro as "full donor" graduation (X1/X2 frame: scout → probe, not donor #3)

**Primary inputs (read-only):**

- [`2026-06-29-auv-core-x2-balatro-consumption-probe-design.md`](2026-06-29-auv-core-x2-balatro-consumption-probe-design.md)
- [`2026-06-29-auv-core-x2-balatro-consumption-probe-evidence.md`](2026-06-29-auv-core-x2-balatro-consumption-probe-evidence.md)
- [`2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md`](2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md)
- [`2026-06-28-auv-core-a2-stage-quality-graduation-review.md`](2026-06-28-auv-core-a2-stage-quality-graduation-review.md)
- `crates/auv-game-balatro/src/card_detection_quality.rs`
- `crates/auv-game-osu/src/detection_eval_quality.rs`
- `crates/auv-game-minecraft/src/training_result_holdout_render_quality.rs`

## Verdict vocabulary

Unchanged from Core-A2/A4. **Admissible ≠ recommended now ≠ extraction pressure.**

---

## Executive verdict table

| Row | Main matrix verdict (X3) | Δ vs A4 | Helper-only candidacy | Next slice |
| --- | --- | --- | --- | --- |
| **69** Quality measurement verdict | **`candidate, not admissible yet`** | unchanged | **defer** (reject shared enum without mapping doc) | **Core-A5a-prep** optional: cross-donor `metric_partial` mapping doc (docs-only) |
| **70** Persisted backend label discipline | **`candidate, not admissible yet`** | unchanged | **defer** (full row); **quality-half** evidence strengthened | **Core-A5b-prep** optional: row-split review (query vs render vs quality backend) |

**Proof matrix verdict columns:** unchanged.

**Triggered falsifiers (helper admissibility):** row 69 — **F3** (`metric_partial`
semantic triangulation confirms **divergence**, not convergence); row 70 — **F2**
(full-row scope), **F3** partially mitigated on quality-half only.

**Resolved / reduced latent risks:**

- **A4 F5 (two-vertical coincidence)** → **reduced** — third probe datapoint exists
- **X2 lineage dishonesty** → **resolved** @ `39577ff` (inline eval; no dangling eval-report path)

---

## What Core-X2 added (evidence snapshot)

Balatro X2 closes an **observe-only** probe chain on committed fixtures:

```text
detection bundle fixture
  → semantic gate (ready / blocked / failed)
  → spatial query (answered / blocked / failed)
  → inline slot-coverage eval → quality manifest + verdict
```

| Surface | Balatro X2 | MC | osu |
| --- | --- | --- | --- |
| Semantic stage | `CardDetectionSemanticStatus` | `TrainingResultSemanticStatus` | `VisualTruthSemanticStatus` |
| Spatial query | `CardDetectionSpatialQueryStatus` | `TrainingResultSpatialQueryStatus` | `VisualTruthSpatialQueryStatus` |
| Quality verdict enum | `CardDetectionQualityVerdict` (4 labels) | `HoldoutRenderQualityVerdict` | `DetectionEvalQualityVerdict` |
| Quality backend enum | **`CardDetectionQualityBackend`** persisted | `HoldoutRenderQualityBackend` (`render_backend`) | free strings on witness |
| Witness artifact role | **deferred** (inline eval only) | MC-16 holdout preview | OSU-WQ1 witness manifest |
| Live admission | **deferred** | MC-19 analog | wired live action |

**Honest framing:** Balatro is a **third-vertical probe**, not a third full consumption
donor comparable to MC-10..17 or osu full chain.

---

## Row 69 — Quality measurement verdict

### Current verdict

**Keep** `candidate, not admissible yet` on the main matrix.

**Helper-only extraction candidacy:** **defer** — shared four-label enum would
**over-normalize** three distinct `metric_partial` policies.

**Core-A5a:** **not now**. Optional **Core-A5a-prep** (docs-only mapping) is the
honest next step.

### Third-donor `metric_partial` triangulation

| Donor | `metric_partial` meaning | Metrics when partial |
| --- | --- | --- |
| MC-17 | Holdout image dimension mismatch | **`metrics: None`** |
| osu WQ1 | Partial frame scoring | **`metrics: Some(...)`** |
| **Balatro X2** | Expected slot coverage incomplete | **`metrics: Some(...)`** + slot counts |

Four-label wire shape recurs. **Derive semantics do not converge** — Balatro adds a
**third** partial policy; it does not align MC and osu.

### Falsifier delta (row 69)

| Falsifier | A4 | X3 |
| --- | --- | --- |
| F3 — semantic collapse | **Triggered** | **Still triggered** |
| F5 — two-vertical coincidence | **Open** | **Reduced** |
| Third vertical same semantics | N/A | **Not met** |

---

## Row 70 — Persisted backend label discipline

### Current verdict

**Keep** `candidate, not admissible yet` on the main matrix.

**Quality-half update:** Balatro adds a **second non-MC donor** with persisted
`quality_backend` enum. Inspect prints `quality_backend=` on Balatro quality section.

### Backend inventory (post-X3)

| Surface | MC | osu | Balatro X2 |
| --- | --- | --- | --- |
| Query backend enum | yes | yes | yes |
| Render backend enum | yes | — | — |
| Quality backend enum | via `render_backend` | free strings | **yes** |

### Falsifier delta (row 70)

| Falsifier | A4 | X3 |
| --- | --- | --- |
| F2 — full row scope | **Triggered** | **Still triggered** |
| F3 — string vs enum on quality path | **Triggered** (osu) | **Partially mitigated** on quality-half |

---

## Balatro probe appendix

Companion: [`2026-06-29-auv-core-x2-balatro-consumption-probe-evidence.md`](2026-06-29-auv-core-x2-balatro-consumption-probe-evidence.md)

| Contract | Verdict after Balatro X2 | Notes |
| --- | --- | --- |
| Quality measurement verdict | **third-vertical probe recurrence** | Third `metric_partial` semantic; main matrix not admissible |
| Persisted backend discipline | **quality-half probe recurrence** | `quality_backend` enum; render half MC-only |
| Live admission | **not satisfied** | deferred |

---

## Explicit defer list

| Item | Trigger to reopen |
| --- | --- |
| Core-A5a quality verdict helper | Accepted cross-donor `metric_partial` mapping + owner names A5a |
| Core-A5b backend label helper | Row 70 split or second `render_backend` donor |
| Helper-only admissible row 69 | Semantic convergence or accepted mapping |
| Balatro full donor | Witness stage + durable lineage; owner-named slice |

## One-sentence summary

Core-X3 confirms Balatro X2 **triangulates** rows 69/70 and **reduces coincidence
risk**, but **does not** upgrade main-matrix helper-only admissibility.
