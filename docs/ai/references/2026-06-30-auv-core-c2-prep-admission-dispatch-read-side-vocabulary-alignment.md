# 2026-06-30 AUV Core-C2-prep admission / dispatch read-side vocabulary alignment

Date: 2026-06-30

Status: **docs-only prep note**. Records the current mismatch surface between
Core-C1 conceptual admission vocabulary and donor read-side / inspect fields.
Defines the smallest honest alignment target for a future owner-named Core-C2
slice. **No code changes, no proof-matrix verdict changes, and no runtime or
controller extraction are approved by this note.**

## One-line summary

Core-C2-prep names the **read-side vocabulary gap** between Core-C1 admission
concepts (`readiness_class`, `attempted`, `refuse_before_dispatch`,
`dispatch_outcome`, deferred `verification_outcome`) and the actual donor fields
currently exposed by MC-19 and osu wired live action — enough to scope a future
inspect/helper alignment slice, not enough to justify runtime or crate
extraction.

## Scope boundary

**In scope:**

- Read-side / inspect vocabulary alignment only
- Donor-to-core field mapping inventory
- Honest gap list between conceptual Core-C1 record shape and current summaries
- Candidate follow-on slice boundary for a future **docs-first or inspect-only**
  Core-C2

**Out of scope:**

- Runtime extraction
- Shared admission crate or trait
- Core-B reopening
- New persisted artifact role or `OperationResult` schema change
- MC-20, planner, registry, arbiter, blackboard, or action lease work
- Rewriting donor runtime wiring
- Re-litigating Core-C1 falsifier outcomes

This note is about **reader vocabulary**, not execution authority.

## Why this note exists now

Core-C1 already closed the **conceptual admission boundary**:

- `readiness_class`
- `attempt_once`
- `refuse_before_dispatch`
- `dispatch_outcome`
- deferred `verification_outcome`

But the actual donor read-side and inspect surfaces still expose a mixed set of
fields:

- donor readiness labels (`click_ready`, `answer_non_clickable`, `not_consumable`)
- donor-specific geometry (`window_point`, `pixel_point`)
- overloaded refusal text
- operation-result summaries that only partially match Core-C1 names

That mismatch is real, but it is still **read-side debt**, not extraction
pressure.

## Primary inputs

- [`2026-06-28-auv-core-c1-action-attempt-admission-design.md`](2026-06-28-auv-core-c1-action-attempt-admission-design.md)
- [`2026-06-28-auv-core-c1-action-attempt-admission-review.md`](2026-06-28-auv-core-c1-action-attempt-admission-review.md)
- [`2026-06-27-minecraft-mc19-query-to-live-click-wiring-design.md`](2026-06-27-minecraft-mc19-query-to-live-click-wiring-design.md)
- `/Users/liuziheng/https-github-com-moeru-ai-auv/src/run_read.rs`
- `/Users/liuziheng/https-github-com-moeru-ai-auv/src/inspect.rs`
- `/Users/liuziheng/https-github-com-moeru-ai-auv/src/contract.rs`

## Core-C1 conceptual record (reference target)

Core-C1's target vocabulary is a **conceptual record**, not a persisted schema:

| Core-C1 field | Meaning |
| --- | --- |
| `attempted` | dispatch invoked or refused before dispatch |
| `readiness_class` | core-neutral label: `ready`, `non_actionable`, `not_consumable` |
| `source_readiness_ref` | pointer to derived readiness provenance |
| `action_point` | generic action coordinate slot (`window_point`, `pixel_point`, etc.) |
| `refusal_reason` | refusal or defensive refusal explanation |
| `dispatch_outcome` | driver/invoke outcome after `attempted=true` |
| `verification_outcome` | later-layer slot, intentionally deferred |

Core-C2-prep uses this record as the **alignment target**, not as an
implementation promise.

## Current donor read-side inventory

### Minecraft donor (MC-14 + MC-19)

Current read-side fields exposed in `/Users/liuziheng/https-github-com-moeru-ai-auv/src/run_read.rs` and `/Users/liuziheng/https-github-com-moeru-ai-auv/src/inspect.rs`:

| Surface | Current fields |
| --- | --- |
| MC-14 derived readiness summary | `action_eligibility`, `window_point`, `refusal_reason`, `issue` |
| MC-19 wired live action summary | `attempted`, `action_eligibility`, `window_point`, `refusal_reason`, `operation_status`, `operation_message`, `dispatch_command`, `dispatch_outcome`, `mc14_action_eligibility`, `issue` |
| Inspect rendering | `attempted`, `action_eligibility`, `window_point`, `refusal_reason`, `dispatch_outcome`, `mc14_action_eligibility` |

Important asymmetry:

- `action_eligibility` stays donor-local (`click_ready`, etc.)
- Core-C1 label is **not** rendered directly
- `mc14_action_eligibility` duplicates upstream donor label rather than mapping
  to core-neutral vocabulary

### osu donor (wired live action)

Current read-side fields exposed in `/Users/liuziheng/https-github-com-moeru-ai-auv/src/run_read.rs` and `/Users/liuziheng/https-github-com-moeru-ai-auv/src/inspect.rs`:

| Surface | Current fields |
| --- | --- |
| Derived readiness summary | `action_eligibility`, `pixel_point`, `refusal_reason`, `issue` |
| Wired live action summary | `attempted`, `action_eligibility`, `pixel_point`, `window_point`, `refusal_reason`, `operation_status`, `operation_message`, `dispatch_command`, `dispatch_outcome`, `readiness_class`, `issue` |
| Inspect rendering | `attempted`, `action_eligibility`, `pixel_point`, `window_point`, `refusal_reason`, `dispatch_outcome`, `readiness_class` |

Important asymmetry:

- osu already exposes a `readiness_class` field in read-side summary
- but that field currently carries donor strings (`click_ready`,
  `answer_non_clickable`, `not_consumable`) rather than Core-C1 neutral labels
- `action_eligibility` and `readiness_class` therefore coexist without true
  semantic separation

### Archived / adjacent seam note

`OperationResult` in `/Users/liuziheng/https-github-com-moeru-ai-auv/src/contract.rs`
already carries:

- `status`
- `operation_id`
- `output`
- `verifications`
- `known_limits`

But it does **not** define first-class admission vocabulary fields. The current
admission-aligned read-side fields are derived from:

- outcome events
- operation-result linkage
- donor readiness derivation

That is acceptable for C1/C2-prep. It is not evidence that `OperationResult`
should be expanded now.

## Alignment matrix: donor fields vs Core-C1 vocabulary

| Core-C1 concept | Minecraft current field | osu current field | Alignment state |
| --- | --- | --- | --- |
| `attempted` | `attempted` | `attempted` | **aligned** |
| `readiness_class` | missing as neutral field; donor-only `action_eligibility` and `mc14_action_eligibility` | present but populated with donor strings | **misaligned** |
| `source_readiness_ref` | implicit via `query_artifact_id` + derived MC-14 readiness | implicit via `query_artifact_id` + derived readiness | **implicit only** |
| `action_point` | `window_point` | `pixel_point` and optional `window_point` | **aligned by shape, donor-specific by name** |
| `refusal_reason` | `refusal_reason` | `refusal_reason` | **aligned, but overloaded by donor text** |
| `dispatch_outcome` | `dispatch_outcome` | `dispatch_outcome` | **aligned enough for read-side** |
| `verification_outcome` | absent | absent | **intentionally deferred** |

## Honest gap list

These are the real gaps. They should be documented as reader alignment debt, not
inflated into runtime extraction pressure.

### Gap 1 — neutral `readiness_class` is not actually shared

Core-C1 names:

- `ready`
- `non_actionable`
- `not_consumable`

Current donor read-side surfaces still center donor labels:

- `click_ready`
- `answer_non_clickable`
- `not_consumable`

osu's `readiness_class` field is especially misleading right now because the
name sounds core-neutral while the values remain donor-local.

**C2-prep stance:** document this as a **read-side naming / mapping gap**, not a
runtime or trait gap.

### Gap 2 — `source_readiness_ref` is conceptual, not explicit

Core-C1 wants an explicit readiness provenance pointer. Current summaries
reconstruct that provenance indirectly through:

- query artifact ids
- derived readiness summaries
- donor-local query lineage

That is honest enough for now, but the concept is not yet surfaced as a named
reader field.

**C2-prep stance:** acceptable defer. Future C2 may align the inspect/read-side
presentation without changing persistence.

### Gap 3 — refusal text still carries donor-specific payloads

Examples already present in inspect:

- Minecraft: `visibility=outside_window`
- Minecraft: `status=failed reason=target_block_absent_from_scene_packet`
- osu: `pixel_visibility=outside_capture`
- osu: `status=failed reason=target_absent_from_visual_truth`

This is honest, but not normalized.

**C2-prep stance:** keep the donor payload text. Do **not** normalize refusal
reasons into a fake core taxonomy unless an owner names that slice explicitly.

### Gap 4 — `dispatch_outcome` is aligned only at reader level

Core-C1 reserves `dispatch_outcome` for Layer 2. Current read-side derives it
from events and operation-result linkage; it is not a first-class persisted
field in `OperationResult`.

That is enough for inspect and review vocabulary. It is **not** enough to claim
runtime contract extraction.

**C2-prep stance:** reader alignment only; no `OperationResult` schema move.

### Gap 5 — `verification_outcome` remains intentionally absent

Neither donor currently exposes a true post-action semantic verification field in
the admission-aligned summaries.

That is a **feature, not a bug**, for this phase. Core-C1 explicitly deferred
verification layering.

**C2-prep stance:** keep deferred. Do not backfill gameplay verification into a
read-side vocabulary cleanup slice.

## Minimum acceptable future Core-C2 slice

If the owner later names a `Core-C2` slice, this prep note recommends the
smallest honest scope:

| Allowed C2 scope | Why acceptable |
| --- | --- |
| Inspect / run_read vocabulary alignment only | Localized read-side debt |
| Explicit mapping table from donor readiness labels to Core-C1 neutral labels | Clarifies semantics without changing runtime |
| Optional new summary fields for neutral labels while preserving donor fields | Can improve inspect honesty without schema inflation |
| Better naming for current read-side summaries | Reader-only improvement |

## Explicit non-goals for future C2

Even if C2 opens later, this prep note does **not** support:

- shared admission runtime crate
- generic dispatch trait
- `OperationResult` schema expansion for admission vocabulary
- Core-B reopening
- replacing donor-local refusal text with forced normalized enums
- gameplay verification platform work
- MC-20 or controller/planner slices

## Recommended vocabulary posture

Until an owner names a real Core-C2:

- keep donor labels **visible**
- keep Core-C1 neutral terms **documented**
- do not pretend current reader fields are already aligned
- do not use vocabulary cleanup as a pretext for extraction

The honest current state is:

> the admission model is conceptually defined, donor evidence exists, and the
> remaining debt is primarily **reader vocabulary alignment**, not runtime
> generalization.

## Related references

- Core-C1 design:
  [`2026-06-28-auv-core-c1-action-attempt-admission-design.md`](2026-06-28-auv-core-c1-action-attempt-admission-design.md)
- Core-C1 review:
  [`2026-06-28-auv-core-c1-action-attempt-admission-review.md`](2026-06-28-auv-core-c1-action-attempt-admission-review.md)
- MC-19 donor wiring:
  [`2026-06-27-minecraft-mc19-query-to-live-click-wiring-design.md`](2026-06-27-minecraft-mc19-query-to-live-click-wiring-design.md)
- Core-A7 pause checkpoint:
  [`2026-06-30-auv-core-a7-extraction-boundary-owner-pause-checkpoint.md`](2026-06-30-auv-core-a7-extraction-boundary-owner-pause-checkpoint.md)
- Read-side implementation surfaces:
  `/Users/liuziheng/https-github-com-moeru-ai-auv/src/run_read.rs`,
  `/Users/liuziheng/https-github-com-moeru-ai-auv/src/inspect.rs`
- Operation-result contract:
  `/Users/liuziheng/https-github-com-moeru-ai-auv/src/contract.rs`

## One-sentence summary

Core-C2-prep records the remaining admission / dispatch **reader vocabulary**
debt after Core-C1 and MC-19: donor evidence is sufficient, but neutral
readiness naming and provenance presentation are still only partially aligned —
that is a future **read-side** cleanup candidate, not runtime extraction
pressure.
