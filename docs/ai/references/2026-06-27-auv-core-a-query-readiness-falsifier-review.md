# 2026-06-27 AUV Core-A query status and action readiness falsifier review

Date: 2026-06-27

Status: design-only falsifier review. Systematically tests graduation-review
falsifiers against MC + osu repo evidence and current read-side consumers.
**No code changes.** Proof-matrix verdict language unchanged.

## Scope

This note closes the post-merge item from
[`2026-06-27-auv-core-query-readiness-helper-extraction-closeout.md`](2026-06-27-auv-core-query-readiness-helper-extraction-closeout.md)
§6: owner-directed falsifier-oriented review after helper extraction landed on
`feat/osu-second-vertical-consumption-probe`.

**Covers:** query status triad and action readiness view only (proof-matrix rows
66/68). Maps three owner questions to graduation-review falsifiers
([§202–236](2026-06-27-auv-core-a-query-readiness-graduation-review.md)),
surveys repo evidence, inventories consumers, and records per-falsifier verdicts.

**Primary inputs (read-only):**

- [`2026-06-27-auv-core-a-query-readiness-graduation-review.md`](2026-06-27-auv-core-a-query-readiness-graduation-review.md)
- [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)
- [`2026-06-27-auv-second-vertical-consumption-probe-osu-design.md`](2026-06-27-auv-second-vertical-consumption-probe-osu-design.md)
- [`2026-06-27-auv-core-query-readiness-helper-extraction-closeout.md`](2026-06-27-auv-core-query-readiness-helper-extraction-closeout.md)

## Non-goals

This review does **not**:

- change runtime, inspect, or game-crate code
- extract query status enum, stage triad, provider compare, or shared `derive_*`
- expand `auv-query-readiness` beyond the landed helper scope
- upgrade proof-matrix row verdicts or graduation-review admissibility language
- open Core-B, MC-20, controller, SceneState / registry / blackboard / arbiter
- wire osu live-click dispatch or dual-backend compare

If a falsifier were **Triggered**, this slice would record it as an observation
only — no fix without owner approval.

## Verdict vocabulary

| Verdict | Meaning |
| --- | --- |
| **Not triggered** | No real vertical or consumer currently falsifies the claim |
| **Latent risk** | Label or shape reuse could mislead a future consumer; mitigations exist in docs / `known_limits` but not always inline at every consumer |
| **Triggered** | Would force revert to local-only or require a fourth class / merged model |
| **Open** | Cross-row structural uncertainty; not a current failure |

## Three owner questions

| # | Owner question | Graduation falsifiers |
| --- | --- | --- |
| 1 | Is a fourth query/readiness state required in a real vertical? | Query: *Third vertical collapses triad*; *Helper mapping lies*; *Dual-backend pressure*. Readiness: *Eligibility triad insufficient* |
| 2 | Can dispatch stay separate from readiness derivation? | Readiness: *Dispatch cannot stay separate* |
| 3 | Is capture-space consumability mistaken for click authority? | Readiness: *Click authority conflation* |

---

## Question 1 — Fourth query/readiness state?

### Query status triad falsifiers

| Falsifier | Claim | Repo evidence | Verdict |
| --- | --- | --- | --- |
| Third vertical collapses triad | A real consumer needs boolean success/failure or merges `answered` with semantic `ready` | MC: `TrainingResultSpatialQueryStatus` — `answered \| blocked \| failed` in `crates/auv-game-minecraft/src/training_result_spatial_query.rs`. osu: `VisualTruthSpatialQueryStatus` — same triad in `crates/auv-game-osu/src/visual_truth_spatial_query.rs`. Semantic `ready` is a **separate** stage (`TrainingResultSemanticStatus` / `VisualTruthSemanticStatus`), not a fourth query status. Third-vertical scan: `auv-game-balatro` has no spatial-query manifest family; archived `candidate-action` has no `SpatialQuery` surface. Only MC + osu persist spatial-query manifests. | **Not triggered** |
| Helper mapping lies | Normalizing MC + osu query **status alone** drops information | MC `answered` + `visibility=outside_window` → readiness `answer_non_clickable` via `derive_action_readiness` (`training_result_spatial_query_action.rs`). osu `answered` + `pixel_visibility=outside_capture` → same eligibility split via `derive_visual_truth_spatial_query_action_readiness` (`visual_truth_spatial_query_action.rs`). osu design L56–68: query `answered` includes outside-capture answers; readiness splits via `answer_non_clickable`. A status-only shared enum would collapse MC visibility and osu pixel side channels. | **Latent risk** (active brake on query-status enum extraction) |
| Dual-backend pressure | Query status alone insufficient when dual-backend compare lands | MC already runs provider/reference compare (`training_result_spatial_query.rs`, `TrainingResultSpatialQueryComparisonVerdict`). osu v1 is single-backend (`playfield_projection_reference`); dual-backend compare deferred per osu design and proof-matrix provider-compare row (`not satisfied`). | **Open** (future pressure; not current failure) |

### Action readiness — eligibility triad

| Falsifier | Claim | Repo evidence | Verdict |
| --- | --- | --- | --- |
| Eligibility triad insufficient | A vertical needs a fourth class (deferred click, permission-gated, activation-only) | MC-14: `click_ready \| answer_non_clickable \| not_consumable` via `auv_query_readiness::DerivedActionEligibility`. osu: same triad; outside-capture maps to `answer_non_clickable`, not a fourth enum. MC-19 dispatch gate (`training_result_spatial_query_action_wiring.rs`) uses `attempted=false` for non-`click_ready` — refusal, not a fourth eligibility. Driver `ReadinessStatus` and archived `activation_only` are **unrelated** readiness domains (helper crate NOTICE cites `auv-driver` separation). | **Not triggered** |

**Question 1 summary:** Triad sufficient for **current two verticals**. Semantic `ready` ≠ query `answered` is intentional layering. Status-only normalization would lie — query-status enum extraction remains deferred.

---

## Question 2 — Dispatch separation?

| Falsifier | Claim | Repo evidence | Verdict |
| --- | --- | --- | --- |
| Dispatch cannot stay separate | A vertical mutates query manifests to express readiness | MC-14: `derive_action_readiness` reads manifest fields; no manifest mutation (`training_result_spatial_query_action.rs`). MC-19: `derive_action_readiness` → `wire_readiness_to_action`; dispatch is downstream gate with `attempted=false` on non-click-ready (`training_result_spatial_query_action_wiring.rs`; `src/minecraft.rs` `wire_spatial_query_manifest_to_action`). osu probe: derived readiness only; no live-click wiring per osu design L21–22. Read-side: `run_read.rs` and `inspect.rs` call vertical `derive_*`; MC-19 section (`MC-19 Query Wired Live Action`) is separate from MC-14 / osu readiness sections. `docs/TERMS_AND_CONCEPTS.md` L255–266: action readiness view does not dispatch. | **Not triggered** |

**Question 2 summary:** Architectural separation holds. MC-19 consumes derived readiness; osu never wired dispatch. Residual risk is **narrative** (shared `click_ready` label across MC-19 and MC-14 sections), not coupling through manifest mutation.

---

## Question 3 — Click authority conflation?

### Consumer inventory

| Consumer | Role | Authority signal | Disclaimer / mitigation | Risk |
| --- | --- | --- | --- | --- |
| `src/inspect.rs` MC-14 section (~L2151) | Text inspect | `window_point=` | Uses MC manifest `visibility`; separate from MC-19 | Low |
| `src/inspect.rs` osu readiness section (~L2274) | Text inspect | `pixel_point=` (not `window_point`) | No inline capture-space disclaimer on the line; design doc + manifest `known_limits` carry it | Medium |
| `src/inspect.rs` MC-19 section (~L2293) | Text inspect | `window_point=`; `attempted` | MC-only; `MC19_V1_D4_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT` on wiring | Low (MC-only) |
| `src/inspect_server_viewer.html` | Viewer JS | `deriveSpatialQueryActionReadiness` → `window_point` | MC manifest path only (~L1747); no osu spatial-query readiness path | Low for osu |
| `src/run_read.rs` | Summary structs | Vertical `derive_*` only; MC `window_point`, osu `pixel_point` | No `auv_query_readiness` import; branching stays donor-local | Low |
| `src/minecraft.rs` MC-19 | Runtime dispatch | `window_point` via `QueryLiveClickExecutor` | `known_limits` on wiring outcome | MC-only |
| `crates/auv-query-readiness/src/lib.rs` | Shared eligibility labels | `click_ready` string is vertical-agnostic | Crate NOTICE: not driver readiness; mapping stays donor-local | Medium (label) |
| osu manifest `known_limits` | Persisted artifact | — | `visual_truth_spatial_query.rs` ~L179–181: pixel-not-window disclaimer | Mitigation |
| osu design L72–79 | Reference doc | — | Explicit capture-space vs click-authority boundary | Mitigation |

| Falsifier | Claim | Repo evidence | Verdict |
| --- | --- | --- | --- |
| Click authority conflation | Shared contract implies window-click authority where osu only has capture pixels | Same `click_ready` string from `DerivedActionEligibility` in MC (window) and osu (capture pixels). Current consumers: osu inspect prints `pixel_point=`; viewer has no osu readiness path; no osu dispatch wiring. Mitigations in osu design, manifest `known_limits`, helper NOTICE. Inspect osu line lacks inline disclaimer. | **Latent risk** |

**Question 3 summary:** Not triggered today — no consumer treats osu `click_ready` as dispatch-safe window authority. Watch item for future osu dispatch wiring (explicitly out of scope).

---

## Cross-row falsifiers

| Falsifier | Evidence | Verdict |
| --- | --- | --- |
| Only two verticals | MC + osu only; balatro and archived AX lack spatial-query manifest families | **Open** |
| No shared consumer yet | `auv-query-readiness` dedupes eligibility glue only; `derive_*` stays per-vertical; no shared derive consumer | **Open** |
| Core-B scope creep | Helper closeout PASS; PR #52 + closeout bound scope to one helper; no bundled enum / inspect / dispatch extraction | **Not triggered** |

---

## Overall verdict

| Item | Stance |
| --- | --- |
| Proof matrix rows 66/68 | Remain **`candidate, not admissible yet`** — main matrix authority unchanged |
| Graduation review admissibility language | Unchanged — helper-only admissible in review language only; default defer |
| Query status enum extraction | **Still deferred** — mapping-lies falsifier active |
| Action readiness enum graduation | **Still deferred** — label conflation latent; two verticals only |
| `auv-query-readiness` helper | **No expansion** — falsifier review does not justify new helper work |
| Next slice | Owner choice only; **not** pre-approved extraction |

**Triggered falsifiers found:** **none**

---

## Observations (latent risks — not approved slices)

1. **Status-only query helper would lie** — any future query-status enum must carry or reference visibility / pixel side channels, or stay deferred.
2. **Shared `click_ready` label** — document-boundary discipline required for future osu dispatch; consider inline inspect disclaimer if dispatch wiring is ever approved.
3. **Dual-backend compare** — when osu gains a second query backend, provider-compare row and query-status triad must be revisited together (open falsifier).
4. **Third vertical** — pattern recurrence across two chains may be coincidence until a third donor appears.

---

## One-sentence summary

Falsifier review finds **no triggered failures** — triad and dispatch separation hold for MC + osu; **latent** risks remain on status-only normalization and `click_ready` label reuse; proof-matrix rows 66/68 stay **`candidate, not admissible yet`**.
