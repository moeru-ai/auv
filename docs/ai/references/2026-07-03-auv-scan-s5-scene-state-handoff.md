# AUV Scan S5 Scene State Product v1 — Handoff

**Date:** 2026-07-03  
**Status:** landed (S5a)

## API (`crates/auv-scan/src/scene_state.rs`)

- `SceneStateInput` — L2 input contract (`bundle`, `observations_by_frame`, optional `lifecycle_events`)
- `build_scene_state_product` — composes motion / association / coverage / lifecycle into draft answers
- `SceneStateProduct`, `TrackSceneSummary`, `SceneDraftAnswers`, `ActionReadiness`
- `summarize_scene_state_text` — metadata-only summary (no IO)

## Draft answers (not closed truth)

S5a outputs **typed draft answers** to S0 five questions. Q1/Q3 are conservative; Q2/Q4/Q5 use stronger projection or blocking tables.

## Frame id semantics

- `as_of_frame_id` — product snapshot frame (`bundle.frames.last()`)
- `last_seen_frame_id` — per-track backward scan over observations
- `latest_observation_present` = `(last_seen_frame_id == as_of_frame_id)`

## Motion boundary

`SceneStateProduct.motion` is **supporting evidence only** — does not drive visibility, presence, or readiness in S5a.
If fewer than two frames are present, S5a degrades motion to `MotionResult::Unknown` rather than failing the scene product.

## Blocking codes (split)

| Code | Meaning |
|------|---------|
| `missing_observations` | invalid / empty observation input |
| `lifecycle_missing_evidence` | lifecycle evaluator `MissingEvidence` |
| `lifecycle_incomplete` | lifecycle input present but empty / non-terminally usable |
| `ambiguous_association` | S2b ambiguity |
| `no_new_observation` | coverage negative evidence |
| `lifecycle_lost` / `lifecycle_observation_failed` | lifecycle terminals |

## Fixtures

`tests/fixtures/scan/scene/` — 6 scenarios: stable, stale, ambiguous, lost, missing_observations, lifecycle_bad_evidence

## Tests

`cargo test -p auv-scan` — 52 tests including 6 scene fixture tests.

## Non-goals

- Durable `scan-scene-state-v0` wire
- B-line / `run_read` / viewer (→ S5b)
- `OutOfViewport` / motion-driven visibility
- `CoverageView` ≠ substrate `CoverageLedger` (lightweight S3 view)

## Related

- [S5 charter](2026-07-03-auv-scan-s5-scene-state-charter.md)
- [S4 lifecycle evaluator handoff](2026-07-02-auv-scan-s4-lifecycle-evaluator-handoff.md)
