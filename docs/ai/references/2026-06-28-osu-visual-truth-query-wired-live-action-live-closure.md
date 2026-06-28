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
- Semantic manifest produced by `validate_visual_truth_semantic` on the probe
  fixture (`visual_truth_manifest.json` + `projection.json`)
- Live gate harness: `examples/osu_query_wired_live_action.rs`
- Dedicated store for closure runs:
  `.tmp/osu-query-wired-live-action-live-closure/store`

## Harness command shape

Semantic gate output (one-time per fixture refresh):

```sh
# Example: write osu-visual-truth-semantic.json under .tmp/.../semantic-out
# using validate_visual_truth_semantic with run_artifact_dir=osu_visual_truth_probe
```

Live wiring harness:

```sh
cargo run --quiet --example osu_query_wired_live_action -- \
  --semantic-manifest .tmp/osu-query-wired-live-action-live-closure/semantic-out/osu-visual-truth-semantic.json \
  --object-index 0 \
  --capture-phase before_dispatch \
  --output-dir .tmp/osu-query-wired-live-action-live-closure/query-out \
  --target-app "osu!" \
  --target-title osu \
  --store-root .tmp/osu-query-wired-live-action-live-closure/store
```

`--target-app` matches benchmark typed-dispatch default (`osu!`). `--target-title`
uses substring `osu` (same as integration tests).

## Three-path smoke table (environment-dependent)

| Path | Upstream condition | Expected wiring | Recorded on 2026-06-28 (local) |
| --- | --- | --- | --- |
| `click_ready` | semantic `ready` + answered `inside_capture` | `attempted=true`, live `window_point` from playfield projection | **Yes** — see run below |
| `answer_non_clickable` | answered `outside_capture` | `attempted=false`, no `input.clickWindowPoint` span | Not run (integration tests only) |
| `not_consumable` | query `failed` / `blocked` | `attempted=false`, preserved status/reason lineage | Not run (integration tests only) |

Non-macOS builds refuse the default library path; integration tests use stub
executor + capture-derived projection only.

## Recorded runs (2026-06-28 local pass)

Store root: `.tmp/osu-query-wired-live-action-live-closure/store`

Environment: osu! open on macOS title screen (user-provided). Fixture object 0 /
`before_dispatch` drives capture-space readiness; live click targets playfield-
projected window coordinates, not the logo under the user cursor.

### 1. click_ready — probe fixture, wiring dispatches non-stub click

- run: `run_1782631533865_61190_0`
- stdout: `attempted=true`, `action_eligibility=click_ready`, `refusal_reason` omitted
  (outcome event: `refusal_reason=none`)
- readiness witness: `pixel_point=400,300` (fixture capture space)
- live dispatch point: `window_point=756.000,474.500` (live playfield projection)
- nested invoke span: `command.resolved` → `input.clickWindowPoint`
- driver outcome: `command.failed` with
  `command input.clickWindowPoint handler failed: main visible window was not found`
  (honest Layer-2 failure after admission; projection resolved a window for
  playfield mapping, click executor used its own window resolve path)
- `operation-result` message: invoke wrapper failure summary (click path reached real
  handler, not stub)
- inspect text section **Osu Visual Truth Query Wired Live Action:** present with
  `dispatch_command=input.clickWindowPoint`, `dispatch_outcome=failed: ... main visible
  window was not found`, `readiness_class=click_ready`

Command (exact):

```sh
cargo run --example osu_query_wired_live_action -- \
  --semantic-manifest .tmp/osu-query-wired-live-action-live-closure/semantic-out/osu-visual-truth-semantic.json \
  --object-index 0 \
  --capture-phase before_dispatch \
  --output-dir .tmp/osu-query-wired-live-action-live-closure/query-out \
  --target-app "osu!" \
  --target-title osu \
  --store-root .tmp/osu-query-wired-live-action-live-closure/store
```

Inspect snippets:

```text
osu.query_wired_live_action.outcome: attempted=true action_eligibility=click_ready refusal_reason=none pixel_point=400,300 window_point=756.000,474.500
command.resolved: resolved input.clickWindowPoint
command.failed: command input.clickWindowPoint handler failed: main visible window was not found
operation-result.operation_id: auv.osu.visual_truth_query_wired_live_action
known_limits includes: osu_query_wired_live_action_capture_space_readiness_live_window_dispatch_no_gameplay_verification
```

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
