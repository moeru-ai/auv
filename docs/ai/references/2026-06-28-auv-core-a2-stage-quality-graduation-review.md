# 2026-06-28 AUV Core-A2 stage, quality, and backend-label graduation review

Date: 2026-06-28

Status: design-only graduation review after osu full-chain closure (PR #54 wired live
action + OSU-WQ1 witness/quality on `main` @ `91577c5`). Covers proof-matrix rows
**65 (stage status triad)**, **69 (quality measurement verdict)**, and **70
(persisted backend label discipline)** only. **No code extraction approved.**

## Scope boundary

**In scope:** re-grade three matrix rows using post-full-chain osu evidence vs
Minecraft MC-10/MC-16/MC-17 donors.

**Out of scope (explicit defer):** query status triad and action readiness view
(already covered by Core-A graduation + falsifier reviews); provider comparison;
Core-B extraction; Core-C2+; MC-20; controller/planner.

**Primary inputs (read-only):**

- [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)
- [`2026-06-27-auv-second-vertical-consumption-probe-osu-evidence.md`](2026-06-27-auv-second-vertical-consumption-probe-osu-evidence.md)
- [`2026-06-28-osu-wq1-witness-quality-evidence-design.md`](2026-06-28-osu-wq1-witness-quality-evidence-design.md)
- [`2026-06-28-osu-visual-truth-query-wired-live-action-design.md`](2026-06-28-osu-visual-truth-query-wired-live-action-design.md)
- [`2026-06-27-minecraft-mc16-holdout-preview-render-inspect-design.md`](2026-06-27-minecraft-mc16-holdout-preview-render-inspect-design.md)
- [`2026-06-27-minecraft-mc17-holdout-render-quality-design.md`](2026-06-27-minecraft-mc17-holdout-render-quality-design.md)
- `crates/auv-game-osu/src/visual_truth_semantic.rs`, `detection_eval_witness.rs`,
  `detection_eval_quality.rs`, `visual_truth_spatial_query.rs`
- `crates/auv-game-minecraft/src/training_result_semantic.rs` (via MC-10),
  `training_result_holdout_preview.rs`, `training_result_holdout_render_quality.rs`

## Verdict vocabulary (unchanged from Core-A)

| Label | Meaning |
| --- | --- |
| `candidate, not admissible yet` | Second-vertical or falsifier gates not met for helper-only admissibility language |
| `candidate, helper-only admissible` | Review language only — pattern recurs across two verticals; **default defer extraction** |
| `probe-local recurrence` | Appendix evidence; does not lift main-matrix admissibility by itself |

**Admissible ≠ recommended now ≠ next slice ≠ extraction pressure.**

---

## Question 1 — Stage status triad (proof matrix row 65)

**Owner question:** Can verdict remain `candidate, not admissible yet` or upgrade
to `candidate, helper-only admissible`?

### Evidence landscape change

Prior osu probe (2026-06-27) assessed stage triad as **partial (structurally
shallow)** — semantic + query only. Query uses `answered/blocked/failed` (query
status triad, row 66), not stage `ready/blocked/failed`.

After OSU-WQ1 + wired live action chain on `main` @ `91577c5`, osu now persists
**three** `ready/blocked/failed` stages with lineage-carrying artifacts:

| Stage | osu donor enum | Persisted artifact role | MC analog |
| --- | --- | --- | --- |
| Semantic gate | `VisualTruthSemanticStatus` | `osu-visual-truth-semantic` | `TrainingResultSemanticStatus` (MC-10) |
| Witness | `DetectionEvalWitnessStatus` | `osu-detection-eval-witness` | `HoldoutPreviewStatus` (MC-16) |
| Quality evidence | `DetectionEvalQualityStatus` | `osu-detection-eval-quality` | `HoldoutRenderQualityStatus` (MC-17) |

Positive + negative paths: `detection_eval_witness.rs` / `detection_eval_quality.rs`
unit tests; frozen fixture chain under `crates/auv-game-osu/tests/`.

### Contract sameness vs shape similarity

| Dimension | Assessment |
| --- | --- |
| Label triad | **Same** — `ready \| blocked \| failed` at each persisted stage in both verticals |
| Stage count | **Met (≥2)** — osu now has three stage gates, not one |
| Lineage | **Same discipline** — each stage manifest carries upstream refs (`source_*_path`, witness paths) |
| Domain semantics | **Similar shape, different meaning** — MC witness selects holdout frame + render basis; osu witness aligns detection vs visual-truth frames. MC quality is photometric render metrics; osu quality is detection recall / spatial scoring |
| Counter-evidence falsifier | **Not triggered** — no vertical needs a fourth stage status; `blocked` vs `failed` preserved in both chains |

### Graduation gates (proof matrix §39–58)

| Gate | Stage triad assessment |
| --- | --- |
| Second-vertical evidence | **Met** — osu semantic + witness + quality stages are non-Minecraft persisted producers |
| Positive + negative paths | **Met** — unit tests cover blocked/failed witness and quality gates |
| Donor-neutral naming | **Not met for extraction** — symbols remain `VisualTruth*`, `DetectionEval*`, `Holdout*` |
| Smaller shared boundary | **Would be enum-only** — if ever extracted |
| Owning layer | **Implicit** — runtime contract enum; not named in extraction slice |

### Recommended verdict

| Surface | Verdict | Reasoning |
| --- | --- | --- |
| Main matrix row 65 | **`candidate, helper-only admissible`** (review language) | osu closes the row's positive evidence ask: same `ready/blocked/failed` split across **≥2 persisted stages** with lineage. Prior "partial/shallow" assessment is **superseded** by WQ1 witness + quality stages |
| Extraction | **Defer** | Two verticals only; stage semantics differ (witness/quality meaning is analog, not identical contract); no shared consumer |

**Not upgraded to core graduation** — helper-only admissible records that re-litigating
"does the stage triad recur?" is no longer necessary; it does **not** approve enum
extraction.

---

## Question 2 — Quality measurement verdict (proof matrix row 69)

**Owner question:** Does osu WQ1 establish second-vertical recurrence for
witness + quality — same contract vs similar evidence shape only?

### Side-by-side: MC-17 vs OSU-WQ1

| Dimension | MC-17 (`HoldoutRenderQualityVerdict`) | OSU-WQ1 (`DetectionEvalQualityVerdict`) |
| --- | --- | --- |
| Chain | MC-16 witness → MC-17 quality | WQ1 witness → Q1 quality |
| Verdict variants | `measured_only \| metric_partial \| blocked \| failed` | **Identical four labels** |
| Witness gate | `HoldoutPreviewStatus` must be `ready` | `DetectionEvalWitnessStatus` must be `ready` |
| `measured_only` trigger | Image sizes match; photometric metrics present | `projection_kind=playfield_to_pixels` and `spatial_unscored_frames=0` |
| `metric_partial` trigger | Image dimension mismatch; **metrics omitted** | Partial scoring (`spatial_unscored_frames>0` or projection not full); **metrics still present** |
| `blocked` / `failed` | Pre-render gates / command failure / witness not ready | Witness missing/blocked/failed lineage |
| Thresholds | Evidence-only; D3 adds optional thresholds separately | Evidence-only; `OSU_WQ1_V1_QUALITY_KNOWN_LIMIT` |
| Backend label | `render_backend=external_command`; raw `--render-command` excluded | `detector_model_id` optional string; `projection_kind` string — **no `quality_backend` enum** |

### Contract sameness vs shape similarity

| Claim | Verdict |
| --- | --- |
| Same four-label verdict enum | **Shape match** — variant names and serde labels align |
| Witness-bound quality chain | **Pattern recurrence** — both verticals derive quality from a witness manifest |
| `metric_partial` semantics | **Not same contract** — MC omits metrics on size mismatch; osu retains partial metrics. A shared enum would hide this divergence |
| Measurement domain | **Not same contract** — photometric (l1/mse/psnr) vs detection recall; falsifier "partial measurement meaningless" not triggered but domains differ |
| Second vertical for row positive evidence | **Met at shape level** — osu measures witness-bound quality and uses all four verdicts |

### Recommended verdict

| Surface | Verdict | Reasoning |
| --- | --- | --- |
| osu probe appendix | **satisfied as second-vertical probe-local recurrence** (upgrade from "candidate") | Full witness→quality chain landed with tests |
| Main matrix row 69 | **`candidate, not admissible yet`** (unchanged) | Recurrence is **evidence-shape + verdict-label** alignment, not contract sameness. `metric_partial` split logic diverges materially. Two verticals only |
| Extraction | **Defer** | Shared verdict enum would over-normalize domain-specific partial semantics |

**Conservative call:** WQ1 strengthens probe evidence but does **not** warrant
helper-only admissible on the main matrix row. Shape similarity ≠ graduated contract.

---

## Question 3 — Persisted backend label discipline (proof matrix row 70)

**Owner question:** Does osu now form a second real donor supporting discipline
beyond policy-level?

### Evidence inventory

| Discipline surface | Minecraft donor | osu donor | Second vertical? |
| --- | --- | --- | --- |
| Query backend label | `TrainingResultSpatialQueryBackend` persisted; raw provider text excluded | `VisualTruthSpatialQueryBackend::PlayfieldProjectionReference` in query manifest + inspect (`query_backend=` in `inspect.rs`) | **Yes** |
| Render / quality backend label | `HoldoutRenderQualityBackend::ExternalCommand`; raw `--render-command` excluded from artifacts | WQ1 uses `projection_kind`, optional `detector_model_id` strings — **no stable backend enum** | **No** |
| Policy docs | MC-12 + MC-17 designs state rule explicitly | osu spatial-query design L93; WQ1 design silent on backend enum | Partial |

### Assessment

osu independently hits the **query-layer** rule MC-12 established: stable
`query_backend` label in persisted manifests and inspect, without embedding
transient runtime command text.

osu does **not** yet mirror MC-17's `render_backend` enum on the quality stage.
Detector provenance is optional metadata, not the same discipline proof.

### Recommended verdict

| Surface | Verdict | Reasoning |
| --- | --- | --- |
| osu probe appendix | **satisfied as probe-local recurrence (query backend layer)** | Upgrade from "partial (second vertical)" |
| Main matrix row 70 | **`candidate, not admissible yet`** (unchanged) | Row covers **both** query and render/quality backend discipline. Second donor covers **half** the row. Render-side discipline remains MC-only |
| Extraction | **Defer** | No shared rule extracted; query-only recurrence insufficient for full row |

---

## Cross-row observations

1. **Full osu chain does not auto-promote all rows** — semantic→query→readiness→live
   admission→witness→quality closes vertical evidence; matrix rows grade independently.
2. **Stage triad is the only row in this review warranting helper-only admissible**
   language upgrade on the main matrix.
3. **Quality and backend rows gain stronger probe appendix language** without main-matrix
   admissibility lift — honest conservative re-grading.

## Explicit defer list

| Item | Trigger to reopen |
| --- | --- |
| Core-B enum extraction for stage/quality/backend | Owner names slice + concrete repetition pain in shared consumer |
| Quality row helper-only admissible | Third vertical with same witness→quality verdict **semantics** (not just labels), or osu/MC `metric_partial` semantics converge with documented mapping |
| Backend row helper-only admissible | osu (or third vertical) adds persisted `quality_backend` / render-backend enum hitting MC-17 rule |
| Core-C2+ admission field alignment | Owner names C2 slice — see [`2026-06-28-auv-core-a2-full-chain-falsifier-review.md`](2026-06-28-auv-core-a2-full-chain-falsifier-review.md) |

## Related references

- Core-A3 helper extraction (implemented):
  [`2026-06-29-auv-core-a3-stage-status-triad-helper-design.md`](2026-06-29-auv-core-a3-stage-status-triad-helper-design.md)
- Core-A4 quality/backend falsifier gate (rows 69/70 re-adjudication; verdicts unchanged):
  [`2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md`](2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md)

- Full-chain falsifier + Core-C1 re-review:
  [`2026-06-28-auv-core-a2-full-chain-falsifier-review.md`](2026-06-28-auv-core-a2-full-chain-falsifier-review.md)
- Prior query/readiness graduation:
  [`2026-06-27-auv-core-a-query-readiness-graduation-review.md`](2026-06-27-auv-core-a-query-readiness-graduation-review.md)
- Proof matrix (updated footnotes):
  [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)

## One-sentence summary

After osu full-chain closure, **stage status triad** upgrades to **helper-only
admissible** (review language); **quality measurement verdict** and **backend
label discipline** strengthen probe-local recurrence but **remain not admissible
yet** on the main matrix — shape recurrence without contract sameness on quality,
and query-only backend proof without render-side second donor.
