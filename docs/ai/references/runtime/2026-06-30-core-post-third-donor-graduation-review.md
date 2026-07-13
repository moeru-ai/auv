# 2026-06-30 AUV Core-X5 post-X4 third-donor graduation review

Date: 2026-06-30

Status: design-only graduation review after **Core-X4 Balatro witness-lineage closure**
(`main` @ `fd42c67`). Re-adjudicates proof-matrix rows **69 (quality measurement
verdict)** and **70 (persisted backend label discipline)** with witness-bound
Balatro quality on the third probe. **No code extraction approved.**

## Scope boundary

**In scope:**

- Re-grade rows 69 and 70 after Core-X4 witness closure
- Delta vs Core-X3 (inline-eval half-chain) and vs Core-A5a/A5b prep docs
- Honest proof-matrix footnote / blocker update only

**Out of scope:**

- Core-A5a/A5b helper extraction implementation
- Core-B, Core-C2, MC-20, controller/planner/registry work
- Row 65 stage triad re-litigation (Core-A2/A3 closed)
- Rows 66–68, 67 provider compare (prior reviews stand)
- Balatro live admission or action readiness
- Trainer quality, splat usefulness, or real-source restored validation claims

**Primary inputs (read-only):**

- [`2026-06-30-auv-core-x4-balatro-witness-lineage-closure-design.md`](../apps/balatro/2026-06-30-balatro-consumption-probe.md)
- [`2026-06-30-auv-core-x4-balatro-witness-lineage-closure-evidence.md`](../apps/balatro/2026-06-30-balatro-consumption-probe.md)
- [`2026-06-30-auv-core-x3-third-donor-graduation-review.md`](2026-06-30-core-third-donor-graduation-review.md)
- [`2026-06-30-auv-core-a5a-prep-metric-partial-cross-donor-mapping.md`](2026-06-30-query-readiness-closeout.md)
- [`2026-06-30-auv-core-a5b-prep-backend-label-discipline-split-review.md`](2026-06-30-query-readiness-closeout.md)
- [`2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md`](2026-06-30-query-readiness-closeout.md)
- `crates/auv-game-balatro/src/card_detection_eval_witness.rs`
- `crates/auv-game-balatro/src/card_detection_quality.rs`

## Verdict vocabulary

Unchanged from Core-A2/A4/X3. **Admissible ≠ recommended now ≠ extraction pressure.**

| Ruling label | Meaning in this review |
| --- | --- |
| **keep app-specific** | Donor-local enums and derive rules stay local; no shared helper candidacy |
| **helper-only admissible** | Review language only — pattern recurs with contract sameness on the scoped row |
| **defer** | Reopen when named triggers fire; default extraction posture |

---

## Executive verdict table

| Row | Main matrix verdict (X5) | Δ vs X3 | Helper-only candidacy | Core-A5 gate |
| --- | --- | --- | --- | --- |
| **69** Quality measurement verdict | **`candidate, not admissible yet`** | unchanged | **defer** (reject shared enum without disambiguation) | **Core-A5a still deferred** |
| **70** Persisted backend label discipline | **`candidate, not admissible yet`** | unchanged | **defer** (full row); quality-half lineage strengthened, not graduated | **Core-A5b still deferred** |

**Proof matrix verdict columns:** unchanged on admissibility language; blocker text
updated to record X4 witness closure (footnote¹¹).

**Triggered falsifiers (helper admissibility):** row 69 — **F3** (`metric_partial`
semantic triangulation still **divergence**); row 70 — **F2** (full-row scope),
**F3** (osu quality path still lacks enum discipline).

**Resolved / strengthened (not graduation):**

- **X3 inline-eval lineage dishonesty** → **resolved** @ `fd42c67` (witness stage;
  quality reads witness only)
- **Balatro witness artifact role** → **satisfied** (persisted eval witness manifest)
- **A4 F5 coincidence risk** → remains **reduced** (third datapoint holds post-X4)

---

## What Core-X4 changed vs Core-X3

Core-X3 graded Balatro with **inline eval** on a `semantic → query → quality`
half-chain (no durable witness artifact). Core-X4 closes witness lineage:

```text
semantic → spatial query → witness → quality
```

| Dimension | Core-X3 (X2 + inline eval @ `39577ff`) | Core-X4 (`fd42c67`) |
| --- | --- | --- |
| Witness artifact | **deferred** — eval inline from semantic + expected_slots | **`balatro-card-detection-eval-witness.json`** persisted |
| Quality inputs | semantic manifest + bundle paths (X2) | **`witness_manifest_path` only** |
| Quality schema | v1 (semantic-bound) | **v2** — witness-bound; v1 not wire-compatible |
| `metric_partial` derive | slot coverage incomplete → metrics retained | **unchanged rule** — derive from witness eval payload |
| `quality_backend` | on quality manifest only | on **witness + quality** manifests (lineage carry-through) |
| Chain parity vs osu WQ1 | partial (no witness stage) | **closer** — witness gate + witness-bound quality |
| Chain parity vs MC-16/17 | partial | **closer** on witness→quality seam; MC still has render stage |

**Honest framing:** X4 improves **lineage honesty** and **stage completeness** on
the Balatro probe. It does **not** change cross-donor **verdict semantics** (row 69)
or collapse the three backend surfaces into one contract (row 70).

---

## Row 69 — Quality measurement verdict

### Ruling

| Layer | X5 ruling |
| --- | --- |
| Main matrix verdict | **`candidate, not admissible yet`** — **keep app-specific** |
| Helper-only admissible language | **defer** |
| Core-A5a extraction | **defer** — implementation gate **not** unblocked |

### Why X4 does not upgrade row 69

Core-X4 witness closure leaves the Core-A5a-prep triangulation **unchanged**:

| Donor | `metric_partial` policy class | Metrics when partial | Measurement domain |
| --- | --- | --- | --- |
| MC-17 | **Policy A** — omit metrics | `metrics: None` | Photometric comparability (dimension mismatch) |
| osu WQ1 | **Policy B** — retain metrics | `metrics: Some(...)` | Frame scoring coverage |
| Balatro X4 | **Policy B** — retain metrics | `metrics: Some(...)` | Slot coverage (from witness eval payload) |

Four-label wire shape still recurs. **`metric_partial` derive semantics still do
not converge** — Balatro partial is still slot-coverage gap, not MC comparability
gap or osu frame-scoring gap. Moving eval payload onto the witness manifest
changes **where** partial is decided, not **what** partial means cross-donor.

### Falsifier delta (row 69)

| Falsifier | X3 | X5 |
| --- | --- | --- |
| F3 — semantic collapse | **Triggered** | **Still triggered** |
| F5 — two-vertical coincidence | **Reduced** | **Reduced** (unchanged) |
| Third vertical same semantics | **Not met** | **Not met** |
| Witness-bound quality parity | partial (osu only) | Balatro **joins** osu pattern; MC uses holdout preview — **stage shape closer**, **verdict semantics still diverge** |

### Core-A5a implementation gate (still blocked)

| Blocker | State after X5 |
| --- | --- |
| F3 semantic convergence | **Open** — three incompatible partial policies documented in A5a-prep |
| Owner accepts mapping as read-side contract | **Not recorded** — prep doc exists, not acceptance |
| Shared partial policy dimension or disambiguation fields | **Not designed** — enum-only extraction still inadmissible |
| Owner names Core-A5a slice | **Not named** |

**Unlocked by X5 (observation only):** Balatro quality tests now exercise
witness-bound `metric_partial` paths (`quality_metric_partial_from_witness_ready_partial_coverage`,
etc.) — stronger **probe regression anchors**, not helper extraction approval.

---

## Row 70 — Persisted backend label discipline

### Ruling

| Layer | X5 ruling |
| --- | --- |
| Main matrix verdict (full row) | **`candidate, not admissible yet`** — **keep app-specific** |
| Query-half surface | **probe recurrence satisfied** (three donors) — **not** helper-only admissible under monolithic row 70 |
| Quality-half surface | **partial** — **defer** (not helper-only admissible) |
| Render-half surface | **partial** (MC-only) — **defer** |
| Core-A5b extraction | **defer** — implementation gate **not** unblocked |

### What X4 changed on quality-half

| Dimension | X3 | X5 |
| --- | --- | --- |
| Balatro `quality_backend` enum | on quality manifest | on **witness manifest + quality manifest** (witness is source of truth) |
| Quality consumption | inline eval path | reads witness only; `quality_backend` copied from witness when `ready` |
| osu quality path | free strings on witness | **unchanged** — still `projection_kind` / `detector_model_id` |
| MC quality path | `render_backend` on MC-17 | **unchanged** — no `quality_backend` field |

X4 **strengthens** Balatro quality-backend **lineage discipline** (stable enum
on persisted witness, propagated to quality). It does **not**:

- add a second `render_backend` donor (render half still MC-only)
- add osu `quality_backend` enum discipline
- harmonize MC `render_backend` vs Balatro `quality_backend` field semantics

### Per-surface verdict (post-X5, aligns with A5b-prep + X4 delta)

| Surface | Cross-donor verdict | Helper-only admissible? |
| --- | --- | --- |
| **Query backend** | **satisfied as probe recurrence** (MC, osu, Balatro) | **defer** — monolithic row 70 blocks query-only graduation without owner row split |
| **Render backend** | **partial** (MC only) | **defer** |
| **Quality backend** | **partial** — MC `render_backend` vs Balatro `quality_backend` vs osu strings | **defer** — quality-half does **not** reach helper-only admissible |

### Falsifier delta (row 70)

| Falsifier | X3 | X5 |
| --- | --- | --- |
| F2 — full-row scope | **Triggered** | **Still triggered** |
| F3 — string vs enum on quality path (osu) | **Triggered** | **Still triggered** |
| Balatro quality-half enum discipline | mitigated (quality manifest) | **strengthened** (witness + quality) — **does not** close osu gap or full row |

### Core-A5b implementation gate (still blocked)

| Blocker | State after X5 |
| --- | --- |
| F2 full-row recurrence | **Open** — render half MC-only; quality half split |
| osu quality-backend enum | **Open** |
| MC vs Balatro quality field semantics | **Open** — `render_backend` vs `quality_backend` |
| Owner accepts A5b-prep split as read-side contract | **Not recorded** |
| Owner row split (query vs render-quality) or second render donor | **Not done** |
| Owner names Core-A5b slice | **Not named** |

**Unlocked by X5 (observation only):** Balatro witness manifest regression anchors
for `quality_backend` on witness JSON — tighter evidence for **future** quality-half
review; **not** full-row or quality-half helper-only admissible language.

---

## Balatro probe appendix (post-X4)

Companion evidence:
[`2026-06-30-auv-core-x4-balatro-witness-lineage-closure-evidence.md`](../apps/balatro/2026-06-30-balatro-consumption-probe.md)

| Contract | Verdict after Core-X4 | Notes |
| --- | --- | --- |
| Quality measurement verdict | **third-vertical probe recurrence** | Witness-bound partial; **same** `metric_partial` policy as X3; main matrix **not admissible** |
| Persisted backend discipline | **quality-half probe recurrence strengthened** | `quality_backend` on witness + quality; render half MC-only; main matrix **not admissible** |
| Witness stage | **satisfied** on probe | osu WQ1-shaped gate; not live admission |
| Live admission | **not satisfied** | deferred |

---

## Explicit defer list

| Item | Trigger to reopen |
| --- | --- |
| Core-A5a quality verdict helper | Owner accepts A5a-prep mapping **and** names convergence or disambiguation strategy (partial policy dimension, not enum-only) |
| Core-A5b backend label helper | Second `render_backend` donor, osu `quality_backend` enum, owner row split, or owner-accepted quality/render provenance mapping |
| Helper-only admissible row 69 | Semantic convergence **or** accepted mapping + disambiguation — X4 witness alone insufficient |
| Helper-only admissible row 70 (full row) | Full three-surface recurrence or owner splits row 70 |
| Helper-only admissible row 70 (quality-half only) | osu enum discipline **and** MC/Balatro field-semantics mapping — X4 does **not** suffice |
| Balatro full donor graduation | Live admission; owner-named slice beyond probe |

## One-sentence summary

Core-X5 confirms Core-X4 witness closure makes the Balatro third probe **lineage-honest**
and **witness-shaped like osu WQ1**, but **does not** upgrade rows 69/70 to
helper-only admissible — `metric_partial` semantics still diverge (F3) and backend
discipline still fails full-row scope (F2) with osu quality-half gap (F3).
