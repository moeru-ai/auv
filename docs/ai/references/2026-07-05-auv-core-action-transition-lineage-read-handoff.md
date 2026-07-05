# AUV Core Action Transition Lineage — Slice 3 Read Handoff

**Date:** 2026-07-05  
**Status:** implemented — read-side `ActionTransitionLineage` projection  
**Prerequisites:**
- [Slice 1 audit](2026-07-05-auv-core-action-seam-audit-handoff.md)
- [Slice 2 L8b reconnect](2026-07-05-auv-core-action-seam-l8b-reconnect-handoff.md)

## Hard sentence

Read-side uses L8b embedded decision as primary fact; L8a decision is **planned comparator** only — different semantics, must not be mixed.

## `ActionTransitionLineage` fields

| Field | Source |
|-------|--------|
| `pre_state` | `source_candidate_promotion_artifact` + promotion ids |
| `effective_decision` | L8b `action_resolver_decision` (reconciled effective) |
| `planned_decision` | optional — join L8a via `source_candidate_action_decision_artifact` |
| `driver_result` | L8b `input_action_result` |
| `post_state` | `operation_result_artifact` + `detail.operation_status` |
| `verification` | `project_verification_outcome_from_claims` on embedded `operation_result` |
| `known_limits` / `status` / `issue` | L8b limits + defensive divergence checks |

## Join path

Primary: `CandidateActionExecutionArtifact.source_candidate_action_decision_artifact` → `resolve_artifact_ref` → load L8a JSON.

`g3-binding-fact` remains denormalized fallback only (not implemented as primary join in this slice).

## Partial / issue policy

| Condition | Result |
|-----------|--------|
| Missing effective decision | `partial` + `missing_action_resolver_decision` |
| `plan_delivery_mismatch` in L8b limits | `partial` + limits visible |
| planned ≠ effective without recorded mismatch | `partial` + `plan_effective_method_divergence` |
| Malformed execution JSON | `malformed` |
| No verification claims | `verification_outcome=absent` (C3 D2) |

## Code touchpoints

| Symbol | Location |
|--------|----------|
| `extract_action_transition_lineage` | `src/run_read.rs` |
| `list_action_transition_lineage` | `src/run_read.rs` (public) |
| `[action.transition.lineage]` section | `src/inspect.rs` `render_run_text` |
| `InspectRunResponse.action_transition_lineage` | `src/inspect_server/mod.rs` |

## Non-goals

- No producer/schema changes
- No new runtime execution types
- No viewer overhaul (HTTP JSON + text inspect first)

## Validation

```sh
cargo test -p auv-cli action_transition_lineage
cargo test -p auv-cli plan_delivery_mismatch
cargo test -p auv-cli inspect::
cargo test -p auv-cli inspect_server::
git diff --check
```
