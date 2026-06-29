# MC-20 D3: Layer-3 passed/failed live closure

Date: 2026-06-30

## Summary

MC-20 D3 closes **G6 `passed`** and **G7 `failed`** on the canonical CLI
`auv minecraft query-wired-live-click` using **synthetic block-removal +
inventory-delta witnesses** and `--verification-expected-item-id minecraft:stone`.
Layer-3 evidence lives on `operation-result.verifications` (not read-side mapper
hacks). Read-side `verification_outcome` is corroboration only.

Design reference:
[`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md)

Prior matrix (D2.1 G0–G5):
[`2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md`](2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md)

## Evidence matrix (G6–G7)

| Gate | ID | Witness shape | `--verification-expected-item-id` | Layer-3 `VerificationResult` | `verification_outcome` |
| --- | --- | --- | --- | --- | --- |
| Semantic pass | **G6** | block removed + inv 1→2 | `minecraft:stone` | `semantic_matched: true`, `state_changed: true` | `passed` |
| Semantic fail | **G7** | block removed + inv flat | `minecraft:stone` | `semantic_matched: false`, `failure_layer: state_changed_no_match` | `failed` |

G0 parse guards (including `--verification-expected-item-id requires --sample`) are
covered by `cargo test parse_minecraft_query_wired_live_click` (11/11 on 2026-06-30).

## Preconditions

- Reuse D2.1 preconditions: MC-18 semantic manifest, closed-scene fixture, target
  app/title, dedicated store `.tmp/mc20-d3-live/store`
- Target block: `511,73,728` (MC-18/20 chain)

## Witness prep (one-time)

```sh
mkdir -p .tmp/mc20-d3-live/witness

python3 << 'PYEOF'
import json
from pathlib import Path

target = {"x": 511, "y": 73, "z": 728}
identity = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]

def base_frame(frame_id, tick, ts):
    return {
        "spatial_frame_id": frame_id,
        "world_tick": tick,
        "monotonic_timestamp_ms": ts,
        "telemetry_session_id": None,
        "viewport": {"width": 800, "height": 600},
        "view_matrix": identity,
        "projection_matrix": identity,
        "player_pose": {"eye_position": {"x": 0.0, "y": 0.0, "z": 0.0}, "yaw": 0.0, "pitch": 0.0},
        "nearby_entities": [],
        "screenshot_artifact_ref": None,
        "mc_capture_skew_ms": None,
        "screen_state": "in_game",
        "resource_pack_ids": [],
    }

pre = base_frame("frame-1", 10, 1000)
pre["raycast_hit"] = {"block_pos": target, "face": "north", "block_id": "minecraft:stone"}
pre["nearby_blocks"] = [{"block_pos": target, "block_id": "minecraft:stone"}]
pre["inventory_summary"] = [{"item_id": "minecraft:stone", "count": 1}]

g6_post = base_frame("frame-2", 11, 1050)
g6_post["raycast_hit"] = None
g6_post["nearby_blocks"] = []
g6_post["inventory_summary"] = [{"item_id": "minecraft:stone", "count": 2}]

g7_post = base_frame("frame-2", 11, 1050)
g7_post["raycast_hit"] = None
g7_post["nearby_blocks"] = []
g7_post["inventory_summary"] = []

witness_dir = Path(".tmp/mc20-d3-live/witness")
for name, frame in [
    ("g6-pre.jsonl", pre), ("g6-post.jsonl", g6_post),
    ("g7-pre.jsonl", pre), ("g7-post.jsonl", g7_post),
]:
    (witness_dir / name).write_text(json.dumps(frame) + "\n")
PYEOF
```

**NOTICE:** Matrices must be flat `[f64; 16]` arrays (same discipline as D2.1 G5).

Honesty: synthetic witnesses prove **semantic assertion plumbing** (`passed`/`failed`),
not real Minecraft break/harvest gameplay.

## Canonical CLI — G6 (passed)

```sh
cargo run --quiet -- minecraft query-wired-live-click \
  --training-result-semantic-manifest .tmp/mc18-live/setup/semantic.json \
  --target-block 511,73,728 \
  --target-face north \
  --target-semantics hit_face_center \
  --query-provider closed-scene-toy \
  --closed-scene-fixture crates/auv-game-minecraft/tests/fixtures/mc18/visible.json \
  --output-dir .tmp/mc20-d3-live/g6-pass/query-output \
  --target-app com.todesktop.230313mzl4w4u92 \
  --target-title Cursor \
  --sample .tmp/mc20-d3-live/witness/g6-pre.jsonl \
  --post-sample .tmp/mc20-d3-live/witness/g6-post.jsonl \
  --verification-expected-item-id minecraft:stone \
  --store-root .tmp/mc20-d3-live/store
```

Recorded **runId:** `run_1782730403862_30193_0`

```sh
cargo run --quiet -- inspect run_1782730403862_30193_0 --store-root .tmp/mc20-d3-live/store
```

Inspect excerpt (Verifications + MC-19 summary):

```text
Verifications:
- method=semantic_match executed=true state_changed=true semantic_matched=true failure_layer=n/a evidence=2 observed_label=n/a
MC-19 Query Wired Live Action:
- ... verification_outcome=passed verification_source=kind=operation_result artifact_id=artifact_0006 run_id=run_1782730403862_30193_0 ...
```

`operation-result.verifications[0]` (Layer-3 producer evidence):

```json
{
  "method": {"kind": "semantic_match"},
  "executed": true,
  "state_changed": true,
  "semantic_matched": true,
  "failure_layer": null,
  "observed_label": null
}
```

## Canonical CLI — G7 (failed)

```sh
cargo run --quiet -- minecraft query-wired-live-click \
  --training-result-semantic-manifest .tmp/mc18-live/setup/semantic.json \
  --target-block 511,73,728 \
  --target-face north \
  --target-semantics hit_face_center \
  --query-provider closed-scene-toy \
  --closed-scene-fixture crates/auv-game-minecraft/tests/fixtures/mc18/visible.json \
  --output-dir .tmp/mc20-d3-live/g7-fail/query-output \
  --target-app com.todesktop.230313mzl4w4u92 \
  --target-title Cursor \
  --sample .tmp/mc20-d3-live/witness/g7-pre.jsonl \
  --post-sample .tmp/mc20-d3-live/witness/g7-post.jsonl \
  --verification-expected-item-id minecraft:stone \
  --store-root .tmp/mc20-d3-live/store
```

Recorded **runId:** `run_1782730530951_31089_0`

```sh
cargo run --quiet -- inspect run_1782730530951_31089_0 --store-root .tmp/mc20-d3-live/store
```

Inspect excerpt:

```text
Verifications:
- method=semantic_match executed=true state_changed=true semantic_matched=false failure_layer=state_changed_no_match evidence=2 observed_label=n/a
MC-19 Query Wired Live Action:
- ... verification_outcome=failed verification_reason=state_changed_no_match ...
```

`operation-result.verifications[0]`:

```json
{
  "method": {"kind": "semantic_match"},
  "executed": true,
  "state_changed": true,
  "semantic_matched": false,
  "failure_layer": "state_changed_no_match",
  "observed_label": null
}
```

## Honest limits / follow-ups

- Synthetic witness only; real gameplay telemetry is a **D3.1** candidate slice.
- `dispatch_outcome=failed` → `verification_outcome=absent` gate remains unchanged
  (not re-tested here).
- D3 does **not** edit `run_read.rs`; projection reuses Core-C3 D2 mapper.
- MC-20 controller / planner lane remains paused.
