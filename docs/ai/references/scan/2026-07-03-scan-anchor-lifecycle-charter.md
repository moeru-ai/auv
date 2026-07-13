# AUV Scan S4 Anchor Lifecycle — Design Charter

**Date:** 2026-07-03  
**Status:** charter — evidence-first; no durable `scan-anchor-track-v0` in v1

## Donor mapping

A-line `ReacquireOutcome` / freshness gate → S-line `LifecycleEvent` + `TransitionEvidence`. Do not copy NetEase sidebar semantics.

## Failure layers

`missing_evidence`, `observation_failed`, `ambiguous_association`, `ambiguous_reacquire`, `lost` — see [evaluator handoff](2026-07-03-scan-temporal-core-landed.md).

## Rules

- `tracked` = association result (S2b), not lifecycle state
- `reacquiring` not in durable model v1
- Substrate 7-state diagram = roadmap vocabulary only
