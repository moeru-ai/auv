# AUV Core L9 Inspect Surface Handoff

**Date:** 2026-07-05  
**Prerequisite:** [L8 closeout](2026-07-05-auv-core-l8-closeout-review.md) verdict `close_for_core_seam_surface_gap_only`

## Scope

**Viewer only** — `src/inspect_server_viewer.html` consumes existing `GET /runs/{id}.action_transition_lineage`.

**Non-goals:** no API schema change; no producer change; no `CandidateActionExecutionLineage` removal.

## Delivered

| Surface | Status |
|---------|--------|
| Server JSON | pre-existing |
| CLI `[action.transition.lineage]` | pre-existing |
| Viewer panel `#action-transition-lineage` | **this slice** |

### UI contract (hard acceptance)

Per execution entry the viewer shows:

- `effective_decision.selected_method` (primary fact in summary grid)
- Planned vs effective **mismatch tension** when methods diverge or `plan_delivery_mismatch` ∈ `known_limits` (banner + card highlight)
- `status=partial` via status pill + dedicated **Known limits / partial** section (not JSON-only)
- `verification.verification_outcome` + `driver_result.selected_path`

### Wiring

- `loadRunDetail` / `refreshViewParserProofFromRunDetail` → `renderActionTransitionLineage`
- `selectRun` / errors → `clearActionTransitionLineage`
- `mergeRunDetail` preserves `action_transition_lineage` across partial merges

### Regression

- Viewer inline: `selfTestActionTransitionLineage()` (mismatch banner, limits, partial visibility)
- Server: `root_serves_inline_viewer_html` asserts panel + self-test present
- Existing: `cargo test -p auv-cli inspect_server::tests::run_route_includes`

## L9 consumption rule

Viewer reads **`ActionTransitionLineage` only** — not `candidate_action_execution_lineage` (per closeout drift verdict A).

## Next gate

[L9-R1 closeout](2026-07-06-auv-core-l9-r1-inspect-surface-closeout-landed.md) — ATL consumption discipline (issue hard table, hint secondary, CLI seam-first). [App Command Pack gate](2026-07-05-auv-core-app-command-pack-gate.md) remains owner-triggered; do not open ACP-C before L9-R1 merge gate G1–G7.
