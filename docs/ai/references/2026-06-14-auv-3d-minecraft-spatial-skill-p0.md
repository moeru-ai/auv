# AUV 3D / Minecraft Spatial-Skill — P0 Architecture & Execution Plan

> AUV remains a skill substrate. The Minecraft sidecar is read-only truth and
> verifier. All actions go through the AUV driver.

Date: 2026-06-14

Status: proposed P0 architecture, written for slice-by-slice execution by Claude
Code on the Mac (the planning sandbox is Linux and cannot build the macOS-targeted
crates). This is the 3D analog of the osu lane: `world truth -> projection ->
real input -> verification/refusal`, one dimension up.

This document is **not**: a Minecraft Agent Roadmap, a 3DGS World Model, or an
Autonomous Bot. It is the first proof that Minecraft can be AUV's **3D osu**: AUV
takes a 3D target, projects it to screen, acts with real input, and
verifies-or-refuses with evidence.

## 0. Hard boundary + kill gates (read first)

AUV stays a skill/perception/verification substrate. The upstream agent (AIRI /
Claude / Codex / human) decides goals; AUV answers "where is it, where on screen,
is it visible, did the action match, what is the evidence." AUV never generates
its own intent.

```text
KG1  No real camera matrix => do NOT start AUV P0. Build the Fabric telemetry mod first.
KG2  Any action delivered via Mineflayer/MCP (goTo/dig/place/pathfind) => this line FAILS by definition.
KG3  No mismatch-refusal => P0 is NOT done. A happy-path-only demo does not qualify as AUV.
```

The single load-bearing rule: **MCP / sidecar / Mineflayer is read-only truth and
the verification answer-key. It is never an action channel. Every action is real
mouse/keyboard delivered by the AUV driver into the rendered Minecraft window.**
This is exactly how osu used `.osu` beatmap truth as the answer-key while AUV
delivered real clicks — one dimension up.

## 1. The setup correction that shapes everything

Headless **Mineflayer does not render**: it has world state but no camera, no
frame, no window. So the camera/visual truth **cannot** come from Mineflayer. It
must come from a **Fabric telemetry mod running inside the real, rendered
Minecraft Java client** — the same client AUV screenshots and drives input into.

P0 therefore needs exactly one rendered, Fabric-modded client. AIRI's Mineflayer
MCP is the eventual upstream-agent connection (and an optional extra world-state
source) but is **not** the P0 camera source and **not** required for P0.

## 2. Architecture (three roles, one execution model)

```text
        upstream agent (AIRI / Claude / human)      ← decides GOALS (LATER, not P0)
                       │  world target: block {x,y,z}(+face) / entity id
                       ▼
┌───────────────────────────────────────────────────────────────────────────┐
│  crates/auv-game-minecraft   (NEW vertical crate — MC-specific, NOT core)    │
│   observeSpatial → projectTarget → (resolve+act) → verifyWorldAction         │
│   reuses AUV seam: recognition → ActionResolver → InputActionResult → Verify │
└──────┬────────────────────────┬─────────────────────────┬───────────────────┘
       │ read truth             │ project + bind capture   │ deliver action / verify
       ▼                        ▼                          ▼
  Fabric telemetry mod    AUV window capture          AUV input driver
  (rendered MC client)    (auv-driver capture)        (auv-driver / -macos)
   • view_matrix           • screenshot of the SAME    • real mouse move / click /
   • projection_matrix       client window               key into the SAME window
   • raycast_hit           • bound to the frame at     • NEVER Mineflayer action
   • world snapshot          the same instant
   • tick + monotonic ts
       │
       └─── READ-ONLY TRUTH + VERIFIER (never an action channel)

  All of it lands in: run / store / inspect   (AUV core, reused unchanged in P0)
  3D Gaussian Splatting (spatial memory) = ACT 3, offline artifact first — NOT P0.
```

- **Truth + verifier**: Fabric mod (camera matrices, raycast, world snapshot,
  tick/timestamp). Read-only.
- **Eyes + hands**: AUV (`auv-driver` window capture + input driver), reused.
- **Memory (later)**: 3DGS, deferred to act 3 (see §8).

## 3. Where each piece lives in the repo

```text
NEW (this lane)
  sidecar/minecraft-telemetry/        Fabric mod (Java) — read-only telemetry. External to the Rust workspace.
  crates/auv-game-minecraft/          NEW vertical crate (mirrors crates/auv-game-osu): sidecar client,
                                      SpatialFrame, projection math, overlay artifact, P0 task harness,
                                      verification-against-sidecar, the minecraft.* skills.

REUSED, UNCHANGED in P0 (AUV core)
  crates/auv-driver{,-macos}          window capture + real input delivery + InputActionResult + disturbance
  src/contract.rs                     OperationResult / VerificationResult (reuse; no third schema)
  src/recording, run_builder, store   run/artifact recording (RecordingHandle from the C2 work)
  src/inspect, run_read               inspect/read of the recorded run
  src/action_resolver_decision.rs     the seam; MC adds a world→screen projection stage inside the vertical

GRADUATES TO CORE LATER (only after osu + MC both prove the shape — see §7)
  crates/auv-driver/src/geometry.rs   add World/Camera coordinate space + a projection basis (G4)
  crates/auv-inference-common         projected DetectionCoordinateSpace variant (G4)
  timestamped same-instant capture binding (G3); frame/action correlation key (G2)
```

## 4. Core P0 contracts (all start INSIDE the vertical, not core)

```text
SpatialFrame                       the atomic, same-instant bound unit (G3's 2nd consumer)
  spatial_frame_id
  world_tick, monotonic_timestamp_ms
  viewport_size {w, h}
  view_matrix [4x4], projection_matrix [4x4]     // READ from the mod; never estimated
  player_pose {eye_xyz, yaw, pitch}              // for sanity/debug, not the projection source
  raycast_hit {block_pos, face, block_id} | none // crosshair ground-truth occlusion
  nearby_blocks[], nearby_entities[], inventory_summary
  screenshot_artifact_ref                        // AUV capture bound at the same instant
  capture_skew_ms                                // |screenshot_ts - frame_ts|; reject if too large

WorldTarget          { block_pos {x,y,z}, face? }  |  { entity_id }
ProjectedScreenTarget
  screen_point {x, y}        // world → clip(P·V·w) → NDC(/w) → viewport transform (document the convention)
  visibility: Visible | BehindCamera | OutOfFrustum | OutsideWindow | Occluded
  match_radius_px            // from the block AABB projected (osu's CS→radius rule, one dim up)
  basis_frame_id, confidence
ProjectionArtifact   // staged into the run: matrices, target, projected point, visibility, overlay PNG
```

Projection is the osu `PlayfieldProjection` one dimension up: 2D affine → full
camera (perspective). **Derive from the structured signal (the mod's matrices),
verify against a real captured frame before trusting it** — same discipline as osu
P4a (derivation channel separate from verification channel).

## 5. Sliced ladder (the 6 cuts, mapped to AUV sub-slice discipline)

Each cut = one risk-coherent, independently validatable, committable slice. Finish,
report, stop, let the owner pick the next.

```text
MC-0  Sidecar capability probe (docs-only)                 ← Codex starts here
MC-1  Fabric read-only telemetry mod (Java, external)      ← then here, then STOP
MC-2  auv-game-minecraft: projection + overlay (NO click)
MC-3  P0 real input: fixed target via AUV driver + world-diff verify
MC-4  Mismatch-refusal P0 (the negative cases)             ← KG3 gate
MC-5  G2/G3/G4 graduation design (minimal common shape → core)
MC-6  Spatial dataset recorder (run bundle)
MC-7+ Offline 3DGS artifact → 3DGS-assisted verify → vision-only transfer   (ACT 3, deferred)
```

Dependency: MC-2 is gated on MC-1 (KG1). MC-3 needs MC-2. MC-4 needs MC-3. MC-5
needs osu + MC-2/3 (two real consumers). MC-6 needs MC-3 stable. MC-7+ needs MC-6.

## 6. P0 slices in detail (what Claude Code executes)

### MC-0 — Sidecar capability probe (docs-only; do this first)

Do not write AUV yet. Determine what the AIRI Mineflayer service and a rendered
Fabric client can actually expose, and what's missing.

Probe for: player camera eye position; yaw/pitch; FOV; **view matrix**;
**projection matrix**; crosshair **raycast hit block**; depth/visibility; same-tick
**world snapshot**; **tick id / monotonic timestamp**; window/viewport size.

Output: `docs/ai/references/2026-xx-xx-minecraft-sidecar-capability-probe.md` —
what exists now, what's missing, the P0 minimum, and exactly which fields the
Fabric mod must add. Decide KG1 here: if no real matrices, MC-1 (the mod) is the
first engineering task, not AUV.

Gate: report the probe doc; stop.

### MC-1 — Fabric read-only telemetry mod (external; Java)

Build the mod that exports, per rendered frame, the `SpatialFrame` telemetry over
one local **read-only** transport. `MC-1` should start with append-only JSONL,
not HTTP/WS, because it is the lightest path that matches AUV's existing
run/store/read-side discipline and keeps the sidecar boundary strictly
one-directional.

Minimum `MC-1` schema (keep the main shape narrow):
`spatial_frame_id, world_tick, monotonic_timestamp, viewport_size,
player_pose, raycast_hit, nearby_blocks, nearby_entities, inventory_summary`.
Small context keys like `dimension_id` and `player_id/session_id` are allowed if
needed to keep frames attributable, but do not widen the schema into world dumps,
training labels, planner state, or render-buffer artifacts.

Sampling boundary:
- Use one **tick-side** sampling spine for `world_tick`, `player_pose`,
  `raycast_hit`, `nearby_blocks`, `nearby_entities`, and
  `inventory_summary`.
- Treat `viewport_size` as **frame-side** data.
- Record `monotonic_timestamp` from a monotonic clock, not from world time.
- Do not derive camera truth from HUD/render output.

Hooking boundary (research result):
- Preferred client-side spine: `ClientTickEvents.END_CLIENT_TICK` for the
  tick-side fields.
- Preferred viewport read path: client window/framebuffer size from the window
  abstraction at render time.
- `view_matrix` and `projection_matrix` must come from a render-time hook with a
  real world-render context; tick-spine `GL11.glGetFloatv` reads are not
  acceptable for truth.
- Do **not** anchor business sampling in deep render internals such as
  `MinecraftClient#render` mid-pipeline, `GameRenderer`/`WorldRenderer` private
  internals, `BufferBuilder`/`VertexConsumer`/`RenderSystem`, or HUD/screen draw
  mixins. Those are version-fragile and couple telemetry to presentation.

Persistence/read-side boundary:
- The first implementation should write telemetry as persisted facts, e.g. a
  `telemetry.jsonl` stream under the run artifact root or equivalent sidecar
  output directory.
- Readers should consume only **flushed durable records**, never live runtime
  memory.
- If AIRI later consumes `MC-1` telemetry, the least invasive attachment point is
  a debug-server-side read-only reader, not the query runtime, reflex context,
  or cognitive engine state.

Hard line (KG2): **no `goTo` / `dig` / `place` / `pathfind` / any Mineflayer
action.** This mod is an answer-key, not a controller.

Current environment caveat from the probe:
- The local machine shows Minecraft `1.21.11` plus Fabric/Forge/NeoForge loader
  traces, but the current visible client directory still lacks `logs/`, `mods/`,
  and `instances/` evidence.
- That does **not** block `MC-1` design work, but it **does** block confident
  implementation validation until the real instance root and launch logs are
  visible.

Gate: show a live telemetry sample (at minimum `world_tick`,
`monotonic_timestamp`, `viewport_size`, `player_pose`, `raycast_hit`) from a
running client; stop. **Claude Code stops here — do not free-wheel into MC-2+.**

### MC-2 — auv-game-minecraft: projection + overlay (no real click)

New vertical crate `crates/auv-game-minecraft` (mirror `auv-game-osu`). Read a
`SpatialFrame` from the sidecar, bind it to an AUV window screenshot at the same
instant (record `capture_skew_ms`), project a `WorldTarget` block `{x,y,z}` to a
`ProjectedScreenTarget`, and stage a `ProjectionArtifact` with an overlay PNG
(projected box + crosshair + raycast marker).

Prove (no input yet): a given block projects to the right screen pixel; the
projected point agrees with the mod's raycast and with the screenshot. Keep all
types inside the vertical — do not graduate to core yet.

Validation: unit tests on the projection math at fixed matrices/viewports
(known world point → known pixel); a real-frame check where the overlay visibly
lands on the right block. Gate: report the overlay PNG + skew; stop.

### MC-3 — P0 real input (fixed map, fixed target)

Fixed local world, fixed target (e.g. mine a marked red-wool block at a known
`{x,y,z}`). Flow: `WorldTarget` → `ProjectedScreenTarget` → ActionResolver →
**AUV driver** delivers real aim/click/hold into the MC window → query the sidecar
for the **world diff** (block became `air`, or inventory gained the item) →
`VerificationResult`.

Reuse the existing seam and `auv-driver` input path unchanged. No Mineflayer
action. Record the full run (frames, matrices, action, verification) via the
existing recording/store.

Validation: real-app smoke; the target block actually changes and the run records
a passing `VerificationResult` with the run id. Gate: report the run id + world
diff; stop.

### MC-4 — Mismatch-refusal P0 (KG3 — the real acceptance)

Make AUV refuse, with evidence, on every mismatch class — and prove each:

```text
target window is not Minecraft        → refuse
screenshot is menu / black / loading  → refuse
capture_skew_ms over threshold        → refuse (matrix ↔ screenshot not same instant)
projected point outside window         → refuse
target behind camera (clip.w<=0) / out of frustum → refuse
raycast hit != target block (occluded) → refuse
post-action world diff != expected     → fail (verification), with evidence
```

Each refusal emits a structured reason + the `SpatialFrame` evidence through the
existing refusal / `VerificationResult` seam. This is the AUV-defining behavior:
it knows when **not** to click.

Validation: a test/smoke per mismatch class showing refuse + recorded reason, not
a blind click. Gate: report the refusal matrix; stop. **Only now is P0 done.**

### MC-5..MC-7 (sketch; later slices, do not start in P0)

- **MC-5** graduation design: with osu + MC as two consumers, lift the **minimal
  common** shapes to core — G2 (frame/action correlation key), G3 (same-instant
  timestamped capture binding), G4 (source→screen projection basis: frame id,
  timestamp, basis, confidence). Core must never learn `dirt_block` / `creeper` /
  `chunk`. Design-note-first per the graduation gate.
- **MC-6** spatial dataset recorder: each run → a bundle (`screenshots/`,
  `spatial_frames/`, `actions/`, `verification/`, `overlays/`, `run.json` with
  versions + commits). This is the labeled gym for later perception, not present
  showmanship.
- **MC-7+** 3DGS: see §8.

## 7. What graduates vs what stays in the vertical (osu gate, one dim up)

```text
MAY graduate to core (after MC-5, owner-approved design note, no MC semantics):
  same-instant timestamped capture binding            (G3, 2nd consumer = MC)
  frame/action/target/verification correlation key     (G2, 2nd consumer = MC)
  source→screen projection basis + projected coord space (G4, 2nd consumer = MC)

NEVER graduates (stays in crates/auv-game-minecraft):
  block ids / faces / chunk / entity taxonomy / inventory semantics
  Minecraft camera quirks, the sidecar wire format, the play task policy
```

MC is the second consumer the parked G2/G3/G4 were waiting for — but the osu
discipline holds: prove in the vertical, graduate only the generic shape.

## 8. 3DGS — act 3, deliberately deferred (not load-bearing for MC)

With a Fabric mod you have **raycast + depth from the truth source** — a stronger,
cheaper, exact occlusion/visibility signal than 3DGS. So in modded MC you do not
need 3DGS for occlusion. Its real role: **MC is the answer-key gym to incubate a
vision-only spatial-memory capability you will deploy later on 3D apps that do NOT
expose truth.** Do not pre-commit; let the "no truth source" second scenario pull
it in. First 3DGS version is an **offline inspect artifact only** (expected-view
renders, coverage, diff vs real screenshots) — never in the action path until it
has earned it.

## 9. Validation + execution discipline

- Per-slice standard block on the Mac: `cargo fmt --check && cargo check &&
  cargo test && git diff --check`, plus the slice's real-app smoke with run ids
  recorded. (The planner sandbox cannot run `cargo` on the macOS crates.)
- The Fabric mod is Java/external and validated by a live telemetry sample, not
  cargo.
- **Claude Code executes MC-0 and MC-1 only, then stops for owner selection.** Do
  not "helpfully" start the vertical or refactor adjacent code.

## 10. One-line truth

A minimal, undeniable MC-P0: same-instant spatial frame + world→screen projection
+ real input + negative-case refusal. Spatial dataset after it is stable. 3DGS is
act 3, not the opening cinematic.
