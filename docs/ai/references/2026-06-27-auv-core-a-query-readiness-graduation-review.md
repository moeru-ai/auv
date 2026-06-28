# 2026-06-27 AUV Core-A query status and action readiness graduation review

Date: 2026-06-27

Status: design-only graduation review. Covers **two proof-matrix rows only**:
query status triad and action readiness view. No code extraction is approved by
this note. **Default action: defer extraction.**

## Goal

Compress Core-A uncertainty after the osu! second-vertical consumption probe.
The probe direction is **correct** as brake evidence: it upgrades two matrix
**recurrence** for two read/consumption contracts (probe-local), without
opening Core-B or platformizing consumption runtime.

This review answers one narrow question:

```text
Do query status triad and action readiness view move from
`candidate, not admissible yet` to
`candidate, helper-only admissible if concrete repetition emerges`?
```


**Critical distinction:**

- **Admissible** (in review/docs language) = a future helper slice would not
  need to re-litigate whether the pattern recurs across verticals.
- **Admissible ≠ recommended now**
- **Admissible ≠ next slice**
- **Admissible ≠ extraction pressure**

It does **not** approve shared enum extraction, manifest structs, runtime
dispatch wiring, witness/quality/live-click parity, or any other matrix row.

## Current evidence pointers

### Proof matrix (osu probe rows)

`docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`
— osu! second-vertical probe table (~lines 83–86):

- Query status triad: **satisfied (second vertical, probe-local)**
- Action readiness view: **satisfied (second vertical, probe-local)**

Main matrix table (~lines 66–68) still lists both rows as
`candidate, not admissible yet` pending this review.

### osu! probe evidence and design

- Evidence:
  `docs/ai/references/2026-06-27-auv-second-vertical-consumption-probe-osu-evidence.md`
- Design:
  `docs/ai/references/2026-06-27-auv-second-vertical-consumption-probe-osu-design.md`

osu-local symbols:

- `VisualTruthSpatialQueryStatus` — `answered / blocked / failed`
- `derive_visual_truth_spatial_query_action_readiness` —
  `click_ready / answer_non_clickable / not_consumable`
- Frozen fixture:
  `crates/auv-game-osu/tests/fixtures/osu_visual_truth_probe/`
- Positive and negative paths: unit tests + fixture matrix in design doc

### Minecraft donor references (MC-12, MC-14)

- MC-12 spatial query contract:
  `docs/ai/references/2026-06-27-minecraft-mc12-spatial-query-contract-design.md`
  — `TrainingResultSpatialQueryStatus` with distinct `answered` from semantic
  `ready`; block-target query over MC-10 semantic lineage
- MC-14 action-facing consumer:
  `docs/ai/references/2026-06-27-minecraft-mc14-spatial-query-action-facing-consumer-design.md`
  — derived-only readiness from MC-12 manifest; no dispatch, no fourth
  artifact role
- Admission table (donor classification):
  `docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-admission-table.md`
  — both symbols marked `candidate core contract` with second-vertical
  requirement

### Related pattern context (not extraction approval)

- Core-A stage pattern:
  `docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-pattern.md`
- Core-B2 compare helper precedent (helper-internal enums, donor enums stay
  local):
  `docs/ai/references/2026-06-27-auv-core-b2-dual-backend-query-compare-helper-extraction.md`


## Owner review verdict (2026-06-27)

| Dimension | Verdict |
| --- | --- |
| Implementation (osu probe) | **Pass** — direction correct, boundary largely honest |
| Evidence | **Pass, probe-local only** |
| Graduation claim | **Pass, toned down** — admissibility language only |
| Extraction pressure | **Not pass — continue defer** |

Short form: proves **second-vertical recurrence exists**; does **not** prove
**extraction is worth doing now**.

## Weak equivalence: MC-14-analog, not MC-14 parity

osu walks **MC-14-analog derived consumption** (semantic → query → derived
readiness). MC-14 readiness is window reachability; osu is **capture-space
consumability only** — not dispatch-safe readiness. Shared part: derived-only
three-class eligibility **shape** over persisted query truth.

## Verdict per row

Graduation rules from the proof matrix apply. Rows below are assessed against
all five gates, with honest weight on what osu adds and what it does not.

### 1. Query status triad

**Recommended verdict (review language):** `candidate, helper-only admissible if concrete repetition emerges`

| Gate | Assessment |
| --- | --- |
| Second-vertical evidence | **Met (probe-local).** MC-12 (`TrainingResultSpatialQueryStatus`) + osu (`VisualTruthSpatialQueryStatus`). Non-Minecraft producer artifacts (`visual_truth_manifest.json`, `projection.json`). |
| Positive + negative paths | **Met.** MC-12: answered / blocked / failed under semantic gate pressure. osu: fixture positive + unit negatives (semantic blocked, target absent, corrupt JSON). |
| Extracted name not donor-shaped | **Not met for full enum graduation.** Current symbols remain `TrainingResult*` and `VisualTruth*`. A shared enum would need neutral naming and an owning layer decision first. |
| Shared boundary smaller than donor | **Met in principle** for a label helper only; **not met** for manifest or producer extraction. |
| Owning layer explicit | **Partial.** Helper would live in a narrow read/compare utility crate or domain-helper module — not runtime, not inspect presentation. Must be named before implement. |

**Footnote:** Probe-local recurrence only — single backend, no dual-backend
compare, **not extraction-pressure evidence**. A future label helper may be
admissible in principle; **default remains defer**. Full enum graduation still
not met (donor-shaped names; two verticals).

**Why not `candidate, not admissible yet`:** Single-donor blocker is cleared.
Keeping the pre-probe verdict would ignore recorded second-vertical evidence
without adding safety.

**Why not Core-B / full contract extraction:** Only two verticals; osu v1 has
no dual-backend compare; query target semantics differ (block face vs playfield
object index). A shared core enum would overfit MC-12 wire shape.

### 2. Action readiness view

**Recommended verdict (review language):** `candidate, helper-only admissible if concrete repetition emerges`

| Gate | Assessment |
| --- | --- |
| Second-vertical evidence | **Met (probe-local).** MC-14 (`derive_action_readiness`, three eligibility classes) + osu (`derive_visual_truth_spatial_query_action_readiness`). |
| Positive + negative paths | **Met.** MC-14: `click_ready`, `answer_non_clickable` (e.g. outside window), `not_consumable` (blocked/failed query). osu: `inside_capture` → `click_ready`, outside bounds → `answer_non_clickable`, query not answered → `not_consumable`. |
| Extracted name not donor-shaped | **Not met for full contract graduation.** Derivation functions and eligibility enums remain vertical-local. |
| Shared boundary smaller than donor | **Met** for a derived read-model **shape** helper (eligibility triad + optional refusal reason slot); **not met** for dispatch or persisted readiness artifacts. |
| Owning layer explicit | **Partial.** Same as query status: read-side / domain-helper ownership; explicitly not `ActionResolver` or runtime dispatch. |

**Footnote:** Derived triad **shape** recurrence only — osu is capture-space
consumability, not click authority. Future helper limited to triad labels +
refusal-reason slot + derived shape. **Default remains defer**; no generic
readiness layer.

**Why not `candidate, not admissible yet`:** The matrix’s MC-14-only blocker
is cleared by osu’s parallel derivation with inspect sections and frozen
fixture evidence.

**Why not Core-B:** osu explicitly disclaims window-click authority (pixel
coordinates from benchmark capture). MC-14 uses `projected_window_point` and
visibility. A generic readiness contract would either lie about click authority
or sprawl into platform-specific branches — exactly what Core-B must not do
yet.

## Minimal admissible extraction shape

If **concrete repetition pain** appears later — not as the natural next step
after this review — only these shapes are admissible.

### Allowed (helper-only)

| Row | Admissible shape | Owning layer | Example intent (not approved names) |
| --- | --- | --- | --- |
| Query status triad | Helper-internal status label or string mapping; optional `fn classify_query_status(answered, blocked, failed) -> HelperTriad` | Narrow utility crate or private module used by tests/docs | Map `TrainingResultSpatialQueryStatus` and `VisualTruthSpatialQueryStatus` labels to one helper enum for cross-vertical regression tables — **donor enums stay in place** |
| Action readiness view | Helper-internal eligibility triad + optional reason string; derived struct shape without manifest fields | Read-side or domain-helper module | `ReadinessEligibility { ClickReady, AnswerNonClickable, NotConsumable }` used by inspect formatters or shared test assertions — **not** a persisted artifact role |

Constraints that must hold:

- Extraction removes repetition in **label discipline or derived shape** only
- No `minecraft`, `osu`, `visual_truth`, `training_result`, or equivalent
  donor vocabulary in **public** helper names (internal test modules may map
  from donors)
- Helper is smaller than either vertical’s manifest + inspect surface
- Callers keep vertical-specific derivation functions; helper does not become
  the producer entrypoint

### Forbidden (explicit non-extractions)

Do **not** extract now or smuggle under “helper”:

| Category | Examples | Why forbidden |
| --- | --- | --- |
| Shared donor enums | `VisualTruthSpatialQueryStatus` ↔ `TrainingResultSpatialQueryStatus` unified enum in core | Names and wire shapes still donor-bound; only two verticals; falsifiers below not closed |
| Shared manifest structs | Query manifest, semantic manifest, inspect report unification | Violates “smaller than donor” and mixes persisted truth with presentation |
| Runtime dispatch wiring | `ActionResolver`, live click from readiness, MC-19-style wiring in core | Explicitly deferred; readiness must stay derived-only |
| Generic readiness layer | `trait SpatialQueryReadinessDeriver`, shared `derive_action_readiness` | Two derivations differ in visibility/bounds semantics; would invent false generality |
| Provider compare | `TrainingResultSpatialQueryComparisonVerdict` extraction | osu probe intentionally deferred dual-backend compare; row not satisfied |
| Stage status triad | `ready/blocked/failed` across persisted stage families | osu partial only (semantic + query); no second MC-style multi-stage family |
| Quality / witness / backend discipline | MC-16/17/19 surfaces | Out of scope for this review |
| SceneState / registry / blackboard / arbiter | Any cross-vertical runtime platform | Owner explicitly does not want now |

## Falsifiers / counter-evidence still required before full enum graduation

Helper-only admissible does **not** close these. Any one would revert toward
`candidate, not admissible yet` or keep extraction local indefinitely.

### Query status triad

1. **Third vertical collapses the triad** — a real consumer needs only
   boolean success/failure or merges `answered` with semantic `ready`.
2. **Helper mapping lies** — normalizing MC-12 and osu statuses drops
   information (e.g. osu `answered` outside capture vs MC-12 `answered` +
   `outside_window` visibility split).
3. **Dual-backend pressure** — when osu gains truth-vs-detection compare,
   query status alone may be insufficient without compare-verdict row (still
   not satisfied).

### Action readiness view

1. **Dispatch cannot stay separate** — a vertical requires mutating query
   manifests to express readiness, breaking MC-14 / osu derived-only boundary.
2. **Eligibility triad insufficient** — a vertical needs a fourth class (e.g.
   deferred click, permission-gated, or activation-only) that does not map
   honestly to the three labels.
3. **Click authority conflation** — shared contract implies window-click
   authority where osu only has capture pixels; consumers treat helper output
   as dispatch-safe without reading vertical disclaimers.

### Cross-row (both)

1. **Only two verticals** — pattern may be coincidence between Minecraft
   3DGS training-result chain and osu visual-truth eval chain.
2. **No shared consumer yet** — parallel local implementations do not prove a
   shared API reduces real duplication without inventing concepts.
3. **Core-B scope creep** — extraction PR bundles query status, readiness,
   inspect cards, or runtime wiring in one change.

## Recommended sequence

1. **This review** — record admissibility-only verdict; **default defer
   extraction**; main matrix rows stay `candidate, not admissible yet` until
   owner explicitly accepts graduation language.
2. **Defer helper extraction** — optional only after concrete repetition pain;
   **not** the natural next slice.
3. **Defer** osu witness/quality/live-click parity, dual-backend compare,
   stage-status triad graduation, provider comparison, and backend-label
   discipline — per owner scope and proof-matrix partial/not-satisfied rows.

Do not start Core-B, a new core crate, or cross-vertical runtime platform work
from this review.

## One-sentence summary

The osu! probe is **brake evidence** that query-status and readiness
**shapes** recur across two verticals — **helper-only admissible** in review
language only if repetition later hurts — **not** extraction pressure now, **not**
a Core-B starter, **not** dispatch-safe readiness proof.
