# AUV Core L9-R1 — Inspect Surface Closeout (ATL Consumption Discipline)

**Date:** 2026-07-06  
**Prerequisite:** [L9 handoff](2026-07-05-auv-core-l9-inspect-surface-handoff.md), [L8 closeout](2026-07-05-auv-core-l8-closeout-review.md)

> **Slice goal:** Fix `ActionTransitionLineage` consumption discipline — not a viewer polish collection.

## Scope

| In scope | Out of scope |
|----------|--------------|
| Viewer issue hard-table banners | JSON schema change |
| Hint secondary-only disambiguation | `run_read` / producer change |
| CLI seam-first section order + labels | CAEL removal |
| CAEL anti-regression grep test | ACP-C third app pack |
| | Unified multi-app proof hint (ACP-B2c defer) |

## Forced decision 1 — Consumption matrix

| Surface | CAEL | ATL | Rule |
|---------|------|-----|------|
| Viewer seam panel | **not** seam source | **only** seam data source | `renderActionTransitionLineage*` reads `action_transition_lineage` only |
| CLI `inspect_run` text | section 2 **(ledger)** | section 1 **(seam surface)** | seam-first order |
| `GET /runs/{id}` JSON | field retained | field retained | no schema change |

## Forced decision 2 — Issue mapping hard table (viewer)

**Split rule:** `issue` → Issue banner; `plan_delivery_mismatch:` in `known_limits` → Mismatch banner (not duplicated in limits grid). Order: mismatch → issue → summary.

| Trigger | Required UI | Forbidden |
|---------|-------------|-----------|
| `issue === "missing_action_resolver_decision"` | headline + explanation | read decision from CAEL |
| `issue === "missing_input_action_result"` | headline + explanation | read driver from CAEL |
| `issue` prefix `plan_effective_method_divergence:` | tension headline + issue as detail | raw issue only |
| `known_limits` `startsWith("plan_delivery_mismatch")` | mismatch banner + limit text | swallow into generic limits row |

## Forced decision 3 — Hint secondary-only

When `action_transition_lineage.length > 0`:

- ATL panel is **primary** seam surface
- `#netease-select-proof-hint` is **secondary** packaging note only
- Hint text must **not** contain: `seam`, `resolver`, `driver_result`, `verification_outcome`, `graduation`, `passed`

## Forced decision 4 — No CAEL in seam renderer

- `inspect_server_viewer.html` must have **zero** `candidate_action_execution_lineage` references
- Test: `viewer_seam_panel_does_not_reference_cael`

## Merge gate (G1–G7)

| ID | Criterion | Verification |
|----|-----------|--------------|
| G1 | Issue hard-table UI | `selfTestActionTransitionLineage` + node test |
| G2 | issue vs known_limits split | selfTest banner asserts |
| G3 | Hint secondary; banned words | `selfTestNeteaseSelectProofHint` |
| G4 | ATL + hint coexist | selfTest coexist case |
| G5 | Zero CAEL in viewer | `rg` + `viewer_seam_panel_does_not_reference_cael` |
| G6 | CLI seam-first | `inspect.rs` order test |
| G7 | This handoff + landed doc | docs review |

## Validation

```sh
cargo fmt --check && cargo check
cargo test -p auv-cli inspect_server::tests::viewer_action_transition
cargo test -p auv-cli inspect_server::tests::viewer_action_transition_self_test_executes_in_node
cargo test -p auv-cli inspect_server::tests::viewer_renders_netease_select_proof_hint
cargo test -p auv-cli inspect_server::tests::viewer_seam_panel_does_not_reference_cael
cargo test -p auv-cli inspect::tests render_run_text_includes_run_span
rg 'candidate_action_execution_lineage' src/inspect_server_viewer.html  # expect 0
git diff --check
```

## Next gate

ACP-C only after G1–G7 land and owner names a third app. Until then: **no third app pack**.
