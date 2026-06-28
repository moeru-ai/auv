# 2026-06-29 AUV Core-X2 Balatro consumption probe design

Date: 2026-06-29

Status: implemented probe slice on `feat/core-x2-balatro-consumption-probe`.

Parent contracts:

- [`2026-06-29-auv-core-x1-third-vertical-admissibility-mvp.md`](2026-06-29-auv-core-x1-third-vertical-admissibility-mvp.md)
- [`2026-06-27-auv-core-spatial-result-consumption-pattern.md`](2026-06-27-auv-core-spatial-result-consumption-pattern.md)

## Chain closed in Core-X2

```text
committed detection bundle fixture
  → semantic gate (ready / blocked / failed)
  → spatial query (answered / blocked / failed)
  → inline eval report → quality manifest + verdict
```

Observe-only: unit tests consume committed JSON; live ONNX remains optional behind `#[ignore]` integration tests.

## Build gap closed vs Core-X1

| Component | Core-X1 gap | Core-X2 closure |
| --- | --- | --- |
| Producer artifact | missing | `CardDetectionBundleManifest` + fixtures under `tests/fixtures/balatro_consumption_probe/` |
| Semantic gate | missing | `card_detection_semantic.rs` → `balatro-card-detection-semantic.json` |
| Spatial query | missing | `card_detection_spatial_query.rs` → `balatro-card-detection-spatial-query.json` |
| Quality + verdict | missing | `card_detection_quality.rs` → `balatro-card-detection-quality.json` |
| `quality_backend` enum | missing | `CardDetectionQualityBackend` persisted on quality manifest |
| Run-store wiring | missing | `src/balatro.rs` + inspect/read sections |

## `metric_partial` policy table (row 69 pressure)

| Donor | `metric_partial` meaning | Metrics when partial |
| --- | --- | --- |
| MC-17 | Image dimension mismatch | **`metrics: None`** |
| osu WQ1 | Partial frame scoring | **`metrics: Some(...)`** |
| **Balatro X2** | Expected slot coverage incomplete (target slots unscored or below confidence) | **`metrics: Some(...)`** with `unscored_slot_count` / `expected_slot_count` — partial **coverage**, not partial frames |

Balatro derive rule (v1): `measured_only` when all expected slots score at or above per-slot `min_confidence`; otherwise `metric_partial` with metrics populated. Threshold is per-slot confidence in `expected_slots.json`, not a global gameplay pass/fail gate.

## `quality_backend` enum (row 70 pressure)

Persisted on quality manifest when `status=ready`:

- `ultralytics_onnx_ui`
- `ultralytics_onnx_entities`

v1 slot-coverage eval uses `ultralytics_onnx_entities` for hand/joker entity detections. Raw HF hub paths and runtime command text are **not** persisted as backend labels (`detector_model_id` remains supplementary lineage only).

## Non-goals (explicit deferrals)

- Live admission / Core-C1 wired action
- `operation.rs` real wire (placeholders unchanged)
- Persisted witness artifact role (eval report inline only)
- Action readiness derive / query-wired live click
- Core-A5a/A5b helper extraction
- Dual-backend query compare (row 67)
- CLI catalog subcommands (library + example only)

## Runtime surfaces

Library operations in `src/balatro.rs`:

- `run_balatro_card_detection_semantic_validation`
- `run_balatro_card_detection_spatial_query`
- `run_balatro_card_detection_quality`
- `run_balatro_consumption_probe_chain`

Example: `examples/balatro_consumption_probe.rs`

Inspect sections: `Balatro Card Detection Semantic/Spatial Query/Quality`
