# AUV Core Action Seam L8b Reconnect — Slice 2 Handoff

**Date:** 2026-07-05  
**Status:** implemented — L8b writes post-execution **reconciled effective decision**  
**Prerequisite:** [Slice 1 audit](2026-07-05-auv-core-action-seam-audit-handoff.md)

## Hard sentences

1. L8b writes a **reconciled effective decision**, not an L8a pre-execution plan clone, and **not** proof of an independent runtime resolver subsystem.
2. L8b `action_resolver_decision` is post-execution reconciled effective decision; inspect must not treat L8a and L8b decision fields as the same semantic.

## Semantic table

| Location | Role | Reconcile inputs |
|----------|------|------------------|
| L8a | pre-execution plan | `decide_candidate_action` |
| L8b | reconciled effective | L8a plan + `CandidateActionDeliveryPlan` + `InputActionResult` |

Plan-side fields (`policy`, `primary_method`, `target_query`, `operation`) **inherit L8a**. Delivery-side fields (`selected_method`, `fallback_*`, disturbance metadata) align from driver result — **not** rebuilt from `InputActionResult` alone.

## `plan_delivery_mismatch` (hard acceptance)

When L8a `selected_method` ≠ reconciled effective `selected_method`:

- `known_limits` **must** include `plan_delivery_mismatch: l8a_selected=... effective=...`
- `detail.plan_method` / `detail.effective_method` set on execution artifact

Primary regression: L8a `ax-action` plan + `WindowTargetedMouse` delivery → mismatch recorded.

## Code touchpoints

| Symbol | Role |
|--------|------|
| `reconcile_effective_decision` | `candidate_action_decision.rs` |
| `build_candidate_action_execution_artifact` | uses reconcile instead of L8a clone (`:637` removed) |

## Non-goals

- No invoke/session wiring
- No `OperationResult` new fields
- No multi-path AX runtime resolver
- No Surface SLAM / streaming observation cross-links

## Validation

```sh
cargo fmt --check
cargo check
cargo test -p auv-cli plan_delivery_mismatch
cargo test -p auv-cli candidate_action_decision::
git diff --check
```
