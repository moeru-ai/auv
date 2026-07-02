# AUV Scan S5 Scene State Product — Design Charter

**Date:** 2026-07-03  
**Status:** charter — L2 read-model; no durable `scan-scene-state-v0` in S5a

## Role

S5 packages S1–S4 scan evidence into **product-consumable draft answers** to the S0 five questions. S5a outputs **typed draft answers**, not closed semantic truth.

## Contract layers

| Layer | Name | S5a |
|-------|------|-----|
| L0 | `scan-frame-v0` | read-only |
| L1 | motion / association / coverage / lifecycle read-models | compose inputs |
| **L2** | `SceneStateInput` + `SceneStateProduct` | **new crate contract** (memory + fixture JSON) |
| L3 | in-memory inspect projection | S6a (landed); charter旧称 S5b — see [S6a handoff](2026-07-03-auv-scan-s6a-scene-state-inspect-handoff.md); B-line/run_read bridge remains S6b candidate |

`SceneStateInput.observations_by_frame` is intentional: observations are not in `scan-frame-v0` wire yet.

## Five questions → draft answers (S5a)

| # | Question | S5a field | Strength |
|---|----------|-----------|----------|
| Q1 | Still present at as_of? | `latest_observation_present` + `as_of_frame_id` | conservative |
| Q2 | Same target? | `identity_assessment` | strong (S2b projection) |
| Q3 | Visible / stale? | `visibility_assessment` | conservative (`Visible` / `StaleCandidate` / `Unknown` only) |
| Q4 | Enough to act? | `action_readiness.blocking_codes` | conservative |
| Q5 | Next observation? | `recommended_observations` | rule table |

## Frame id semantics

- `SceneStateProduct.as_of_frame_id` — evaluation snapshot (`bundle.frames.last()`).
- `TrackSceneSummary.last_seen_frame_id` — last frame with a matching observation (backward scan).
- `latest_observation_present` = `(last_seen_frame_id == as_of_frame_id)`.

## Motion boundary

`SceneStateProduct.motion` is **supporting evidence only** in S5a. It must not drive visibility, presence, or readiness.
When fewer than two frames are available, S5a degrades motion to `MotionResult::Unknown` instead of failing the whole product.

## Blocking codes (split)

- `missing_observations` — empty / length mismatch / cannot establish track input.
- `lifecycle_missing_evidence` — `evaluate_lifecycle` → `MissingEvidence`.
- `lifecycle_incomplete` — lifecycle input was provided but has no terminally usable verdict (including empty stream).
- See handoff for full blocking table.

## Donor mapping

- Substrate `SceneStateProduct` — shape donor only.
- S4 charter — `tracked` = association; lifecycle separate field.
- `CoverageView` ≠ substrate `CoverageLedger` (lightweight S3 view).

## Non-goals (S5a)

- Durable `scan-scene-state-v0` wire
- B-line / `run_read` / viewer
- `OutOfViewport` / occlusion semantics
- NetEase / ViewMemory business semantics

## Related

- [S-line substrate](2026-07-03-s-line-streaming-observation-substrate.md)
- [S4 lifecycle charter](2026-07-03-auv-scan-s4-anchor-lifecycle-charter.md)
- [S5a handoff](2026-07-03-auv-scan-s5-scene-state-handoff.md)
