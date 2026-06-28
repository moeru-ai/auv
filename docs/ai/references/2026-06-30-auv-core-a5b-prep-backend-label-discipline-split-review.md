# 2026-06-30 AUV Core-A5b-prep persisted backend label discipline split review

Date: 2026-06-30

Status: **docs-only prep** for proof-matrix row **70 (persisted backend label
discipline)**. Splits the monolithic row into three backend surfaces — **query**,
**render**, and **quality** — and records per-donor discipline evidence across
MC, osu, and Balatro X2. **No code extraction approved.**

## Scope boundary

**In scope:**

- Three-way surface split (query / render / quality backend discipline)
- Per-surface, per-donor inventory with inspect/read and raw-text-exclusion notes
- Per-surface cross-donor verdict (`satisfied as probe recurrence` / `partial` /
  `not satisfied`)
- Falsifier notes for what must converge before Core-A5b helper extraction
- Cross-links to Core-A4, Core-X3, Core-A5a-prep, and proof-matrix row 70

**Out of scope (explicit non-goals):**

- Core-A5b backend label helper or trait extraction
- Proof-matrix verdict column changes or helper-only admissible language upgrade
- Owner row split into separate proof-matrix rows (documented as reopen trigger only)
- Donor manifest field renames or new backend enums
- Quality verdict semantics (row 69) — see Core-A5a-prep

**Primary inputs (read-only):**

- [`2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md`](2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md)
- [`2026-06-30-auv-core-x3-third-donor-graduation-review.md`](2026-06-30-auv-core-x3-third-donor-graduation-review.md)
- [`2026-06-30-auv-core-a5a-prep-metric-partial-cross-donor-mapping.md`](2026-06-30-auv-core-a5a-prep-metric-partial-cross-donor-mapping.md)
- [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)
- `crates/auv-game-minecraft/src/training_result_spatial_query.rs`
- `crates/auv-game-minecraft/src/training_result_holdout_render_quality.rs`
- `crates/auv-game-osu/src/visual_truth_spatial_query.rs`
- `crates/auv-game-osu/src/detection_eval_quality.rs`
- `crates/auv-game-balatro/src/card_detection_spatial_query.rs`
- `crates/auv-game-balatro/src/card_detection_quality.rs`
- `src/inspect.rs`, `src/run_read.rs`

## Why this note exists

Proof-matrix row 70 names **one** contract — stable backend labels in persisted
artifacts, raw runtime command text excluded — but donors implement backend
provenance on **three distinct surfaces** with different field names, enum
cardinality, and inspect flattening. Core-A4 **F2** (full-row scope) and **F3**
(osu quality path lacks enum discipline) block helper-only admissibility; Core-X3
adds Balatro as a third donor on query and quality halves but does **not**
close render-half recurrence or harmonize quality-backend semantics.

This note is the **documented three-way split** that Core-A4 and Core-X3 listed
as a reopen trigger for honest Core-A5b discussion. It prepares owner review; it
does **not** approve extraction.

## Shared discipline rule (surface-agnostic)

Each surface is judged against the same rule row 70 encodes:

1. **Stable backend family labels** belong in persisted manifests (serde
   `snake_case` enum labels or an owner-accepted equivalent).
2. **Raw runtime command text** (provider render commands, shell invocations,
   transient CLI strings) does **not** belong in persisted artifacts.
3. **Inspect/read** should expose the stable label where the surface exists;
   auxiliary measurement metadata must not be mistaken for backend-family labels.

Trainer/job lineage strings (`trainer_backend`, `job_backend`) and capture
fixture `backend` strings are **out of row-70 scope** (training/capture lineage,
not spatial-query or quality-measurement backend discipline).

---

## Three-way split table

### Query backend discipline

| Dimension | MC (MC-12) | osu (visual-truth spatial query) | Balatro X2 (card detection spatial query) |
| --- | --- | --- | --- |
| **Enum type** | `TrainingResultSpatialQueryBackend` | `VisualTruthSpatialQueryBackend` | `CardDetectionSpatialQueryBackend` |
| **Variants (v1)** | `command_provider`, `checkpoint_native`, `closed_scene_toy`, `projection_reference` | `playfield_projection_reference` (single) | `detection_bundle_reference` (single) |
| **Manifest field** | `selected_backend: Option<…>` | `query_backend` | `query_backend` |
| **Raw command excluded?** | **Yes** — provider command path does not persist shell text | **Yes** — v1 reference-only; dual-backend compare deferred | **Yes** — bundle reference only |
| **Inspect flattening** | `selected_backend=` on MC-12 manifest line | `query_backend=` on query artifact line | `query_backend=` on query artifact line |
| **Regression anchor** | dual-backend selection tests (`checkpoint_native`, `closed_scene_toy`, etc.) | `visual_truth_spatial_query` unit tests | `card_detection_spatial_query` unit tests |

**Cross-donor surface verdict:** **satisfied as probe recurrence** — all three
donors persist stable query-backend enums and exclude raw runtime command text.
Cardinality differs (MC four variants vs single-reference v1 on osu/Balatro) but
the **discipline rule** recurs.

**Partial caveat (not a discipline failure):** MC uses manifest field name
`selected_backend` while osu/Balatro use `query_backend`; inspect mirrors that
asymmetry. Harmonization is a **convergence** item, not proof that discipline is
absent.

### Render backend discipline

| Dimension | MC (MC-17) | osu | Balatro X2 |
| --- | --- | --- | --- |
| **Enum type** | `HoldoutRenderQualityBackend` | — | — |
| **Variants (v1)** | `external_command` | — | — |
| **Manifest field** | `render_backend` on holdout render quality manifest | — | — |
| **Raw command excluded?** | **Yes** — `render_command` input not persisted; JSON carries `render_backend` only | N/A | N/A |
| **Inspect flattening** | MC-17 manifest inspect line omits `render_backend=` today; `run_read` summaries carry enum | — | — |
| **Regression anchor** | `render_backend` serde + absence of raw command in manifest JSON tests | — | — |

**Cross-donor surface verdict:** **partial** — MC satisfies render-backend
discipline on one donor only. osu and Balatro have **no render stage** on their
consumption chains (witness/quality paths do not execute external holdout render).

**Not satisfied** as second-vertical (or third-vertical) probe recurrence for the
render surface.

### Quality backend discipline

| Dimension | MC (MC-17) | osu (WQ1) | Balatro X2 |
| --- | --- | --- | --- |
| **Stable backend enum?** | **Via render path** — `HoldoutRenderQualityBackend` on `render_backend` (no separate `quality_backend` field) | **No** — `detector_model_id: Option<String>`, `projection_kind: String` on witness manifest | **Yes** — `CardDetectionQualityBackend` on `quality_backend` |
| **Enum variants** | `external_command` (render-family label reused for photometric quality lineage) | — | `ultralytics_onnx_ui`, `ultralytics_onnx_entities` |
| **Raw runtime text excluded?** | **Yes** for render command | N/A — strings are witness metadata, not command text | **Yes** — ONNX path label only |
| **Inspect flattening** | MC-17 line: verdict/metrics; no `quality_backend=` | Witness line: `projection_kind=`, `detector_model_id=` — **not** `quality_backend=` | Quality lines: `quality_backend=` |
| **Regression anchor** | photometric quality + `render_backend` JSON tests | WQ1 quality derive tests | `quality_full_coverage_yields_measured_only_with_backend` |

**Cross-donor surface verdict:** **partial** — enum discipline recurs on **two**
donors (MC via `render_backend`, Balatro via `quality_backend`) but **field
semantics diverge** (render-family vs detector-family label; MC has no
`quality_backend` field). osu uses **free strings** on the witness path; Core-A4
**F3** still applies — strings are measurement context, not stable backend-family
labels under row-70 rules.

**Not satisfied** as a single cross-donor quality-backend contract.

---

## Per-surface verdict summary

| Surface | MC | osu | Balatro X2 | Cross-donor verdict |
| --- | --- | --- | --- | --- |
| **Query backend** | enum + discipline | enum + discipline | enum + discipline | **satisfied as probe recurrence** |
| **Render backend** | enum + discipline | no surface | no surface | **partial** (one donor only) |
| **Quality backend** | enum via `render_backend` | free strings (no enum) | `quality_backend` enum | **partial** (split semantics + osu gap) |

---

## Why full row 70 stays `not admissible yet`

Proof-matrix row 70 wording covers **query and render/quality** backend rules in
one candidate contract. Helper-only admissibility requires the **full row** to
recur, not a single surface.

| Falsifier | Claim | State after A5b-prep | Effect on row 70 |
| --- | --- | --- | --- |
| **F2 — full-row scope** | Half-row evidence cannot upgrade the monolithic row | Query half recurs on three donors; render half MC-only; quality half split across MC/Balatro with osu string gap | **Still triggered** |
| **F3 — string vs enum on quality path** | Witness metadata strings do not satisfy quality-backend enum discipline | osu still uses `projection_kind` / `detector_model_id`; Balatro mitigates quality-half only | **Still triggered** for full row (osu) |
| **F1 — raw command in artifacts** | Stable labels insufficient | Not triggered — MC excludes render command text | No |
| **F4 — shared trait needed now** | Extraction pressure from consumers | No cross-vertical shared consumer demands trait yet | Open |
| **F5 — A3 creates backend pressure** | Stage helper implies backend helper | A3 explicitly defers backend discipline | No |

**Main matrix verdict column:** **unchanged** — `candidate, not admissible yet`.

**Helper-only admissible language:** **still defer** — this split documents
**where** recurrence exists; it does not collapse the three surfaces into one
extractable contract.

---

## What must converge before Core-A5b helper extraction

| Gap | Current state | Convergence needed |
| --- | --- | --- |
| **Render-half second donor** | Only MC-17 persists `render_backend` | Second donor with persisted render-backend enum under same discipline rule, **or** owner splits row 70 so render is a separate matrix row |
| **Quality-backend contract sameness** | MC uses `render_backend`; Balatro uses `quality_backend`; osu uses witness strings | Owner-accepted mapping of quality vs render provenance **or** osu adds persisted `quality_backend` enum; **or** row split isolates query-only helper (`Core-A5b-query` per A4) |
| **Field naming** | `selected_backend` (MC query) vs `query_backend` (osu/Balatro) | Harmonization or documented read-side alias contract before shared helper |
| **Inspect parity** | MC-17 inspect omits flattened `render_backend=`; Balatro prints `quality_backend=` | Not blocking discipline in JSON manifests; would matter for shared read consumer |
| **Enum cardinality** | MC query four variants vs single-reference v1 elsewhere | Acceptable for query-half helper **if** row narrowed; full row still needs render/quality convergence |

### Triggers to reopen Core-A5b (extraction candidacy)

| Trigger | Would unlock |
| --- | --- |
| Owner accepts **this split review** as read-side contract | Revisit helper-only admissible **language** on row 70 (still not auto-extract) |
| Second donor with `render_backend` enum discipline | Render-half recurrence; full-row re-review |
| osu adds persisted `quality_backend` enum (or owner accepts string→enum mapping policy) | Quality-half F3 mitigation for osu |
| Owner **splits** proof matrix row 70 into query vs render-quality rows | Narrow `Core-A5b-query` helper review (A4 out-of-scope without owner split) |
| ≥3 backend enums share identical serde + `as_str` discipline **on the same surface** | Tiny shared label helper design review |
| Shared read-side consumer needs donor-neutral backend label surface | Owner-named extraction slice |

**Not sufficient alone:** Balatro quality-backend enum — render half remains
MC-only; MC/Balatro quality fields still disagree on render vs quality naming.

---

## Relationship to proof-matrix row 70

| Item | Status after A5b-prep |
| --- | --- |
| Main matrix verdict column | **Unchanged** — `candidate, not admissible yet` |
| Row 70 blockers | **Clarified** — three-surface split; query recurrence explicit; render/quality gaps explicit |
| Helper-only admissible language | **Still defer** — F2 full-row scope remains |
| Core-A5b extraction | **Still not now** — owner must accept split and name convergence or row split |

## Related references

- Core-A4 falsifier gate (row 70 F2/F3 triggers):
  [`2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md`](2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md)
- Core-X3 third-donor triangulation (quality-half update):
  [`2026-06-30-auv-core-x3-third-donor-graduation-review.md`](2026-06-30-auv-core-x3-third-donor-graduation-review.md)
- Core-A5a-prep (row 69; independent quality-verdict mapping):
  [`2026-06-30-auv-core-a5a-prep-metric-partial-cross-donor-mapping.md`](2026-06-30-auv-core-a5a-prep-metric-partial-cross-donor-mapping.md)
- Proof matrix row 70:
  [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)
- MC-17 design (render_backend discipline):
  [`2026-06-27-minecraft-mc17-holdout-render-quality-design.md`](2026-06-27-minecraft-mc17-holdout-render-quality-design.md)
- osu WQ1 design (witness strings on quality path):
  [`2026-06-28-osu-wq1-witness-quality-evidence-design.md`](2026-06-28-osu-wq1-witness-quality-evidence-design.md)
- Balatro X2 probe (quality_backend enum):
  [`2026-06-29-auv-core-x2-balatro-consumption-probe-evidence.md`](2026-06-29-auv-core-x2-balatro-consumption-probe-evidence.md)

## One-sentence summary

Core-A5b-prep splits row 70 into query (three-donor recurrence), render
(MC-only), and quality (MC `render_backend` vs Balatro `quality_backend` vs osu
witness strings) so the full row stays honestly **not admissible** until render-
half and quality-backend contracts converge or the owner splits the matrix row —
**not** query-half extraction dressed as full-row graduation.
