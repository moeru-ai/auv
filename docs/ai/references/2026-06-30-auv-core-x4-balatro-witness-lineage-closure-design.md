# 2026-06-30 AUV Core-X4 Balatro witness-lineage closure design

Date: 2026-06-30

Status: Phase 1–2 landed (witness module, quality refactor, runtime chain, read/inspect, evidence).

Parent contracts:

- [`2026-06-29-auv-core-x2-balatro-consumption-probe-design.md`](2026-06-29-auv-core-x2-balatro-consumption-probe-design.md)
- [`2026-06-27-auv-core-spatial-result-consumption-pattern.md`](2026-06-27-auv-core-spatial-result-consumption-pattern.md)

Reference mirror: osu WQ1 witness + quality (`crates/auv-game-osu/src/detection_eval_witness.rs`, `detection_eval_quality.rs`).

## Chain target

```text
semantic → spatial query → witness → quality
```

**Lineage honesty (X4):** quality manifest reads **only** `card_detection_eval_witness_manifest_path` + `witness_status`. Witness manifest carries semantic + spatial-query + expected_slots + bundle lineage and persists slot-coverage eval payload inline.

**Query non-gate:** witness records `card_detection_spatial_query_manifest_path` for durable lineage. Slot-coverage eval does **not** require spatial query `answered`; query failure must not be mis-bound to quality half-chain.

## Artifact roles (witness slice)

| File | Role constant (Phase 2) |
| --- | --- |
| `balatro-card-detection-eval-witness.json` | `BALATRO_CARD_DETECTION_EVAL_WITNESS_ROLE` |
| `balatro-card-detection-eval-witness-inspect.json` | `BALATRO_CARD_DETECTION_EVAL_WITNESS_INSPECT_ROLE` |

Existing X2 roles unchanged until runtime wiring lands.

## Witness manifest fields

`CardDetectionEvalWitnessManifest` (`CARD_DETECTION_EVAL_WITNESS_MANIFEST_SCHEMA_VERSION = 1`):

- `schema_version`, `generated_at_millis`
- `card_detection_semantic_manifest_path`
- `card_detection_spatial_query_manifest_path`
- `expected_slots_path`
- `source_detection_bundle_dir`
- Eval payload: `expected_slot_count`, `scored_slot_count`, `unscored_slot_count`, `below_confidence_slot_count`
- `quality_backend`, `detector_model_id`, `slot_scores`
- `status: StageStatus` (`ready` / `blocked` / `failed`)
- `reason: Option<CardDetectionEvalWitnessReason>`
- `known_limits` (includes `BALATRO_X4_WITNESS_KNOWN_LIMIT`)

## Witness gate table

| Status | Trigger | Fixture |
| --- | --- | --- |
| **ready** | semantic `Ready`; query manifest readable + lineage matches; expected_slots + bundle readable; eval completes | `tests/fixtures/balatro_consumption_probe/` + `expected_slots.json` |
| **blocked** | semantic not `Ready`; missing query manifest; query lineage mismatch; missing expected_slots | `broken/bad_schema/`, lineage mismatch test |
| **failed** | semantic parse fail; query manifest parse fail; expected_slots parse fail; bundle reload fail | `witness/failed_expected_slots.json`, bad query JSON test |

`CardDetectionEvalWitnessReason`: `semantic_not_ready`, `semantic_failed`, `missing_expected_slots`, `expected_slots_parse_failed`, `bundle_unavailable`, `missing_query_manifest`, `query_manifest_parse_failed`, `query_lineage_mismatch`.

## Quality mapping (witness-bound)

Quality reads witness manifest only (`CardDetectionQualityInputs { witness_manifest_path, output_dir }`).

| Witness `status` | Quality `status` | Quality `reason` | Quality `verdict` |
| --- | --- | --- | --- |
| missing file | `blocked` | `missing_witness_manifest` | `blocked` |
| parse fail | `failed` | `witness_manifest_parse_failed` | `failed` |
| `blocked` | `blocked` | `witness_blocked` / `witness_not_ready` | `blocked` |
| `failed` | `failed` | `witness_failed` | `failed` |
| `ready` + full slot coverage | `ready` | — | `measured_only` |
| `ready` + partial coverage | `ready` | — | `metric_partial` |

Quality manifest adds `card_detection_eval_witness_manifest_path`, `witness_status`. Removes direct `card_detection_semantic_manifest_path` and `source_detection_bundle_dir`.

**Schema break (X4):** `CARD_DETECTION_QUALITY_MANIFEST_SCHEMA_VERSION` and `CARD_DETECTION_QUALITY_INSPECT_REPORT_SCHEMA_VERSION` bump to **2**. X2 semantic-bound quality artifacts at schema version 1 are not wire-compatible; read-side consumers must not treat v1 and v2 as the same contract.

Known limit: `BALATRO_X4_WITNESS_BOUND_QUALITY_KNOWN_LIMIT` replaces retired `BALATRO_X2_INLINE_EVAL_KNOWN_LIMIT`.

## X2 inline eval retirement

Core-X2 quality derived eval inline from semantic + expected_slots. Core-X4 moves `build_eval_report` into `card_detection_eval_witness.rs` and persists eval payload on witness manifest. Quality no longer reloads semantic bundle or expected_slots.

## Phase 1 scope (this slice)

- `card_detection_eval_witness.rs` + unit tests (`ready` / `blocked` / `failed`)
- `card_detection_quality.rs` witness-only consumption + updated tests
- `lib.rs` exports

## Deferred (Core-X5+)

- Proof matrix row 69/70 re-review
- Live admission / action readiness
