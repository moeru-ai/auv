# 2026-06-15 Minecraft series handoff

Status: handoff-only summary for the current Minecraft lane. This note records what is already closed, what remains open, which bug-fix wording slices already landed, and what the next implementation cuts should be. It is not itself an approval to widen scope.

## Scope classification

`docs-only`

Why this classification is correct:
- This file summarizes verified repo state across MC-1 through MC-5.
- It does not add runtime behavior, contracts, or CLI surfaces.
- It exists to let the next slice start from a stable boundary instead of re-deriving recent work.

## One-line state

The Minecraft lane now has:
- crate-local MC-2 projection/overlay logic
- crate-local MC-3 world-diff verdict logic
- crate-local MC-4 mismatch-refusal logic
- a build-gated MC-1 telemetry mod tree
- a verified real running-client MC-1 durable telemetry sample, imported through the runtime artifact seam into a recorded run (sample is local-only / gitignored, not committed to the repo)
- a read-side-visible MC-3/MC-4 execution evidence closure in AUV core

What it still does **not** have is a live screenshot/telemetry binding (MC-2 screenshot anchor), or a full live MC-2/MC-3/MC-4 end-to-end proof. The MC-1 real-sample gap is now closed; the remaining live gaps are not.

## Source documents

Primary references for this handoff:
- `docs/ai/references/apps/minecraft/2026-06-14-3d-minecraft-spatial-skill.md`
- `docs/ai/references/apps/minecraft/2026-06-16-minecraft-probe-2-reference.md`
- `docs/ai/references/apps/minecraft/2026-06-18-minecraft-probe-5-reference.md`

Recent implementation commits relevant to the lane:
- `58d70c1 feat(game): add minecraft mc-2 projection crate`
- `2efb30c feat(auv-game-minecraft): add offline mc-3 verdict logic`
- `aa8b6f5 feat(auv-game-minecraft): add mc-4 mismatch refusal closure`
- `292bbbd feat(candidate-action): record mc-5 g3 binding fact artifact`
- `18c34f1 feat(read-side): surface candidate action evidence closure`

Recent wording / bug-fix commits that tightened boundary claims:
- `3606b6b docs(ai): clarify mc-5 design closure status`
- `f12140e docs(ai): tighten mc-2 to mc-4 closure wording`
- `37e3006 docs(ai): tighten mc-5 boundary wording`

## MC-1 — telemetry producer + durable sample

### What is already true

- A sidecar tree exists at `devtools/auv-game-minecraft/`.
- The lane already documents append-only JSONL as the intended first persistence shape.
- The MC-2/MC-3/MC-4 handoff records that the sidecar tree is buildable and includes a telemetry writer shape.
- A **real running-client telemetry sample now exists and was imported as a durable run artifact.** The dev client (`./gradlew runClient`) was run live and produced `devtools/auv-game-minecraft/run/auv/telemetry.jsonl`, which was imported through the committed runtime artifact seam (`2407d90 feat(runtime): record mc-1 telemetry sample artifact`) and persisted under the local run store. This closes the MC-1 "a real running-client sample exists as durable evidence" gap.

### MC-1 real-sample closure evidence

- **Source sample**: `devtools/auv-game-minecraft/run/auv/telemetry.jsonl` (produced by a live `runClient` session, which exited cleanly).
- **Import run id**: `run_1781537928075_75501_0`
- **Artifact id**: `artifact_0001`
- **Durable artifact path**: `.auv/runs/run_1781537928075_75501_0/artifacts/artifact_0001_telemetry.jsonl` — **local-only / gitignored** via `.gitignore` `.auv/`; this sample is durable on disk, not committed to the repo.
- **Disk-verifiable facts**: artifact size is `263180489` bytes (~263MB); the first JSONL record begins `{"spatial_frame_id":"frame-11-21297606287458","world_tick":11,"monotonic_timestamp_ms":21297606,...}`, so the sample carries the expected MC-1 `SpatialFrame` shape (`spatial_frame_id`, `world_tick`, `monotonic_timestamp_ms`). `git check-ignore` confirms the path is ignored and `git ls-files` confirms it is not tracked.
- **Caveat — read-side summary is NOT part of this closure**: a telemetry-aware `inspect` summary (line count, first/last tick, timestamp range) was prototyped locally but is **not** committed and is **not** owner-approved per the MC-1 boundary in `2026-06-14-auv-3d-minecraft-spatial-skill-p0.md` (which keeps the sidecar external and defers query-runtime/inspect surfacing). Treat the summary numbers as un-promoted local observations, not durable repo evidence.

### What is **not** yet true

- The durable sample is local-only; it is **not** committed to the repo (gitignored). MC-1 closure is "a real running-client sample exists as a durable run artifact," not "sample is checked into version control."
- This closure does **not** imply MC-2 screenshot/telemetry binding, nor any live MC-3 / MC-4 end-to-end proof. Those remain open.

### Current boundary

MC-1's real-sample / durable-evidence gap is **closed**. The remaining Minecraft live gaps (MC-2 screenshot binding, live MC-3/MC-4 end-to-end) are independent and are **not** discharged by this MC-1 closure.

### Most relevant files for the next MC-1 slice

- `devtools/auv-game-minecraft/`
- `src/run_builder.rs`
- `src/model.rs`
- `src/inspect.rs`
- `src/inspect_server/mod.rs`
- `src/mcp.rs`

## MC-2 — projection / screenshot / overlay evidence boundary

### What is already true

- `crates/auv-game-minecraft/` contains crate-local projection, artifact, and overlay logic.
- The current MC-2 handoff now clearly states that this is an offline geometry/projection artifact closure, not live end-to-end proof.
- The current projection contract already distinguishes what is implemented from what remains out of scope.

### What is **not** yet true

- No live screenshot/frame binding proof exists yet.
- No real overlay-on-frame proof has been recorded as durable evidence.
- No runtime/store/read-side bridge yet makes MC-2 projection evidence first-class in AUV core.

### Current boundary

MC-2 should next move by bridging projection/screenshot evidence into the existing artifact seam, not by expanding visual polish or inventing a new result family.

### Most relevant files for the next MC-2 slice

- `crates/auv-game-minecraft/src/projection.rs`
- `crates/auv-game-minecraft/src/artifact.rs`
- `crates/auv-game-minecraft/src/overlay.rs`
- `crates/auv-game-minecraft/src/input_target.rs`
- and, only if necessary for persistence/read-side visibility:
  - `src/candidate_action_decision.rs`
  - `src/run_read.rs`
  - `src/inspect.rs`

## MC-3 / MC-4 — runtime/store/read-side evidence closure

### What is already true

This lane now has a minimal AUV-core read-side closure for candidate action execution evidence.

Specifically, `18c34f1 feat(read-side): surface candidate action evidence closure` added:
- a derived `CandidateActionExecutionClosureState` in `src/run_read.rs`
- read-side extraction of closure state from existing execution artifacts
- inspect rendering of `closure_state` in `src/inspect.rs`

The read-side can now distinguish:
- `evidence_closed`
- `semantic_open`
- `blocked_by_readiness`

This means MC-3/MC-4 execution evidence is now:
- persisted through the existing runtime/store path
- extracted through the existing read-side lineage path
- visible in inspect without introducing a new action-result schema

### What is **not** yet true

- This is not yet a full live-client MC-3/MC-4 closure.
- The lane still lacks a live screenshot binding and live driver-on-client proof. (The MC-1 real-sample gap is now closed — see the MC-1 section — but that is independent of the MC-3/MC-4 live path.)
- MC-4 refusal categories are still not backed by a real-client refusal sample matrix recorded through live evidence.

### Current boundary

The closure that is now done is **read-side visibility of execution evidence**, not full live acceptance.
That distinction should remain explicit in future work.

### Files changed for the completed closure

- `src/run_read.rs`
- `src/inspect.rs`

### Validation that already passed

For the completed read-side closure slice, the following passed:
- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `git diff --check`

## MC-5 — graduation design boundary

### What is already true

- MC-5 remains a design-boundary note, not a whole-lane implementation closure.
- The repo now also contains one minimal G3 proof through the existing candidate-action artifact seam.
- No third action-result schema was introduced.
- No Minecraft-specific nouns were graduated into core contracts.

### What is **not** yet true

- G2 is still open.
- G4 is still open.
- No full graduation claim is justified yet.
- Live MC-3 / MC-4 evidence gates still remain. (The MC-1 real-sample gate is now closed and no longer blocks here.)

### Current boundary

Treat MC-5 as “design eligibility + one narrow G3 proof,” not as Minecraft graduation complete.

## Bug-fix history already closed

The following bug-fix slices are done and should not need to be re-opened unless a new contradiction is introduced:

1. `docs(ai): clarify mc-5 design closure status`
   - fixed wording that made MC-5 read too much like an implementation/graduation completion claim

2. `docs(ai): tighten mc-2 to mc-4 closure wording`
   - fixed wording that made crate-local/offline closures read too much like live end-to-end closure

3. `docs(ai): tighten mc-5 boundary wording`
   - removed remaining “final closure” tone from the MC-5 design note

These cleaned up the major P1 documentation misreads.
Remaining issues are now primarily implementation/evidence gaps, not wording bugs.

## Current recommended next order

If continuing implementation rather than more wording cleanup, the most sensible order is:

1. ~~**MC-1 real telemetry durable sample**~~ — **done.** A real running-client sample was flushed and imported as a durable run artifact via the committed runtime seam (local-only / gitignored). See the MC-1 section above.
2. **MC-2 screenshot / projection evidence bridge**
   - make projection/screenshot/overlay evidence persist through the current artifact seam
3. **MC-3 / MC-4 live-client promotion of the already-closed read-side path**
   - feed real client evidence into the closure path already exposed in inspect/read-side

## What to avoid next

Do **not** do these unless explicitly approved:
- widen MC work into a broad multi-slice refactor
- introduce a third action-result schema
- graduate Minecraft nouns into core
- treat the read-side closure as proof that live MC-3/MC-4 are now done
- spend the next slice polishing archived AX-like verticals instead of the active core runtime seam

## Fast restart checklist for the next agent

Before editing, re-read:
- `CLAUDE.md`
- `AGENTS.md`
- `docs/ai/references/apps/minecraft/2026-06-14-3d-minecraft-spatial-skill.md`
- `docs/ai/references/apps/minecraft/2026-06-16-minecraft-probe-2-reference.md`
- `docs/ai/references/apps/minecraft/2026-06-18-minecraft-probe-5-reference.md`

If continuing from the completed read-side closure, inspect these first:
- `src/run_read.rs`
- `src/inspect.rs`
- `src/candidate_action_decision.rs`
- `crates/auv-game-minecraft/src/projection.rs`
- `crates/auv-game-minecraft/src/artifact.rs`
- `devtools/auv-game-minecraft/`
