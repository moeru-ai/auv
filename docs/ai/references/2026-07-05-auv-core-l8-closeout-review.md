# AUV Core L8 Closeout Review

**Date:** 2026-07-05  
**Branch evidence:** `3745c419` docs → `ac4e4e0b` L8b → `9483f2d6` read-side  
**Status:** closeout complete — **verdict below**

**Prerequisites:** [Slice 1 audit](2026-07-05-auv-core-action-seam-audit-handoff.md), [L8b handoff](2026-07-05-auv-core-action-seam-l8b-reconnect-handoff.md), [ATL read handoff](2026-07-05-auv-core-action-transition-lineage-read-handoff.md)

---

## 1. Producer matrix

| Path | `action_resolver_decision` | `input_action_result` | Reconcile | Notes |
|------|---------------------------|----------------------|-----------|-------|
| L8a `candidate-action-decision` | pre-execution plan | absent (by design) | N/A | decide-only |
| L8b `candidate-action-execution` (canonical) | **reconciled effective** via `reconcile_effective_decision` | present | yes — `:576-580` in `build_candidate_action_execution_artifact` | `detail.plan_method` / `detail.effective_method` align with L8a plan vs effective |
| L8b blocked-not-ready | reconciled (blocked driver result) | present (`blocked_input_action_result`) | yes — same build function | `side_effect=BlockedNotReady` |
| L8b plan/delivery mismatch | effective `selected_method` ≠ L8a plan | present | yes + `plan_delivery_mismatch` in `known_limits` | 3 regression tests (`plan_delivery_mismatch` filter) |
| L8b unknown delivery path | reconciled + `effective_method_unmapped_delivery_path` limit | present | yes | `reconcile_effective_decision_records_unmapped_delivery_path_limit` |
| `g3-binding-fact` | **absent** | present | N/A | denormalized bridge at `:689-707`; **deferred** — not L8b primary artifact |
| invoke `input.*` | absent | present | N/A | deferred |
| `session.act_with_result` | absent | in-memory only | N/A | deferred |

**Primary evidence:** code path through `build_candidate_action_execution_artifact` + `cargo test -p auv-cli --lib plan_delivery_mismatch` (3 passed).

**Auxiliary sanity:** `rg 'action_resolver_decision\.clone\(\)' src/candidate_action_decision.rs` → **0 matches** (not used as verdict basis).

---

## 2. Read-model matrix

| `ActionTransitionLineage` field | Source | New facts? |
|---------------------------------|--------|------------|
| `effective_decision` | L8b embedded `action_resolver_decision` → `project_action_resolver_decision` | no — copy projection only |
| `planned_decision` | L8a join via `source_candidate_action_decision_artifact` | no — comparator only |
| `driver_result` | L8b `input_action_result` clone | no |
| `verification` | `project_verification_outcome_from_claims` on embedded `operation_result`; fallback to `verification_result` | no — C3 D2 projection |
| `pre_state` | promotion refs on L8b | no |
| `post_state` | `operation_result_artifact` + `detail.operation_status` | no |
| `known_limits` / `status` / `issue` | L8b limits + classify rules | no — passthrough + defensive `plan_effective_method_divergence` |

**Hard rule satisfied:** read-side does **not** rebuild `policy` / `primary_method` / `target_query` from `InputActionResult` alone.

---

## 3. Compatibility matrix

### Core correctness

| Scenario | Expected | Evidence |
|----------|----------|----------|
| Canonical L8b + mismatch | `partial` + `plan_delivery_mismatch` in limits | `action_transition_lineage_surfaces_plan_delivery_mismatch_from_l8b` |
| Legacy artifact (optional fields) | `partial` + `missing_action_resolver_decision` or `missing_input_action_result` | `action_transition_lineage_marks_legacy_missing_decision_as_partial` |
| Malformed JSON | `malformed` + parse issue | execution lineage + ATL tests |
| Unresolved L8a ref | `planned_decision=None`; effective from L8b | join optional by design |

### Surface correctness

| Surface | `partial` + `known_limits` visible? | Notes |
|---------|-------------------------------------|-------|
| CLI `inspect_run` `[action.transition.lineage]` | yes | `inspect.rs` text section |
| Server JSON `GET /runs/{id}` | yes | `inspect_server` contract test |
| **Viewer** `inspect_server_viewer.html` | **no at L8 audit time** | L8 verdict gap; **closed in L9** — see [L9 handoff](2026-07-05-auv-core-l9-inspect-surface-handoff.md) |

---

## 4. Parallel lineage drift verdict

| Type | Role | L9 consumption |
|------|------|----------------|
| `CandidateActionExecutionLineage` | flat execution **ledger** (consent, closure_state, readiness, detail strings) | keep — existing CLI section unchanged |
| `ActionTransitionLineage` | **seam join projection** (effective vs planned, driver_result typed, C3 verification) | **L9 viewer consumes this only** |

**Drift table (intentional differences):**

| Concern | Execution lineage | Transition lineage |
|---------|-------------------|-------------------|
| `selected_method` | L8b effective (flat) | `effective_decision` + optional `planned_decision` |
| `verification` | `detail.verification` string | `verification.verification_outcome` (C3 D2) |
| `driver_result` | `selected_path` string in detail | typed `InputActionResult` |
| Mismatch visibility | `known_limits` only | `partial` status + planned/effective pair |

**Verdict:** **Option A** — dual track retained; transition lineage is the seam read surface. Execution lineage is not a deprecate candidate for this slice.

---

## 5. Deferred by boundary

| Item | Boundary | Reopen trigger |
|------|----------|----------------|
| `g3-binding-fact` without `action_resolver_decision` | denormalized MC-5 bridge | owner L8b-g3 slice |
| invoke `input.smartPress` decision pair | invoke lane | owner + L8 stable |
| `session.act_with_result` artifacts | session lane | owner |
| `ActionResolver` module (`contract.rs` cite, file absent) | archived / future | owner |
| App Command Pack | app packaging lane | [gate doc](2026-07-05-auv-core-app-command-pack-gate.md) |
| S / Surface Memory | observation lane | [lane discipline](2026-07-05-auv-core-surface-memory-lane-discipline.md) |
| M/G 3DGS / SLAM | research lane | owner |

---

## 6. Verdict

### Selected: `close_for_core_seam_surface_gap_only`

Core producer + read-model + compatibility **core** layers pass. The only material gap is **viewer** not rendering `action_transition_lineage` (CLI + server JSON already wired).

### Forced answers

1. **L8b artifact 主路径是否真实共存 `decision + driver_result`?**  
   **Yes.** `build_candidate_action_execution_artifact` always sets both after `reconcile_effective_decision` (ready, blocked, mismatch paths).

2. **read-side 是否只读投影、未偷生产新事实?**  
   **Yes.** Effective/planned/driver/verification follow artifact join + C3 D2 projection only.

3. **old artifact 缺字段是否稳定 `partial`（core 层）?**  
   **Yes.** Legacy read path + unit tests; malformed → `malformed`.

4. **`CandidateActionExecutionLineage` 与 `ActionTransitionLineage` 是否暂时都应保留?**  
   **Yes.** Ledger vs seam projection; **L9 viewer 只消费 ATL.**

### Next gate

Proceed to **L9 Inspect Surface** (viewer only). Do not reopen L8b producer unless a new failing evidence appears.

**Superseded by:** [L9-R2 final closeout](2026-07-06-auv-core-l9-r2-core-action-seam-final-closeout-review.md) — seam upgraded to **`close`** after L9 + L9-R1.

---

## Validation commands (recorded)

```sh
cargo fmt --check
cargo test -p auv-cli --lib plan_delivery_mismatch        # 3 passed
cargo test -p auv-cli --lib action_transition_lineage   # 2 passed
cargo test -p auv-cli inspect_server::tests::run_route_includes
cargo test -p auv-cli inspect_server::tests::root_serves_inline_viewer_html
cargo test -p auv-cli inspect_server::tests::viewer_renders_action_transition_lineage_hooks
rg 'action_resolver_decision\.clone\(\)' src/candidate_action_decision.rs  # 0 (auxiliary)
git diff --check
```
