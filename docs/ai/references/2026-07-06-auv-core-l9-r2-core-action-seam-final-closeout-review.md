# AUV Core L9-R2 — Core Action Seam Final Closeout Review

**Date:** 2026-07-06
**Prerequisites:** [L8 closeout](2026-07-05-auv-core-l8-closeout-review.md) (`close_for_core_seam_surface_gap_only`), [L9 inspect surface](2026-07-05-auv-core-l9-inspect-surface-handoff.md), [L9-R1 landed](2026-07-06-auv-core-l9-r1-inspect-surface-closeout-landed.md)
**Slice:** docs-only evidence review — no runtime, viewer, MCP, or ACP-C changes

> **Owner summary（中文）：** L8 已确认 producer/read-model/compatibility 核心层合格，唯一缺口是 viewer 未消费 `action_transition_lineage`。L9 补齐 viewer 面板，L9-R1 固化 ATL 消费纪律（issue 硬表、hint 次级、CLI seam-first、viewer 零 CAEL）。本审查对照代码与测试复验五条强制问题；**核心 action seam 可从 `close_for_core_seam_surface_gap_only` 升级为正式 `close`**。ACP-C 仍属 packaging lane，需 owner 点名第三 app，与 seam 毕业正交。

---

## Status / verdict

| Field | Value |
|-------|-------|
| **Prior status (L8)** | `close_for_core_seam_surface_gap_only` — viewer seam consumption gap |
| **L9 gap closure** | [L9 handoff](2026-07-05-auv-core-l9-inspect-surface-handoff.md) + [L9-R1 G1–G7](2026-07-06-auv-core-l9-r1-inspect-surface-closeout-landed.md) |
| **This review verdict** | **`close`** — core action seam formally closed for L8/L9 scope |
| **Upgrade** | `close_for_core_seam_surface_gap_only` → **`close`** |

**Rationale:** All five owner questions pass on current evidence. The L8 material gap (viewer not rendering ATL with partial/mismatch discipline) is closed. Producer (`candidate-action-execution`), read projection (`ActionTransitionLineage`), inspect consumption (CLI + server JSON + viewer), and compatibility boundaries (canonical / legacy / malformed) are aligned and regression-tested. Remaining items are **explicit lane deferrals**, not seam blockers.

---

## Forced questions (evidence matrix)

| # | Question | Verdict | Primary evidence |
|---|----------|---------|------------------|
| 1 | Is `candidate-action-execution` a stable carrier for **reconciled effective decision + `input_action_result`**? | **pass** | `build_candidate_action_execution_artifact` always runs `reconcile_effective_decision` then sets both fields on `CandidateActionExecutionArtifact` (`src/candidate_action_decision.rs` ~576–651). `detail.plan_method` / `detail.effective_method` record L8a plan vs reconciled effective. Regression: `reconcile_effective_decision_records_plan_delivery_mismatch_*`, `reconcile_effective_decision_records_unmapped_delivery_path_limit`, `action_transition_lineage_surfaces_plan_delivery_mismatch_from_l8b`. |
| 2 | Is `ActionTransitionLineage` (ATL) the **sole inspect seam read surface**? | **pass** | Read: `extract_action_transition_lineage` projects from `candidate-action-execution` only (`src/run_read.rs` ~5670–5705). CLI: `[action.transition.lineage] (seam surface)` first (`src/inspect.rs` ~2698–2748). Viewer: `renderActionTransitionLineage` reads `run.action_transition_lineage` only; **zero** `candidate_action_execution_lineage` refs (`rg` + `viewer_seam_panel_does_not_reference_cael`). Server JSON: both fields retained (schema unchanged); seam discipline is consumption-side, not field removal. |
| 3 | Is CAEL clearly relegated to **ledger** role? | **pass** | Dual track retained per L8 Option A. CLI labels ledger section `(ledger)` with explicit pointer to ATL (`src/inspect.rs` ~2751–2753). CAEL still extracted for consent/closure/detail (`extract_candidate_action_execution_lineage`); not used by viewer seam panel. L9-R1 G5/G6 lock this. |
| 4 | Are legacy / malformed / partial compatibility boundaries clear enough? | **pass** | ATL: canonical → `action_transition_lineage_entry`; legacy missing fields → `legacy_action_transition_lineage_entry` (`status=partial`, typed issues); parse/mime failures → `malformed_action_transition_lineage`. Tests: `action_transition_lineage_marks_legacy_missing_decision_as_partial`, `action_transition_lineage_surfaces_plan_delivery_mismatch_from_l8b`, malformed extraction in `extract_action_transition_lineage` test block. Viewer: issue hard-table + mismatch banner split (`selfTestActionTransitionLineage` cases for missing decision/driver, divergence, malformed). |
| 5 | Can L8/L9 core seam be formally **closed** now? | **pass → `close`** | L8 core layers + L9 surface + L9-R1 consumption discipline satisfy the gate that blocked full close. No new failing evidence on L8b producer. |

---

## Merge gates (seam closeout — adapted from L9-R1 G1–G7)

| ID | Criterion | Status | Verification |
|----|-----------|--------|--------------|
| S1 | L8b artifact coexists reconciled `action_resolver_decision` + `input_action_result` on canonical path | **pass** | `build_candidate_action_execution_artifact`; blocked-not-ready path uses `blocked_input_action_result` then same reconcile |
| S2 | ATL read-side is projection-only (no policy rebuild from driver alone) | **pass** | `action_transition_lineage_entry` clones embedded decision + `input_action_result`; planned joins L8a ref only |
| S3 | ATL is sole **seam** inspect surface (CLI + viewer) | **pass** | CLI order test `render_run_text_includes_run_span_*`; viewer `rg` 0 CAEL; `viewer_seam_panel_does_not_reference_cael` |
| S4 | CAEL ledger labeled and secondary | **pass** | CLI `(ledger)` section + guidance string |
| S5 | Legacy → `partial`; malformed → `malformed`; mismatch → `partial` + limits | **pass** | `classify_action_transition_lineage`, legacy entry, unit tests |
| S6 | Viewer surfaces partial/mismatch/issue (not JSON-only) | **pass** | L9 UI contract + R1 issue hard-table; `viewer_renders_action_transition_lineage_hooks` |
| S7 | L9-R1 G1–G7 landed | **pass** | [L9-R1 landed doc](2026-07-06-auv-core-l9-r1-inspect-surface-closeout-landed.md) |
| S8 | ACP orthogonality preserved (seam close ≠ pack graduation) | **pass** | [ACP gate](2026-07-05-auv-core-app-command-pack-gate.md); hint `packaging lane only` in viewer tests |

---

## Inspect consumption matrix (post L9-R1)

| Surface | Seam source | Ledger | Rule |
|---------|-------------|--------|------|
| Viewer `#action-transition-lineage` | `action_transition_lineage` only | not referenced | ATL cards, mismatch/issue banners |
| CLI `inspect_run` text | section 1 `(seam surface)` | section 2 `(ledger)` | seam-first order |
| `GET /runs/{id}` JSON | `action_transition_lineage` field | `candidate_action_execution_lineage` field | schema unchanged; consumers must follow discipline |

---

## Producer path summary (unchanged from L8, re-verified)

```text
L8a candidate-action-decision (plan)
  → L8b candidate-action-execution (reconcile_effective_decision + input_action_result)
  → ATL extract_action_transition_lineage (effective + planned join + driver + C3 verification)
  → inspect: CLI / server JSON / viewer
```

**Canonical artifact fields** (`CandidateActionExecutionArtifact`): `action_resolver_decision` (reconciled effective), `input_action_result`, `operation_result`, `known_limits` (includes reconcile limits). Role: `candidate-action-execution`; version: `candidate_action_execution_artifact_v0`.

**Auxiliary bridge (deferred, not primary):** `g3-binding-fact` staged alongside execution (`record_candidate_action_execution_artifact` ~689–707) — denormalized MC-5 bridge; does not displace L8b pair.

---

## What would falsify `close`

Adversarial checks — any of these would force downgrade to `close_with_notes` or reopen L8b:

1. **Producer regression:** `build_candidate_action_execution_artifact` path that omits `action_resolver_decision` or `input_action_result` on ready/blocked/mismatch runs without an explicit deferral boundary.
2. **Read-side fact invention:** ATL projection reconstructing `policy` / `primary_method` / `target_query` from `InputActionResult` alone (L8 hard rule violation).
3. **Viewer seam regression:** `candidate_action_execution_lineage` reintroduced in `inspect_server_viewer.html` seam render path.
4. **Unstable partial/malformed:** legacy artifacts without optional fields no longer map to stable `partial` + typed `issue`, or JSON parse failures not `malformed`.
5. **New failing evidence** on plan/delivery mismatch not surfacing in ATL `known_limits` + `partial` status.

None observed in current tree at review time.

---

## Known intentional gaps (not seam blockers)

| Item | Lane | Boundary | Reopen trigger |
|------|------|----------|----------------|
| `g3-binding-fact` without embedded decision | MC-5 bridge | denormalized adjunct | owner L8b-g3 slice |
| invoke `input.*` without decision pair | invoke lane | deferred | owner + L8 stable |
| `session.act_with_result` artifacts | session lane | in-memory / no artifact pair | owner |
| `ActionResolver` module in `contract.rs` cite | archived / future | not active producer | owner |
| **ACP-C** third app command pack | **app packaging** | [gate doc](2026-07-05-auv-core-app-command-pack-gate.md#gate-conditions-all-required) | owner names app + slice |
| S / Surface Memory | observation lane | orthogonal | lane discipline doc |
| qqmusic unified proof hint (ACP-B2c) | packaging | deferred | owner |
| CAEL removal / viewer ledger panel | inspect UX | explicitly out of L9-R1 scope | separate owner slice |
| JSON schema dropping CAEL | API breaking | not required for seam close | owner API slice |

---

## Explicit deferrals — what opens only after this close

| Gate | Condition | Notes |
|------|-----------|-------|
| **ACP-C** | Owner dispatch + third app named | Seam `close` satisfies ACP gate condition 1; still requires owner-named app (condition 3). Do not conflate with seam graduation. |
| **L8b producer reopen** | New failing evidence only | Not triggered by packaging or S-lane work. |
| **TERMS_AND_CONCEPTS sync** | Optional follow-up | No term gap blocked this verdict; add close status to shared vocabulary only if owner wants doc parity. |

---

## Cross-reference chain

```text
L8 closeout (close_for_core_seam_surface_gap_only)
  → L9 viewer ATL panel
  → L9-R1 consumption discipline (G1–G7)
  → L9-R2 this review (close)
```

---

## Validation commands (recorded at review time)

Docs slice:

```sh
git diff --check -- docs/ai/references/
```

Evidence re-check (representative; not re-run for docs-only slice unless owner requests):

```sh
cargo test -p auv-cli --lib plan_delivery_mismatch
cargo test -p auv-cli --lib action_transition_lineage
cargo test -p auv-cli inspect_server::tests::viewer_seam_panel_does_not_reference_cael
cargo test -p auv-cli inspect_server::tests::viewer_renders_action_transition_lineage_hooks
cargo test -p auv-cli inspect::tests render_run_text_includes_run_span
rg 'candidate_action_execution_lineage' src/inspect_server_viewer.html  # expect 0
```

---

## Next gate

- **Core lane:** proceed without reopening L8b/L9 unless falsifier above appears.
- **RI3-A:** [Runtime invoke surface parity audit](2026-07-06-auv-core-ri3-runtime-invoke-surface-parity-audit-handoff.md) — active core question after seam `close`; owner picks RI3-B from Table 3.
- **Packaging lane:** ACP-C only after owner names third app; re-proof inspect discipline on distant domain if needed.
- **Optional:** TERMS_AND_CONCEPTS close-status note (owner-triggered).
