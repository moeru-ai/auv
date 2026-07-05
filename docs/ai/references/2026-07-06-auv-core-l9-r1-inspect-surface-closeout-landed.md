# AUV Core L9-R1 — Inspect Surface Closeout Landed

**Date:** 2026-07-06  
**Gate:** [L9-R1 gate/handoff](2026-07-06-auv-core-l9-r1-inspect-surface-closeout-handoff.md)  
**Supersedes scope:** [L9 handoff](2026-07-05-auv-core-l9-inspect-surface-handoff.md) viewer baseline — R1 adds consumption discipline only.

## Landed

| Slice | Files | Summary |
|-------|-------|---------|
| R1a | `src/inspect_server_viewer.html` | Issue hard-table banners; `plan_delivery_mismatch` limits only in mismatch banner; extended `selfTestActionTransitionLineage` |
| R1b | `src/inspect_server_viewer.html` | `#netease-select-proof-hint` secondary packaging note; banned seam vocabulary; ATL+hint coexist selfTest |
| R1c | `src/inspect.rs` | Seam-first CLI text: `[action.transition.lineage] (seam surface)` before `[candidate.action.execution.lineage] (ledger)` |
| G5 | `src/inspect_server/mod.rs` | `viewer_seam_panel_does_not_reference_cael` |

## Merge gate checklist

| ID | Status |
|----|--------|
| G1 Issue hard-table UI | pass — selfTest + node test |
| G2 issue vs known_limits split | pass — mismatch limits excluded from limits grid |
| G3 Hint secondary; banned words | pass — `selfTestNeteaseSelectProofHint` |
| G4 ATL + hint coexist | pass — selfTest coexist case |
| G5 Zero CAEL in viewer | pass — `rg` 0 + unit test |
| G6 CLI seam-first | pass — `render_run_text_includes_run_span_*` order assert |
| G7 Handoff | pass — gate + this landed doc |

## Consumption discipline (fixed)

- **Viewer seam panel:** `action_transition_lineage` only — never `candidate_action_execution_lineage`
- **CLI text:** seam surface section first, ledger second
- **Hint:** packaging lane only; ATL remains primary when both visible

## Orthogonality

App Command Pack hints (NetEase select proof) do **not** imply L8 seam graduation. See [ACP gate](2026-07-05-auv-core-app-command-pack-gate.md#orthogonality-callout-mandatory-in-every-acp-handoff).

## Non-goals (confirmed not done)

- JSON schema change, `run_read` / producer change
- CAEL removal, viewer ledger panel
- qqmusic / unified proof hint (ACP-B2c defer)
- ACP-C third app pack

## Validation (recorded)

```sh
cargo fmt --check && cargo check
cargo test -p auv-cli viewer_action_transition
cargo test -p auv-cli viewer_seam_panel
cargo test -p auv-cli viewer_renders_netease_select_proof
cargo test -p auv-cli viewer_renders_action_transition_lineage
cargo test -p auv-cli render_run_text_includes_run_span
rg 'candidate_action_execution_lineage' src/inspect_server_viewer.html  # 0 matches
git diff --check
```

## Next gate

ACP-C only after owner names third app **and** needs packaging + inspect discipline re-proof on a distant domain. Otherwise continue core lane work without adding pack sample count.
