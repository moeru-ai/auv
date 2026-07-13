# 2026-06-28 AUV Core-C1 action attempt admission falsifier review

Date: 2026-06-28

Status: design-only falsifier review for [`2026-06-28-auv-core-c1-action-attempt-admission-design.md`](2026-06-28-core-action-attempt-admission-design.md). Tests whether Core-C1 can name the admission boundary without smuggling runtime platform work. **No code changes.** Does not open Core-C2/C3 implementation.

## Scope

Core-C1 proposes a **design-only** admission layer between derived action readiness (Core-A) and vertical dispatch wiring (MC-19 donor). This review applies four owner falsifiers against repo evidence and the design note.

**Primary inputs (read-only):**

- [`2026-06-28-auv-core-c1-action-attempt-admission-design.md`](2026-06-28-core-action-attempt-admission-design.md)
- [`2026-06-27-minecraft-mc19-query-to-live-click-wiring-design.md`](../apps/minecraft/2026-06-27-minecraft-probe-19-reference.md)
- [`2026-06-27-auv-core-a-query-readiness-falsifier-review.md`](2026-06-30-query-readiness-closeout.md)
- `crates/auv-game-minecraft/src/training_result_spatial_query_action_wiring.rs`
- `crates/auv-query-readiness/src/lib.rs`

## Non-goals

This review does **not**:

- change proof-matrix main verdicts or Core-A graduation language
- change MC-17/18/19 technical conclusions
- approve Core-C2 helper extraction, inspect renames, or crate moves
- approve generic runtime traits, action lease, or persisted admission artifacts
- push osu dispatch wiring or MC-20

## Verdict vocabulary

| Verdict | Meaning |
| --- | --- |
| **PASS** | Falsifier not triggered; design boundary holds |
| **FAIL** | Falsifier triggered; design would need rework or scope shrink |
| **PASS with explicit defer** | Boundary acceptable as design-only; named gap stays intentionally open for a later owner slice |

## Falsifier 1 — Readiness stolen as authority

**Claim:** Core-C1 would let derived readiness become dispatch authority — consumers treat `ready` / `click_ready` as proof of click-safe or semantically correct action without reading vertical disclaimers.

| Evidence | Assessment |
| --- | --- |
| Design maps `click_ready` → core `ready` with explicit disclaimer that `ready` is not window authority or verification | Boundary stated in design §Donor→core vocabulary |
| Core-A falsifier review: click-authority conflation **latent risk**, not triggered — osu uses `pixel_point`, MC-19 MC-only | Core-C1 inherits risk; does not remove mitigations |
| MC-19 `known_limits` and design `MC19_V1_D4_*` remain on wiring outcome | Admission record slots `dispatch_outcome` / optional `verification_outcome` keep layers separate |
| `auv-query-readiness` NOTICE separates driver readiness domain | Helper labels stay eligibility-only |

**Verdict:** **PASS with explicit defer** — design preserves readiness-as-derived only and three failure layers; **defer** inline inspect disclaimer and any future osu dispatch wiring to explicit vertical slices (same latent risk as Core-A falsifier review).

## Falsifier 2 — Core-C1 stolen as runtime / controller

**Claim:** Core-C1 design secretly introduces registry, arbiter, planner, blackboard, action lease, or generic controller/runtime platform.

| Evidence | Assessment |
| --- | --- |
| Design admission results: only `attempt_once` \| `refuse_before_dispatch` | No queue, plan, lease, or retry vocabulary |
| Explicit non-goals table rejects registry, blackboard, planner, lease, generic runtime trait | Matches MC-19 design non-goals |
| MC-19 `QueryLiveClickExecutor` stays vertical-local injectable trait; design says not a core trait | No extraction pressure from C1 alone |
| No new Core-B or runtime crate named in design | Core-B reopening explicitly forbidden |

**Verdict:** **PASS** — Core-C1 is vocabulary + record shape only; falsifier not triggered.

## Falsifier 3 — Needs new persisted schema / artifact

**Claim:** Honest admission recording requires a new persisted artifact role or schema version beyond existing operation-result / trace surfaces.

| Evidence | Assessment |
| --- | --- |
| MC-19 `QueryActionWiringOutcome` already records `attempted`, eligibility, `refusal_reason`, `window_point`, `click_summary` on wiring path | Donor shape fits minimal record without fourth query artifact |
| Design requires `source_readiness_ref` as lineage pointer, not new readiness persistence | Aligns with MC-14 derived-only rule |
| `verification_outcome` explicitly deferred to later layer | Avoids schema creep in C1 |
| No `ActionAdmissionManifest` or similar proposed as required | Record extends existing surfaces |

**Verdict:** **PASS with explicit defer** — current MC-19 evidence suffices for design closure; **defer** any cross-vertical persisted admission schema until Core-C2+ and owner names concrete repetition pain.

## Falsifier 4 — Minecraft donor names in core contract

**Claim:** Core-C1 core vocabulary smuggles Minecraft or `auv-query-readiness` donor strings (`click_ready`, `window_point`, `TrainingResult*`) into the **public core contract** as if already graduated.

| Evidence | Assessment |
| --- | --- |
| Core labels: `ready`, `non_actionable`, `not_consumable` — neutral | Donor mapping table is explicit one-way |
| Admission results: `attempt_once`, `refuse_before_dispatch` — not MC-19 names | No `QueryActionWiringOutcome` in core contract |
| `action_point` generic slot vs donor `window_point` / `pixel_point` | Vertical geometry stays donor-local |
| Design forbids moving Minecraft wiring code in C1 | Names stay in design doc mapping table only |

**Verdict:** **PASS** — falsifier not triggered; donor names appear only as mapping references, not as graduated public API.

## Overall verdict

| Item | Stance |
| --- | --- |
| Core-C1 design boundary | **PASS with explicit defer** |
| Falsifiers triggered | **none** |
| Explicit defers | (1) click-authority latent risk inherited from Core-A; (2) persisted cross-vertical admission schema until C2+; (3) no C2/C3 implement without owner slice |
| Core-A proof-matrix rows 66/68 | **Unchanged** — still `candidate, not admissible yet` |
| Core-A graduation admissibility language | **Unchanged** — helper-only admissible in review language only; default defer |
| MC-19 technical conclusions | **Unchanged** — wiring proof stands; C1 owns generic vocabulary only |
| Next slice | **Owner choice only** — not Core-C2, Core-B, or MC-20 from this review |

## Observations (not approved work)

1. **Label alignment debt** — `auv-query-readiness` still emits donor strings (`click_ready`); Core-C2 may map to core labels in inspect/helpers only if owner approves.
2. **Second vertical dispatch** — osu donor wiring landed (PR #54:
`visual_truth_spatial_query_action_wiring` + `run_osu_query_wired_live_action`;
live closure `run_1782631533865_61190_0`). Vertical evidence only, **not** generic
core extraction. Core-A2 re-review: authority conflation **latent, boundary exercised**
by two donors — see
[`2026-06-28-auv-core-a2-full-chain-falsifier-review.md`](2026-06-30-query-readiness-closeout.md).
3. **Verification slot** — Layer 3 remains intentionally thin; do not backfill gameplay verification into C1 admission records without a named verification slice.
4. **Dispatch outcome field split** — Core-C1 conceptual record uses `dispatch_outcome` for Layer 2 (post-attempt driver/invoke failure); MC-19 donor `QueryActionWiringOutcome` overloads `refusal_reason` for executor `Err` when `attempted=true` and has no `dispatch_outcome` field. **Defer** donor-to-core field alignment; no implementation promise in C1.

## One-sentence summary

Core-C1 falsifier review **passes with explicit defer** — admission vocabulary and record shape close honestly without controller/runtime/schema graduation; implementation stays deferred to owner-named C2+ slices.
