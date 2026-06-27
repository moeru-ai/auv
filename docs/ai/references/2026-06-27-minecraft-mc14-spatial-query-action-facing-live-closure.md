# MC-14 spatial query action-facing live closure

Date: 2026-06-27

## Summary

This note records derived action-readiness consumption from existing MC-12 spatial
query runs. MC-14 adds no new CLI and does not rerun spatial query; it only derives
`action_eligibility`, `window_point`, and `refusal_reason` from persisted MC-12
manifest lineage.

This is **derived read-side evidence closure** only. It does **not** claim click
dispatch, candidate runtime wiring, Gaussian-native inference, or render preview.

## Reused MC-12 live runs

From `docs/ai/references/2026-06-27-minecraft-mc12-spatial-query-live-closure.md`:

| Run | MC-12 scenario | MC-14 expected `action_eligibility` |
|-----|----------------|-------------------------------------|
| `run_1782543398186_14786_0` | visible / reference-only gate | `click_ready` |
| `run_1782543551237_21825_0` | provider stub `outside_window` | `answer_non_clickable` |
| `run_1782543409758_15819_0` | absent negative control (`failed`) | `not_consumable` |

## Inspect text gate

```sh
cargo run --quiet -- inspect run_1782543398186_14786_0
cargo run --quiet -- inspect run_1782543551237_21825_0
cargo run --quiet -- inspect run_1782543409758_15819_0
```

Expected MC-14 section on each run:

```text
MC-14 Training Result Spatial Query Action Readiness:
- query_artifact=... action_eligibility=click_ready window_point=... refusal_reason=n/a ...
- query_artifact=... action_eligibility=answer_non_clickable refusal_reason=visibility=outside_window ...
- query_artifact=... action_eligibility=not_consumable refusal_reason=status=failed reason=target_block_absent_from_scene_packet ...
```

## Viewer smoke (manual)

1. Start inspect server for a run containing MC-12 query artifacts.
2. Open the query manifest artifact card.
3. Confirm derived fields appear on the existing manifest card:
   `action_eligibility`, `window_point`, `refusal_reason`.
4. Confirm no new artifact role detector was added.

## Verdict

MC-14 is **live-closed** when the three MC-12 gate runs each render the expected
`action_eligibility` in `auv inspect` and the manifest viewer card shows the same
derived fields without introducing a fourth persisted artifact role.

## Related references

- MC-14 design:
  `docs/ai/references/2026-06-27-minecraft-mc14-spatial-query-action-facing-consumer-design.md`
- MC-12 live closure:
  `docs/ai/references/2026-06-27-minecraft-mc12-spatial-query-live-closure.md`
- MC-13 read-side live closure:
  `docs/ai/references/2026-06-27-minecraft-mc13-spatial-query-read-side-live-closure.md`
