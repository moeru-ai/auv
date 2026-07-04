# AUV Surface SLAM Direction

**Date:** 2026-07-05
**Status:** direction note — docs-only; opens the next planning lane after S9b, does not approve implementation
**Scope:** 2D video-stream consumption, temporal surface modeling, action/state transition proof
**Related:** [S-line streaming observation substrate](2026-07-03-s-line-streaming-observation-substrate.md), [S-line graduation review](2026-07-04-auv-s-line-graduation-review.md), [S9b adjacent tracks wire](2026-07-10-auv-scan-s9b-adjacent-tracks-wire-handoff.md)

## One-line decision

After S9b, pause the S-line implementation lane and open a separate **Surface SLAM**
direction:

```text
2D video stream
  -> multi-channel frame observation
  -> temporal tracking
  -> keyframe selection
  -> 2D surface graph
  -> action graph
  -> command distillation
  -> optional spatial grounding
```

This is not a `2D video -> 3DGS` roadmap. The short-term product target is a
stable **interactive surface model**, not geometric reconstruction.

## Why not make this "YOLO to 3D"

YOLO answers a low-level question:

```text
What objects are visible in this frame?
```

AUV needs higher-level continuity and action semantics:

```text
Which visible things are the same entity across frames?
Where did they move?
What changed after an action?
Can this entity still be acted on?
How can the action be verified or recovered?
```

Treat YOLO as one `FrameObservation` channel beside OCR, AX, segmentation, cursor,
focus, and window metadata. Do not let a detector own the substrate.

## Relationship to existing lanes

| Lane | Status | Boundary |
| --- | --- | --- |
| **S line** | Pause after S9b for now | Provides frame, timeline, coverage, and adjacent track artifacts. It is still the observation substrate, but should not keep expanding blindly. |
| **Surface SLAM** | New direction note | Turns a video stream into a stable interactive surface graph and action/state transitions. |
| **M line** | Still deferred | 3DGS, SLAM backends, dense maps, pose graphs. They consume Surface SLAM or S-line outputs later. |
| **G line** | Still deferred as main line | Games or telemetry adapters can validate spatial grounding later, but should not drive this first slice. |
| **B line** | Product consumption | Should eventually inspect keyframes, tracks, state transitions, and command candidates. |

## Core idea: 2D Surface SLAM

The analogy to SLAM is useful, but the map is not a 3D world map.

| SLAM concept | AUV surface equivalent |
| --- | --- |
| camera pose | app/window/route/modal/scroll/focus context |
| landmark | UI element, text region, icon, object, affordance |
| keyframe | stable screen state worth retaining |
| map | 2D surface graph |
| tracking | entity identity across frames |
| loop closure | recognizing the same page/state again |
| bundle adjustment | merging repeated observations to correct weak identity or stale claims |
| exploration coverage | UI flow / region coverage |

The first useful product is:

```text
2D video -> stable interactive surface model
```

not:

```text
2D video -> 3D model
```

## Proposed minimal concepts

Names are provisional. This note defines direction, not final Rust API.

### 1. `FrameObservation`

A single frame with multiple evidence channels:

```text
FrameObservation
  - frame_id
  - timestamp
  - screenshot / image artifact
  - OCR text boxes
  - AX elements
  - detector objects
  - segmentation masks
  - cursor / focus / window metadata
  - quality flags
```

YOLO belongs here as `detector objects`, not as the identity or state layer.

### 2. `TrackedEntity`

The first abstraction above per-frame detection:

```text
TrackedEntity
  - entity_id
  - kind
  - current_bbox
  - previous_bboxes
  - visual_signature
  - text_signature
  - ax_signature
  - stability_score
  - first_seen / last_seen
  - interaction_affordance
  - freshness
```

The hard question is identity continuity, not detector confidence in one frame.

### 3. `Keyframe`

A retained screen state or transition point:

```text
Keyframe
  - keyframe_id
  - frame_id
  - selected_reason
  - stable_entities
  - changed_regions
  - coverage_delta
  - open_uncertainties
```

Keyframes keep the stream inspectable and prevent every transient frame from
becoming durable product state.

### 4. `SurfaceState`

The current model of a surface:

```text
SurfaceState
  - surface_state_id
  - app/window context
  - modal stack / route hints
  - visible regions
  - tracked entities
  - changed regions
  - stale regions
  - action candidates
  - evidence quality
```

This is the layer ordinary app automation needs more often than 3D geometry.

### 5. `ActionTransition`

The bridge from observation to command learning:

```text
ActionTransition
  - from_surface_state
  - action_candidate
  - input_event
  - to_surface_state
  - changed_entities
  - verifier
  - recovery_hint
```

Every command candidate must carry a verifier. A click coordinate without a
post-action verification story is not a command.

## First acceptance gates

The first implementation plan should choose a narrow fixture and prove these
properties before touching heavier model backends:

1. Preserve one entity identity across 5-10 adjacent frames.
2. Detect which entities disappear and appear during scrolling or modal change.
3. Select keyframes from a short human operation video.
4. Record one action transition from pre-action state to post-action state.
5. Emit at least one command candidate with a verifier.
6. Explain uncertainty: ambiguous identity, stale observation, detector-only
   evidence, or missing verification.

## Non-goals for the first implementation slice

- No 3DGS, dense reconstruction, or SLAM backend.
- No Minecraft/game telemetry adapter as the main line.
- No general YOLO integration as the headline deliverable.
- No broad detector abstraction rewrite.
- No command distillation product claim without verifiers.
- No `S10` label by default; S-line is paused and this direction needs its own
  charter before code.

## Spatial grounding boundary

3D should enter as an optional **Spatial Grounding Layer** only when the surface
model cannot answer the task:

```text
2D Surface Map
  default for app / browser / desktop UI

2.5D Depth Layer
  for games, maps, canvas tools, video, spatial apps

3D Scene Map
  only when camera pose, occlusion, or navigation are required
```

Spatial grounding answers:

```text
Where is this object in spatial relation?
Can it be reacquired after viewpoint motion?
Is it occluded, behind, above, or around another object?
```

It does not replace the ordinary app automation questions:

```text
Can this control be operated now?
What changed after the operation?
How is success verified?
How is failure recovered?
```

## Recommended next slice shape

Open a narrow plan before code:

```text
Surface-SLAM-0 charter
  -> choose one short video / frame-sequence fixture
  -> define FrameObservation channel boundaries
  -> define TrackedEntity identity rules
  -> define Keyframe selection reasons
  -> define ActionTransition verifier requirements
  -> explicitly defer Spatial Grounding
```

The first code slice should be hermetic and fixture-first. Runtime capture, live
video, detector integration, B-line UI, and 3D backends are follow-ons.
