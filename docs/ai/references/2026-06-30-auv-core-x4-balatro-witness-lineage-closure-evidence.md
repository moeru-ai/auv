# 2026-06-30 AUV Core-X4 Balatro witness-lineage closure evidence

Date: 2026-06-30

Design handoff: [`2026-06-30-auv-core-x4-balatro-witness-lineage-closure-design.md`](2026-06-30-auv-core-x4-balatro-witness-lineage-closure-design.md)

## Chain

```text
semantic → spatial query → witness → quality
```

## Artifact roles (8)

| File | Role |
| --- | --- |
| `balatro-card-detection-semantic.json` | `balatro-card-detection-semantic` |
| `balatro-card-detection-semantic-inspect.json` | `balatro-card-detection-semantic-inspect` |
| `balatro-card-detection-spatial-query.json` | `balatro-card-detection-spatial-query` |
| `balatro-card-detection-spatial-query-inspect.json` | `balatro-card-detection-spatial-query-inspect` |
| `balatro-card-detection-eval-witness.json` | `balatro-card-detection-eval-witness` |
| `balatro-card-detection-eval-witness-inspect.json` | `balatro-card-detection-eval-witness-inspect` |
| `balatro-card-detection-quality.json` | `balatro-card-detection-quality` |
| `balatro-card-detection-quality-inspect.json` | `balatro-card-detection-quality-inspect` |

## Fixture matrix (witness)

| Path | Witness status | Reason |
| --- | --- | --- |
| `tests/fixtures/balatro_consumption_probe/` + `expected_slots.json` | `ready` | full slot-coverage eval |
| `broken/empty_detections/` (semantic blocked) | `blocked` | `semantic_not_ready` |
| `witness/failed_expected_slots.json` (malformed) | `failed` | `expected_slots_parse_failed` |

## Validation commands

```sh
cargo fmt --check
cargo check
cargo test -p auv-game-balatro card_detection
cargo test render_run_text_renders_balatro
git diff --check
```

## Example run (optional)

```sh
cargo run --example balatro_consumption_probe -- \
  --bundle crates/auv-game-balatro/tests/fixtures/balatro_consumption_probe \
  --expected-slots crates/auv-game-balatro/tests/fixtures/balatro_consumption_probe/expected_slots.json \
  --work-dir /tmp/balatro-probe-work
```

Expected stdout includes `witness_status=ready` and `quality_verdict=measured_only`.

## Honest limits

- No live admission or action readiness wiring
- Balatro remains a **donor candidate** probe, not a graduated third donor
- Proof matrix row 69/70 verdict columns unchanged (Core-X5 re-review pending)
- Quality derives metrics/verdict only from persisted witness manifest
