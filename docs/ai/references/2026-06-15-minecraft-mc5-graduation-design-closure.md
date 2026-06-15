# 2026-06-15 Minecraft MC-5 graduation design closure

Status: docs-only design closure for the owner-approved MC-5 graduation note. This does **not** mean MC-5 is implemented, fully graduated, or complete. It only closes the design question for what may become eligible for later graduation from the Minecraft vertical into AUV core, and what must remain vertical-only until live MC-1/MC-3/MC-4 evidence exists.

## Scope classification

`docs-only`

Why this classification is correct:
- `docs/ai/references/2026-06-14-auv-3d-minecraft-spatial-skill-p0.md` defines MC-5 as **graduation design**, not implementation.
- The current handoff says the next hard runtime blocker is still a real MC-1 sample plus screenshot binding, not core graduation.
- `CLAUDE.md` and `AGENTS.md` both require one focused slice, explicit owner approval, and no broadening from “design note” into adjacent runtime changes.

## Purpose

Record the current design boundary for the first Minecraft-driven graduation candidates:

- **G2** frame/action/target/verification correlation key
- **G3** same-instant timestamped capture binding
- **G4** source→screen projection basis + projected coordinate space

The goal is to define the **minimal common shape** that two consumers (osu + Minecraft) justify, while preserving the current AUV core seam:

```text
recognition / AX / candidates
  -> ActionResolver
  -> auv-driver InputActionResult
  -> OperationResult / VerificationResult / trace artifacts
```

This note is intentionally narrow: it documents current eligibility constraints for what *may* graduate later and what *must not* graduate yet. It does not authorize code changes by itself.

## Current repo truth

### Minecraft side

The current Minecraft vertical proves crate-local shapes only:
- `MinecraftSpatialFrame`
- `MinecraftProjectedPoint`
- `MinecraftProjectionArtifact`
- `WorldDiffVerdict`
- `MismatchRefusal`

But it still lacks the live blockers named in the handoff:
- real MC-1 telemetry sample
- screenshot ↔ telemetry binding proof
- real overlay-on-frame proof
- real input dispatch through `auv-driver`
- runtime/store/read-side integration of MC-3/MC-4 evidence

So Minecraft currently proves **vertical value and candidate shared concepts**, not a finished core contract.

### Existing core seam

The current reusable core seam already exists in stable form:
- upper action decision seam: `ActionResolverDecision`
- lower input delivery seam: `auv_driver::InputActionResult`
- persisted read-side seam: `OperationResult`, `VerificationResult`, `ObservationSnapshot`, `ArtifactRef`
- read-side extraction: `run_read::extract_verifications`, `extract_observation_snapshots`

The repository already forbids a third parallel action-result schema. MC-5 must extend attachment points around this seam rather than creating a new result family.

### Existing convergence precedent outside Minecraft

The detector-recognition and game-consumer design notes already establish two relevant rules:
- runtime-facing recognition stays on `RecognitionResult`, not a new detector-only contract
- downstream typed consumers should resolve typed handles from persisted lineage rather than teaching recipes/runtime glue a new schema

MC-5 should preserve the same convergence discipline if later implementation is approved.

## Exploration ledger

### Evidence inputs read for this closure

Primary MC docs:
- `docs/ai/references/2026-06-14-auv-3d-minecraft-spatial-skill-p0.md`
- `docs/ai/references/2026-06-15-minecraft-mc2-closure-mc3-handoff.md`

Core seam / contract:
- `CLAUDE.md`
- `AGENTS.md`
- `src/contract.rs`
- `src/runtime.rs`
- `src/run_read.rs`
- `src/action_resolver_decision.rs`
- `crates/auv-driver/src/input.rs`
- `crates/auv-driver/src/geometry.rs`

Vertical comparisons:
- `crates/auv-game-minecraft/src/types.rs`
- `crates/auv-game-minecraft/src/projection.rs`
- `crates/auv-game-minecraft/src/artifact.rs`
- `crates/auv-game-minecraft/src/input_target.rs`
- `crates/auv-game-minecraft/src/verify.rs`
- `crates/auv-game-osu/src/projection.rs`
- `crates/auv-inference-common/src/types.rs`

Related convergence notes:
- `docs/ai/references/2026-06-05-detector-manifest-recognitionresult-mapping-v0.md`
- `docs/ai/references/2026-06-10-game-recognition-recipe-consumer-seam.md`
- `docs/ai/references/2026-06-10-auv-tracing-driver-runtime-recording-split.md`

### Subagent lanes used

- G2 survey: correlation-key candidates across runtime / read-side / vertical evidence
- G3 survey: capture-binding candidates and screenshot/skew attachment points
- G4 survey: projection-basis overlap between osu and Minecraft
- contract-seam review: preserved core seam and no-third-schema boundary
- doc reviewer: classification and approved boundary
- risk reviewer: likely over-graduation failure modes

### Main-thread arbitration

Some subagent runs only returned startup chatter because of read-parameter errors. I used only results that were corroborated by direct file reads in the main thread. The conclusions below are based on verified repo state, not on unverified subagent summaries.

## Skill selection plan

For the future implementation slice, choose in this order:

1. **Field attachment before new types**
   - First ask whether the needed meaning can attach to existing `ArtifactRef`, `OperationResult`, `VerificationResult`, `ObservationSnapshot`, or trace artifacts.
   - Only propose a new core type if attachment would hide invariants or create ambiguous ownership.

2. **Producer/consumer proof before graduation**
   - Keep proving shapes in vertical crates first.
   - Graduate only after two consumers genuinely need the same invariant.

3. **Read-side-first evidence discipline**
   - Any graduated shape should remain inspectable from persisted run data.
   - Avoid abstractions that exist only in live memory or only inside one runtime call stack.

4. **Geometry semantics separate from action semantics**
   - G4 may graduate as projection/capture metadata.
   - It must not smuggle click policy, world-diff policy, or app-family semantics into core.

## Allowed / forbidden change map

### Allowed in a future MC-5 implementation slice

These are the only candidate categories that this closure judges eligible for later graduation:

#### G2 — correlation key
A minimal generic correlation record may later connect:
- source/basis frame id
- action/run/span/artifact lineage references
- optional verification linkage

The core purpose is not “Minecraft targeting”; it is auditable cross-step evidence stitching.

#### G3 — same-instant capture binding
A minimal generic binding may later express:
- bound capture artifact ref
- source/basis frame id (or equivalent source observation id)
- capture skew / timing delta
- enough provenance to tell whether a screenshot and a structured source were bound at the same instant

#### G4 — projection basis / projected coordinate context
A minimal generic basis may later express:
- basis id / basis frame id
- timestamp
- projected coordinate space kind
- confidence
- optional match radius / tolerance
- derivation family tag

The shared core value is “how did this source coordinate become an action-meaningful pixel/window point?”, not Minecraft math specifically.

### Forbidden to graduate

The following must remain vertical-only:
- block ids / faces / chunk coordinates
- entity taxonomy
- inventory semantics
- world-diff rules
- mismatch refusal reasons specific to Minecraft visibility/raycast policy
- telemetry wire format details
- GL timing caveats and matrix sourcing quirks
- first-target scenario policy
- menu/black/loading classifier policy
- osu circle-size / playfield-specific scaling policy
- app/game-specific task policy

### Forbidden architectural moves

- No third action-result schema beside `ActionResolverDecision` and `InputActionResult`
- No new top-level result object parallel to `OperationResult` / `VerificationResult`
- No core field that bakes in Minecraft nouns or archived AX product assumptions
- No graduation that depends on unproven live MC acceptance
- No runtime/CLI/store wiring justified only by crate-local verdicts/refusals

## G2 / G3 / G4 closure decisions

## G2 — frame/action/target/verification correlation key

### What is already reusable

The repo already has reusable identity/evidence anchors:
- `ArtifactRef { run_id, artifact_id, span_id, captured_event_id }`
- `CandidateRef`
- `VerificationResult.consumed_*`
- `OperationResult.evidence_artifacts`
- read-side lineage extraction in `src/run_read.rs`

This means G2 should **not** start as a brand-new “correlation framework.” It should start as a thin generic link layer that makes existing recorded facts composable.

### Minimal graduation candidate

A future G2 core shape should be able to answer:

```text
Which structured source/basis produced this projected target?
Which action consumed it?
Which verification/refusal evidence was attached afterward?
```

Minimum candidate fields:
- `basis_frame_id` or equivalent source-observation id
- `run_id`
- `span_id` or an equivalent operation-local correlation point
- optional action artifact ref
- optional verification artifact ref
- optional target artifact ref

This is intentionally smaller than a “workflow object.”

### Why it is not implemented now

Minecraft still has no live runtime/store/read-side integration for MC-3/MC-4 evidence, so any exact core field set would be partially guessed. The design closure is: **attach to existing artifact/run lineage, do not invent a new result family**.

## G3 — same-instant timestamped capture binding

### What is already reusable

Minecraft already proves the need for:
- `screenshot_artifact_ref`
- `mc_capture_skew_ms`
- `basis_frame_id`

Core already proves the right persistence surfaces:
- `ArtifactRef`
- `ObservationSnapshot.scope.capture_contract_ref`
- run/artifact persistence and read-side inspection

### Minimal graduation candidate

G3 may later graduate as the smallest record that says:

```text
this structured source observation and this capture artifact were bound closely enough in time to be action/verification evidence
```

Minimum candidate fields:
- bound capture artifact ref
- source/basis observation id
- monotonic timing delta / skew
- optional note / known-limit string when the binding is degraded

### What must stay vertical

- exact skew thresholds
- refusal categories driven by skew
- Minecraft-specific screenshot validity policy
- source-specific binding mechanics

### Closure

G3 is justified as a **binding fact**, not as a generic screenshot subsystem.

## G4 — source→screen projection basis + projected coordinate space

### What is already reusable

osu already proves one source→screen derivation family:
- `PlayfieldProjection`
- `ProjectionArtifact`
- derivation metadata and projected point/radius behavior

Minecraft proves a second family:
- `view_matrix` / `projection_matrix` driven projection
- `MinecraftProjectedPoint { screen_point, visibility, match_radius_px, basis_frame_id, confidence }`
- viewport/window-relative point handling

`auv-driver::geometry` already provides neutral endpoint geometry primitives:
- `Point`
- `ScreenPoint`
- `WindowPoint`
- `Rect`
- `CoordinateSpace`

`auv-inference-common` also already names a projection-basis idea, but only as `ProjectionBasis::Unavailable`; that is a useful precedent for keeping projection context explicit rather than hidden.

### Minimal graduation candidate

G4 should graduate only the common metadata, not either vertical’s math model.

Minimum candidate meaning:
- basis id / basis frame id
- timestamp
- projected coordinate space kind
- confidence
- optional match radius / tolerance
- derivation family tag

The core question is: *what basis justifies treating this projected point as action-grade evidence?*

### What must stay vertical

- matrix conventions, frustum rules, occlusion policy
- osu layout constants and circle-size formulas
- viewport-relative vs screen-relative correction policy until live MC binding is proven
- Minecraft `ProjectionVisibility` refusal semantics

### Closure

G4 is justified as a **projection provenance contract**, not as a shared projector implementation.

## Recommended future core shape direction

If the owner later approves implementation, prefer this graduation order:

1. **G3 first** — same-instant binding fact
   - smallest, most auditable, least semantic risk
2. **G2 second** — correlation key through existing artifact/run lineage
   - extends evidence stitching without inventing new result families
3. **G4 last** — projection provenance metadata only
   - highest ambiguity, because two consumers share the category but not the derivation math

This order minimizes the chance of over-graduating game semantics into core.

## Validation route for the future implementation slice

This closure itself is docs-only. No Cargo validation is required for this note.

For the later implementation slice, validation must include:
- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `git diff --check`

And it must additionally prove all of the following before claiming graduation success:

### G2 proof
- read-side lineage can resolve the new correlation shape from persisted run data
- no new parallel action-result schema appears

### G3 proof
- one producer and one consumer can record/read the binding fact from persisted artifacts
- degraded/absent binding remains explicit rather than silently accepted

### G4 proof
- two consumers share the graduated metadata without forcing one consumer’s derivation math onto the other
- geometry metadata remains separate from input delivery semantics

## Rejected alternatives

### Rejected: graduate `MinecraftSpatialFrame` directly
Rejected because it carries Minecraft-only nouns and live-binding uncertainty.

### Rejected: graduate `MinecraftProjectedPoint` directly
Rejected because `ProjectionVisibility`, window-relative assumptions, and MC-specific semantics are not yet generic enough.

### Rejected: invent a new “projection result” top-level core schema
Rejected because the repo already centers `ActionResolverDecision`, `InputActionResult`, `OperationResult`, `VerificationResult`, and trace artifacts.

### Rejected: treat offline crate-local MC-3/MC-4 as sufficient graduation proof
Rejected because the handoff still marks live telemetry sample, screenshot binding, and runtime/store/read-side integration as unresolved blockers.

### Rejected: widen core around archived AX copilot proof
Rejected because current roadmap evidence must come from active core runtime surfaces, not the archived vertical.

## Risk list

Ordered by severity:

1. **Critical** — introduce a third action-result schema
   - Prevention: extend existing artifact/verification attachment points instead.

2. **Critical** — graduate Minecraft nouns into core
   - Prevention: core names must remain app/game-agnostic.

3. **High** — confuse capture binding metadata with acceptance/refusal policy
   - Prevention: bind facts in core, keep refusal policy in verticals.

4. **High** — graduate projection metadata before live MC binding proof exists
   - Prevention: require persisted producer/consumer proof, not just crate-local math.

5. **Medium** — treat MC-5 as implementation approval for runtime/CLI wiring
   - Prevention: keep this note docs-only until a separate owner-approved implementation slice is named.

## Decision record

MC-5 is now closed at the **design** level with the following decisions:

- Classification: `docs-only`
- Allowed graduation candidates later: **G2, G3, G4 only**, and only as minimal common shape
- Graduation order if later approved: **G3 -> G2 -> G4**
- Core seam to preserve without exception:

```text
recognition / AX / candidates
  -> ActionResolver
  -> auv-driver InputActionResult
  -> OperationResult / VerificationResult / trace artifacts
```

- Forbidden: Minecraft nouns in core, archived-vertical product revival, third action-result schema, or runtime wiring justified only by offline crate-local logic

## Implementation follow-up note

A later narrow implementation slice has now landed the smallest approved G3 proof inside the existing candidate-action execution recording path:

- producer-side staging of a `g3-binding-fact` artifact
- emitted from `record_candidate_action_execution_artifact`
- attached alongside the existing `operation-result` and `candidate-action-execution` artifacts
- no new parallel action-result schema
- no new top-level result family
- no Minecraft-specific nouns in core contracts

This implementation does **not** mean MC-5 as a whole is "graduated" into core. It only proves that one minimal G3 binding fact can be recorded through an already-approved runtime seam.

What remains unchanged after this slice:
- G2 correlation-key graduation is still open
- G4 projection-provenance graduation is still open
- no shared core contract shape has been added yet for G3/G2/G4
- live MC-1 / MC-3 / MC-4 evidence gates remain unchanged

## What this closes

This note closes the question “what may graduate from the current Minecraft vertical into AUV core, and under what boundary?”

It does **not** close:
- live MC-1 telemetry proof
- live MC-3 input proof
- live MC-4 refusal proof
- the actual implementation of any graduation candidate beyond the narrow G3 binding-fact recording slice
