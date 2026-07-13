# Stateful Session Daemon And JS REPL Direction

Date: 2026-06-10

Status: design direction, docs-only. This records the owner discussion and
agreed boundaries for a future implementation plan. It does not approve
implementing the whole daemon, JS RPC, REPL, or domain-package ecosystem in one
slice.

## Purpose

AUV's current product crates, especially `auv-netease-music` and
`auv-game-balatro`, are too turn-based. Each CLI invocation tends to reopen
driver state, recapture the target, rerun recognition, reload inference
providers, and return a static result. That makes workflows slow and prevents
callers from reusing observed state across a longer interaction.

The desired direction is a stateful local session daemon plus typed client APIs
that can keep device connections, app sessions, model providers, detected UI
state, resource handles, and observation caches alive across calls. The daemon
should make AUV usable from JS/RPC and future REPL/browser/inspect surfaces
without turning CLI commands into the core API.

This document records the decisions from the design discussion so future agents
do not drift back toward stringly command RPC, Rust-generated HTML previews, or
domain-specific hardcoding in the daemon.

## Existing Project Anchors

The direction is already partially anticipated by existing contracts:

- `docs/TERMS_AND_CONCEPTS.md` defines `Device` and `Session`. A session groups
  target app/window defaults, observation cache, run recording state, and
  per-session permission/capability profile.
- `src/trace.rs` defines `DeviceId`, `SessionId`, `auv.device.id`, and
  `auv.session.id`.
- `src/model.rs` threads `device_id` and `session_id` through
  `DriverRunContext`.
- `src/model.rs` has provisional `CommandNamespace` metadata for future RPC
  routing, but that metadata is not the desired user-facing JS API.
- `src/inspect_server` already proves a local HTTP/WebSocket service,
  loopback/token write security, a user-private session descriptor, run update
  broadcasting, and browser viewer consumption.
- `docs/ai/references/inspect/2026-05-21-live-inspect-recording-design.md` records the
  inspect server write and live-stream precedent.
- `docs/ai/references/ops/2026-05-25-dream-architecture-rust-engineering.md`
  already calls for resource-style JS/REPL bindings and first-class
  device/session ownership.
- `docs/ai/references/apps/netease-music/2026-06-03-netease-cloud-music-domain-api-design.md`
  records the need to make `auv-netease-music` a Rust domain API before treating
  the CLI as the core surface.

## Decisions From This Discussion

The owner wants the future API to preserve these decisions:

1. The stateful daemon should manage sessions and reusable state, not merely
   expose old CLI commands over RPC.
2. JS RPC should feel like a typed API in the style of Playwright resource
   handles. Callers should not manually pass `session_id`, `window_ref`, or
   `candidate_id` in ordinary code.
3. The protocol may internally route by resource id and operation name, but the
   JS user-facing API should expose typed classes and methods such as `Client`,
   `Session`, `Window`, `Observation`, `Node`, and domain-package objects.
4. Domain-specific behavior should not become built-in properties of core
   daemon resources. NetEase Cloud Music, Balatro, and similar domains should
   be implemented by Rust domain crates and JS packages that compose core
   handles.
5. A package such as `@auvjs-community/netease-cloud-music` should be able to
   export `NeteaseCloudMusic`, plus package-owned types such as `Playlist` and
   `Song`.
6. The Rust side should gain a better domain API before RPC lands. For
   NetEase, the shape should move toward `NeteaseCloudMusic`, playlist/song
   services, typed observations, and action methods rather than standalone
   `run_live_scan` / `run_playlist_play_candidate_id` style functions.
7. AUV should grow a REPL capability that can interactively script, parse, and
   preview live application state through the session daemon.
8. REPL/browser/inspect previews should be dynamic and reactive. Rust should
   produce live IR, resources, versioning, invalidation, and evidence links; it
   should not convert observations into HTML.
9. Browser, TUI, Swift, Kotlin, and other client runtimes should map the same
   live IR into their own UI systems.
10. The daemon should keep expensive state alive: ORT sessions, OCR or detector
    providers, app/window handles, detected states, node positions, ref handles,
    frame hashes, and cache entries.
11. The first useful version does not need full continuous realtime. Explicit
    refresh, invalidation after actions, debounce/watch streams, and provider
    reuse are enough to prove the model.

## Inspect Server Relationship

The inspect server is a strong precedent but not the execution daemon.

Reuse these ideas:

- local HTTP/WebSocket shape
- loopback-by-default serving
- write token and user-private descriptor security
- store plus broadcaster separation
- conflict rejection instead of silent overwrite
- browser viewer loading a snapshot and then subscribing to updates

Do not reuse these semantics:

- `InspectServerSession` is a discovery descriptor for inspect reporting. It is
  not the automation session.
- `/runs/{run_id}/stream` streams run updates. It is not the app/session
  observation stream.
- The inspect server does not own driver sessions, action locks, app target
  defaults, resource handles, model providers, or observation caches.

The intended split is:

```text
auv session daemon
  owns devices, sessions, resource handles, provider caches, observation state
  executes observe/action operations
  records runs through RunRecordingBackend
  may report run data to inspect server

auv inspect server
  reads stored run data and artifacts
  streams run updates to viewers
  may render links to session resources when such bridging exists
```

## Core Resource Model

The daemon protocol should be resource-oriented. The stable resources are
domain-neutral:

- `Device`
- `Session`
- `Window`
- `Observation`
- `SurfaceNode` / `Node`
- `Candidate`
- `Run`
- `Artifact`
- provider/cache resources when they need explicit lifetime management

Handles are opaque to clients. A JS user should usually write:

```javascript
const client = new Client();
await client.connect();

const session = await client.sessions.open({
  target: { app: "com.netease.163music" },
});

const window = await session.mainWindow();
const observation = await window.observe();
const node = await observation.find({ text: /Trance vol.2/ });
await node.click();
```

The transport may internally send:

```json
{
  "target": "node:n_456",
  "op": "click",
  "params": {}
}
```

That resource id is a wire detail, not the ordinary JS authoring model.

## JS Core API Shape

The preferred style is class-based and typed:

```javascript
import { Client } from "@auvjs/core";

const client = new Client();
await client.connect();

const session = await client.sessions.open({
  target: { app: "com.apple.Music" },
});

const window = await session.mainWindow();
const observation = await window.observe();
const rows = await observation.findAll({ role: "row" });
await rows[0].click();
```

`new Client()` is appropriate because it owns the daemon/RPC connection. Users
should not normally call `new Session()` to allocate a daemon session, because
the daemon must allocate the session id, resource table entries, action lock
state, and recording policy. A `Session` class may exist to wrap an existing
session ref returned by the client.

`CommandSpec.namespace` and legacy command ids may help compatibility, but the
JS API should not center on `session.invoke("some.string.command", params)`.
That would preserve the wrong CLI-shaped model.

## Domain Packages

Domain-specific behavior belongs in packages that compose core handles.

Example:

```javascript
import { Client } from "@auvjs/core";
import { NeteaseCloudMusic } from "@auvjs-community/netease-cloud-music";

const client = new Client();
await client.connect();

const session = await client.sessions.open({
  target: { app: "com.netease.163music" },
});

const music = new NeteaseCloudMusic(session);
const playlist = await music.playlists.findByName("Trance vol.2");
await playlist.play();
```

`@auvjs-community/netease-cloud-music` may define its own `Playlist`, `Song`,
`Playback`, and projection services. Those objects should hold or derive from
core handles and evidence refs. For example, a `Playlist` can carry a
`NodeHandle`, source `ObservationHandle`, confidence, and artifact refs, then
perform reacquisition if the node becomes stale.

The core daemon should not expose built-in APIs such as
`window.netease.sidebar()` or `session.balatro.state()` as part of the generic
resource model. Domain packages can expose those names in their own namespace.

## Rust Domain API Requirement

Before RPC, product crates should move toward the same typed model in Rust.

For NetEase Cloud Music, the direction is:

```rust
let music = NeteaseCloudMusic::new(session);
let playlist = music.playlists().find_by_name("Trance vol.2")?;
playlist.play()?;
let current = music.playback().current_song()?;
```

The CLI should become a thin adapter that parses arguments, opens or attaches a
session, calls the typed domain API, and renders output. It should not own the
workflow logic.

For Balatro, the analogous direction is:

```rust
let game = Balatro::new(session);
let state = game.state().observe()?;
state.hand().card(0).select()?;
state.buttons().play().click()?;
```

These examples are illustrative. A later implementation plan should choose one
domain and one narrow slice rather than implementing both at once.

## REPL And Live Programming Surface

The desired REPL is not a turn-based command runner. It should let users write
scripts that hold handles and state across statements, derive projections from
live observations, preview those projections, and call back into daemon
resources for real actions.

Example authoring shape:

```javascript
const window = await session.mainWindow();
const observation = await window.observe();

const rows = await observation.findAll({ role: "row" });
preview(rows);

await rows[3].click();
```

The preview should be reactive. A browser REPL may render rows as HTML, a TUI
may render them as selectable list rows, and a Swift client may render them as
SwiftUI state. Rust should not generate those UIs.

The daemon should provide:

- resource handles
- observation IR
- resource versioning
- invalidation events
- action start/finish events
- artifact and run links
- subscriptions or watch streams

The client runtime should provide:

- signals/effects/listeners over daemon updates
- projection functions from IR into UI-specific state
- event handlers that call daemon resource methods
- domain-specific parsing and live preview logic

This supports a future `repl browser` or `repl inspect` where a user can script
interactive parsing and preview the current app state without relying on an
agent turn.

## Live IR, Not Rust-Generated HTML

The daemon is responsible for IR and resource lifecycle, not presentation.

Rust-produced data should look like neutral observation and resource updates:

```text
resource_changed
node_added
node_updated
node_removed
snapshot_invalidated
action_started
action_finished
artifact_created
run_linked
```

Clients can derive presentation:

```javascript
const playlists = createMemo(() => projectPlaylists(observation.nodes()));

effect(() => {
  renderHtmlList(playlists(), {
    onClick: item => item.node.click(),
  });
});
```

The same IR should support browser DOM, TUI lists, SwiftUI views, Kotlin
Compose, or other client environments.

## Daemon Cache And Provider Ownership

The stateful daemon should keep these categories alive when possible:

- device connection and permission/capability state
- target app/window defaults
- device-level mutating action lock
- ORT sessions and loaded models
- OCR, AX, capture, detector, and segmentation providers
- latest observation snapshots
- detected app states
- node positions and coordinate contracts
- resource handles and liveness metadata
- model outputs keyed by frame hash, model id, and config
- cropped-region and OCR result caches
- scroll/list accumulation state
- candidate reacquisition indexes

This is especially important for:

- `auv-game-balatro`, where entity/UI detector ORT sessions and latest
  `BalatroState` should not be reloaded on every CLI call.
- `auv-netease-music`, where sidebar projection, playlist candidate maps,
  scroll evidence, playback state, and OCR caches should survive across
  related operations.

## Invalidation And Freshness

Caching requires explicit freshness semantics. A future implementation should
include resource versions and stale reasons:

```text
ObservationHandle
  id
  version
  source_frame_hash
  captured_at_millis
  scope
  stale_reason?

NodeHandle
  id
  observation_id
  node_id
  bounds
  liveness
  version
```

Default action policy:

```text
click/type/scroll
  -> mark affected observation resources stale
  -> emit resource invalidation
  -> optionally schedule refresh
  -> next read returns stale error or refreshes according to caller policy
```

The v0 does not need continuous high-frequency observation. Explicit refresh,
action invalidation, debounce/watch streams, and provider reuse are enough to
prove the model.

## Suggested Narrow First Slice

The first implementation slice should be small and verifiable:

```text
Stateful local session resource v0 with one proof domain or fixture
```

Recommended scope:

- local device only
- in-process Rust API first, with protocol DTOs shaped for RPC
- resource table with `Session`, `Window`, `Observation`, and `Node`
- resource versions and stale/invalidation events
- one provider cache proof, preferably a fixture provider or Balatro ORT model
  reuse if fixture coverage is insufficient
- JS client smoke that opens a session, observes a window/fixture, holds node
  handles across statements, performs one action, observes invalidation, and
  closes the session
- run recording link from session operations to existing `RunRecordingBackend`

Non-goals for the first slice:

- full gRPC implementation
- remote devices
- browser REPL UI
- all NetEase and Balatro domain objects
- Rust-generated HTML previews
- replacing inspect server
- exposing CLI command ids as the JS API

## Open Questions

- Whether the first transport adapter should be HTTP JSON plus WebSocket,
  stdio JSON-RPC, Unix socket JSON-RPC, or another local channel.
- Whether the first proof domain should be a fixture UI, NetEase sidebar, or
  Balatro detection state.
- How much of the JS REPL should run in browser versus Node in v0.
- Whether domain packages should publish their own preview projection schemas
  or rely entirely on client-side code conventions.
- How resource garbage collection should work when clients disconnect without
  closing handles.

