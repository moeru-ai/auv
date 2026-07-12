# Slay the Spire Observe-Only Recognition Fixture Boundary

Date: 2026-06-06

Status: docs/test-only boundary

Base: local worktree after detector-recognition read-side closure

## Purpose

Define the first game-specific vertical as an **observe-only** slice:

```text
Slay the Spire screenshot fixture
  -> synthetic/manual DetectionEvidenceManifest
  -> RecognitionResult
  -> detector-recognition artifact
  -> run_read / inspect / inspect_server lineage
```

This slice exists to prove that a game screenshot can enter AUV as structured
recognition evidence without reopening Candidate/action work.

It does **not** implement:

- `Candidate`
- click/action delivery
- Steam launcher abstraction
- autoplay or combat planning
- real YOLO training
- repeated-frame state tracking
- generic game runtime APIs

## Why Slay the Spire

Slay the Spire is the cleanest first game vertical for AUV's current phase:

- single-player and offline
- turn-based, so no real-time input/timing pressure
- stable 2D UI with clear cards / enemies / energy / buttons
- easier to reason about recognition evidence than darker, noisier, or networked games

The goal is **not** "make AUV play Slay the Spire". The goal is to prove:

```text
game screenshot structure
  -> RecognitionResult evidence
  -> readable lineage
```

## Boundary

Current allowed chain:

```text
fixture screenshot
  -> hand-authored or synthetic DetectionEvidenceManifest
  -> detector-backed RecognitionResult
  -> detector-recognition runtime artifact
  -> read-side lineage
```

Current forbidden chain:

```text
fixture screenshot
  -> Candidate
  -> click
  -> action
  -> game planner
```

The fixture may use manual boxes and labels. It does **not** need a trained
model yet. For this slice, the important property is that the evidence chain
is honest and readable.

## Fixture Scope

The first fixture may contain only a few coarse semantic regions:

- `card_region`
- `enemy_region`
- `energy_region`
- `end_turn_button_region`

These are recognition evidence labels, not action targets.

The fixture should prefer one stable screenshot and one deterministic manifest
over multiple partial examples. One clean frame is worth more than five vague
ones in this phase.

## Runtime Evidence Requirements

Any observe-only Slay the Spire fixture path must still satisfy the detector
recognition evidence rules already established elsewhere:

- `DetectionEvidenceManifest` remains inference-scoped
- `RecognitionResult` remains evidence-only
- `detector-recognition` artifact must carry runtime `capture-image` evidence
- read-side lineage must expose:
  - `status`
  - `source`
  - `backend`
  - `model_id`
  - `capture_artifact`
  - `evidence_artifacts`
  - `all_count`
  - `filtered_count`
  - `known_limits`

For a manual/synthetic fixture, the "backend" may remain a custom/manual value
so long as it is explicit and does not pretend to be a trained detector.

## Test Shape

The first test for this vertical should prove:

```text
synthetic/manual Slay the Spire fixture
  -> DetectionEvidenceManifest
  -> RecognitionResult
  -> detector-recognition runtime artifact
  -> run_read lineage = ready
```

If it also checks `inspect` text and `/runs` JSON, that is useful, but the
minimum proof is still runtime artifact recording plus read-side lineage.

## Non-Goals

This boundary does not approve:

- `auv-steam`
- `auv-game-slay-the-spire`
- game-state planner contracts
- detector-to-candidate promotion
- any clickability assumption from `end_turn_button_region`

Those belong to later slices, after observe-only evidence is stable.

## Follow-Up Order

The intended order after this boundary is:

```text
1. observe-only fixture evidence
2. RecognitionResult -> draft game state mapping
3. repeated-frame consistency
4. candidate promotion design
5. only then maybe action
```

That order is intentional. It prevents game vertical work from collapsing into
"we found a rectangle, so click it".
