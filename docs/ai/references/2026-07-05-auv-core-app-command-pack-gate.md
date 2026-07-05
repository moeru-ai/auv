# AUV Core App Command Pack — Entry Gate

**Date:** 2026-07-05  
**Lane:** app packaging (not core seam)

## Gate conditions (all required)

1. [L8 closeout](2026-07-05-auv-core-l8-closeout-review.md) verdict ∈ `{ close, close_for_core_seam_surface_gap_only }` (or [L8-R2](2026-07-06-auv-core-l8-r2-post-acp-closeout-review.md) `close_with_documented_gaps` for continued packaging)
2. [L9 inspect surface](2026-07-05-auv-core-l9-inspect-surface-handoff.md) viewer hard acceptance green
3. Owner names target app + command pack slice

## Orthogonality callout (mandatory in every ACP handoff)

> **Pack pass ≠ seam re-proof.** Completing an App Command Pack (hermetic invoke + persist + inspect JSON) does **not** upgrade the [L8](2026-07-05-auv-core-l8-closeout-review.md) / [L8-R2](2026-07-06-auv-core-l8-r2-post-acp-closeout-review.md) core action seam verdict. ACP lanes do not write `candidate-action-execution`, `action_resolver_decision`, or `ActionTransitionLineage` producer facts unless a separate owner-approved L8b slice says so.

## Reuse (do not reinvent)

- L8b artifact pair: `action_resolver_decision` + `input_action_result` (reconciled effective)
- Read: `ActionTransitionLineage` via runtime inspect / `GET /runs`
- Surface: CLI text + server JSON + L9 viewer

## Non-goals

- Third action-result schema
- Bypass verification boundary (`activation_only` vs semantic success)
- Mixing S/Surface Memory observation work into ACP slice
- Reopening L8b producer without new failing evidence

## Trigger

Owner dispatch: `feat(auv-<app>): <command-pack-slice>` with explicit command list and proof fixtures.
