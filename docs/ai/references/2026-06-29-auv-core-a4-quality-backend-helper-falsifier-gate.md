# 2026-06-29 AUV Core-A4 quality and backend helper falsifier gate

Date: 2026-06-29

Status: design-only falsifier gate before any helper extraction. Re-adjudicates
proof-matrix rows **69 (quality measurement verdict)** and **70 (persisted
backend label discipline)** after Core-A3 landed `auv-stage-status` on `main` @
`61376a4`. **No code extraction approved.**

## Scope boundary

**In scope:**

- Falsifier pass for rows 69 and 70 only
- Same-contract vs similar-shape analysis (MC-17 vs osu WQ1; query vs render vs
  quality backend surfaces)
- Honest proof-matrix footnote update if evidence changed

**Out of scope (explicit non-goals):**

- Core-A5 extraction implementation
- Core-B, Core-C2, MC-20, controller/planner
- Stage status triad re-litigation (row 65 closed in Core-A2/A3)
- Query status triad, action readiness, provider comparison (prior reviews stand)

**Primary inputs (read-only):**

- [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)
- [`2026-06-28-auv-core-a2-stage-quality-graduation-review.md`](2026-06-28-auv-core-a2-stage-quality-graduation-review.md)
- [`2026-06-28-auv-core-a2-full-chain-falsifier-review.md`](2026-06-28-auv-core-a2-full-chain-falsifier-review.md)
- [`2026-06-29-auv-core-a3-stage-status-triad-helper-design.md`](2026-06-29-auv-core-a3-stage-status-triad-helper-design.md)
- [`2026-06-27-minecraft-mc16-holdout-preview-render-inspect-design.md`](2026-06-27-minecraft-mc16-holdout-preview-render-inspect-design.md)
- [`2026-06-27-minecraft-mc17-holdout-render-quality-design.md`](2026-06-27-minecraft-mc17-holdout-render-quality-design.md)
- [`2026-06-28-osu-wq1-witness-quality-evidence-design.md`](2026-06-28-osu-wq1-witness-quality-evidence-design.md)
- `crates/auv-game-minecraft/src/training_result_holdout_render_quality.rs`
- `crates/auv-game-osu/src/detection_eval_quality.rs`
- `crates/auv-game-minecraft/src/training_result_spatial_query.rs`
- `crates/auv-game-osu/src/visual_truth_spatial_query.rs`
- `src/inspect.rs`, `src/run_read.rs` (read-side backend labels)

## Verdict vocabulary

| Label | Meaning |
| --- | --- |
| `candidate, not admissible yet` | Second-vertical or falsifier gates not met for helper-only admissibility |
| `candidate, helper-only admissible` | Review language only — pattern recurs with **contract sameness**; default defer extraction |
| **admit** (helper candidacy) | Row may use helper-only admissible language on main matrix |
| **defer** (helper candidacy) | Keep not admissible; reopen when named triggers fire |
| **reject** (helper candidacy) | Shared helper would normalize away material donor semantics |

**Admissible ≠ recommended now ≠ next slice ≠ extraction pressure.**

## What changed since Core-A2

| Event | Impact on rows 69/70 |
| --- | --- |
| Core-A3 `auv-stage-status` landed (`61376a4`) | Confirms stage-triad helper pattern works; **explicitly defers** quality verdict and backend discipline |
| No new vertical since A2 | osu + Minecraft remain the only donors |
| No `metric_partial` semantic convergence | MC-17 still omits metrics on dimension mismatch; osu WQ1 still retains metrics on partial scoring |
| No osu `quality_backend` / render-backend enum | Row 70 render/quality half still MC-only |

Core-A4 is a **re-litigation gate**, not new evidence. The question is whether
A2's conservative blockers still hold after A3 — they do.

---

## Executive verdict table

| Row | Contract | Main matrix verdict (A4) | Δ vs A2 | Helper-only candidacy | Core-A5 recommendation |
| --- | --- | --- | --- | --- | --- |
| **69** Quality measurement verdict | `measured_only \| metric_partial \| blocked \| failed` | **`candidate, not admissible yet`** | unchanged | **defer** (reject admissible language) | **later** — `Core-A5a` only after `metric_partial` semantics converge or documented cross-donor mapping |
| **70** Persisted backend label discipline | stable backend labels in artifacts; raw runtime command text excluded | **`candidate, not admissible yet`** | unchanged | **defer** | **later** — `Core-A5b` after render/quality backend enum on second donor, or owner narrows row to query-only helper |

**Proof matrix row verdicts:** unchanged. Footnote⁵ records A4 re-confirmation.

**Triggered falsifiers (helper admissibility):** row 69 — `metric_partial` semantic
divergence; row 70 — row scope covers query **and** render/quality, second donor
covers query only.

---

## Row 69 — Quality measurement verdict

### Owner question

Does osu WQ1 + post-A3 state warrant upgrading row 69 to helper-only
admissible, or opening Core-A5a extraction now?

### Current verdict

**Keep** `candidate, not admissible yet` on the main matrix.

**Helper-only extraction candidacy:** **defer** (treat **reject** for admissible
language — shared enum would over-normalize).

**Core-A5a:** **not now**. Reopen when triggers below fire.

### Evidence snapshot (2026-06-29, `main` @ `61376a4`)

| Dimension | MC-17 (`HoldoutRenderQualityVerdict`) | OSU-WQ1 (`DetectionEvalQualityVerdict`) |
| --- | --- | --- |
| Variant labels + serde | `measured_only \| metric_partial \| blocked \| failed` | **Identical** |
| Witness gate | `HoldoutPreviewStatus::Ready` required | `DetectionEvalWitnessStatus::Ready` required |
| `measured_only` | Image sizes match → photometric metrics populated | `projection_kind=playfield_to_pixels` AND `spatial_unscored_frames=0` AND `total_frames>0` → recall metrics populated |
| `metric_partial` | Dimension mismatch → **metrics field `None`** | Partial scoring OR non-full projection → **metrics always `Some`** when `total_frames>0` |
| `blocked` / `failed` | Pre-render gates, command failure, witness not ready | Missing/unreadable witness, witness blocked/failed |
| Measurement domain | Photometric (l1/mse/psnr) | Detection recall / spatial scoring |
| Stage status | `HoldoutRenderQualityStatus` → `auv_stage_status::StageStatus` (A3) | `DetectionEvalQualityStatus` → same helper (A3) |
| Quality verdict enum | **Still donor-local** — not extracted in A3 | **Still donor-local** |

Derive logic anchors:

- MC-17: `metric_partial` when `source_rgb` dimensions ≠ `rendered_rgb` →
  `metrics: None` (`training_result_holdout_render_quality.rs` ~L845–865).
- osu WQ1: `metric_partial` when witness has frames but not full scoring →
  `metrics: Some(...)` (`detection_eval_quality.rs` ~L324–342).

### Same-contract vs similar-shape

| Claim | Assessment |
| --- | --- |
| Four-label enum wire shape | **Similar shape** — labels align |
| Witness-bound quality chain | **Pattern recurrence** — both derive quality from witness manifest |
| `metric_partial` meaning | **Not same contract** — MC = "evidence incomplete (no metrics)"; osu = "evidence partial but metrics present" |
| Read-side inspect | Both print `verdict=`; osu adds `projection_kind`, `detector_model_id` on witness line — no shared quality-backend field |
| A3 stage helper precedent | **Does not transfer** — stage triad had same label semantics at each gate; quality verdict labels collide but **derive rules diverge** |

### Falsifier table (row 69)

| Falsifier | Claim | Evidence | Verdict |
| --- | --- | --- | --- |
| F1 — Partial measurement meaningless | A vertical needs no distinct partial state | Both verticals use `metric_partial` in tests with distinct paths | **Not triggered** |
| F2 — Thresholds inseparable from evidence | Shared enum cannot separate measurement from pass/fail | Both document evidence-only known_limits; thresholds deferred | **Not triggered** |
| F3 — `metric_partial` semantic collapse | Shared helper can treat partial as label-only glue | MC omits metrics; osu retains metrics — **readers would mis-infer completeness** | **Triggered (latent → admissibility block)** |
| F4 — Fourth verdict state needed | Five-or-more labels required | No donor needs fifth label | **Not triggered** |
| F5 — Only two verticals | Coincidence risk | Still two donors only | **Open** |
| F6 — No shared consumer | Extraction is repetition-only | No cross-vertical read helper demands shared verdict type | **Open** |
| F7 — A3 creates extraction pressure | Stage helper implies verdict helper | A3 design **explicitly lists quality verdict as non-goal** | **Not triggered** |

**Triggered falsifiers for helper-only admissibility:** **F3** (semantic divergence
on `metric_partial`).

### Latent risks

1. **Label collision** — inspect/read consumers may assume `metric_partial` implies
   the same metric availability policy across verticals.
2. **Stage vs verdict conflation** — A3 `StageStatus` on quality manifests must not
   be read as quality verdict authority (`status=ready` coexists with
   `verdict=metric_partial` in both donors).
3. **Premature Core-A5a** — extracting enum-only glue before semantics converge
   repeats the risk Core-A2 flagged for Core-B over-normalization.

### Helper-only candidacy adjudication

| Gate | Row 69 |
| --- | --- |
| Second-vertical evidence | Met at **shape** level (osu WQ1) |
| Positive + negative paths | Met (unit tests both verticals) |
| Contract sameness | **Not met** — `metric_partial` derive semantics differ materially |
| Donor-neutral naming | Would be possible for enum-only (`EvidenceVerdict`) |
| Smaller shared boundary | Enum-only **possible** but **misleading** without semantic mapping doc |
| Owning layer | Not named; no shared consumer |

**Recommendation:** **defer** helper-only admissible language; **reject** opening
Core-A5a **now**.

### Triggers to reopen

| Trigger | Would unlock |
| --- | --- |
| Documented cross-donor `metric_partial` mapping accepted by owner | Revisit admissible language (still not auto-extract) |
| osu or MC changes derive rules so partial ⇒ same metrics presence policy | `Core-A5a` enum helper candidacy |
| Third vertical with same witness→quality **semantics** (not labels alone) | Main matrix upgrade review |
| Shared read-side consumer needs donor-neutral verdict surface | Extraction pressure (owner-named slice) |

---

## Row 70 — Persisted backend label discipline

### Owner question

Does osu query-backend persistence plus MC render-backend discipline warrant
helper-only admissible on the full row, or a query-only helper slice?

### Current verdict

**Keep** `candidate, not admissible yet` on the main matrix.

**Helper-only extraction candidacy:** **defer**.

**Core-A5b:** **not now**. Reopen when render/quality backend enum exists on a
second donor, or owner splits the row.

### Backend label inventory (2026-06-29)

| Surface | Role | Minecraft | osu | Stable enum in artifact? | Raw runtime text excluded? |
| --- | --- | --- | --- | --- | --- |
| Query backend | Spatial query provenance | `TrainingResultSpatialQueryBackend` (`command_provider`, `checkpoint_native`, `closed_scene_toy`, `projection_reference`) | `VisualTruthSpatialQueryBackend::PlayfieldProjectionReference` | **Yes** both | **Yes** — MC excludes raw provider command from manifest; osu v1 single reference backend |
| Render backend | MC-17 holdout render provenance | `HoldoutRenderQualityBackend::ExternalCommand` → `render_backend` | — | MC only | **Yes** — `render_command` input not persisted |
| Quality / detector provenance | WQ1 measurement lineage | — | `detector_model_id: Option<String>`, `projection_kind: String` on witness manifest | **No enum** — free strings | N/A — no render command on osu quality path |
| Trainer / job backend strings | Training lineage (out of row scope) | `trainer_backend`, `job_backend` on many manifests | — | String fields, not row-70 discipline proof | — |

Read-side:

- `src/inspect.rs` prints `query_backend=` for osu query manifests (~L2270).
- `src/inspect.rs` prints `projection_kind=` and `detector_model_id=` on witness
  (~L2354) — **not** a `quality_backend=` or `render_backend=` line for osu.
- `src/run_read.rs` maps MC `render_backend` and osu `query_backend` into summaries.

### Same-contract vs similar-shape

| Claim | Assessment |
| --- | --- |
| Query-layer discipline | **Same contract** — stable snake_case enum label in persisted manifest + inspect |
| Render/quality-layer discipline | **MC-only enum** — osu uses optional metadata strings, not `quality_backend` |
| Row 70 matrix wording | Covers **both** query and render/quality backend rules |
| Policy docs | MC-12 + MC-17 explicit; osu spatial-query design L93; WQ1 silent on backend enum |

Query backend enums differ in **cardinality** (MC four variants vs osu one) but
share the **discipline rule**: persisted label, not transient command text.

Render/quality discipline is **not** recurring — osu does not persist
`quality_backend=…` analogous to `render_backend=external_command`.

### Falsifier table (row 70)

| Falsifier | Claim | Evidence | Verdict |
| --- | --- | --- | --- |
| F1 — Backend provenance needs raw command text | Stable labels insufficient | MC-17 + MC-12 persist enums only; tests assert JSON lacks raw command | **Not triggered** |
| F2 — Query-only satisfies full row | Half-row evidence enough for admissible | Matrix row explicitly names render/quality discipline alongside query | **Triggered (scope)** |
| F3 — String metadata equals backend enum | `detector_model_id` / `projection_kind` substitute for `quality_backend` | Strings are measurement context, not stable backend-family labels; no inspect `quality_backend=` | **Triggered (shape)** |
| F4 — Shared trait needed now | Multiple consumers need `as_str()` glue | Two query enums + one render enum — repetition low; no shared consumer | **Open** |
| F5 — A3 creates backend pressure | Stage helper implies backend helper | A3 **explicitly defers** backend discipline | **Not triggered** |

**Triggered falsifiers for helper-only admissibility:** **F2** (row scope), **F3**
(osu quality path lacks enum discipline).

### Latent risks

1. **Query-only overreach** — extracting a tiny `BackendLabel` trait from query
   enums alone would not satisfy row 70 as written and might imply render
   discipline is solved.
2. **String drift** — osu `projection_kind` on witness is witness metadata, not
   the same contract as MC `render_backend`.
3. **Inspect asymmetry** — MC holdout quality inspect includes `render_backend`;
   osu quality inspect has no parallel field.

### Helper-only candidacy adjudication

| Gate | Row 70 |
| --- | --- |
| Second-vertical evidence (full row) | **Not met** — query yes, render/quality no |
| Second-vertical evidence (query half) | **Met** |
| Contract sameness (query half) | **Met** |
| Contract sameness (full row) | **Not met** |
| Extraction shape | Rule doc or tiny trait — **premature** without render/quality second donor |

**Recommendation:** **defer** helper-only admissible on **full row**; **not now**
for Core-A5b. If owner later **narrows** row 70 to query-backend-only, a
separate graduation review could admit query-half helper — **out of scope for A4**
without owner row split.

### Triggers to reopen

| Trigger | Would unlock |
| --- | --- |
| osu (or third vertical) adds persisted `quality_backend` / render-backend enum matching MC-17 rule | Revisit full-row admissible language |
| Owner splits proof matrix into query-backend row vs render-quality-backend row | Query-half helper review (`Core-A5b-query`?) |
| ≥3 persisted backend enums share identical `as_str` + serde discipline | Tiny shared label helper candidacy |

---

## Cross-row observations

1. **A3 success does not lower the bar for 69/70** — stage status had contract
   sameness at the label level; quality verdict and backend discipline do not.
2. **Shape recurrence remains honestly recorded** in osu probe appendix; main
   matrix admissibility language stays conservative.
3. **Split Core-A5** — if extraction ever opens, **quality verdict** (`Core-A5a`)
   and **backend label** (`Core-A5b`) must stay separate slices; do not merge
   into one "quality helper" crate.
4. **Rows 69 and 70 are independent** — satisfying one does not satisfy the other.

## Proof matrix impact

| Item | Change |
| --- | --- |
| Row 69 verdict column | **No change** — `candidate, not admissible yet` |
| Row 70 verdict column | **No change** — `candidate, not admissible yet` |
| Row 69 blockers | **Clarified** — A4 confirms `metric_partial` semantic divergence |
| Row 70 blockers | **Clarified** — A4 confirms query-half vs render/quality-half split |
| Footnote⁵ | **Added** — points to this review |

## Explicit defer list

| Item | Trigger to reopen |
| --- | --- |
| Core-A5a quality verdict helper | `metric_partial` semantics converge + owner names slice |
| Core-A5b backend label helper | Second donor render/quality backend enum OR row split |
| Helper-only admissible language (row 69) | Third vertical semantic match or documented mapping |
| Helper-only admissible language (row 70) | Full-row second-vertical backend discipline |
| Core-B enum graduation | Separate from helper-only; not unlocked by A4 |
| Core-X2 third-vertical consumption probe | Owner accepts Core-X1 scouting + MVP; Balatro default scout |

## Related references

- Core-X1 third-vertical scouting (footnote⁶; verdict columns unchanged):
  [`2026-06-29-auv-core-x1-third-vertical-scouting-design.md`](2026-06-29-auv-core-x1-third-vertical-scouting-design.md),
  [`2026-06-29-auv-core-x1-third-vertical-admissibility-mvp.md`](2026-06-29-auv-core-x1-third-vertical-admissibility-mvp.md)
- Core-A2 graduation (initial row 69/70 grading):
  [`2026-06-28-auv-core-a2-stage-quality-graduation-review.md`](2026-06-28-auv-core-a2-stage-quality-graduation-review.md)
- Core-A2 full-chain falsifier:
  [`2026-06-28-auv-core-a2-full-chain-falsifier-review.md`](2026-06-28-auv-core-a2-full-chain-falsifier-review.md)
- Core-A3 stage helper (explicit non-goals for 69/70):
  [`2026-06-29-auv-core-a3-stage-status-triad-helper-design.md`](2026-06-29-auv-core-a3-stage-status-triad-helper-design.md)
- Proof matrix:
  [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)
- osu second-vertical probe:
  [`2026-06-27-auv-second-vertical-consumption-probe-osu-evidence.md`](2026-06-27-auv-second-vertical-consumption-probe-osu-evidence.md)

## One-sentence summary

Core-A4 re-confirms rows **69** and **70** stay **`candidate, not admissible yet`**
— quality verdict labels match but **`metric_partial` semantics diverge**; backend
discipline recurs on the **query half only** — **defer** Core-A5a/Core-A5b until
named triggers fire; **no proof-matrix verdict upgrade**.
