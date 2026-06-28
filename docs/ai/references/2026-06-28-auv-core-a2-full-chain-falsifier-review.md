# 2026-06-28 AUV Core-A2 full-chain falsifier review

Date: 2026-06-28

Status: design-only falsifier review after osu completes
`semantic → query → readiness → live admission/dispatch → witness → quality
evidence` on `main` @ `91577c5` (PR #54 + OSU-WQ1). Re-tests Core-A dispatch
separation and Core-C1 boundaries with osu as **second vertical live admission
donor**. **No code changes.**

## Scope

**Covers:**

- Full-chain falsifier pass vs prior Core-A falsifier (query/readiness only)
- Core-C1 / dispatch separation re-review (owner question 4)
- Triggered vs latent risk inventory after wired live action

**Does not:**

- upgrade proof-matrix rows 66/68 (query/readiness — prior reviews stand)
- open Core-C2, Core-B, MC-20, controller/planner
- change runtime or inspect code

**Primary inputs (read-only):**

- [`2026-06-27-auv-core-a-query-readiness-falsifier-review.md`](2026-06-27-auv-core-a-query-readiness-falsifier-review.md)
- [`2026-06-28-auv-core-c1-action-attempt-admission-design.md`](2026-06-28-auv-core-c1-action-attempt-admission-design.md)
- [`2026-06-28-auv-core-c1-action-attempt-admission-review.md`](2026-06-28-auv-core-c1-action-attempt-admission-review.md)
- [`2026-06-28-osu-visual-truth-query-wired-live-action-design.md`](2026-06-28-osu-visual-truth-query-wired-live-action-design.md)
- [`2026-06-28-osu-visual-truth-query-wired-live-action-live-closure.md`](2026-06-28-osu-visual-truth-query-wired-live-action-live-closure.md)
- `crates/auv-game-osu/src/visual_truth_spatial_query_action_wiring.rs`
- `src/osu.rs` (`run_osu_query_wired_live_action`), `src/inspect.rs`, `src/run_read.rs`

Companion graduation review (rows 65/69/70):
[`2026-06-28-auv-core-a2-stage-quality-graduation-review.md`](2026-06-28-auv-core-a2-stage-quality-graduation-review.md)

## Verdict vocabulary

| Verdict | Meaning |
| --- | --- |
| **Not triggered** | No real vertical or consumer falsifies the claim |
| **Latent risk** | Boundary could mislead future consumers; mitigations exist but require discipline |
| **Triggered** | Would force revert to local-only or require merged / fourth model |
| **Open** | Future pressure; not current failure |

---

## Full-chain evidence snapshot

```text
visual_truth + projection
  → VisualTruthSemanticStatus          [ready/blocked/failed]
  → VisualTruthSpatialQueryStatus      [answered/blocked/failed]
  → derive_visual_truth_spatial_query_action_readiness
  → wire_readiness_to_action           [Core-C1 donor — PR #54]
  → InvokeWindowPointClickExecutor / stub
  → operation-result + inspect wired-live section

visual_eval_report.json
  → DetectionEvalWitnessStatus         [ready/blocked/failed]
  → DetectionEvalQualityVerdict        [measured_only|metric_partial|blocked|failed]
```

Recorded live closure: `run_1782631533865_61190_0` in
[`2026-06-28-osu-visual-truth-query-wired-live-action-live-closure.md`](2026-06-28-osu-visual-truth-query-wired-live-action-live-closure.md)
— `attempted=true`, `pixel_point=400,300`, `window_point=756.000,474.500`, honest
Layer-2 `input.clickWindowPoint` failure.

---

## Question 4 — Core-C1 / dispatch separation

**Owner question:** Prior claim — *"authority conflation not yet hit by second
vertical"* — still true?

### Re-test: dispatch cannot stay separate

| Falsifier | Claim | Post-PR #54 evidence | Verdict |
| --- | --- | --- | --- |
| Manifest mutation for readiness | Vertical mutates query manifests to express readiness | `derive_visual_truth_spatial_query_action_readiness` reads manifest; wiring reads readiness output only (`visual_truth_spatial_query_action_wiring.rs`) | **Not triggered** |
| Readiness/dispatch coupling | Dispatch path skips admission gate | `wire_readiness_to_action`: `click_ready` → executor; other eligibilities → `attempted=false` with `refusal_reason` — mirrors MC-19 | **Not triggered** |
| Osu live path absent | Second vertical never exercises admission | `run_osu_query_wired_live_action` in `src/osu.rs`; inspect **Osu Visual Truth Query Wired Live Action** section; integration + live closure runs | **Supersedes prior "no osu dispatch" evidence** — boundary now **exercised**, not absent |

**Dispatch separation summary:** **Still holds.** osu wired live action is a
vertical donor proof parallel to MC-19, not a core extraction or readiness
authority upgrade.

### Re-test: click authority conflation

| Falsifier | Claim | Post-PR #54 evidence | Verdict |
| --- | --- | --- | --- |
| Shared `click_ready` implies window authority | Consumers treat osu `click_ready` as dispatch-safe without reading disclaimers | Readiness section: `pixel_point=` only. Wired section: **both** `pixel_point` and `window_point`, `readiness_class`, `dispatch_command`, `dispatch_outcome`. `known_limits` includes `osu_query_wired_live_action_capture_space_readiness_live_window_dispatch_no_gameplay_verification` on operation-result | **Latent risk** (elevated exposure, not triggered) |
| Stale pixel used for live click | Dispatch clicks manifest capture pixels | Design + wiring: re-resolve playfield from `VisualTruthFrame`, map via **live** `PlayfieldProjection::for_window` — same as benchmark typed dispatch | **Not triggered** |
| Inspect omits capture-space disclaimer on wired path | Wired inspect line conflates readiness and dispatch authority | Wired section prints both coordinate spaces; readiness-only section unchanged. Inline one-line disclaimer still absent on readiness line (same as Core-A falsifier) | **Latent risk** (unchanged from Core-A) |

**Authority conflation summary:** Prior statement *"not yet hit by second vertical"*
is **no longer accurate as written**. The second vertical **now hits the boundary**
via live admission wiring, but falsifier remains **not triggered** — mitigations
hold and dispatch uses live playfield projection, not capture-pixel authority.

**Updated formulation:** *Authority conflation is a **latent risk exercised by
two vertical live admission donors** (MC-19 + osu); not triggered under current
inspect/run_read discipline.*

---

## Core-C1 falsifier re-pass (osu as second donor)

| Core-C1 falsifier | MC-19 only (2026-06-28 C1 review) | + osu PR #54 | Verdict |
| --- | --- | --- | --- |
| Readiness stolen as authority | PASS with defer | osu maps `click_ready` → admission with explicit capture-vs-window disclaimers; Layer 2 failure recorded honestly on live run | **PASS with explicit defer** |
| C1 stolen as runtime/controller | PASS | No registry/planner; `VisualTruthQueryLiveClickExecutor` vertical-local | **PASS** |
| Needs new persisted schema | PASS with defer | `VisualTruthQueryActionWiringOutcome` on operation-result path; no fourth artifact role | **PASS with explicit defer** |
| Donor names in core contract | PASS | Core-C1 labels remain design-only; osu donor fields local | **PASS** |

No Core-C1 falsifier **triggered** by second vertical wiring.

---

## Full-chain falsifier table (incremental vs Core-A)

| Falsifier | Prior Core-A (pre-wiring) | Core-A2 (full chain) | Δ |
| --- | --- | --- | --- |
| Fourth query/readiness state | Not triggered | Not triggered | — |
| Status-only query helper lies | Latent risk | Latent risk | — |
| Dual-backend compare pressure | Open | Open (osu still single query backend) | — |
| Dispatch cannot stay separate | Not triggered (osu unwired) | **Not triggered (osu wired)** | Evidence upgraded |
| Click authority conflation | Latent (osu unwired) | **Latent (osu wired, mitigations hold)** | Boundary exercised |
| Only two verticals | Open | Open | — |
| No shared consumer | Open | Open | — |
| Stage triad shallow | Partial | **Witness + quality stages landed** | See A2 stage review |
| Quality verdict recurrence | Candidate probe | **Probe-local satisfied** | Main row unchanged |
| Gameplay / action verification conflation | N/A (out of scope) | WQ1 + wired action both document evidence-only / no hit verification | **Not triggered** |

**Triggered falsifiers found:** **none**

---

## Consumer inventory (post-wiring delta)

| Consumer | New/changed since Core-A falsifier | Authority risk |
| --- | --- | --- |
| `src/inspect.rs` **Osu Visual Truth Query Wired Live Action** (~L2319) | **New section** — `pixel_point`, `window_point`, `dispatch_outcome`, `readiness_class` | Low if read as pair; medium if `click_ready` read alone |
| `src/run_read.rs` `OsuQueryWiredLiveActionSummary` | **New summary struct** | Low — mirrors inspect fields |
| `src/osu.rs` `run_osu_query_wired_live_action` | **New runtime entry** | Low — vertical-local; macOS gate |
| `examples/osu_query_wired_live_action.rs` | Live closure harness | Evidence only |
| WQ1 inspect sections | Witness + quality read-side | Low — no dispatch |

---

## Overall verdict

| Item | Stance |
| --- | --- |
| Core-C1 design boundary | **PASS with explicit defer** — unchanged |
| Dispatch separation | **Holds** with osu as second live admission donor |
| Authority conflation | **Latent risk, boundary exercised** — prior "not hit by second vertical" **retired** |
| Proof matrix rows 66/68 | **Unchanged** — `candidate, not admissible yet` / helper-only admissible in prior review language only |
| Proof matrix row 65 | See A2 stage review — **helper-only admissible** |
| Proof matrix rows 69/70 | **Unchanged** on main matrix — see Core-A4⁵ post-A3 re-confirmation |
| Next slice | **Owner choice** — not Core-C2, Core-B, MC-20 from this review; Core-A5a/A5b deferred per A4⁵ |

## Observations (not approved work)

1. **Inline inspect disclaimer** — consider one-line capture-space note on osu
   readiness inspect line if owner approves inspect polish (same defer as Core-A).
2. **Layer 2 field overload** — osu + MC-19 both overload outcome message fields
   for post-attempt failures; Core-C2 defer stands.
3. **Live closure is not semantic success** — `run_1782631533865_61190_0` proves
   admission + dispatch honesty, not osu gameplay hit.
4. **Third vertical** — two live admission donors reduce coincidence argument for
   Core-C1 vocabulary but not for Core-A enum extraction. Core-X1⁶ scouts
   third-vertical candidates (Balatro primary scout, not existing donor) for
   rows 69/70 and coincidence risk — see
   [`2026-06-29-auv-core-x1-third-vertical-scouting-design.md`](2026-06-29-auv-core-x1-third-vertical-scouting-design.md).

## Explicit defer list

| Slice | Trigger |
| --- | --- |
| Core-C2 admission helper / inspect label alignment | Owner names slice + repetition pain |
| Core-B query/readiness/quality enum extraction | Helper-only admissible rows + shared consumer need |
| MC-20 / controller | Owner opens planner slice |
| osu gameplay verification (Layer 3) | Owner names verification slice |
| Inline inspect disclaimer | Owner approves read-side polish |
| Core-X2 third-vertical consumption probe | Owner accepts Core-X1 MVP + names candidate (Balatro default) |

## Related references (post-A2)

- Core-X1 third-vertical scouting (rows 69/70 donor scout; footnote⁶):
  [`2026-06-29-auv-core-x1-third-vertical-scouting-design.md`](2026-06-29-auv-core-x1-third-vertical-scouting-design.md),
  [`2026-06-29-auv-core-x1-third-vertical-admissibility-mvp.md`](2026-06-29-auv-core-x1-third-vertical-admissibility-mvp.md)
- Core-A4 quality/backend falsifier gate (rows 69/70):
  [`2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md`](2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md)

## One-sentence summary

Full-chain falsifier review finds **no triggered failures** — dispatch separation
holds with MC-19 and osu wired live action as dual Core-C1 donors; **authority
conflation remains latent but is now exercised by the second vertical**, not
absent from it.
