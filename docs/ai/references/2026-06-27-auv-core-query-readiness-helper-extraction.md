# 2026-06-27 AUV Core query readiness helper extraction

Date: 2026-06-27

Status: implemented helper-only extraction. This note records a narrow code move.
It does **not** graduate query status triad or action readiness view in the
proof matrix.

## Why this slice exists

Core-A graduation review froze the default action as defer extraction for query
status triad and action readiness view enum graduation. It did allow a future
**helper-only** slice when concrete repetition emerged across verticals:

- `docs/ai/references/2026-06-27-auv-core-a-query-readiness-graduation-review.md`

The osu! second-vertical consumption probe confirmed the same derived-action
eligibility triad and not-consumable refusal formatting in both Minecraft and
osu!. That repetition justified moving only the duplicate glue into a shared
helper without touching manifest derive logic or inspect/read surfaces.

## What changed

Added a new crate:

- `crates/auv-query-readiness`

Current scope of that crate is intentionally narrow:

- `DerivedActionEligibility` — `NotConsumable | AnswerNonClickable | ClickReady`
- `DerivedActionReadiness` — `{ eligibility, refusal_reason: Option<String> }`
- `DerivedActionReadiness::{not_consumable, answer_non_clickable, click_ready}`
- `format_query_not_consumable_refusal(status_label, reason_label)`

The helper owns only:

- the shared eligibility triad labels (`as_str`)
- the shared refusal-reason carrier shape for derived readiness
- the shared `status=… reason=…` not-consumable refusal formatter

It does **not** own manifest parsing, visibility keys, point geometry,
vertical-specific answer-non-clickable wording, or dispatch wiring.

**NOTICE:** `crates/auv-driver/src/readiness.rs` is window-probe readiness
(unrelated). The new crate name must not be read as driver dispatch readiness.

## Dependency diagram

```text
auv-query-readiness
  ├── DerivedActionEligibility / DerivedActionReadiness / format helper
  │
  ├── auv-game-minecraft
  │     training_result_spatial_query_action.rs
  │       type alias TrainingResultSpatialQueryActionEligibility
  │       derive_action_readiness (manifest + window_point logic stays local)
  │
  └── auv-game-osu
        visual_truth_spatial_query_action.rs
          type alias VisualTruthSpatialQueryActionEligibility
          derive_visual_truth_spatial_query_action_readiness (capture logic stays local)

auv-cli (unchanged)
  src/run_read.rs / src/inspect.rs
    still call vertical derive_* wrappers; no direct helper dependency
```

## Why this helper is admissible now

This extraction satisfies the helper-only bar from the graduation review:

- repeated in more than one owned vertical (Minecraft MC probe + osu probe)
- extraction removes glue duplication without creating a new domain contract
- helper names are derived-action readiness names, not donor manifest names

This is enough repetition to justify helper extraction, but **not** enough to
change the proof-matrix row verdict from `candidate, not admissible yet`.

## Deliberate non-goals

This slice intentionally does **not**:

- extract `TrainingResultSpatialQueryStatus` or `VisualTruthSpatialQueryStatus`
- extract shared `derive_*` from manifests
- move `window_point` or `pixel_point` into core
- add a generic trait or runtime readiness layer
- wire dispatch or live-click consumption
- change `src/run_read.rs` or `src/inspect.rs`
- add `auv-cli` dependency on `auv-query-readiness`

Those remain blocked by
[`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md).

## Touched files

- `Cargo.toml` — workspace member
- `crates/auv-query-readiness/` — new helper crate
- `crates/auv-game-minecraft/Cargo.toml` — dependency
- `crates/auv-game-minecraft/src/training_result_spatial_query_action.rs` — thin adapter
- `crates/auv-game-osu/Cargo.toml` — dependency
- `crates/auv-game-osu/src/visual_truth_spatial_query_action.rs` — thin adapter

## Behavior preserved on purpose

- All manifest-to-readiness branching stays in each vertical `derive_*` function.
- Vertical-specific refusal strings for answer-non-clickable paths stay local.
- Donor-facing type aliases preserve existing public symbol names.
- Inspect/read eligibility labels and refusal strings remain identical.

## Validation

Implemented and validated with:

```bash
cargo fmt --check
cargo check -p auv-query-readiness -p auv-game-minecraft -p auv-game-osu
cargo test -p auv-query-readiness
cargo test -p auv-game-minecraft training_result_spatial_query_action
cargo test -p auv-game-osu visual_truth_spatial_query_action
cargo test -p auv-cli osu_visual_truth
git diff --check
```

## Honest conclusion

This is a **shared helper extraction**, nothing more, and explicitly **not**
a contract graduation.

The repository now has a reusable derived-action eligibility helper serving
Minecraft and osu spatial-query consumption probes. Query status triad enum
graduation, dispatch wiring, and proof-matrix verdict changes remain deferred.
