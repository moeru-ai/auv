# AUV Scan S4 Lifecycle Evaluator v1 — Handoff

**Date:** 2026-07-03  
**Status:** landed

## API (`crates/auv-scan/src/lifecycle.rs`)

`evaluate_lifecycle`, `LifecycleEvent`, `LifecycleVerdict`, `TransitionEvidence`, `LifecycleError`

## Companion read-models

- `motion.rs` — `estimate_viewport_motion` (no durable wire)
- `association.rs` — `associate_adjacent_frames`
- `coverage.rs` — `build_coverage_view` (optional in-memory; negative evidence + completeness claim)

## Tests

43 total in `cargo test -p auv-scan` including 4 lifecycle evaluator tests.

## Non-goals

Durable anchor-track wire; live reacquire loop; `run_read` changes.
