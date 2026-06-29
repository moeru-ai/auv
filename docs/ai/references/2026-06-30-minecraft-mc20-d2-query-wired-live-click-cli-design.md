# Minecraft MC-20 D2: Query-wired live click CLI entry

Date: 2026-06-30

Status: **D2 implemented** — stable vertical CLI for the MC-19+MC-20 D1 library chain.
**D2.1 live closure recorded**; **D2.2 inspect/store-root closure**; **D3 semantic pass/fail closure**; **D4 live evidence closeout (G0–G8) closed** (2026-06-30) — see
[`2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md`](2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md),
[`2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md`](2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md).
MC-20 controller / planner lane remains **paused** after this slice.

## One-line summary

`auv minecraft query-wired-live-click` is the canonical operator entry for
`query → readiness → admission → dispatch → MC-20 post-action verification` without
`cargo run --example ...`. The binary parses flags and dispatches to
`run_minecraft_query_wired_live_action` only; no verification glue in `main.rs`.

## Operation id

`auv.minecraft.query_wired_live_action` (unchanged from MC-19 D3 / MC-20 D1 library seam)

## Command shape

```sh
auv minecraft query-wired-live-click \
  --training-result-semantic-manifest <path> \
  --target-block <x,y,z> \
  [--target-face north|south|east|west|up|down] \
  [--target-semantics hit_face_center|block_center] \
  [--query-provider checkpoint-native|closed-scene-toy] \
  [--closed-scene-fixture <path>] \
  [--query-command <shell-command>] \
  --output-dir <dir> \
  --target-app <bundle-id> \
  --target-title <window-title-substring> \
  [--sample <pre-telemetry.jsonl>] \
  [--post-sample <post-telemetry.jsonl>] \
  [--verification-expected-item-id <minecraft:item_id>] \
  [--store-root <path>] \
  [inspect client flags]
```

## Flag table

| Group | Flag | Required | Maps to |
| --- | --- | --- | --- |
| Query | `--training-result-semantic-manifest` | yes | `QueryWiredLiveActionInputs.training_result_semantic_manifest_path` |
| Query | `--target-block` | yes | `target_block` (validated `x,y,z`) |
| Query | `--target-face` | no | `target_face` |
| Query | `--target-semantics` | no (default `hit_face_center`) | `target_semantics` |
| Query | `--query-provider checkpoint-native` | no | `use_checkpoint_native_provider` |
| Query | `--query-provider closed-scene-toy` | no | `use_closed_scene_toy_provider` + requires `--closed-scene-fixture` |
| Query | `--closed-scene-fixture` | only with `--query-provider closed-scene-toy` (parse error otherwise) | `closed_scene_fixture_path` |
| Query | `--query-command` | no (mutually exclusive with providers) | `query_command` |
| Query | `--output-dir` | yes | `output_dir` |
| Live dispatch | `--target-app` | yes | `target_app` |
| Live dispatch | `--target-title` | yes | `target_title` |
| MC-20 witness | `--sample` | no | `telemetry_witness.pre_telemetry_sample` |
| MC-20 witness | `--post-sample` | no (requires `--sample`) | `telemetry_witness.post_telemetry_sample` |
| MC-20 D3 semantic | `--verification-expected-item-id` | no (requires `--sample`) | `verification_expected_item_id` |
| Inspect client | `--store-root`, `--inspect-*` | no | `InspectClientOptions` |

Provider rules mirror `query-3dgs-training-result`, plus:

- `--closed-scene-fixture` without `--query-provider closed-scene-toy` → parse error (no silent ignore)
- `--query-provider closed-scene-toy` without `--closed-scene-fixture` → parse error
- `--query-provider checkpoint-native` with `--closed-scene-fixture` → parse error

Historical example wrapper, when used, follows the same provider/fixture and
`--post-sample requires --sample` rules as the canonical CLI.

## Stdout contract

On success the CLI prints at minimum:

| Field | Source |
| --- | --- |
| `runId` | recorded run id |
| `queryStatus` | `output.value.query.manifest.status` |
| `wiringAttempted` | `output.value.wiring.attempted` |
| `actionEligibility` | `output.value.wiring.action_eligibility` |
| `operationResultArtifact` | `output.value.operation_result_artifact_id` |

When wiring dispatch succeeded (`click_summary` present) **and local inspect write is enabled**
(`--inspect-local-write` not set to `false`), also print:

```text
inspectHint: run `auv inspect <run-id> [--store-root <path>]` to view verification_outcome
```

**Honesty:** verification reports world-diff witness honesty (`passed` / `failed` /
`unreliable` / `inconclusive`), not Minecraft gameplay or trainer quality. Absent
`--sample` → D1 `unreliable` branch (`MC20_V1_QUERY_WIRED_WITNESS_ABSENT_KNOWN_LIMIT`).

## Producer branch table

**Reuse MC-20 D1 unchanged** — see
[`2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md`](2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md).

## Comparison with sibling commands

| Command | Operation id | Chain |
| --- | --- | --- |
| `query-3dgs-training-result` | `auv.minecraft.query_3dgs_training_result` | MC-12 query only |
| `live-click` | `auv.minecraft.live_click` | projection click + optional world-diff (separate path) |
| **`query-wired-live-click`** | **`auv.minecraft.query_wired_live_action`** | **full MC-12→MC-14→MC-19→MC-20** |

Do not conflate `live-click` with this entry; different operation ids and glue.

## Dependency direction

```text
src/cli.rs + src/main.rs (parse + thin dispatch)
  → verticals/minecraft::run_minecraft_query_wired_live_action
    → auv-game-minecraft (query / readiness / wiring / verify)
    → verticals/minecraft/verification (VerificationResult staging)
      → operation-result artifact
        → run_read / inspect (unchanged read-side projection)
```

## Explicit non-goals

| Item | Reason |
| --- | --- |
| `run_read` / `inspect` mapper edits | D1 Core-C3 D2 projection sufficient |
| `auv-cli-invoke` registry | vertical subcommand, not invoke catalog |
| Rewrite `minecraft live-click` | separate operation id |
| osu CLI symmetry | separate owner slice |
| MC-20 controller / planner | paused orchestration lane |
| Verification glue in `main.rs` | lives in `verticals/minecraft/verification.rs` |

## D2.2 inspect / store-root closure

- `auv inspect <run-id> [--store-root <path>]` reads the same store used by
  `--store-root` on producer commands.
- **`inspectHint` gate (D2.2):** prints only when **both** conditions hold in
  `main.rs`:
  1. `query_wired_verification_readable(&wiring)` — `attempted=true` and
     `click_summary.is_some()` (dispatch succeeded; MC-20 Layer-3 was eligible to
     run, including `unreliable` without witness).
  2. `should_write_local(&inspect)` — `--inspect-local-write` is not `false`
     (default: local write enabled).
- When dispatch fails (`click_summary` absent, G8 `absent` path), **no**
  `inspectHint` is printed; use explicit `auv inspect <runId> --store-root`.
- Custom store roots are echoed in the hint when printed.

## Paused after D2 — reopen triggers (observation only)

- osu `query-wired-live-click` CLI symmetry
- Proxy `live-click` through query-wired library path
- MC-20 controller / action lease
- ~~D2.1 macOS live closure evidence doc with witness paths~~ **closed** →
  [`2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md`](2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md)

## Related

- MC-20 D1: [`2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md`](2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md)
- MC-19 wiring closure: [`2026-06-27-minecraft-mc19-query-to-live-click-wiring-live-closure.md`](2026-06-27-minecraft-mc19-query-to-live-click-wiring-live-closure.md)
- Canonical CLI replaces historical `examples/mc19_query_wired_live_action.rs` harness (example retained as thin wrapper when present)

## D3 semantic pass/fail (closed 2026-06-30)

- Design: [`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md)
- Live G6/G7: [`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md)

## D4 live evidence closeout (closed 2026-06-30)

- Design: [`2026-06-30-minecraft-mc20-d4-live-evidence-closeout-design.md`](2026-06-30-minecraft-mc20-d4-live-evidence-closeout-design.md)
- Graduation G0–G8: [`2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md`](2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md)


## Final closeout / pause decision

MC-20 final closeout and pause boundary are recorded in
[`2026-06-30-minecraft-mc20-final-closeout-pause-decision.md`](2026-06-30-minecraft-mc20-final-closeout-pause-decision.md).
