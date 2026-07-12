# Minecraft MC-14: Spatial Query Action-Facing Consumer Design

Date: 2026-06-27

## Purpose

MC-14 closes the **action-facing derived read model** for MC-12 spatial query
manifests. It answers whether action/candidate code can read, from an existing
query manifest lineage:

- whether the query is consumable at all
- whether the selected answer is honestly non-clickable
- whether a window-relative click point is available

MC-14 is a **consumer only**. It does **not** add persisted artifacts, CLI
commands, MC-12 schema changes, click dispatch, or `CandidatePromotion` /
`ActionResolver` runtime wiring.

## Definition

> **MC-14 = derived action-readiness view from MC-12 query manifest lineage.**

```text
minecraft-3dgs-training-result-query  (MC-12 persisted)
        │
        ▼ derive_action_readiness(manifest)
MC-14 action-readiness view             (derived only — never persisted)
        │
        ├── inspect text section
        ├── viewer derived fields on manifest card
        └── run_read helper return type
```

## Consumed artifact roles

Same as MC-12 / MC-13:

- `minecraft-3dgs-training-result-query`
- `minecraft-3dgs-training-result-query-inspect` (optional paired audit context)

No fourth artifact role is introduced.

## Eligibility model

Domain module: `crates/auv-game-minecraft/src/training_result_spatial_query_action.rs`

| Condition | `action_eligibility` | Notes |
|-----------|----------------------|-------|
| `status != answered` | `not_consumable` | blocked / failed, including absent negative control |
| `status == answered` + `visibility == visible` + `screen_point` | `click_ready` | via `input_target::projected_window_point` |
| `status == answered` + non-visible visibility | `answer_non_clickable` | e.g. `outside_window`, `behind_camera` |
| answered + visible but missing screen point | `answer_non_clickable` | honest non-click refusal |

Input uses the MC-12 manifest **selected answer** only.

## Surfaces closed

1. **`auv-game-minecraft`** — `derive_action_readiness` + unit tests
2. **`src/run_read.rs`** — `MinecraftTrainingResultSpatialQueryActionReadinessSummary` +
   `derive_minecraft_training_result_spatial_query_action_readiness`
3. **`src/inspect.rs`** — `MC-14 Training Result Spatial Query Action Readiness:` section
4. **`src/inspect_server_viewer.html`** — derived fields on existing MC-12 manifest card

Paired inspect reports still use MC-13 business-key pairing
(`spatial_query_manifest_matches_report`).

## Explicit non-goals

- New artifact role / persisted JSON / CLI command
- `list_/extract_` scan for a fourth artifact role
- MC-12 schema or producer changes
- candidate_promotion / ActionResolver runtime wiring (**MC-14+**)
- live-click-from-query
- Gaussian-native provider (**MC-15**)
- render inspect / quality gate (**MC-16**)

## Deferred slices

```text
MC-19   query-to-live-click minimal wiring (see design note below)
MC-16   render inspect / holdout preview consumer
```

MC-19 design:
`docs/ai/references/2026-06-27-minecraft-mc19-query-to-live-click-wiring-design.md`

## Related references

- MC-12 live closure:
  `docs/ai/references/2026-06-27-minecraft-mc12-spatial-query-live-closure.md`
- MC-13 read-side inspect design:
  `docs/ai/references/2026-06-27-minecraft-mc13-spatial-query-read-side-inspect-consumer-design.md`
- MC-14 live closure:
  `docs/ai/references/2026-06-27-minecraft-mc14-spatial-query-action-facing-live-closure.md`
