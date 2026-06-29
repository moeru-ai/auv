# Minecraft MC-20 D3: Query-wired Layer-3 passed/failed semantic closure

Date: 2026-06-30

Status: **D3 implemented** — producer chain wires `expected_item_id` through
`QueryWiredPostActionWitness` so live `query-wired-live-click` can emit honest
Layer-3 `VerificationResult` claims with `semantic_matched: Some(true/false)` and
read-side `verification_outcome` `passed` / `failed`. Synthetic witness fixtures
close G6/G7; no `run_read` mapper changes.

## One-line summary

D2.1 closed `not_attempted` / `unreliable` / `inconclusive` (G0–G5). D3 closes
**`passed` / `failed`** by threading `--verification-expected-item-id` from CLI
through glue into `verify_query_wired_live_action_semantic`, reusing
`evaluate_world_diff` + `with_expected_item_id` semantics already proven in
`auv-game-minecraft`.

## Cross-references

| Slice | Doc |
| --- | --- |
| D1 post-action verification seam | [`2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md`](2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md) |
| D2 canonical CLI entry | [`2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md`](2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md) |
| D2.1 G0–G5 live closure | [`2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md`](2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md) |
| D3 G6/G7 live closure | [`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md) |

## Owner boundary

| In scope | Out of scope |
| --- | --- |
| Producer: `expected_item_id` → `evaluate_world_diff` → real `VerificationResult` | `run_read` / inspect mapper edits |
| CLI `--verification-expected-item-id` + library input pass-through | Core-C3 D2 read-side vocabulary patches |
| G6 `passed` / G7 `failed` synthetic witness closure | MC-20 controller / planner |
| Domain + glue + integration regression tests | Real Minecraft break/harvest gameplay (D3.1 candidate) |

## Domain contract (`auv-game-minecraft`)

```rust
pub struct QueryWiredPostActionWitness {
  pub target_block: BlockPosition,
  pub pre_frame: MinecraftSpatialFrame,
  pub post_frame: MinecraftSpatialFrame,
  pub expected_item_id: Option<String>,
}

pub fn verify_query_wired_live_action_semantic(
  witness: &QueryWiredPostActionWitness,
) -> WorldDiffVerdict;
```

Behavior:

- `expected_item_id: None` → **identical to D1**:
  `WorldDiffRequest::new(target).allow_same_block_state_change()` only;
  `semantic_matched` stays `None` for tick-advance / block-removal witnesses.
- `expected_item_id: Some(item)` →
  `WorldDiffRequest::new(target).allow_same_block_state_change().with_expected_item_id(item)`;
  delegates inventory delta semantics to `evaluate_world_diff`.

**Discipline:** when `expected_item_id` is set, tick-advance-only witnesses (G5
shape) resolve to `semantic_matched: Some(false)` → `failed`, not `inconclusive`.
D3 synthetic witnesses must be **block removal + inventory delta** shaped (see
live closure doc).

## Glue contract (`auv-cli`)

```rust
pub struct QueryWiredPostActionVerificationInput<'a> {
  // ...existing fields...
  pub verification_expected_item_id: Option<String>,
}

pub struct QueryWiredLiveActionInputs {
  // ...existing fields...
  pub verification_expected_item_id: Option<String>,
}
```

Parse validation (CLI):

- `--verification-expected-item-id <minecraft:item_id>` requires `--sample`
  (same discipline as `--post-sample requires --sample`).

## Producer branch table (extends D1)

| Condition | `operation_result.verifications` | read-side `verification_outcome` |
| --- | --- | --- |
| *(D1 rows unchanged through G5 `inconclusive`)* | | |
| `attempted=true`, dispatch succeeded, witness, `expected_item_id` set, block removed + inventory delta matches | `semantic_matched: Some(true)`, `state_changed: true` | **`passed`** (G6) |
| `attempted=true`, dispatch succeeded, witness, `expected_item_id` set, block removed + inventory flat/missing | `failure_layer: StateChangedNoMatch`, `semantic_matched: Some(false)` | **`failed`** (G7) |
| `attempted=true`, dispatch succeeded, witness, `expected_item_id` set, tick-advance only | `semantic_matched: Some(false)` (no inventory rise) | **`failed`** (not G5 `inconclusive`) |

Read-side [`project_verification_outcome_from_claims`](../../src/run_read.rs) already
maps `passed`/`failed`; D3 only supplies producer evidence.

## Dependency direction

```text
CLI --verification-expected-item-id
  → QueryWiredLiveActionInputs.verification_expected_item_id
    → QueryWiredPostActionVerificationInput
      → QueryWiredPostActionWitness.expected_item_id
        → verify_query_wired_live_action_semantic
          → evaluate_world_diff
            → VerificationResult on operation-result
              → run_read projection (unchanged)
```

## Explicit non-goals

- `run_read.rs` mapper changes
- MC-20 controller reopen
- Faking `passed` in summary without `VerificationResult` support
- Real gameplay telemetry for D3 (owner chose `synthetic_first`)

## Verification commands

```bash
cargo fmt --check
cargo check
cargo test query_wired_live_action_semantic
cargo test parse_minecraft_query_wired_live_click
cargo test verify_query_wired_live_action_semantic
git diff --check
```
