# 2026-06-28 AUV Core-C1: action attempt admission design

Date: 2026-06-28

Status: **design-only boundary note**. Defines when derived readiness may admit
**one** action attempt versus **refuse before dispatch**, and how that decision
is recorded **without** pretending to be a controller, runtime platform, or
new persisted artifact role. **No code changes** are approved by this document.

## One-line summary

Core-C1 names the **admission boundary** between derived action readiness and
**one honest live attempt or explicit pre-dispatch refusal** — vocabulary and
record shape only; MC-19 is the current donor proof, not the generic core
runtime.

## Core question

When a derived action readiness view says a query answer is consumable for
action-facing code, can AUV:

1. admit **exactly one** controlled action attempt, or
2. **refuse before dispatch** with preserved upstream semantics,

and record that decision honestly **without** inventing controller authority,
action leases, or a fourth persisted artifact role?

Core-C1 answers **how to name and record** that gate. It does **not** implement
dispatch, driver wiring, or verification.

## Position in the consumption chain

```text
Producer Artifact
  → Semantic Gate
  → Spatial Query                    [persisted query truth]
  → Action Readiness View            [derived only — Core-A]
  → Action Attempt Admission         [Core-C1 — this note]
  → Dispatch / driver attempt        [vertical wiring — e.g. MC-19]
  → Verification                     [separate layer — later]
```

Core-A closed the **readiness view** shape (eligibility triad, optional refusal
reason, derived-only). Core-C1 closes the **admission vocabulary** that sits
**immediately downstream** of readiness and **immediately upstream** of vertical
dispatch wiring.

## Donor → core vocabulary

Core-C1 uses **neutral core labels**. Vertical donors keep local names; mapping
is explicit at the vertical boundary.

| Donor label (MC-14 / `auv-query-readiness`) | Core-C1 `readiness_class` | Admission when derived |
| --- | --- | --- |
| `click_ready` | `ready` | `attempt_once` — one live action attempt may proceed to dispatch |
| `answer_non_clickable` | `non_actionable` | `refuse_before_dispatch` — answered but not actionable at the derived point |
| `not_consumable` | `not_consumable` | `refuse_before_dispatch` — upstream query or precondition not consumable |

**Mapping rules:**

- Core `ready` does **not** imply window-click authority, gameplay success, or
  semantic verification. It means derived readiness permits **one attempt** at
  the vertical's declared action point (when present).
- Core `non_actionable` preserves MC-14 / osu visibility or bounds side channels
  via `refusal_reason`; Core-C1 does not collapse them into a boolean.
- Core `not_consumable` preserves upstream query `status` / `reason` lineage; no
  re-labeling into fake readiness.

## Admission results (only two)

Core-C1 admits **only** these outcomes at the admission boundary:

| `admission_result` | Meaning |
| --- | --- |
| `attempt_once` | Readiness is `ready`; exactly **one** dispatch may be attempted. No replan, no retry policy, no lease renewal. |
| `refuse_before_dispatch` | Readiness is `non_actionable` or `not_consumable` (or defensive refusal when `ready` lacks a required action point); **no** driver/input dispatch. |

There is **no** third admission class such as `defer`, `queue`, `plan`, or
`activation_only` at this layer. Vertical-specific activation-only boundaries
stay in driver / verification domains, not Core-C1.

## Minimal admission record shape

Core-C1 defines a **conceptual record** for inspect and run lineage. Vertical
implementations may embed these fields in existing operation-result / trace
surfaces; Core-C1 does **not** require a new persisted artifact role.

| Field | Required | Role |
| --- | --- | --- |
| `attempted` | yes | `true` iff dispatch was invoked; `false` iff refused before dispatch |
| `readiness_class` | yes | Core vocabulary: `ready` \| `non_actionable` \| `not_consumable` |
| `source_readiness_ref` | yes | Pointer to derived readiness provenance (query manifest path, artifact ref, or vertical lineage bundle) — not a new persisted readiness artifact |
| `action_point` | optional | Vertical action coordinates when known (e.g. `window_point`, `pixel_point`); omitted when not applicable or refused |
| `refusal_reason` | optional | Present when `attempted=false` or defensive refusal; copies MC-14 / osu refusal text without re-labeling |
| `dispatch_outcome` | optional | Driver or invoke summary when `attempted=true`; structured error string on dispatch failure **after** attempt |
| `verification_outcome` | optional, later layer | Semantic / gameplay verification — **not** part of Core-C1 admission; recorded separately when a vertical owns verification |

**Recording discipline:**

- Admission record **describes** the gate decision; it is not a controller state machine.
- `attempted=false` with `readiness_class=ready` is allowed only for **defensive** refusal (e.g. missing action point while eligibility says ready) — still `refuse_before_dispatch`, not a fourth eligibility class.
- Prefer extending existing `OperationResult` / operation-result artifacts over inventing `ActionAdmissionManifest` or similar fourth roles.

## Three failure layers

Core-C1 separates failures so inspect and replay stay honest:

```text
Layer 1 — Semantic refusal before dispatch
  readiness_class ∈ { non_actionable, not_consumable }
  OR defensive refusal while nominally ready
  → attempted=false, refusal_reason set, no dispatch_outcome

Layer 2 — Dispatch / driver failure after attempt
  readiness_class=ready, attempted=true
  → dispatch_outcome records driver/invoke error; not a readiness upgrade

Layer 3 — Post-action verification failure (later layer)
  attempted=true, dispatch may succeed at input layer
  → verification_outcome records semantic failure; not conflated with Layer 1
```

Layer 1 is Core-C1's primary scope. Layer 2 is still **admission-adjacent** (dispatch outcome on the same record). Layer 3 is explicitly **out of scope** for Core-C1 v1; MC-19 v1 documents `MC19_V1_D4_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT` (no gameplay verification).

## What Core-C1 is not

| Not this | Why |
| --- | --- |
| Generic runtime / controller | No registry, arbiter, blackboard, planner, or action lease |
| Core-B reopening | No shared enum extraction, manifest unification, or `derive_*` move |
| New persisted artifact role | Admission fields extend existing operation / trace surfaces only |
| Readiness authority upgrade | Derived readiness still does not create new upstream truth |
| Verification platform | `verification_outcome` is a later-layer slot, not defined here |
| Minecraft code move | MC-19 wiring stays in `auv-game-minecraft` until an owner names a different extraction slice |
| Multi-action orchestration | `attempt_once` only — no plans, queues, or retries |

## MC-19 positioning

MC-19 (`training_result_spatial_query_action_wiring.rs`) is the **current donor proof** for Core-C1-shaped behavior, not the generic core implementation.

| MC-19 surface | Core-C1 mapping |
| --- | --- |
| `derive_action_readiness` → eligibility | Upstream `readiness_class` donor (`click_ready` → `ready`, etc.) |
| `wire_readiness_to_action` | Admission gate: `attempt_once` vs `refuse_before_dispatch` |
| `QueryActionWiringOutcome.attempted` | Record field `attempted` |
| `refusal_reason`, `window_point` | Record fields `refusal_reason`, `action_point` |
| `click_summary` / executor error | Record field `dispatch_outcome` |
| `QueryLiveClickExecutor` | Vertical injectable dispatch — **not** a core trait |

MC-19 proves **query → derived readiness → one honest live attempt or refusal** with lineage intact. It does **not** prove generic cross-vertical admission API worth extracting now, controller / lease / authority semantics, or osu / third-vertical dispatch wiring.

Core-C1 **owns the generic admission vocabulary**; MC-19 **owns Minecraft wiring evidence** until owner approves code extraction (Core-C2+ — not opened here).

## Relationship to Core-A

| Layer | Owner | Core-C1 interaction |
| --- | --- | --- |
| Core-A query status triad | Derived query `answered/blocked/failed` | Upstream of readiness; unchanged by Core-C1 |
| Core-A action readiness view | `auv-query-readiness` helper + vertical `derive_*` | Feeds `readiness_class`; Core-C1 does not change Core-A verdicts or upgrade helper-only admissible to core runtime |
| Core-A falsifier review | No triggered falsifiers on dispatch separation | Core-C1 preserves derived-only → admission → dispatch ordering |

## Explicit non-goals

This design note intentionally does **not**:

- define `trait ActionAdmission` or generic runtime hooks
- add provider registry, SceneState, or blackboard seams
- specify action lease, retry, or multi-step planner semantics
- create persisted `ActionAdmissionArtifact` or fourth query-adjacent role
- move `wire_query_manifest_to_action` into a core crate
- reopen Core-B enum graduation or shared manifest extraction
- wire osu live-click or MC-20 slices
- claim trainer quality, splat usefulness, or gameplay success from admission records alone

## Suggested follow-on slices (not approved here)

| Slice | Scope | Owner gate |
| --- | --- | --- |
| Core-C2 | Helper or inspect vocabulary alignment using Core-C1 labels | Owner names extraction boundary |
| Core-C3 | Optional cross-vertical admission record formatter | Only after C2 and concrete repetition pain |
| MC-20+ | Any new vertical dispatch wiring | Separate vertical slice |

Do not start Core-C2/C3 or Core-B from this document alone.

## Related references

- Core-A stage pattern: [`2026-06-27-auv-core-spatial-result-consumption-pattern.md`](2026-06-27-auv-core-spatial-result-consumption-pattern.md)
- Core-A graduation review: [`2026-06-27-auv-core-a-query-readiness-graduation-review.md`](2026-06-27-auv-core-a-query-readiness-graduation-review.md)
- Core-A falsifier review: [`2026-06-27-auv-core-a-query-readiness-falsifier-review.md`](2026-06-27-auv-core-a-query-readiness-falsifier-review.md)
- MC-19 wiring design: [`2026-06-27-minecraft-mc19-query-to-live-click-wiring-design.md`](2026-06-27-minecraft-mc19-query-to-live-click-wiring-design.md)
- Query readiness helper (`DerivedActionEligibility` donor labels): `crates/auv-query-readiness/src/lib.rs`
- Core-C1 falsifier review: [`2026-06-28-auv-core-c1-action-attempt-admission-review.md`](2026-06-28-auv-core-c1-action-attempt-admission-review.md)

## One-sentence summary

Core-C1 freezes **attempt_once vs refuse_before_dispatch** admission vocabulary and a minimal honest record shape above derived readiness — MC-19 proves the wiring chain; Core-C1 does not become controller, runtime, or persisted artifact platform.
