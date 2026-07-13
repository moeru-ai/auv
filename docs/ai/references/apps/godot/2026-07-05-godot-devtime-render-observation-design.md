# 2026-07-05 AUV Godot dev-time render observation design

Date: 2026-07-05

Status: proposed design with a validated MVP slice for the current AIRI Godot
Stage development feature.

This document records the design boundary and the current implementation state.
It does not approve the broader future Godot automation surface, editor bridge,
test-time AUV path, or remote-debug transport.

Update note, 2026-07-05:

- The primary path is still same-instance Godot runtime observation.
- This design does not remove communication. It removes dependency on the
  AIRI product bridge and does not make Godot remote debug the first-slice
  transport.
- The Godot-side entry point is described as a dev observation adapter/endpoint
  until the concrete transport is confirmed.
- Godot remote debug remains a candidate adapter for future editor/debugger
  workflows, not the current edge-light render observation mainline.
- After implementation discussion, the MVP allows a final window screenshot as
  presentation evidence. Intermediate render stages still come from Godot-side
  render export, not from window capture, OCR, or pixel inference outside Godot.

## 1. Current target

`auv-godot` should first help coding agents work on AIRI's Godot Stage rendering
features. The immediate feature pressure is avatar edge-light development, where
the agent needs repeatable render observations while changing the AIRI Godot
Stage compositor.

The first useful slice is therefore:

```text
AIRI keeps the current Godot Stage instance running
  -> AUV discovers that same Godot process
  -> AUV connects to Godot's dev observation adapter/endpoint
  -> AUV requests scoped debug-view/export operations
  -> Godot exports render-stage outputs without disconnecting from AIRI
  -> AUV may capture the final visible window as presentation evidence
  -> AUV collects a structured artifact bundle for the coding agent
```

This slice is development-time tooling. It is not the later game-like QA tool
that clicks UI, manipulates the avatar as a player, or verifies exported runtime
behavior through real input.

## 2. Confirmed constraints

### User constraints

- Focus current work on the active feature development path.
- Do not design the first slice around general OS computer use.
- Do not use screenshot, window capture, OCR, or platform drivers as the
  intermediate render-stage observation mechanism.
- The MVP may include a final window screenshot as presentation evidence, so the
  coding agent can compare Godot viewport export against what the human sees in
  the same running instance.
- Prefer Godot-native capabilities. AUV should avoid reimplementing capabilities
  Godot already exposes.
- For the current render feature, input can start with camera/view control.
- Outputs are tool-specific. Edge-light observation may need render layers and
  intermediate compositor outputs, but future tools may emit different artifact
  shapes.
- Agent and human should observe the same running Godot Stage instance during
  development.
- AUV must not disconnect, replace, or hijack the AIRI <-> Godot runtime
  channel.
- AUV still needs an observation communication path. The constraint is that the
  path must be separate from AIRI's product runtime bridge and must not depend
  on platform screen capture for compositor-stage observation.

### AUV repository facts

- `crates/auv-godot` exists and is scaffold-only:
  - `crates/auv-godot/Cargo.toml`
  - `crates/auv-godot/src/lib.rs`
- The crate is already included in the AUV Cargo workspace.
- The crate comment says command surfaces, protocol bindings, editor-debugger
  integration, and AIRI Godot Stage QA operations should be added only after
  their contracts are named.

### AIRI Godot Stage facts

The current AIRI Godot Stage already has product runtime code and development
scaffold mixed in the same project.

Product/runtime code remains in AIRI:

- `engines/stage-tamagotchi-godot/scripts/StageRoot.cs`
- `engines/stage-tamagotchi-godot/scripts/scene/**`
- `engines/stage-tamagotchi-godot/scripts/transport/**`
- `engines/stage-tamagotchi-godot/scripts/view/**`
- `engines/stage-tamagotchi-godot/scripts/visuals/**`
- `engines/stage-tamagotchi-godot/scripts/vrm/**`
- `engines/stage-tamagotchi-godot/scenes/stage-root.tscn`

Development scaffold candidates:

- `engines/stage-tamagotchi-godot/tools/verifyVisualBaseline.mjs`
- `engines/stage-tamagotchi-godot/tools/exportXiaoerEdgeReferenceStages.py`
- `engines/stage-tamagotchi-godot/tools/captureWindowClientPng.ps1`

The last file is specifically a historical scaffold for window capture. It
should not become the new `auv-godot` observation path.

## 3. Existing AIRI runtime bridge and render controls

### Runtime bridge

Current AIRI Stage runtime already has a private runtime bridge:

```text
AIRI Electron main
  -> localhost WebSocket server
  -> Godot StageRoot connects with --airi-ws-url
  -> Electron forwards host.* messages to the current Godot peer
  -> Godot returns stage.* messages
```

This bridge is owned by AIRI. It carries model/scene/view lifecycle state and
must remain connected while AUV observes the running Godot process.

The existing `--airi-ws-url` endpoint is not the AUV dev endpoint. AUV should
not connect to that URL because the current Electron manager treats WebSocket
connections as Godot peers, not external controllers.

### Render controls

`StageRoot.cs` currently accepts host messages for:

- `host.view.patch`
- `host.view.request_snapshot`
- `host.view.capture_png`
- `host.render.set_debug_view`
- `host.render.set_avatar_edge_light`

The render debug view path is already present in AIRI:

```text
StageRoot.cs
  -> StageRenderEffectsRuntime.SetDebugView(...)
  -> StagePostProcessCompositorEffect.DebugView
  -> StagePostProcessCompositorEffect.Pipeline.cs
```

Known debug views:

- `final`
- `scene-copy`
- `avatar-mask`
- `avatar-edge-mask`
- `after-avatar-edge-light`
- `after-avatar-glow`

The current compositor pipeline is:

```text
SceneCopy -> AvatarMask -> AvatarEdgeLight -> AvatarGlow -> FinalColorMapping
```

The existing `verifyVisualBaseline.mjs` script can already:

- launch the Godot Stage project
- apply a view preset
- request view snapshots
- switch render debug views
- toggle avatar edge light
- dump named render-stage views

Its current render-stage dump path still captures the Godot window client area.
That is the part this design replaces. The current MVP still captures one final
window screenshot, but render-stage outputs are exported from inside Godot.

## 4. Godot source constraints

Official Godot 4.6 command-line docs confirm that Godot can be launched against
a project with `--path`, run a game from the project path, run a specific scene,
and run in `--headless` mode for server/script-style workflows:

- https://docs.godotengine.org/en/4.6/tutorials/editor/command_line_tutorial.html#setting-the-project-path
- https://docs.godotengine.org/en/4.6/tutorials/editor/command_line_tutorial.html#running-the-game
- https://docs.godotengine.org/en/4.6/tutorials/editor/command_line_tutorial.html#cmdoption-headless

Official Godot docs also define `RenderingDevice` as the lower-level abstraction
for modern graphics APIs, while `RenderingServer` works with Godot's rendering
subsystems:

- https://docs.godotengine.org/en/stable/classes/class_renderingdevice.html

For future editor-time tooling, `EditorInterface` exposes editor scene roots,
open scenes, selected paths, editor viewports, and active editor cameras:

- https://docs.godotengine.org/en/4.6/classes/class_editorinterface.html

Godot also exposes local networking primitives that can support a dev-only
observation channel. `TCPServer` can listen for TCP connections, and
`WebSocketPeer` represents WebSocket connections:

- https://docs.godotengine.org/en/4.6/classes/class_tcpserver.html
- https://docs.godotengine.org/en/stable/classes/class_websocketpeer.html

These sources support a Godot-native design. They do not imply that AUV can read
Godot GPU textures from an external process without Godot-side cooperation.

Godot remote debug is a real engine capability, but it has a specific shape.
The command-line option is:

- `--remote-debug <uri>`, documented as remote debug over a URI such as
  `tcp://127.0.0.1:6007`

Official `EngineDebugger` docs describe it as communication between the editor
and the running game. Official `EditorDebuggerPlugin` docs show the editor-side
integration point: an `EditorDebuggerPlugin` receives debugger sessions and can
send messages through `EditorDebuggerSession`. On the running game side,
`EngineDebugger.register_message_capture(...)` receives messages for a named
capture.

The implication for this design:

- `EngineDebugger.register_message_capture(...)` is useful if we add a
  Godot-side capture for dev tooling.
- It does not by itself make AUV a ready-made remote debug client.
- AUV would still need either an editor-side debugger plugin, or its own
  compatible implementation of Godot's debugger host/protocol, before it can use
  remote debug as a direct CLI transport.
- Built-in scene debugger messages such as camera override and screenshot are
  useful references, but they do not export AIRI compositor intermediate
  textures.

## 5. Decision

The first `auv-godot` design line is **dev-time render observation through
direct connection to the current AIRI-owned Godot Stage instance**.

Primary runtime shape:

```text
AIRI Electron <-> StageBridge <-> StageRoot
  runtime channel; stays connected

AUV <-> Godot Dev Observation Adapter <-> StageRoot / render runtime
  dev observation channel; direct-connects to the same Godot process
```

AIRI owns normal runtime lifecycle:

- start and stop the Godot sidecar
- materialize and send the selected model
- apply scene input
- maintain the user-facing runtime bridge
- keep the current view/runtime state alive

AUV owns dev-time observation orchestration:

- discover the current Godot dev observation endpoint
- connect to the running Godot process without going through AIRI Electron as a
  command proxy
- request a scoped camera/view/debug state when needed
- request one tool-specific render export run
- collect files and metadata into an AUV artifact bundle
- optionally capture the final visible Godot window as presentation evidence
- report the bundle through the normal AUV command/output conventions

Godot owns render state and render output production:

- keep the AIRI runtime bridge connected while serving AUV requests
- expose a dev-only observation endpoint from the running Stage process
- apply camera/view control inside the Godot process
- select render debug views or compositor outputs
- export final viewport-rendering results and required intermediate render
  outputs from inside the Godot process
- write files into a caller-provided artifact directory

AUV should not:

- connect to the existing `--airi-ws-url` bridge
- treat Godot remote debug as the current primary transport without proving the
  editor/debugger-host side of the protocol
- require AIRI Electron to proxy observation commands for the primary path
- start a separate Godot Stage instance for normal interactive development
- call Windows/macOS/Linux screen-capture APIs to infer compositor stages or
  replace Godot-side render export
- OCR the Godot window
- infer render layers from pixels captured outside Godot
- read Godot GPU resources directly from another process
- encode edge-light-specific output fields as the global `auv-godot` contract

## 6. Initial feature: AIRI edge-light observation

The edge-light observation tool is the first consumer of the general dev-time
render observation shape.

Current AIRI edge-light goal:

- reproduce the Xiaoer toolbox `边缘光` look from `绯英working.blend`
- keep the effect avatar-scoped
- avoid hard white outlines
- avoid exposing low-poly triangle faces on continuous surfaces

Current candidate-D parameters in AIRI are:

```text
WidthPixels: 14.0
VerticalScale: 0.6
DepthThresholdStart: 0.0005
DepthThresholdEnd: 0.0030
Strength: 1.0
ValueBoost: 2.1
LocalDepthThresholdStart: 0.00012
LocalDepthThresholdEnd: 0.0011
LocalDepthRadiusPixels: 5.0
```

The first edge-light observation bundle should be able to contain:

- manifest with tool name, AIRI project path, Godot version if available, commit
  if available, and timestamp
- observation mode, expected to start as `same-instance-direct`
- Godot pid and dev endpoint identity
- AIRI bridge status if Godot can report it
- camera/view input used for the run
- render settings relevant to the run
- final rendered output from Godot export
- final visible-window screenshot when the user-facing presentation needs to be
  compared against the exported viewport
- requested intermediate outputs, such as `scene-copy`, `avatar-mask`,
  `avatar-edge-mask`, `after-avatar-edge-light`, and `after-avatar-glow`
- optional comparison board against reference artifacts
- logs and failure metadata

This list is tool-specific. It should not be promoted to a universal output
schema for every future `auv-godot` development tool.

## 7. Ownership boundary

### Stays in AIRI

AIRI owns the Godot Stage product runtime:

- avatar loading
- scene application
- view runtime
- compositor implementation
- shader code
- visual preset behavior
- host-side stage transport currently required by AIRI

Any export hook that needs access to compositor textures, view state, or Godot
rendering internals must live in AIRI's Godot process or in a Godot addon loaded
by that process.

AIRI's Electron main remains the owner of product lifecycle and model/scene
delivery. It does not need to proxy AUV observation commands in the primary
path.

### Moves toward AUV

AUV should own reusable development tooling around Godot:

- discovery of the current Godot dev observation endpoint
- same-instance attach orchestration
- artifact directory layout
- observation command contracts
- result manifests
- comparison and report generation
- wrappers around AIRI's current visual verification scripts after the render
  export path is no longer window-capture-based

`verifyVisualBaseline.mjs` is the main migration candidate. Its orchestration
shape is useful, but the current window-capture dependency should be replaced by
Godot-side render output export.

## 8. Proposed layer split

This is a candidate split for implementation planning, not an approved API.

```text
auv-godot core
  discovery / connection / artifact bundle helpers

auv-godot airi-stage adapter
  AIRI Stage project conventions, dev observation contract, view presets

tool-specific observers
  edge-light observation first
  future animation/material/avatar/perf observers later
```

The split is intended to keep `auv-godot` from becoming an edge-light-only crate.
The first feature can still be edge-light-focused as long as the shared core
only models discovery, connection, scoped observation sessions, and artifact
collection.

## 9. First implementation path

### Phase 1: freeze the current design boundary

- Keep this document as the primary `vertical/godot` reference.
- Do not resurrect the old remote-debug runtime introspection document as the
  first-slice design.
- Do not accept the remote-debug simplification proposal as the current
  mainline unless its debugger-host side is implemented or explicitly replaced
  by a Godot editor plugin.
- Do not make AIRI Electron the primary observation proxy.
- Do not make AUV-owned separate Godot processes the primary development path.

### Phase 2: define the Godot dev observation contract

Define the stable request/response surface before implementing the concrete
transport. The first contract should cover:

- capability query
- AIRI bridge status query if available
- camera/view preset application
- explicit camera state application for reproducible artifacts
- render debug view selection
- render output export to caller-provided directory
- failure reporting with enough context for an AUV artifact

Current MVP status:

- Implemented: `capability.query` and `render.export_stages`.
- Partially covered: AIRI bridge status is returned inside `capability.query`,
  not through a separate `status.bridge` request.
- Not implemented yet: `camera.apply_preset`, `camera.apply_state`, standalone
  render debug-view selection, and named camera presets.

The first transport decision is WebSocket+JSON. The MVP has validated the Godot
C# server-side path with `TcpServer` and `WebSocketPeer` inside the AIRI Stage
process. Remote debug remains deferred because it requires a debugger-host or
editor-plugin side before it can be treated as AUV's direct CLI transport.

#### WebSocket message envelope

The first message envelope shape is request/response pairs with explicit request
IDs for correlation:

```jsonc
// Request
{
  "type": "render.export_stages",
  "requestId": "uuid-v4",
  "payload": {
    "outputDir": "/absolute/path/to/artifacts",
    "stages": ["scene-copy", "avatar-mask", "avatar-edge-mask"],
    "cameraPreset": "upper-body"
  }
}

// Success response
{
  "type": "render.export_stages.response",
  "requestId": "uuid-v4",
  "status": "success",
  "result": {
    "exportedFiles": ["scene-copy.png", "avatar-mask.png", "avatar-edge-mask.png"],
    "cameraState": {
      "position": [0.0, 1.5, 3.0],
      "rotation": [0.0, 0.0, 0.0],
      "fov": 75.0
    },
    "renderSettings": {
      "edgeLightEnabled": true,
      "debugView": "final"
    }
  }
}

// Error response
{
  "type": "render.export_stages.response",
  "requestId": "uuid-v4",
  "status": "error",
  "error": {
    "code": "EXPORT_FAILED",
    "message": "Failed to export stage 'avatar-mask': compositor texture not available",
    "details": {
      "failedStage": "avatar-mask",
      "availableStages": ["scene-copy", "final"]
    }
  }
}
```

First-slice message types:

- `capability.query` / `capability.query.response` — query available render
  stages, camera presets, and dev observation features
- `status.bridge` / `status.bridge.response` — query AIRI bridge connection
  status
- `camera.apply_preset` / `camera.apply_preset.response` — apply named camera
  preset such as `upper-body`
- `camera.apply_state` / `camera.apply_state.response` — apply explicit camera
  transform for reproducible artifacts
- `render.export_stages` / `render.export_stages.response` — export named
  render stages to caller-provided directory

Implemented MVP message types:

- `capability.query` / `capability.query.response`
- `render.export_stages` / `render.export_stages.response`

Error codes:

- `UNKNOWN_METHOD` — unrecognized message type
- `INVALID_PAYLOAD` — payload validation failed
- `EXPORT_FAILED` — render export operation failed
- `CAMERA_FAILED` — camera application failed
- `NOT_READY` — dev observation adapter not ready (e.g., AIRI bridge not
  connected, scene not loaded)

This envelope avoids JSON-RPC 2.0 machinery for simplicity. The `requestId`
field enables request/response correlation without requiring a stateful session
protocol.

### Phase 3: define Godot dev observation discovery

The running Godot process should publish a local discovery record if the chosen
transport is not already discoverable from AIRI's dev launch state. For example:

```json
{
  "schemaVersion": 1,
  "pid": 45678,
  "projectPath": "D:/TAworkspace/AIRIworkspace/airi/engines/stage-tamagotchi-godot",
  "devObservationEndpoint": "127.0.0.1:54321",
  "devObservationTransport": "websocket-json",
  "token": "...",
  "airiBridgeConnected": true,
  "startedAt": "2026-07-05T00:00:00Z"
}
```

The first-slice discovery location is:

```text
~/.airi/godot-stage/dev/instances/<pid>.json
~/.airi/godot-stage/dev/current.json
```

`current.json` should point to the most recent active instance. AUV should also
be able to filter by `projectPath` so multiple Godot Stage instances do not
overwrite a single global record.

The contract requirement is that AUV discovers Godot's dev endpoint without
interrupting AIRI's runtime bridge.

Current MVP status:

- Implemented: per-instance discovery record and `current.json`.
- Implemented: AUV reads `current.json` and then reads the referenced instance
  record.
- Not implemented yet: project-path filtering for multiple simultaneous AIRI
  Godot Stage instances.

### Phase 4: define artifact shape before code

Define an edge-light observation artifact shape with:

- observation mode
- Godot endpoint identity
- input camera/view state
- selected render outputs
- output directory
- manifest
- logs
- known limits

The first render format is PNG. The manifest must record format and color-space
limits so later EXR/HDR support can be added without changing the basic artifact
layout.

This phase should avoid naming a permanent Rust command surface until the shape
has been reviewed.

Current MVP status:

- Implemented: `manifest.json`, exported stage PNGs, final window screenshot,
  `context/context.json`, `context/view-snapshot.json`, and
  `context/scene.json`.
- Implemented: camera pose, avatar bounds, scene/model path, Godot pid, dev
  endpoint identity, and AIRI bridge status are recorded.
- Not implemented yet: Godot version, GPU backend, git commit, color-space
  limits, logs, comparison board, and full failure metadata.

### Phase 5: add Godot-side dev observation adapter and render export

Add a Godot-side dev observation adapter that runs inside the current Stage
process and accepts AUV observation requests. If it uses a local network
transport, it should only bind to `127.0.0.1`, require a token, and be disabled
outside development builds or explicit dev configuration.

The first enable flag is:

```text
AIRI_GODOT_STAGE_DEV_MODE=1
```

This flag means Godot Stage development-time tooling is enabled. The dev
observation adapter is the first consumer of that mode; future editor,
diagnostic, profiling, or render development tools may use the same mode with
separate capability flags if needed.

The first render export command should be equivalent to:

```text
render.export_outputs(output_dir, stages, restore_state)
```

The public dev request can enter through `StageRoot`, but render-specific work
should remain in the render runtime/compositor owner rather than expanding
`StageRoot` into a renderer.

The selected hook must be able to access the relevant render outputs from inside
the Godot process.

Current MVP status:

- Implemented: a Godot-side dev observation adapter in the running Stage process.
- Implemented: local WebSocket+JSON endpoint bound to `127.0.0.1`.
- Implemented: token-bearing discovery records.
- Implemented: render-stage export through StageRoot into the existing render
  debug-view pipeline.
- Known follow-up: render-specific work still enters through `StageRoot`; future
  cleanup should move more renderer-specific export behavior toward the render
  runtime/compositor owner.

### Phase 6: move orchestration into AUV

After the dev observation adapter and render export path exist, `auv-godot` can
own the standard observation flow:

- read discovery
- connect to the current Godot process
- confirm AIRI bridge status if available
- apply view preset or explicit camera
- request render export
- collect artifact bundle
- emit AUV command output

The orchestration path is Rust-only. `verifyVisualBaseline.mjs` remains useful
as historical scaffold and behavior reference, but the new AUV path should not
call into Node.js or depend on Node.js as an intermediate bridge.

Current MVP status:

- Implemented: root CLI exposes `auv godot capability-query` and
  `auv godot render-observe`.
- Implemented: Rust orchestration reads discovery, connects to the current
  Godot process, confirms capabilities, requests render export, captures final
  visible window evidence on Windows, writes context files, and emits command
  output.
- Not implemented yet: camera preset/state application before export.

### Phase 7: retire the legacy render-stage screenshot path

`captureWindowClientPng.ps1` can remain as historical evidence or emergency
diagnostic tooling, but it should not be used by the edge-light development
observation path.

This phase refers to the historical path where render-stage dumps were produced
by switching debug views and capturing the Godot window client area. The MVP
replaces that with Godot-side render-stage export.

The MVP still includes a final visible-window screenshot as presentation
evidence. That screenshot is not used to derive intermediate render stages.

## 10. Deferred topics

### Remote debug

Godot remote debug can still be useful for scene-tree inspection or live runtime
introspection. It can also become a future transport adapter if AUV implements a
compatible debugger host or if a Godot editor plugin forwards AUV requests into
an `EditorDebuggerSession`.

It is not the current mainline because the current problem is render output
observation from the same AIRI-owned runtime process. The required compositor
intermediate outputs still need Godot-side export code, and the remote debug
path has an additional editor/debugger-host requirement that the current AUV
crate does not yet satisfy.

### Editor plugin bridge

Editor-time automation is still relevant for future development tooling. It is
deferred here because the current AIRI sidecar path runs the Stage runtime, and
the current feature needs controlled render outputs rather than editor scene
editing.

### Test-time AUV

The game-like QA path is deferred. It may later need real input, UI interaction,
avatar manipulation, and test scenarios. That belongs to a different design
slice.

### Controlled replica mode

AUV-owned Godot runs remain useful for CI, hermetic fixtures, and no-AIRI
fallbacks. They are not the primary interactive development path because they do
not automatically share AIRI's current model, scene input, or live runtime
state.

### Cross-platform OS differences

For this feature, OS-specific screen capture should not be the intermediate
render-stage observation path. Godot-side export should remain responsible for
render layers and compositor-stage outputs.

The MVP uses Windows window capture only for the final visible-window
presentation artifact. Cross-platform parity for that final presentation
artifact is a separate follow-up from the Godot-side render export path.

If Godot writes files from inside the process, the cross-platform work should
mostly be path handling, local networking, process discovery, and file IO.
Renderer/backend differences may still matter for visual parity and must be
recorded in artifacts.

## 11. First-slice decisions

1. **Transport:** use WebSocket+JSON with simple request/response envelope (see
   Phase 2 for message schema). The MVP has validated the Godot C# server-side
   path with `TcpServer` and `WebSocketPeer`.
2. **Discovery:** write per-instance records under
   `~/.airi/godot-stage/dev/instances/<pid>.json` and maintain
   `~/.airi/godot-stage/dev/current.json`. Stale record handling: AUV validates
   PID existence and endpoint connectivity before use; Godot writes its own
   `<pid>.json` on startup and attempts best-effort deletion on clean shutdown.
3. **Port allocation:** validated in the MVP. Godot first binds to an
   OS-allocated local port with `TcpServer.Listen(0, "127.0.0.1")`, then writes
   the actual `GetLocalPort()` result to discovery. If that path fails, the
   adapter falls back to a fixed local port range.
4. **Enable flag:** use `AIRI_GODOT_STAGE_DEV_MODE=1`.
5. **Render format:** use PNG first; record format and color-space limits in the
   manifest for future HDR/EXR support.
6. **Orchestration:** implement the new AUV path in Rust only. Node.js scripts
   are references, not runtime dependencies.
7. **Final presentation evidence:** allow a final visible-window screenshot in
   the MVP, while keeping intermediate render stages Godot-exported.
8. **Camera input:** start with the current camera state in the MVP. Add
   `upper-body` preset and explicit camera state application as the next
   reproducibility step.
9. **Scope:** implement AIRI-specific Stage conventions first. Keep the split
   between `auv-godot` core, AIRI Stage adapter, and edge-light observer so
   future generic Godot tooling can be extracted later.

## 12. Current implementation state

Validated MVP, 2026-07-05:

- AIRI dev mode enables the Godot dev observation adapter by default for local
  development.
- AUV attaches to the current AIRI-owned Godot Stage process through discovery.
- `capability.query` reports `capability.query`, `render.export_stages`,
  available render stages, current pid, project path, and AIRI bridge status.
- `render-observe` exports the current render stages and writes a structured
  artifact bundle.
- The validated artifact contains six stage PNGs, a final visible-window
  screenshot, `manifest.json`, `context.json`, `view-snapshot.json`, and
  `scene.json`.

Not complete:

- Camera preset/state application.
- Project-path filtering for multiple simultaneous instances.
- Stable separation between generic `auv-godot` core, AIRI Stage adapter, and
  edge-light observer.
- Godot version, GPU backend, git commit, color-space limits, logs, comparison
  board, and richer failure metadata.
- Migration or retirement of `verifyVisualBaseline.mjs`.

## 13. Remaining open questions

1. Which edge-light intermediate outputs are mandatory for the first artifact
   bundle, and which are optional diagnostics?
2. Which additional camera presets are required after `upper-body`?
3. Should the artifact manifest record Godot version, GPU backend, or other
   renderer-specific metadata for reproducibility tracking?
