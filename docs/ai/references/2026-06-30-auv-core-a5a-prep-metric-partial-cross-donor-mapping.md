# 2026-06-30 AUV Core-A5a-prep cross-donor `metric_partial` semantic mapping

Date: 2026-06-30

Status: **docs-only prep** for proof-matrix row **69 (quality measurement
verdict)**. Documents how three vertical probes/donors use the shared four-label
wire shape (`measured_only | metric_partial | blocked | failed`) when
`verdict=metric_partial`. **No code extraction approved.**

## Scope boundary

**In scope:**

- Cross-donor semantic mapping for `metric_partial` only (plus contrast with
  `measured_only`, `blocked`, `failed` where needed for reader safety)
- Side-by-side trigger, payload, inspect, and honest non-goal columns
- Falsifier notes for what must converge before Core-A5a helper extraction
- Cross-links to Core-A4, Core-X3, and proof-matrix row 69

**Out of scope (explicit non-goals):**

- Core-A5a quality verdict enum/helper extraction
- Core-A5b backend label discipline (row 70) — see follow-up candidate below
- Proof-matrix verdict column changes
- Helper-only admissible language upgrade
- Donor derive-rule changes or semantic convergence claims

**Primary inputs (read-only):**

- [`2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md`](2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md)
- [`2026-06-30-auv-core-x3-third-donor-graduation-review.md`](2026-06-30-auv-core-x3-third-donor-graduation-review.md)
- [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)
- `crates/auv-game-minecraft/src/training_result_holdout_render_quality.rs`
- `crates/auv-game-osu/src/detection_eval_quality.rs`
- `crates/auv-game-balatro/src/card_detection_quality.rs`

## Why this note exists

Core-A4 falsifier **F3** and Core-X3 triangulation both record the same fact:
the four-label quality verdict enum recurs across MC-17, osu WQ1, and Balatro X2,
but **`metric_partial` does not mean the same thing** in each donor. Without an
accepted mapping layer, a shared enum or inspect consumer would over-normalize
and mis-infer metric availability.

This note is the **documented cross-donor mapping** that Core-A4 and Core-X3
listed as a reopen trigger. It prepares honest Core-A5a discussion; it does
**not** approve extraction.

## Shared four-label wire shape (labels only)

All three donors persist the same serde labels on donor-local verdict enums:

| Label | Role (label-level only) |
| --- | --- |
| `measured_only` | Witness/pre-render gates passed; measurement domain considered complete under donor rules |
| `metric_partial` | Measurement ran but donor considers coverage or comparability incomplete |
| `blocked` | Upstream witness or gate not ready; no honest measurement attempt |
| `failed` | Measurement attempt failed (parse, command, bundle load, etc.) |

**Stage status is separate.** In all three donors, `status=ready` can coexist
with `verdict=metric_partial`. Quality verdict authority must not be inferred
from `auv_stage_status::StageStatus` alone (Core-A3 precedent applies to stage
triad only, not quality verdict semantics).

## Cross-donor `metric_partial` mapping table

| Dimension | MC-17 (`HoldoutRenderQualityVerdict`) | osu WQ1 (`DetectionEvalQualityVerdict`) | Balatro X2 (`CardDetectionQualityVerdict`) |
| --- | --- | --- | --- |
| **Donor slice** | MC-17 holdout render quality | OSU-WQ1 detection eval quality | Core-X2 card detection slot-coverage quality |
| **Source module** | `training_result_holdout_render_quality.rs` | `detection_eval_quality.rs` | `card_detection_quality.rs` |
| **Upstream witness** | MC-16 holdout preview manifest (`HoldoutPreviewStatus::Ready`) | OSU-WQ1 witness manifest (`DetectionEvalWitnessStatus::Ready`) | Semantic manifest ready + inline slot eval (no persisted witness artifact in v1) |
| **`metric_partial` trigger** | Holdout screenshot and rendered image **width/height differ** after successful external render | `total_frames > 0` and **not** full scoring: `projection_kind != "playfield_to_pixels"` **or** `spatial_unscored_frames > 0` | `expected_slot_count > 0` and **not** full slot coverage: `unscored_slot_count > 0` **or** `below_confidence_slot_count > 0` |
| **`measured_only` contrast** | Image sizes match → photometric metrics computed | `projection_kind == "playfield_to_pixels"` AND `spatial_unscored_frames == 0` AND `total_frames > 0` | All expected slots scored at or above per-slot `min_confidence` |
| **Stage `status` when partial** | `ready` (render succeeded; comparability incomplete) | `ready` (witness ready; scoring incomplete) | `ready` (semantic + bundle gates passed; coverage incomplete) |
| **`metrics` payload when partial** | **`None`** — no L1/MSE/PSNR emitted | **`Some(DetectionEvalQualityMetrics)`** — frame counts, recall fields, `projection_kind`; recalls may be `None` when denominators are zero | **`Some(CardDetectionQualityMetrics)`** — `expected_slot_count`, `scored_slot_count`, `unscored_slot_count`, `below_confidence_slot_count`, `slot_coverage_ratio` |
| **Partial semantics (plain language)** | **Comparability gap** — cannot compare pixels without resize/crop (deferred in D1) | **Scoring coverage gap** — some frames lack spatial score or projection is not full playfield→pixels | **Slot coverage gap** — expected UI slots missing detections or below per-slot confidence |
| **Verdict label when partial** | `metric_partial` | `metric_partial` | `metric_partial` |
| **Inspect / read behavior** | `verdict=`, `image_size_match=false`, `l1_mean/mse/psnr=n/a` when metrics absent; `render_backend=` on manifest line | `verdict=`, `label_recall=`, `spatial_recall=`, `spurious=` flattened on manifest line; inspect adds `label_recall_available`, `spatial_recall_available` | `verdict=`, `quality_backend=`, slot counts and `slot_coverage_ratio=` on manifest line; inspect adds `slot_coverage_ratio_available` |
| **Known limits (honest non-goals)** | Photometric evidence only; no pass/fail thresholds; no resize/crop/auto-align on mismatch; SSIM deferred | Evidence only; no model usefulness or gameplay success; no global pass/fail thresholds | Slot-coverage evidence only; inline eval (no durable eval-report path); no live admission; per-slot `min_confidence` is scoring input, not product pass/fail |
| **Regression test anchor** | `metric_partial_on_dimension_mismatch_records_quality_evidence` | `quality_metric_partial_when_spatial_unscored` | `quality_partial_slot_coverage_yields_metric_partial_with_metrics` |

### Metrics-presence policy summary

| Policy class | Donors | Reader rule |
| --- | --- | --- |
| **A — omit metrics on partial** | MC-17 only | `metric_partial` ⇒ treat `metrics` as absent; do not infer zero error |
| **B — retain metrics on partial** | osu WQ1, Balatro X2 | `metric_partial` ⇒ `metrics` present; interpret via donor-specific count/ratio fields |

Balatro aligns with osu on **policy B** (metrics retained) but differs on
**measurement domain** (slot coverage vs frame recall). MC is the sole **policy A**
donor today.

## Why a shared four-label enum is insufficient without this mapping

1. **Label collision** — `metric_partial` reads as one English phrase but encodes
   three incompatible partial policies (comparability vs frame scoring vs slot
   coverage).
2. **Metrics field ambiguity** — Without policy A vs B, consumers cannot know
   whether `metrics: null` means "partial" or "blocked/failed". MC and osu/Balatro
   disagree on this invariant.
3. **Stage vs verdict conflation** — `status=ready` + `verdict=metric_partial` is
   valid in all three; a shared enum does not carry "ready but incomplete
   measurement" semantics per domain.
4. **Inspect field asymmetry** — MC exposes `image_size_match`; osu exposes
   `projection_kind` on witness (not `quality_backend`); Balatro exposes slot
   counts and `quality_backend`. A bare enum export would hide which auxiliary
   fields disambiguate partial state.
5. **Extraction would over-normalize** — Core-A4 **F3** and Core-X3 both judge
   that enum-only glue before this mapping is **misleading**, not merely
   incomplete.

A future shared type must either:

- attach a **donor-neutral partial policy** dimension (e.g. metrics presence +
  coverage kind), or
- stay donor-local with this mapping doc as the read-side contract.

Enum-only extraction without one of the above **remains inadmissible**.

## Falsifier notes — what must converge before Core-A5a extraction

Reuses Core-A4 falsifier IDs; updates post-Core-X3 third donor.

| Falsifier | Claim | Current state (post A5a-prep) | Blocks Core-A5a? |
| --- | --- | --- | --- |
| **F1** — Partial meaningless | No vertical needs `metric_partial` | All three use it in unit tests | No |
| **F2** — Thresholds inseparable | Cannot separate measurement from pass/fail | All three document evidence-only `known_limits` | No |
| **F3** — Semantic collapse | Shared helper can treat partial as label-only | **Three distinct policies** documented here; MC policy A vs osu/Balatro policy B | **Yes** |
| **F4** — Fifth label needed | More than four verdict states required | No donor needs a fifth label | No |
| **F5** — Coincidence risk | Two donors might be accidental alignment | **Reduced** by Balatro third datapoint (Core-X3); divergence confirmed, not convergence | Partial (reduced, not blocking alone) |
| **F6** — No shared consumer | Extraction is repetition-only | No cross-vertical read helper demands shared verdict type yet | Open |
| **F7** — A3 extraction pressure | Stage helper implies verdict helper | A3 explicitly defers quality verdict (unchanged) | No |

**Still triggered for helper-only admissibility:** **F3**.

### Triggers to reopen Core-A5a (extraction candidacy)

| Trigger | Would unlock |
| --- | --- |
| Owner accepts **this mapping** as the read-side contract | Revisit helper-only admissible **language** on row 69 (still not auto-extract) |
| Donors adopt a **shared partial policy** enum or invariant (e.g. documented metrics-presence rule + coverage kind) | Core-A5a enum helper design review |
| Two or more donors **converge derive rules** so partial ⇒ same metrics-presence policy | Narrower shared boundary candidacy |
| Shared read-side consumer needs donor-neutral verdict + disambiguation fields | Owner-named extraction slice |
| Owner names **Core-A5a** after mapping acceptance | Implementation slice (separate from this prep doc) |

**Not sufficient alone:** third vertical with a **fourth** distinct partial policy
(Balatro already added policy B variant on slots — still does not converge MC
policy A).

## Relationship to proof-matrix row 69

| Item | Status after A5a-prep |
| --- | --- |
| Main matrix verdict column | **Unchanged** — `candidate, not admissible yet` |
| Row 69 blockers | **Clarified** — cross-donor mapping doc exists; semantic divergence explicit |
| Helper-only admissible language | **Still defer** — mapping documents divergence; does not resolve F3 |
| Core-A5a extraction | **Still not now** — owner must accept mapping and name slice |

## Follow-up candidate (not started)

**Core-A5b-prep** — row **70** split review (query vs render vs quality backend
discipline). Independent of this note; see Core-X3 defer list.

## Related references

- Core-A4 falsifier gate (row 69 initial F3 trigger):
  [`2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md`](2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md)
- Core-X3 third-donor triangulation:
  [`2026-06-30-auv-core-x3-third-donor-graduation-review.md`](2026-06-30-auv-core-x3-third-donor-graduation-review.md)
- Proof matrix row 69:
  [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)
- MC-17 design:
  [`2026-06-27-minecraft-mc17-holdout-render-quality-design.md`](2026-06-27-minecraft-mc17-holdout-render-quality-design.md)
- osu WQ1 design:
  [`2026-06-28-osu-wq1-witness-quality-evidence-design.md`](2026-06-28-osu-wq1-witness-quality-evidence-design.md)
- Balatro X2 design (initial policy table):
  [`2026-06-29-auv-core-x2-balatro-consumption-probe-design.md`](2026-06-29-auv-core-x2-balatro-consumption-probe-design.md)

## One-sentence summary

Core-A5a-prep documents three incompatible `metric_partial` policies (MC omits
metrics on dimension mismatch; osu and Balatro retain metrics on partial
coverage) so row 69 stays honestly **not admissible** until owner accepts this
mapping and names a convergence or disambiguation strategy — **not** enum-only
extraction.
