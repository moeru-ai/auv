# 2026-06-30 AUV Core-D1: Action lease / ownership boundary review

Date: 2026-06-30

Status: **docs-only boundary review (D1)** — substrate research and donor inventory.
**No implementation** in this slice. **Core-D paused** after D1 unless owner reopens
with a named producer or read-side slice.

## One-line summary

Core-D1 separates **deferred action-lease orchestration** from **existing
near-homonyms** (`InputPreparationLease`, promotion consent, `attempt_once`
admission), inventories who **owns** which action-adjacent decision today, and
concludes there is **no durable action-lease producer** worth a Core-D2 read-side
projection without owner-named donor work.

## Scope boundary

**In scope:**

- Concept separation: action lease vs input-preparation lease vs consent vs admission
- Ownership / authority boundary inventory (code paths + reference docs)
- Relationship to Core-C1 admission, Core-C3 verification, Core-A7 pause
- Provisional vocabulary candidates (documentation only)
- D2 scope recommendation and reopen triggers

**Out of scope (explicit non-goals — see also §Non-goals):**

- Runtime extraction, persisted schema, generic traits, controller / planner / MC-20
- Implementing lease acquire / renew / release semantics
- Read-side projection fields (D2) without a named donor
- Reopening Core-B enum graduation or shared crate extraction

## Primary inputs

- [`2026-06-27-auv-core-spatial-result-consumption-pattern.md`](2026-06-30-query-readiness-closeout.md) — defer list for generic action lease
- [`2026-06-28-auv-core-c1-action-attempt-admission-design.md`](2026-06-28-core-action-attempt-admission-design.md) — `attempt_once`, anti-lease admission
- [`2026-06-30-auv-core-c3-post-action-verification-outcome-boundary.md`](2026-06-30-core-post-action-verification-outcome-boundary.md) — Layer 1–3 separation (paused after D2)
- [`2026-06-30-auv-core-a7-extraction-boundary-owner-pause-checkpoint.md`](2026-06-30-query-readiness-closeout.md) — MC-20 / controller pause
- [`docs/TERMS_AND_CONCEPTS.md`](../../../TERMS_AND_CONCEPTS.md) — Prepare For Input Options, Action Resolver, Spatial Result Consumption Pattern
- Code: `crates/auv-driver/src/input.rs`, `crates/auv-driver-macos/src/session.rs`,
  `src/candidate_promotion.rs`, `src/candidate_action_decision.rs`,
  `src/candidate_action_command.rs`, `src/minecraft_query_live_action.rs`,
  `src/osu_query_live_action.rs`,
  `crates/auv-game-minecraft/src/training_result_spatial_query_action_wiring.rs`

---

## 1. Four concepts that must not be conflated

Repo evidence uses “lease”, “consent”, “authority”, and “ownership” in different
layers. Core-D1 treats them as **four distinct boundaries**.

```text
(A) Action lease          — deferred orchestration concept (exclusive / time-bounded
                            action authority, renew, multi-step plans)
(B) Input preparation     — driver-scoped temporary foreground / focus prep with
    lease                   restore token (InputPreparationLease)
(C) Execution consent     — archived AX path: promotion + L8b consent record
                            authorizing one bounded candidate execution
(D) Attempt admission     — Core-C1: attempt_once or refuse_before_dispatch;
                            explicitly NOT multi-step lease semantics
```

### (A) Action lease — deferred, not implemented

**What the design corpus means:** a cross-run or cross-step **exclusive right**
to perform actions against a target (window, scene, controller slot) until
released, renewed, or superseded — typically paired with planner / controller /
registry / arbiter surfaces.

**Repo status:** repeatedly listed as **non-goal** or **defer**:

| Source | Statement |
| --- | --- |
| Spatial consumption pattern defer list | “generic action lease / dispatch protocol — still deferred” |
| Core-C1 design | No registry, arbiter, blackboard, planner, or **action lease** |
| Core-C1 non-goals | Does not specify action lease, retry, or multi-step planner semantics |
| MC-19 design | No action lease, blackboard, arbiter |
| Core-C2 / Core-C3 handoffs | Action lease grouped with MC-20 / controller pause |

**Honest inventory:** there is **no** `ActionLease`, `lease_id`, acquire/release
artifact role, or runtime arbiter in `src/` or core crates. Searching the
codebase finds **zero** producers of action-lease semantics.

**Trigger to reopen (A):** owner opens MC-20 / controller slice **or** names a
concrete donor that persists lease state — not Core-D D1 alone.

### (B) Input preparation lease — implemented, different meaning

**Owner:** `auv-driver` / `auv-driver-macos` — input delivery boundary, not
action orchestration.

| Surface | Location | Semantics |
| --- | --- | --- |
| `InputPreparationLease` | `crates/auv-driver/src/input.rs` | Minimal restore token (`restored` flag); `noop()` when no temp state |
| `prepare_for_input` / `restore_input` | `crates/auv-driver-macos/src/session.rs` | Window/session prep before click/type; RAII-style restore |
| Term definition | `docs/TERMS_AND_CONCEPTS.md` §Prepare For Input Options | “input preparation lease that can be passed back to restore the previous state” |

**Discipline:** (B) answers “did we temporarily change foreground/focus for
input delivery, and did we restore?” It does **not** grant exclusive multi-step
action authority, does not appear on MC-19/osu wired live-action summaries, and
is **not** persisted as a run artifact role.

**Naming risk:** the word “lease” here is **driver-local**. Core-D1 recommends
future docs refer to **input preparation lease** in full when near action-lease
discussion, to avoid accidental conflation with (A).

### (C) Execution consent — archived vertical seam, not core action lease

**Owner:** `candidate_promotion` (L7) + `candidate_action_decision` (L8b) —
recognition → candidate → **consent-gated** execution on the archived AX copilot
path.

| Surface | Location | Semantics |
| --- | --- | --- |
| `ActionConsentRecord`, `ConsentGrade`, `ConsentProvenance` | `src/candidate_promotion.rs` | Promotion-time permission record; dev self-minted vs future human gesture deferred |
| `CandidateActionExecutionConsent` | `src/candidate_action_decision.rs` | Execution-time consent must match promotion; `MissingExecutionConsent` / mismatch errors |
| `prepare_for_input` in command | `src/candidate_action_command.rs` | Uses (B) during execute; unrelated to (A) |

**Discipline:** (C) answers “is this **one** promoted candidate execution
explicitly authorized with matching consent metadata?” It is **refusal-first**
and **single-shot** at the candidate-action layer — closer to admission + policy
than to time-bounded exclusive lease. It does **not** implement renew/release,
multi-action queues, or cross-run exclusivity.

**Relationship to Core-D:** consent is **ownership of the promotion→execute
gate**, not ownership of a target application for arbitrary follow-on actions.
Document as **adjacent evidence**, not as an action-lease donor.

### (D) Attempt admission — Core-C1 anti-lease

**Owner:** vertical wiring + read-side projection (Core-C1 / Core-C2); MC-19 and
osu as donors.

| Surface | Semantics |
| --- | --- |
| `attempt_once` vs `refuse_before_dispatch` | One honest attempt or pre-dispatch refusal |
| `attempted`, `refusal_reason`, `dispatch_outcome` | Core-C2 read-side vocabulary on wired summaries |
| MC-19 `QueryLiveClickExecutor` | Injectable one-shot executor — **not** a lease holder |

**Discipline:** (D) explicitly rejects multi-action orchestration in Core-C1
non-goals. **`attempt_once` is the intentional opposite of action lease** for
the spatial query → live click proof chain.

---

## 2. Ownership boundary map (who owns what today)

“Ownership” in active core work means **which layer owns a decision or
artifact**, not serde enum crate placement (that is Core-A5b / Core-A7
extraction ownership — a different axis; see §7).

```text
recognition / query / witness producers     → vertical or operation crate semantics
semantic gate / spatial query               → vertical typed consumers + artifacts
derived readiness / action_eligibility      → Core-A helper + vertical derive_*
admission (attempt vs refuse)               → vertical wiring (MC-19 / osu donors)
dispatch / InputActionResult                → auv-driver (+ platform backend)
method selection                            → ActionResolver (archived AX path)
verification claims                         → VerificationResult contract (Layer 3)
run recording / artifact roles              → runtime + storage
read-side inspect / wired summaries         → run_read + inspect (+ viewer HTML)
promotion / execution consent               → candidate_promotion seam (archived path)
input preparation restore                   → driver session API
```

### Authority vs readiness (do not upgrade)

| Question | Owner | Core-D1 rule |
| --- | --- | --- |
| Is the spatial result consumable for action? | Derived readiness / eligibility | **Not** dispatch authority |
| Should we dispatch once? | Admission gate (C1) | **Not** lease renewal |
| Did input delivery succeed? | Driver / Layer 2 | **Not** semantic success |
| Did the world/UI match expectation after action? | Verification (C3) | **Not** admission |
| Who may execute this promoted candidate? | Consent gate (C) | **Not** exclusive scene lease |

Repo `known_limits` on MC-19 / osu wired paths already disclaim missing
gameplay verification and missing controller semantics — consistent with no
action-lease producer.

### Missing central owner (intentional defer)

These **do not exist** as core surfaces today:

- Action lease registry / arbiter / blackboard
- Scene-wide exclusive action holder
- Multi-step planner with lease renewal
- Persisted `action_lease` artifact role
- Generic `trait ActionLease` or controller runtime

Core-D1 treats their absence as **documented defer**, not an implementation gap
to close in D2.

---

## 3. Donor inventory

| Donor | Path / surface | Lease-like? | Ownership note |
| --- | --- | --- | --- |
| Input prep lease | `crates/auv-driver/src/input.rs` | **(B) only** | Driver restore token |
| macOS prepare/restore | `crates/auv-driver-macos/src/session.rs` | **(B) only** | Session-scoped |
| candidate-action execute | `src/candidate_action_command.rs` | Uses (B); consent via (C) | Archived AX vertical |
| Promotion consent | `src/candidate_promotion.rs` | **(C)** | L7 gate, not lease |
| Execution consent | `src/candidate_action_decision.rs` | **(C)** | L8b match/mismatch |
| MC-19 wiring | `training_result_spatial_query_action_wiring.rs` | **(D)** `attempt_once` | Minecraft donor |
| MC-19 runtime glue | `src/minecraft_query_live_action.rs` | **(D)** | No lease fields |
| osu wiring | `visual_truth_spatial_query_action_wiring.rs` | **(D)** | Second vertical donor |
| osu runtime glue | `src/osu_query_live_action.rs` | **(D)** | No lease fields |
| Core-C2 summaries | `src/run_read.rs` wired live action | **(D)** read-side | prep/admission/dispatch labels |
| Core-C3 summaries | same | verification projection | Layer 3 read-side; paused |

**Conclusion:** no row produces **(A) action lease** evidence. Donors justify
boundary documentation only.

---

## 4. Relationship to adjacent core lanes

### Core-C1 / Core-C2 / Core-C3 (paused)

| Lane | Interaction with Core-D |
| --- | --- |
| Core-C1 admission | Defines single-attempt boundary; action lease explicitly out of scope |
| Core-C2 read-side | Vocabulary for prep/admission/dispatch — **not** lease fields |
| Core-C3 (paused) | Verification Layer 3; non-goals include action lease + MC-20 |

Core-D sits **beside** C3: it documents orchestration authority defer while C3
documented post-action semantic outcome defer. Neither slice authorizes
controller runtime.

### Core-A7 / MC-20 pause

[`2026-06-30-auv-core-a7-extraction-boundary-owner-pause-checkpoint.md`](2026-06-30-query-readiness-closeout.md)
places **MC-20 / controller** outside the Core-A consumption-proof lane.
Action lease semantics, if ever needed, most likely attach to **that**
orchestration lane — not to Core-C2/C3 read-side polish.

**Rule:** Core-D1 does **not** reopen MC-20, planner, registry, or arbiter.

### Archived AX copilot path

The promotion → consent → `ActionResolver` → `InputActionResult` seam remains
valuable as **consent ownership** evidence (C), but the vertical is frozen.
Core-D must not expand candidate-action action classes or product polish under
the guise of lease design.

---

## 5. Provisional vocabulary (documentation only — not implemented)

If a future owner-named slice needs shared labels, prefer **qualified** terms:

| Term | Meaning | Status |
| --- | --- | --- |
| `action_lease` | Exclusive, time-bounded orchestration right over a target | **Deferred (A)** — no producer |
| `input_preparation_lease` | Driver restore token after prepare-for-input | **Exists (B)** — keep driver-local |
| `execution_consent` | One-shot authorized candidate execution | **Exists (C)** — archived path |
| `attempt_admission` | `attempt_once` / refuse before dispatch | **Exists (D)** — Core-C1/C2 |
| `lease_holder` / `lease_epoch` | Arbiter/controller identity | **Not in repo** — do not invent in read-side |
| `ownership_boundary` | Which layer owns a decision | **This review** — meta vocabulary |

**Read-side discipline (if D2 ever opens):** project only fields backed by
persisted or trace-stable producer evidence. Do **not** add
`action_lease_status=none` placeholders across MC-19/osu summaries solely for
symmetry with Core-C2/C3 — that would imply a producer that does not exist.

---

## 6. D2 recommendation

| Option | Verdict |
| --- | --- |
| Core-D2 read-side `action_lease_*` projection on wired summaries | **Not recommended** — no producer; would be synthetic |
| Core-D2 read-side `input_preparation_lease` on wired paths | **Not recommended** — driver-ephemeral; not recorded on MC-19/osu chain |
| Core-D2 consent summary on wired paths | **Out of lane** — consent belongs to candidate-action artifacts, not spatial query wiring |
| Core-D2 documentation cross-links only | **Sufficient** — this D1 note + TERMS disambiguation |

**Recommended posture:** **pause Core-D after D1**. Reopen only when:

1. Owner names **MC-20 / controller** slice with concrete lease acquire/release
   semantics, **or**
2. A vertical persists lease evidence in a named artifact role with tests, **or**
3. Owner explicitly requests read-side projection of an **existing** producer
   (must cite file path — not vocabulary-only).

---

## 7. Extraction “ownership” (Core-A7 axis — separate question)

Core-A7 uses “ownership” for **where types live** (helper vs core crate vs
vertical). That is unrelated to action lease except by name collision.

| Question | Lane |
| --- | --- |
| Should `derive_*` move to shared crate? | Core-A5b / Core-B — owner pause |
| Who owns Minecraft wiring vs core admission vocabulary? | Core-C1 — vertical donor vs generic labels |
| Who owns driver prep lease types? | `auv-driver` — already extracted |

Core-D1 does **not** graduate helpers or open Core-B based on lease vocabulary.

---

## 8. Explicit non-goals

This review intentionally does **not**:

- define `trait ActionLease`, lease registry, arbiter, or blackboard
- add persisted lease artifact roles or run-record fields
- implement acquire / renew / release / TTL semantics
- wire MC-20, planner, or multi-step orchestration
- add read-side lease placeholders to MC-19 / osu wired summaries
- conflate `InputPreparationLease` with deferred action lease in TERMS without qualifier
- reopen candidate-action vertical expansion
- claim exclusive action authority from derived readiness or successful dispatch

---

## 9. Suggested follow-on slices (not approved here)

| Slice | Scope | Owner gate |
| --- | --- | --- |
| MC-20 / controller D* | Orchestration + possible lease producer | Owner opens vertical lane explicitly |
| TERMS tweak | Disambiguate “input preparation lease” vs “action lease” in glossary | Owner names docs-only polish |
| Core-D2 read-side | Only if a real lease producer exists | Owner cites producer path |
| Core-B / shared extraction | Unrelated unless lease types need shared home | Core-A7 reopen rules |

Do not start MC-20, Core-D2, or Core-B from this document alone.

---

## 10. Closure checklist

| Item | Status |
| --- | --- |
| Action lease (A) separated from prep lease (B), consent (C), admission (D) | **Done** |
| Donor inventory with honest “no producer” for (A) | **Done** |
| Ownership map for active core chain | **Done** |
| D2 recommendation = pause | **Done** |
| Relationship to Core-C1/C3 and Core-A7 pause | **Done** |
| Implementation / runtime / read-side code | **Explicitly out of scope** |

**Core-D status after D1:** **paused** — boundary documented; await owner-named
orchestration or producer slice before D2.
