# AUV Core S / Surface Memory — Lane Discipline

**Date:** 2026-07-05  
**Lane:** observation / memory / coverage / temporal continuity (S line)

## Independence rule

S/Surface Memory work **must not** ride L8/L9 action seam slices:

| Lane | Owns |
|------|------|
| L8/L9 | `ActionResolver` decision pair, `ActionTransitionLineage`, inspect viewer seam tension |
| S | view-memory persistence, reacquire, coverage, observation snapshots |

## Forbidden in L8/L9 PRs

- Expanding view-memory schema “while here”
- Coupling action seam producer changes to playlist/view-parser proof
- Using action transition lineage fields as memory write triggers without owner slice

## Allowed cross-reads

- Inspect viewer may show **both** view-parser proof and action transition panels on the same run (read-only, separate projections)
- Shared run storage / artifact roles — no shared producer mutation

## Entry

Owner opens dedicated S slice with reference doc under `docs/ai/references/`; gate unrelated to [App Command Pack](2026-07-05-auv-core-app-command-pack-gate.md) unless owner explicitly links them.
