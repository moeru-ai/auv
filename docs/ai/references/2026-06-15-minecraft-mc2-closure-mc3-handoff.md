# 2026-06-15 Minecraft MC-2 closure, MC-3 closure, and MC-4 handoff

Status: MC-2 crate-local code slice landed and validated; a crate-local MC-3 offline logic closure is landed; a crate-local MC-4 mismatch-refusal closure is landed too. Live MC-3 / MC-4 are still blocked on a real MC-1 client sample, screenshot binding, and runtime wiring.

## What changed

This work now spans three Minecraft vertical slices plus the first real MC-1 build gate.
The new vertical crate `crates/auv-game-minecraft` exists, is wired into workspace
membership, and now carries crate-local projection, world-diff, and mismatch-refusal logic.

Local commits created:
- `58d70c1 feat(game): add minecraft mc-2 projection crate`
- `2efb30c feat(auv-game-minecraft): add offline mc-3 verdict logic`
- `aa8b6f5 feat(auv-game-minecraft): add mc-4 mismatch refusal closure`

### Files added during MC-2
- `crates/auv-game-minecraft/Cargo.toml`
- `crates/auv-game-minecraft/src/lib.rs`
- `crates/auv-game-minecraft/src/types.rs`
- `crates/auv-game-minecraft/src/projection.rs`
- `crates/auv-game-minecraft/src/artifact.rs`
- `crates/auv-game-minecraft/src/overlay.rs`

### Files added during crate-local MC-3 / MC-4
- `crates/auv-game-minecraft/src/input_target.rs`
- `crates/auv-game-minecraft/src/verify.rs`

### Files edited
- `Cargo.toml`
- `Cargo.lock`
- `crates/auv-game-minecraft/src/lib.rs`

## MC-2 scope actually implemented

This stayed inside the approved **pure-crate, no-click** boundary. The result is an offline geometry/projection artifact closure, not a live end-to-end proof.

Implemented:
- Minecraft-local telemetry/data types:
  - `MinecraftSpatialFrame`
  - `MinecraftBlockTarget`
  - `MinecraftProjectedPoint`
  - `ProjectionVisibility`
  - nearby/raycast/inventory support types
- Offline world→screen projection core:
  - `MinecraftProjector`
  - `clip = P * V * w`
  - `clip.w <= 0 => BehindCamera`
  - `NDC -> pixel` mapping with Y flip
  - projected block match radius from block AABB corners
- Projection artifact contract:
  - `MinecraftProjectionArtifact`
  - finite/positive validation
  - serde roundtrip tests
- Overlay renderer:
  - crosshair
  - bounding box
  - raycast marker badge
- Workspace wiring for the new crate

Explicitly not implemented in MC-2:
- live screenshot binding
- `capture_skew_ms` calibration/thresholding
- runtime / CLI integration
- `RecordingHandle` artifact staging
- any input/click path
- any live MC-3 verification path
- any core graduation into shared geometry / inference types

## Crate-local MC-3 scope actually implemented

This stayed inside the owner-chosen **offline logic closure inside the vertical crate only** boundary. It closes the verdict logic against crate-local evidence, not live driver/runtime execution.

Implemented:
- Offline world-diff verdict contract:
  - `WorldDiffRequest`
  - `WorldDiffVerdict`
  - `WorldDiffFailure`
  - `evaluate_world_diff(pre, post, request)`
- Input-target seam for future live dispatch:
  - `projected_window_point(&MinecraftProjectedPoint) -> Option<WindowPoint>`
- MC-3 offline honesty rules encoded in code/tests:
  - unordered or stale pre/post frames => `VerificationUnreliable`
  - no PRE non-air witness at target => `VerificationUnreliable`
  - block removed but expected inventory item did not rise => `StateChangedNoMatch`
  - inventory item rose but target block did not disappear => `SemanticMismatch`
  - non-visible projected target => no input point

Explicitly not implemented in this MC-3 crate-local closure:
- any real `auv-driver` input dispatch
- any `InputActionResult` production
- any `VerificationResult` / runtime / CLI / store wiring
- any real telemetry read path
- any live screenshot/frame binding

## Crate-local MC-4 scope actually implemented

This stayed inside the owner-approved **crate-local mismatch-refusal closure** boundary. It closes refusal logic from crate-local evidence only, not live acceptance.

Implemented:
- Mismatch-refusal contract:
  - `MismatchRefusal`
  - `MismatchRefusalReason`
  - `evaluate_mismatch_refusal(pre, projected, expected_target, screenshot_is_minecraft_window, max_capture_skew_ms)`
- MC-4 refusal cases now closed in code/tests when they can be proven from existing crate-local evidence:
  - target window is not Minecraft => `NotMinecraftWindow`
  - screenshot artifact missing => `ScreenshotUnavailable`
  - screenshot ↔ telemetry binding missing => `ScreenshotUnbound`
  - `mc_capture_skew_ms` exceeds the caller-provided threshold => `CaptureSkewUnreliable`
  - projected target outside window / visible-without-point => `ProjectedOutsideWindow`
  - target behind camera => `TargetBehindCamera`
  - target out of frustum => `TargetOutOfFrustum`
  - raycast hit disagrees with the expected target => `TargetOccluded`
  - telemetry lacks a usable target witness at refusal time => `TelemetryUnreliable`
- Crate re-exports now expose the refusal types and function next to the existing MC-3 verdict seam.

Explicitly not implemented in this MC-4 crate-local closure:
- any live acceptance claim from `refused == false`
- any real screenshot classifier that proves the window is Minecraft
- any calibrated global skew threshold from real samples
- any runtime / CLI / store / inspect wiring
- any live refusal matrix recorded through `VerificationResult`
- any real-client proof for menu / black / loading-screen rejection

## MC-1 telemetry gate status

The repo now also contains the first real **MC-1 telemetry mod build gate** under:
- `devtools/auv-game-minecraft/`

Implemented there:
- Fabric mod skeleton and metadata
- append-only JSONL telemetry writer
- minimal telemetry record shape for:
  - `spatial_frame_id`
  - `world_tick`
  - `monotonic_timestamp_ms`
  - `viewport`
  - `player_pose`
  - `raycast_hit`
- local Gradle wrapper and successful sidecar build

What this means:
- MC-1 is no longer only a research note; there is now a buildable read-only telemetry mod
- but a **real running-client sample is still missing**, so live MC-3 / MC-4 are not yet proven

## Validation completed

Passed locally:
- `cargo fmt --check`
- `cargo check -p auv-game-minecraft`
- `cargo test -p auv-game-minecraft`
- `git diff --check`
- `JAVA_HOME=/Users/liuziheng/Library/Java/JavaVirtualMachines/zulu-21.0.9-arm64.jdk/Contents/Home devtools/auv-game-minecraft/gradlew -p devtools/auv-game-minecraft build`

Not done in these slices:
- no real-frame smoke
- no runtime integration smoke
- no MC client execution
- no live input dispatch
- no real world-diff verification against a running client
- no live refusal matrix recorded from a running client

## Important implementation notes

### 1. Naming drift from the original MC-2 sketch
The implemented names were narrowed to reduce collisions with existing AUV terms:
- `MinecraftSpatialFrame`
- `MinecraftBlockTarget`
- `MinecraftProjectedPoint`
- `MinecraftProjectionArtifact`
- `mc_capture_skew_ms`

This was intentional to avoid overloading generic `SpatialFrame`, `WorldTarget`,
`ProjectionArtifact`-style names too early.

### 2. Current projection contract is still offline-only
The current projector assumes the matrices supplied by future MC-1 telemetry are
already correct and uses them as authoritative. There is no screenshot/frame
binding yet. That means MC-2 still proves **math + artifact shape**, not runtime
truth alignment.

### 3. Visibility still stops at the crate-local boundary
Current visibility/refusal classification now covers:
- `Visible`
- `BehindCamera`
- `OutOfFrustum`
- `OutsideWindow`
- crate-local `TargetOccluded` refusal when raycast disagrees with the target

But this is still not a live acceptance proof; the occlusion refusal is only as
strong as the available crate-local telemetry witness.

### 4. MC-3 and MC-4 remain deliberately crate-local
The verdict and refusal modules were kept inside `crates/auv-game-minecraft`.
They intentionally do **not** import `src/contract.rs`, do **not** touch runtime,
and do **not** construct `InputActionResult` or `VerificationResult`. The goal is
to close the logic loops without pretending live AUV execution exists yet.

### 5. `screen_point` is still treated as window-relative
`projected_window_point` wraps the existing `MinecraftProjectedPoint.screen_point`
into `WindowPoint` because the current projection math emits viewport/window-relative
pixels, matching the osu lane handoff shape. If future MC-1 telemetry proves those
values are true screen-space pixels, live MC-3 wiring must convert them instead of
double-applying the window origin.

### 6. `refused == false` does not mean live acceptance
The new MC-4 refusal seam only says the crate failed to prove a mismatch from the
currently available evidence. It does **not** mean the live target is accepted,
click-safe, or semantically verified. That stronger claim still needs a real client
sample, screenshot binding, and runtime wiring.

## Current blockers after the crate-local MC-4 closure

The things still blocked on real MC-1 output are:
- live telemetry sample from a running client
- screenshot ↔ telemetry binding
- calibrated `capture_skew_ms` thresholds from real samples
- real overlay-on-frame proof
- real input dispatch through the AUV driver
- real world-diff verification against a running client
- runtime/store/read-side integration of MC-3/MC-4 evidence
- menu / black / loading-screen refusal proof from real captures

## Current repo state at handoff moment

Local branch now contains, in order:
- `e5f35b3 docs(ai): add minecraft p0 architecture plan`
- `628dd64 docs(ai): refine minecraft mc-1 research boundary`
- `58d70c1 feat(game): add minecraft mc-2 projection crate`
- `2efb30c feat(auv-game-minecraft): add offline mc-3 verdict logic`
- `aa8b6f5 feat(auv-game-minecraft): add mc-4 mismatch refusal closure`

At the moment of writing this handoff, the working tree is expected to contain:
- this handoff doc update
- the uncommitted `devtools/auv-game-minecraft/` MC-1 telemetry mod tree, if it has not yet been committed in a later slice

## Next-slice recommendation

The next hard requirement is no longer crate-local refusal logic. The next hard
requirement is **one real MC-1 telemetry sample from a running Minecraft client**,
followed immediately by screenshot binding proof.

### Required before meaningful live MC-3 / MC-4 implementation
1. A real MC-1 telemetry sample from a running Minecraft client
   - at minimum: `world_tick`, `monotonic_timestamp`, `viewport_size`,
     `player_pose`, `raycast_hit`
2. A real screenshot/frame binding story
   - enough to compute and inspect `mc_capture_skew_ms`
3. A decision on the first fixed target scenario
   - the current best candidate remains one marked block at a known coordinate
4. A live world-diff contract confirmation
   - confirm whether target disappearance is seen as absence, `minecraft:air`, or another stable representation
5. A real refusal sample matrix
   - at least one proven menu/black/loading or non-Minecraft-window refusal captured from a running client

### Safe next topics
These are safe to do next without overclaiming live success:
- run the real MC-1 telemetry mod and capture a durable sample
- verify the actual block/air representation in telemetry
- verify screenshot ↔ telemetry skew on one real frame
- define the exact first fixed-target scenario with one known block
- decide how live MC-3/MC-4 evidence should later map into `VerificationResult`

### Unsafe premature moves
Do **not** do these as part of the next slice unless explicitly approved:
- graduating Minecraft geometry/types into AUV core
- adding CLI/runtime wiring before real telemetry exists
- claiming semantic or input success from offline verdicts/refusals alone
- designing around Mineflayer action delivery
- calling MC-4 done in the live acceptance sense

## Suggested next prompt after compact

A good restart prompt would be roughly:

> Continue from the crate-local MC-4 closure. The offline verdict, input-target
> mapping, and mismatch-refusal logic are landed, and the MC-1 telemetry mod now
> builds locally. Next, run the mod on a real Minecraft client, capture one durable
> telemetry sample plus its bound screenshot, and only then design the first live
> MC-3/MC-4 acceptance slice.

## One-line summary

MC-2, crate-local MC-3, and crate-local MC-4 are all landed; MC-1 is now a buildable
telemetry mod instead of only a research note, and the next real blocker is a running-client
telemetry sample plus screenshot binding needed for live MC-3/MC-4.
