# 2026-06-29 AUV Core-X2 Balatro consumption probe evidence

Date: 2026-06-29

Branch: `feat/core-x2-balatro-consumption-probe`

Design handoff: [`2026-06-29-auv-core-x2-balatro-consumption-probe-design.md`](2026-06-29-auv-core-x2-balatro-consumption-probe-design.md)

## Fixture paths

Positive bundle:

- `crates/auv-game-balatro/tests/fixtures/balatro_consumption_probe/detection_bundle.json`
- `crates/auv-game-balatro/tests/fixtures/balatro_consumption_probe/expected_slots.json`

Negative semantic:

- `broken/empty_detections/` → `semantic_status=blocked`, reason `empty_detections`
- `broken/bad_schema/` → `semantic_status=failed`, reason `bundle_parse_failed`

Negative query:

- `query/missing_target_slot/` + target `hand:99` → `status=blocked`, reason `target_slot_not_found`
- `query/out_of_bounds/` + target `hand:0` → `status=blocked`, reason `slot_out_of_bounds`

Partial quality:

- `partial_coverage/` + `partial_expected_slots.json` → `verdict=metric_partial` **with** metrics populated

## Validation commands

```sh
cargo fmt --check
cargo check
cargo test -p auv-game-balatro card_detection
cargo test -p auv-cli render_run_text balatro
git diff --check
```

## Example run (optional)

```sh
cargo run --example balatro_consumption_probe --   --bundle crates/auv-game-balatro/tests/fixtures/balatro_consumption_probe   --expected-slots crates/auv-game-balatro/tests/fixtures/balatro_consumption_probe/expected_slots.json   --work-dir /tmp/balatro-probe-work
```

## Honest limits

- No live ONNX required for slice closure
- No proof-matrix graduation; candidate third-donor probe only
- No gameplay usefulness or autoplay claims
