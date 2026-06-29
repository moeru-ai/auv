# MC-20 D2.1: Canonical CLI live closure

Date: 2026-06-30

## Summary

MC-20 D2.1 closes the operator evidence chain for the **canonical CLI**
`auv minecraft query-wired-live-click`. This slice proves that an operator can
run the full MC-12 → MC-14 → MC-19 → MC-20 chain through the stable vertical
entry (not `cargo run --example ...`), with honest wiring and MC-20
post-action verification projected on `auv inspect`.

Evidence scope:

- CLI parse constraints (automated G0 + live negative G1)
- MC-19 wiring honesty on three refusal/click paths (G2–G4)
- MC-20 verification branches: `unreliable` (no witness) and `inconclusive`
  (synthetic pre/post telemetry) (G4–G5)
- Read-side `verification_outcome` on `MC-19 Query Wired Live Action:` inspect
  lines

No production code changes in this slice — documentation and live runs only.

## Preconditions

- macOS with Accessibility permissions for AUV input delivery
- MC-18 semantic fixture (present locally):
  `.tmp/mc18-live/setup/semantic.json`
- MC-18 closed-scene fixtures (committed):
  - `crates/auv-game-minecraft/tests/fixtures/mc18/visible.json`
  - `crates/auv-game-minecraft/tests/fixtures/mc18/outside_window.json`
- Dedicated store for D2.1 runs: `.tmp/mc20-d2-1-live/store` (separate from
  `.tmp/mc19-live/store`)
- Target app / title (reused from MC-19 D4 local record):
  - `--target-app com.todesktop.230313mzl4w4u92`
  - `--target-title Cursor`

## Canonical CLI command template

```sh
cargo run --quiet -- minecraft query-wired-live-click \
  --training-result-semantic-manifest .tmp/mc18-live/setup/semantic.json \
  --target-block <x,y,z> \
  [--target-face north] \
  --target-semantics hit_face_center \
  --query-provider closed-scene-toy \
  --closed-scene-fixture <fixture.json> \
  --output-dir <dir> \
  --target-app com.todesktop.230313mzl4w4u92 \
  --target-title Cursor \
  [--sample <pre.jsonl>] [--post-sample <post.jsonl>] \
  --store-root .tmp/mc20-d2-1-live/store
```

## Evidence matrix (G0–G5)

| Gate | ID | Command要点 | 预期 wiring | 预期 `verification_outcome` | Pass |
| --- | --- | --- | --- | --- | --- |
| Parse（自动） | G0 | `cargo test -p auv-cli parse_minecraft_query_wired_live_click` | — | — | yes (9/9) |
| Parse（live negative） | G1 | orphan `--closed-scene-fixture` 无 provider | CLI exit 1 | — | yes |
| Refusal | G2 | `outside_window.json`, block `511,73,728` | `attempted=false`, `answer_non_clickable` | `not_attempted` | yes |
| Not consumable | G3 | `visible.json`, block `9,9,9` | `attempted=false`, `not_consumable` | `not_attempted` | yes |
| Click + no witness | G4 | `visible.json`, `511,73,728`, **无** `--sample` | `attempted=true`, `click_ready` | `unreliable` | yes |
| Click + witness | G5 | 同上 + `--sample` / `--post-sample` | `attempted=true`, dispatch 可达 real handler | `inconclusive` | yes |

### G0 — automated parse tests

```sh
cargo test -p auv-cli parse_minecraft_query_wired_live_click
```

Result (2026-06-30): **9 passed**, 0 failed (includes orphan fixture, dual
provider, post-sample-without-sample, and related parse guards).

### G1 — live parse negative (orphan fixture)

```sh
cargo run --quiet -- minecraft query-wired-live-click \
  --training-result-semantic-manifest .tmp/mc18-live/setup/semantic.json \
  --target-block 511,73,728 \
  --target-face north \
  --target-semantics hit_face_center \
  --closed-scene-fixture crates/auv-game-minecraft/tests/fixtures/mc18/visible.json \
  --output-dir .tmp/mc20-d2-1-live/g1-orphan/query-output \
  --target-app com.todesktop.230313mzl4w4u92 \
  --target-title Cursor \
  --store-root .tmp/mc20-d2-1-live/store
```

- exit code: **1**
- stderr: `error: --closed-scene-fixture requires --query-provider closed-scene-toy`
- no `runId` (parse failure before run creation)

## Witness prep (G5)

Aligned with integration test
`query_wired_live_action_with_witness_telemetry_tick_advance_projects_inconclusive`
and helpers `mc18_target_frame` / `mc20_post_frame_after_click` in
`src/verticals/minecraft/mod.rs`.

**NOTICE:** `view_matrix` and `projection_matrix` must be flat `[f64; 16]`
arrays (not nested 2×2). Nested matrices produce malformed jsonl and the CLI
reports `no valid minecraft pre frame found`.

One-time prep (from repo root):

```sh
mkdir -p .tmp/mc20-d2-1-live/witness

python3 << 'PYEOF'
import json
from pathlib import Path

target = {"x": 511, "y": 73, "z": 728}
identity = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]

def make_frame(frame_id, tick, ts):
    return {
        "spatial_frame_id": frame_id,
        "world_tick": tick,
        "monotonic_timestamp_ms": ts,
        "telemetry_session_id": None,
        "viewport": {"width": 800, "height": 600},
        "view_matrix": identity,
        "projection_matrix": identity,
        "player_pose": {"eye_position": {"x": 0.0, "y": 0.0, "z": 0.0}, "yaw": 0.0, "pitch": 0.0},
        "raycast_hit": {"block_pos": target, "face": "north", "block_id": "minecraft:oak_button"},
        "nearby_blocks": [],
        "nearby_entities": [],
        "inventory_summary": [],
        "screenshot_artifact_ref": None,
        "mc_capture_skew_ms": None,
        "screen_state": "in_game",
        "resource_pack_ids": [],
    }

pre = make_frame("frame-1", 1, 100)
post = make_frame("frame-2", 2, 150)
witness_dir = Path(".tmp/mc20-d2-1-live/witness")
for name, frame in [("pre.jsonl", pre), ("post.jsonl", post)]:
    (witness_dir / name).write_text(json.dumps(frame) + "\n")
    print(f"Wrote {witness_dir / name}")
PYEOF
```

Honesty: synthetic tick-advance witness proves **world-diff witness plumbing**
(`inconclusive`), not Minecraft gameplay success.

## Recorded runs (2026-06-30 local pass)

Store root: `.tmp/mc20-d2-1-live/store`

**Inspect note (historical D2.1 runs):** recorded before D2.2. Excerpts used
`auv inspect <run-id>` against copied runs in `.auv/runs/`. After D2.2, prefer
`auv inspect <run-id> --store-root .tmp/mc20-d2-1-live/store` for direct read.

### G2 — answer_non_clickable (`run_1782726278209_6291_0`)

```sh
cargo run --quiet -- minecraft query-wired-live-click \
  --training-result-semantic-manifest .tmp/mc18-live/setup/semantic.json \
  --target-block 511,73,728 \
  --target-face north \
  --target-semantics hit_face_center \
  --query-provider closed-scene-toy \
  --closed-scene-fixture crates/auv-game-minecraft/tests/fixtures/mc18/outside_window.json \
  --output-dir .tmp/mc20-d2-1-live/answer-non-clickable/query-output \
  --target-app com.todesktop.230313mzl4w4u92 \
  --target-title Cursor \
  --store-root .tmp/mc20-d2-1-live/store
```

Stdout:

```text
runId: run_1782726278209_6291_0
queryStatus: answered
wiringAttempted: false
actionEligibility: answer_non_clickable
operationResultArtifact: artifact_0003
```

Inspect:

```text
MC-19 Query Wired Live Action:
- operation_result_artifact=artifact_0003 query_artifact=artifact_0001 attempted=false action_eligibility=answer_non_clickable window_point=n/a refusal_reason=visibility=outside_window operation_status=completed operation_message=visibility=outside_window dispatch_command=n/a dispatch_outcome=n/a target_app=com.todesktop.230313mzl4w4u92 target_title=Cursor mc14_action_eligibility=answer_non_clickable readiness_class=non_actionable source_readiness_ref=kind=query_manifest artifact_id=artifact_0001 run_id=run_1782726278209_6291_0 verification_outcome=not_attempted verification_source=kind=layer1_no_dispatch verification_reason=visibility=outside_window issue=n/a
```

### G3 — not_consumable (`run_1782726355246_6795_0`)

```sh
cargo run --quiet -- minecraft query-wired-live-click \
  --training-result-semantic-manifest .tmp/mc18-live/setup/semantic.json \
  --target-block 9,9,9 \
  --target-semantics hit_face_center \
  --query-provider closed-scene-toy \
  --closed-scene-fixture crates/auv-game-minecraft/tests/fixtures/mc18/visible.json \
  --output-dir .tmp/mc20-d2-1-live/not-consumable/query-output \
  --target-app com.todesktop.230313mzl4w4u92 \
  --target-title Cursor \
  --store-root .tmp/mc20-d2-1-live/store
```

Stdout:

```text
runId: run_1782726355246_6795_0
queryStatus: blocked
wiringAttempted: false
actionEligibility: not_consumable
operationResultArtifact: artifact_0003
```

Inspect:

```text
MC-19 Query Wired Live Action:
- operation_result_artifact=artifact_0003 query_artifact=artifact_0001 attempted=false action_eligibility=not_consumable window_point=n/a refusal_reason=status=blocked reason=target_block_absent_from_scene_packet operation_status=completed operation_message=status=blocked reason=target_block_absent_from_scene_packet dispatch_command=n/a dispatch_outcome=n/a target_app=com.todesktop.230313mzl4w4u92 target_title=Cursor mc14_action_eligibility=not_consumable readiness_class=not_consumable source_readiness_ref=kind=query_manifest artifact_id=artifact_0001 run_id=run_1782726355246_6795_0 verification_outcome=not_attempted verification_source=kind=layer1_no_dispatch verification_reason=status=blocked reason=target_block_absent_from_scene_packet issue=n/a
```

### G4 — click_ready, no witness → unreliable (`run_1782726447885_7262_0`)

```sh
cargo run --quiet -- minecraft query-wired-live-click \
  --training-result-semantic-manifest .tmp/mc18-live/setup/semantic.json \
  --target-block 511,73,728 \
  --target-face north \
  --target-semantics hit_face_center \
  --query-provider closed-scene-toy \
  --closed-scene-fixture crates/auv-game-minecraft/tests/fixtures/mc18/visible.json \
  --output-dir .tmp/mc20-d2-1-live/click-ready/query-output \
  --target-app com.todesktop.230313mzl4w4u92 \
  --target-title Cursor \
  --store-root .tmp/mc20-d2-1-live/store
```

Stdout (D2.1 capture; D2.2+ also prints `inspectHint` with `--store-root`):

```text
runId: run_1782726447885_7262_0
queryStatus: answered
wiringAttempted: true
actionEligibility: click_ready
operationResultArtifact: artifact_0004
inspectHint: run `auv inspect run_1782726447885_7262_0 --store-root .tmp/mc20-d2-1-live/store` to view verification_outcome
```

Dispatch: `command.resolved` → `input.clickWindowPoint`, `dispatch_outcome=resolved`
(local pass; window resolve succeeded on this machine).

`operation-result` known_limits includes:
`mc20_v1_query_wired_witness_absent_post_action_semantic_verification_unreliable`

Inspect:

```text
MC-19 Query Wired Live Action:
- operation_result_artifact=artifact_0004 query_artifact=artifact_0001 attempted=true action_eligibility=click_ready window_point=640,360 refusal_reason=n/a operation_status=completed operation_message=clicked window point dispatch_command=input.clickWindowPoint dispatch_outcome=resolved target_app=com.todesktop.230313mzl4w4u92 target_title=Cursor mc14_action_eligibility=click_ready readiness_class=ready source_readiness_ref=kind=query_manifest artifact_id=artifact_0001 run_id=run_1782726447885_7262_0 verification_outcome=unreliable verification_source=kind=operation_result artifact_id=artifact_0004 run_id=run_1782726447885_7262_0 verification_reason=verification_unreliable issue=n/a
```

### G5 — click + witness → inconclusive (`run_1782726624570_10870_0`)

```sh
cargo run --quiet -- minecraft query-wired-live-click \
  --training-result-semantic-manifest .tmp/mc18-live/setup/semantic.json \
  --target-block 511,73,728 \
  --target-face north \
  --target-semantics hit_face_center \
  --query-provider closed-scene-toy \
  --closed-scene-fixture crates/auv-game-minecraft/tests/fixtures/mc18/visible.json \
  --output-dir .tmp/mc20-d2-1-live/click-witness/query-output \
  --target-app com.todesktop.230313mzl4w4u92 \
  --target-title Cursor \
  --sample .tmp/mc20-d2-1-live/witness/pre.jsonl \
  --post-sample .tmp/mc20-d2-1-live/witness/post.jsonl \
  --store-root .tmp/mc20-d2-1-live/store
```

Stdout (D2.1 capture; D2.2+ echoes `--store-root` in hint):

```text
runId: run_1782726624570_10870_0
queryStatus: answered
wiringAttempted: true
actionEligibility: click_ready
operationResultArtifact: artifact_0006
inspectHint: run `auv inspect run_1782726624570_10870_0 --store-root .tmp/mc20-d2-1-live/store` to view verification_outcome
```

`operation-result.verifications`: non-empty (1 entry, `state_changed=true`,
`observed_label=minecraft:oak_button`, evidence refs to pre/post witness
artifacts).

Inspect:

```text
MC-19 Query Wired Live Action:
- operation_result_artifact=artifact_0006 query_artifact=artifact_0001 attempted=true action_eligibility=click_ready window_point=640,360 refusal_reason=n/a operation_status=completed operation_message=clicked window point dispatch_command=input.clickWindowPoint dispatch_outcome=resolved target_app=com.todesktop.230313mzl4w4u92 target_title=Cursor mc14_action_eligibility=click_ready readiness_class=ready source_readiness_ref=kind=query_manifest artifact_id=artifact_0001 run_id=run_1782726624570_10870_0 verification_outcome=inconclusive verification_source=kind=operation_result artifact_id=artifact_0006 run_id=run_1782726624570_10870_0 verification_reason=minecraft:oak_button issue=n/a
```

## Verdict

| Gate | Run ID | Expected wiring | Expected `verification_outcome` | Inspect match | Pass |
| --- | --- | --- | --- | --- | --- |
| G0 | — | — | — | — | yes |
| G1 | — | parse exit 1 | — | stderr match | yes |
| G2 | `run_1782726278209_6291_0` | refused + outside_window | `not_attempted` | yes | yes |
| G3 | `run_1782726355246_6795_0` | refused + absent target | `not_attempted` | yes | yes |
| G4 | `run_1782726447885_7262_0` | attempted + click_ready | `unreliable` | yes | yes |
| G5 | `run_1782726624570_10870_0` | attempted + witness path | `inconclusive` | yes | yes |

**D2.1 closed** for canonical CLI live closure on macOS (2026-06-30).

## Honest limits

- G4/G5 dispatch outcome may be `resolved` or `failed` depending on local
  window title resolution; both count as honest non-stub attempts when
  `input.clickWindowPoint` span is present (MC-19 D4 parity).
- G5 uses **synthetic** pre/post jsonl (tick advance only) — world-diff witness
  honesty, not gameplay verification.
- `passed` / `failed` / `absent` live matrix not exercised in D2.1 (owner
  selected `unreliable` + `inconclusive` only).
- Inspect server write warnings (502) observed during runs; local store and
  inspect text projection unaffected.
- Failed G5 attempt `run_1782726540280_7849_0` (malformed witness matrices)
  superseded by corrected witness prep; not counted in verdict.

## Related

- MC-20 D2 CLI design:
  [`2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md`](2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md)
- MC-20 D1 verification design:
  [`2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md`](2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md)
- MC-19 wiring closure:
  [`2026-06-27-minecraft-mc19-query-to-live-click-wiring-live-closure.md`](2026-06-27-minecraft-mc19-query-to-live-click-wiring-live-closure.md)
- MC-18 provider closure:
  [`2026-06-27-minecraft-mc18-closed-scene-toy-provider-live-closure.md`](2026-06-27-minecraft-mc18-closed-scene-toy-provider-live-closure.md)
