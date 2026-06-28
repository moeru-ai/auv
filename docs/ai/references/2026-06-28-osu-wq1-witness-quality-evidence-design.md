# 2026-06-28 osu WQ1 witness + quality evidence design

Date: 2026-06-28

Status: **OSU-WQ1 owner slice** — closes second-vertical witness + quality
evidence on top of existing `visual_eval_report` / `osu eval-detections`. Evidence
and derived verdict only; **not** Core-B, MC-20, controller, or shared runtime
abstraction.

## One-line summary

Osu closes `semantic → query → readiness → wired action → witness → quality
evidence` by consuming persisted `visual_eval_report.json` into W1 witness
artifacts and Q1 quality measurement evidence, with a thin read-side derived
verdict (`measured_only | metric_partial | blocked | failed`).

## Chain placement

```text
visual_truth + projection + offline detections
        │
        ▼
visual_eval_report.json  (existing P7 / osu eval-detections)
        │
        ▼ W1
osu-detection-eval-witness.json + inspect
        │
        ▼ Q1
osu-detection-eval-quality.json + inspect
        │
        ▼ read-side (optional)
derive_osu_detection_eval_quality_verdict_summary
```

## W1 / Q1 / non-goals

See `detection_eval_witness.rs` and `detection_eval_quality.rs` for manifest
fields. Witness is not action verification. Quality is evidence-only (no model
usefulness). No core extraction.

## Cross-links

- [`2026-06-27-auv-second-vertical-consumption-probe-osu-evidence.md`](2026-06-27-auv-second-vertical-consumption-probe-osu-evidence.md)
- [`2026-06-28-osu-visual-truth-query-wired-live-action-design.md`](2026-06-28-osu-visual-truth-query-wired-live-action-design.md)
- [`2026-06-27-minecraft-mc17-holdout-render-quality-design.md`](2026-06-27-minecraft-mc17-holdout-render-quality-design.md)
- [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)
