# Osu visual truth query wired live action live closure

Date: 2026-06-28

## Summary

This slice closes the wiring evidence chain from osu visual-truth spatial query +
derived action readiness to **one honest, recorded, refusable live click attempt**
via playfield re-projection and live `input.clickWindowPoint`. It proves admission
+ dispatch honesty and run-store completeness only — not osu gameplay success.

## Preconditions

- macOS with Accessibility permissions for AUV input delivery
- Committed probe fixture:
  `crates/auv-game-osu/tests/fixtures/osu_visual_truth_probe/`
- Semantic manifest produced by `run_osu_visual_truth_semantic_validation`
- Live gate harness: `examples/osu_query_wired_live_action.rs`
- Dedicated store for closure runs: `.tmp/osu-wired-live/store` (recommended)

## Harness command shape

```sh
cargo run --quiet --example osu_query_wired_live_action -- \
  --semantic-manifest <path/to/osu-visual-truth-semantic.json> \
  --object-index 0 \
  [--capture-phase before_dispatch] \
  [--object-kind circle] \
  --output-dir <dir> \
  --target-app <bundle-id-or-app-name> \
  --target-title <window-title-substring> \
  [--store-root .tmp/osu-wired-live/store]
```

## Three-path smoke table (environment-dependent)

| Path | Upstream condition | Expected wiring |
| --- | --- | --- |
| `click_ready` | semantic `ready` + answered `inside_capture` | `attempted=true`, live `window_point` from playfield projection |
| `answer_non_clickable` | answered `outside_capture` | `attempted=false`, no `input.clickWindowPoint` span |
| `not_consumable` | query `failed` / `blocked` | `attempted=false`, preserved status/reason lineage |

Non-macOS builds refuse the default library path; integration tests use stub
executor + capture-derived projection only.

## Inspect fields

`inspect_run` text section **Osu Visual Truth Query Wired Live Action:** includes:

- `query_artifact`, `attempted`, `action_eligibility`
- `pixel_point` (capture-space readiness witness)
- `window_point` (live dispatch attempt coordinates)
- `refusal_reason`, `dispatch_command`, `dispatch_outcome`
- `readiness_class` (derived readiness echo for Core-C1 mapping)

`known_limits` on `operation-result` includes
`osu_query_wired_live_action_capture_space_readiness_live_window_dispatch_no_gameplay_verification`.

## Explicit boundary

Closure proves **admission + one honest dispatch/refusal**, not hit verification or
proof-matrix graduation. See design:
`docs/ai/references/2026-06-28-osu-visual-truth-query-wired-live-action-design.md`.
