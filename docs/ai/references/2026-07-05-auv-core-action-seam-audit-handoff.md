# AUV Core Action Seam Audit — Slice 1 Handoff

**Date:** 2026-07-05  
**Status:** read-only audit complete — locks Slice 2 to candidate-action L8b only  
**Related:** [`src/contract.rs`](../../src/contract.rs) seam map; [C3 D2 verification read projection](2026-06-30-auv-core-c3-d2-verification-outcome-read-side-projection.md); [C3 verification boundary](2026-06-30-auv-core-c3-post-action-verification-outcome-boundary.md)

## Scope

Read-only inventory of where `ActionResolverDecision` is constructed, serialized, and read. **No code changes** in this slice.

**Hard sentences (carried forward):**

- **Slice 2:** L8b writes a **reconciled effective decision**, not an L8a plan clone, and not proof of an independent runtime resolver subsystem.
- **Slice 3:** Read-side uses L8b embedded decision as primary fact; L8a is **planned comparator** only — do not conflate semantics.

## ActionResolverDecision semantic roles

| Location | Role | Meaning |
|----------|------|---------|
| L8a `candidate-action-decision` | **pre-execution plan** | Synthetic `decide_candidate_action` mapping; decide-only; no `InputActionResult` |
| L8b `candidate-action-execution` (before Slice 2) | **incorrect clone** | `decision.action_resolver_decision.clone()` at `candidate_action_decision.rs:637` |
| L8b (after Slice 2) | **post-execution reconciled effective** | `delivery plan + InputActionResult` aligned; plan fields inherited from L8a |

`InputActionResult` (`crates/auv-driver/src/input.rs` ~`:317`) has `selected_path`, `attempts`, `fallback_reason`, disturbance — **insufficient alone** to rebuild `policy`, `primary_method`, `target_query`.

## 1. Paths that carry `ActionResolverDecision`

| Path | Artifact role | Construction | Semantic role |
|------|---------------|--------------|---------------|
| L8a decide-only | `candidate-action-decision` | `decide_candidate_action` → `build_candidate_action_decision_artifact` | pre-execution plan |
| L8b execution | `candidate-action-execution` | embedded field (today: L8a clone) | should be reconciled effective (Slice 2) |
| L8b g3-binding-fact | `g3-binding-fact` | JSON at record time `:683-696` | **no** decision today — `input_action_result` only |
| Inspect fixtures | n/a | `inspect.rs` / `inspect_server` test JSON | flattened lineage fields |

**Runtime orchestration:** `runtime.rs` → `run_candidate_action_command` → L8a record + L8b execute (`candidate_action_command.rs`). **`runtime.rs` checked — no seam-carrying role** (transparent delegate).

## 2. Paths that do NOT carry `ActionResolverDecision`

| Path | Carries | Notes |
|------|---------|-------|
| `auv-cli-invoke` `input.*` | `InputActionResult` only (`input-action-result`) | `input.smartPress` stub (`input.rs:101-105`) |
| `session.rs` `act_with_result` | `InputActionResult` in memory events | no artifact pair (`:311`) |
| `OperationResult` | verifications only | no decision field (`contract.rs`) |
| `candidate_promotion` | promotion context | above decision layer |
| `ActionResolverDecision::signals()` | n/a | test-only usage today |

## 3. Gap boundaries

| Boundary | Gap |
|----------|-----|
| L8a → L8b handoff | Clone at `:637` — no reconcile; plan/delivery mismatch silent |
| L8a plan vs executor | `decide_candidate_action` AxNode → `ax-action` (`:1268`); `MacosCandidateActionExecutor` click requires coordinate + pointer-click (`:1103-1108`) |
| Driver-only paths | invoke/session never construct decision |
| Serialization | `g3-binding-fact` omits `action_resolver_decision` |
| Read-side | `run_read` execution lineage flattens decision; **does not project** full `input_action_result`; no `ActionTransitionLineage` join |
| Missing module | `contract.rs` cites `src/driver/macos/control/action_resolver.rs` — **not present**; not in Slice 2 scope |

## Slice 3 join keys (confirmed)

| Join | Field | Location |
|------|-------|----------|
| L8b → L8a | `source_candidate_action_decision_artifact: ArtifactRef` | `CandidateActionExecutionArtifact` `:256` |
| L8b → promotion | `source_candidate_promotion_artifact: Option<ArtifactRef>` | `:257` |
| L8b → operation | `operation_result_artifact: Option<ArtifactRef>` | `:258` |
| Correlation ID | `source_decision_id: String` | `:260` (not primary join key) |
| Read resolver | `resolve_artifact_ref(run, &execution.source_candidate_action_decision_artifact)` | `run_read.rs` `:5775-5778` |

**Primary join:** execution artifact → `source_candidate_action_decision_artifact`. `g3-binding-fact` is denormalized fallback only.

## Slice 2 attachment (locked)

**Only:** `MacosCandidateActionExecutor::execute` + `build_candidate_action_execution_artifact` + `reconcile_effective_decision` in `candidate_action_decision.rs`.

**Deferred:** invoke `input.*`, `session.act_with_result`, `OperationResult` new fields, full ActionResolver module.

## Follow-up table

| Item | Trigger |
|------|---------|
| `input.smartPress` invoke | Slice 2 stable + owner slice |
| App Command Pack | separate slice |
| Streaming observation | separate lane |
