# Minecraft MC-20 D1: Query-wired post-action semantic verification

Date: 2026-06-30

Status: **D1 implemented; D1.1 hardening landed** — closes minimal Layer 3 post-action
semantic verification on the MC-19 `query-wired live click` chain. MC-20 orchestration/controller
lane remains **paused** after this slice.

## One-line summary

After MC-19 dispatches a `click_ready` live click, MC-20 D1 appends **one honest world-diff
post-action verification**, records a real `VerificationResult` on the existing
`operation-result` artifact, and lets Core-C3 D2 read-side projection surface
`passed` / `failed` / `unreliable` / `inconclusive` without mapper changes.

## Owner boundary (this slice)

| In scope | Out of scope |
| --- | --- |
| Minecraft `query-wired live click` only | osu wired symmetry |
| Layer 3 post-action semantic verification | Core-C3/C2 vocabulary changes |
| `query → readiness → admission → dispatch → verification` closure | Core-D lease / planner / controller / SceneState |
| Library + example handoff | `main.rs` new CLI subcommand |
| Reuse `evaluate_world_diff` + existing read-side projection | `trait PostActionVerifier` / provider registry |
| Glue-layer orchestration **after** wiring | Verification inside `wire_query_manifest_to_action` |
| | Core-B runtime changes |
| | `run_read` mapper edits |

## Gap closed

```text
MC-12 query → MC-14 readiness → MC-19 wire → clickWindowPoint
  → operation-result (verifications were Vec::new())
  → read-side verification_outcome=absent
```

MC-20 D1 fills the dashed edge with a single verifier seam and producer branch table below.

## Unique verifier seam (domain — `auv-game-minecraft`)

```rust
pub const MC20_V1_QUERY_WIRED_WITNESS_ABSENT_KNOWN_LIMIT: &str =
  "mc20_v1_query_wired_witness_absent_post_action_semantic_verification_unreliable";

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

- When `expected_item_id` is `None`, calls `evaluate_world_diff` with
  `WorldDiffRequest::new(target).allow_same_block_state_change()` only (D1 behavior).
- When `expected_item_id` is `Some`, also calls `with_expected_item_id` (D3 — see
  [`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md)).
- **No** second verifier trait, registry, or planner hook.

## Glue mapping seam (`auv-cli`)

```rust
pub fn map_world_diff_verdict_to_verification_result(
  verdict: &WorldDiffVerdict,
  evidence: Vec<ArtifactRef>,
) -> VerificationResult;
```

- Extracted from `main.rs` `build_minecraft_world_diff_verification` into
  `src/minecraft_verification.rs`; shared by `live_click` CLI and MC-20 glue.

## Witness input contract (`telemetry_optional`)

```rust
pub struct QueryWiredLiveActionTelemetryWitness {
  pub pre_telemetry_sample: PathBuf,
  pub post_telemetry_sample: Option<PathBuf>, // default = pre path (live_click shape)
}

// QueryWiredLiveActionInputs.telemetry_witness: Option<QueryWiredLiveActionTelemetryWitness>
```

| Witness | Behavior |
| --- | --- |
| `None` | `attempted=true` → one `VerificationUnreliable` claim + `MC20_V1_…_witness_absent` limit |
| `Some` | Read pre frame **before** wiring; read post frame **after** wiring; world-diff verdict → `VerificationResult`; stage pre/post spatial-frame artifacts as evidence. If post read/staging fails, still stage `operation-result` with one `VerificationUnreliable` claim and `observed_label` reason. |

## Producer branch table (implementation contract)

| Condition | `operation_result.verifications` | read-side `verification_outcome` |
| --- | --- | --- |
| `attempted=false` | empty | `not_attempted` (unchanged) |
| `attempted=true`, dispatch failed (`click_summary` absent) | empty | `absent` (MC-19 D4 limit may remain) |
| `attempted=true`, dispatch succeeded, no witness | 1× `VerificationUnreliable` | `unreliable` |
| `attempted=true`, dispatch succeeded, witness capture/world-diff staging failed | 1× `VerificationUnreliable` + `observed_label` | `unreliable` |
| `attempted=true`, dispatch succeeded, witness, `semantic_matched: Some(true)` | 1× `SemanticMatch` | `passed` |
| `attempted=true`, dispatch succeeded, witness, semantic failure | `semantic_mismatch` / `state_changed_no_match` | `failed` |
| `attempted=true`, dispatch succeeded, witness, tick advance / block removal without `expected_item_id` | `state_changed=true`, `semantic_matched: None` | `inconclusive` |
| `attempted=true`, dispatch succeeded, witness, `expected_item_id` set, block removed + inventory match (D3 G6) | `semantic_matched: Some(true)` | `passed` |
| `attempted=true`, dispatch succeeded, witness, `expected_item_id` set, block removed + inventory flat (D3 G7) | `failure_layer: StateChangedNoMatch`, `semantic_matched: Some(false)` | `failed` |

Discipline:

- **`dispatch_outcome=failed` does not map to verification failed** (Core-C1 unchanged).
- **Post-action verification runs only when dispatch succeeded** (`click_summary` present).
- When verification runs (`attempted=true` and verifications non-empty), **remove**
  `MC19_V1_D4_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT` from `operation_result.known_limits`.
- No-witness unreliable path uses **`MC20_V1_QUERY_WIRED_WITNESS_ABSENT_KNOWN_LIMIT`** instead of
  MC-19 D4.
- Without `expected_item_id`, world-diff does **not** assert `semantic_matched: Some(true)`.
  D3 (closed 2026-06-30) adds optional `expected_item_id` via CLI
  `--verification-expected-item-id` for G6/G7 synthetic closure.
- Same-coordinate **block_id replacement without tick advance** is not detected; read-side may
  still show `absent` for unmappable claims — document as honest world-diff boundary.

## Glue orchestration flow

```text
query + stage manifest
→ (optional) read pre_frame from telemetry witness
→ wiring = wire_spatial_query_manifest_to_action(...)
→ if attempted && dispatch succeeded:
      build verifications per branch table
      stage pre/post spatial-frame artifacts → evidence refs (witness path)
→ build_query_wired_live_action_operation_result(..., verifications, witness_present)
→ stage operation-result
```

Verification stays in glue **after** wiring; `wire_query_manifest_to_action` admission semantics
are unchanged.

Glue entry: `build_query_wired_post_action_verifications` in `src/minecraft_verification.rs`.
Verification target block comes from **query manifest** `target_block` (cross-checked against input).

## Dependency direction

```text
auv-game-minecraft::verify (domain verdict)
  → auv-cli::minecraft_verification (VerificationResult + artifact staging)
    → operation-result artifact
      → run_read (existing Core-C3 D2 projection, read-only)
        → inspect / viewer
```

`auv-game-minecraft` must not depend on `auv-cli::contract`.

## Explicit non-goals

| Item | Reason |
| --- | --- |
| osu wired verification symmetry | Separate vertical slice |
| MC-20 controller / planner / action lease | Paused orchestration lane |
| Core-C3 `run_read` mapper changes | D2 projection already sufficient |
| Core-B runtime | Owner deferral |
| `trait PostActionVerifier` / registry | Avoid parallel verification frameworks |
| `main.rs` MC-20 CLI subcommand | D1 = library + example only |
| Gameplay/trainer quality beyond world-diff witness | Honest scope cap |

## Paused after D1 — reopen triggers (observation only)

- Wire MC-20 verification into `minecraft live-click` CLI entry without duplicating glue
- osu query-wired symmetric verification slice
- Generic post-action verifier trait **only** after two verticals share one seam
- MC-20 controller/orchestration lane (explicit owner slice)

Do not auto-open any of the above from D1 landing.

## Verification commands

```bash
cargo fmt --check
cargo check
cargo test -p auv-cli --lib
cargo test -p auv-game-minecraft
git diff --check
```

## D3 follow-on (closed)

MC-20 D3 closed Layer-3 `passed`/`failed` producer evidence — see
[`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md)
and live closure
[`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md).
