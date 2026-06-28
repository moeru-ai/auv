# 2026-06-27 osu! second-vertical consumption probe evidence

Date: 2026-06-27

Status: evidence appendix for Core-A proof matrix after osu probe Slice 1–2.

## Closure summary

osu! probe implements MC-14-analog derived consumption (semantic → query →
derived readiness at shape only; not full MC-14 window-readiness parity):

```text
visual_truth + projection
  -> semantic gate (ready/blocked/failed)
  -> spatial query (answered/blocked/failed)
  -> derived action readiness (click_ready / answer_non_clickable / not_consumable)
```

Positive/negative paths are covered by `auv-game-osu` unit tests and frozen fixture
`crates/auv-game-osu/tests/fixtures/osu_visual_truth_probe/`.

## Proof matrix row assessment

| Row | Verdict | Evidence |
| --- | --- | --- |
| Query status triad | **satisfied as second-vertical probe-local recurrence** | `VisualTruthSpatialQueryStatus` with distinct `answered/blocked/failed`; semantic `ready` kept separate. Single backend (`playfield_projection_reference`); not extraction-pressure evidence. |
| Action readiness view | **satisfied as second-vertical probe-local recurrence** | Derived triad + inspect section without dispatch. **Capture-space consumability only** — not dispatch-safe or authority-bearing readiness. |
| Stage status triad | **partial (structurally shallow)** | Semantic + query persisted stages only; no witness/quality/compare/provider stage family — not "almost done" partial |
| Provider comparison verdict | **not satisfied** | dual-backend compare intentionally deferred |
| Quality measurement verdict | **candidate (OSU-WQ1)** | witness→quality on `visual_eval_report`; MC-17-shaped verdict; evidence-only; not matrix graduation |
| Backend label discipline | **partial** | `query_backend=playfield_projection_reference` persisted; no second vertical backend family yet |

## Admissibility vs extraction pressure

These row verdicts are **proof-appendix conclusions** only. They support
**second-vertical recurrence exists** for Core-A uncertainty compression. They do
**not** create engineering pressure to extract helpers now. Default action:
**defer extraction** until concrete repetition pain appears.

## Non-claims

- No core enum extraction performed or recommended by this probe
- No window-click authority claimed (pixel coordinates only)
- No witness/quality/live-click wiring
- No shared stage-status discipline graduation from this probe alone

## Related docs

- Design: `docs/ai/references/2026-06-27-auv-second-vertical-consumption-probe-osu-design.md`
- Matrix: `docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`


## OSU-WQ1 update (2026-06-28)

Witness + quality evidence chain landed as a **separate slice** on detection eval
(`visual_eval_report.json` → witness → quality). This is **candidate**
second-vertical recurrence for the quality measurement verdict row only; it does
**not** upgrade Core-A proof-matrix verdicts or recommend core extraction.

Design: `docs/ai/references/2026-06-28-osu-wq1-witness-quality-evidence-design.md`

Non-claims unchanged for probe Slice 1–2; WQ1 adds witness/quality wiring but
still excludes action verification, usefulness claims, and core graduation.
