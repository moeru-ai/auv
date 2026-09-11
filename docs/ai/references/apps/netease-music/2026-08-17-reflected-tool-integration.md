# NetEase Music Reflected Tool Integration

Date: 2026-08-17

Status: implemented. Descriptor, Rust, SDK, real daemon Reflection, AIRI
Electron, and AIRI web adapter checks pass.

## Contract

`auv-netease-music` registers the RunnerClass `auv.app.netease_music`. Its
Runner serves standard gRPC Health and Reflection. It also serves app-owned
Application, Player, Playlist, Recommendation, and Song services in the
`auv.netease_music.v1` package. The split keeps operation names short without
losing their domain when discovery projects them into APIs and AIRI later
adapts those APIs into tools.

The daemon is an opaque routing proxy. It starts the configured provider when a
request arrives. It keeps Run and Device affinity. It forwards Reflection and
business RPC frames without a second operation catalog.

`@auv-js/sdk` now supplies the generic client-side adapter:

```text
AIRI / Node / browser
  -> discoverRunner(runnerClass)
  -> standard grpc.reflection.v1.ServerReflection
  -> FileDescriptorSet + AUV method annotations
  -> ProtoJSON input/output + JSON Schema API descriptors
  -> routed unary or server-streaming invocation
```

The discovery interface exposes only methods with AUV's `discoverable`
annotation through its `apis` field. The complete reflected method surface
stays private to the implementation. The effect annotation supplies
presentation and confirmation metadata. It does not authorize an API.
Generated clients remain the normal choice for typed Rust and application
code.

The Runner descriptor set must include all transitive dependencies. The NetEase
build therefore lists AUV's annotations proto as a compiler input. The first
real Node probe found this requirement. A dynamic registry cannot decode an
import when Reflection cannot return the imported file.

The probe also found a server-side data loss. `tonic-reflection` decodes each
`FileDescriptorSet` through `prost_types`. It then encodes each
`FileDescriptorProto` response again. `prost_types` discards protobuf extension
fields. Thus, all AUV `discoverable` and `effect` options disappeared.

`auv-api-server::reflection` now owns the shared v1 Reflection adapter. It uses
`prost-reflect` to index and encode descriptors. This process preserves
registered custom options. NetEase, the local driver, and Balatro use this
adapter. The change corrects the shared protocol boundary.

## NetEase RPC coverage

The Runner reflects the existing app operations rather than a generic argv or
JSON command:

- now-playing read
- dedicated play, pause, toggle-player, next, and previous operations
- seek
- playlist list, select, and play
- Daily Recommended play
- song list from an explicit Daily Recommended or semantic playlist source
- playback-status probe
- open-window

The UI and OCR operations are synchronous at the app or Driver boundary. The
tonic adapter uses `spawn_blocking` for these operations. Thus, a long scan does
not stop unrelated async RPC progress.

`ListPlaylists` and `ListSongs` are server-streaming scan contracts. A
successful stream emits zero or more typed item and diagnostic events, then
exactly one method-specific completed event. RPC failure ends the stream without
a completed event. The completed event carries the emitted count and scan
limits; playlist completion also carries the observation count.

This is a scan-list policy, not a naming rule for every future `List*` RPC.
Operations backed by a bounded in-memory or remote snapshot may remain unary;
operations that traverse live UI content should default to server streaming so
their transport does not need another breaking cardinality change when
incremental scan production lands.

The current app-owned scans still return one aggregate after UI traversal, so
the Runner begins emitting only after the scan finishes. This establishes the
transport and reflection contract without pretending the scan is already
incremental. Progressive emission and cancellation require a typed event sink
and cancellation boundary in the app-owned scan operation; inline deferral
markers identify both call sites.

Most GUI scans support only macOS. `OpenWindow` supports only Windows. Windows
does not have reliable idempotent play and pause controls.

Only `GetNowPlaying` has the `READ_ONLY` annotation. Playlist and song scans can
scroll or navigate the UI. Playback-status inspection can click the song-detail
bar. Control, open, select, and play operations also deliver input.

These methods have the `INPUT` annotation, even when they return read-like
data. Tool hosts must base confirmation policy on actual interaction, not the
RPC name.

## AIRI integration shape

The Electron main process owns the AUV daemon. It accepts provider manifest
paths through `AUV_RUNNER_PROVIDER_PATHS`. Its AUV MCP provider reflects
`auv.app.netease_music`. It converts discoverable unary and server-streaming
methods to `netease_music_<domain>_*` tools. It proxies calls without a
generated NetEase SDK. Daemon owner authority and local IPC stay outside
renderers.

The web adapter cannot start or register a native executable. It connects to a
paired AUV HTTP endpoint that already runs. It uses the daemon's WebSocket route
for Reflection and server streams. It uses HTTP for unary methods.

The adapter installs `neteaseMusic_<domain>_*` tools in AIRI's LLM tool store.
Browser local storage contains the explicit opt-in values. The keys are
`airi.auv.endpoint` and `airi.auv.credential`.

## Integration gaps and boundaries

- RunnerProvider admission is startup configuration. An extension cannot add a
  provider after daemon startup. The daemon must restart with that provider
  manifest.
- The current AUV release archive and AIRI sidecar staging package the `auv`
  daemon binary but not `auv-runner-netease-music`. The Electron integration is
  runnable in development with `AUV_RUNNER_PROVIDER_PATHS`. A distributable
  AIRI bundle still needs a versioned and signed Runner asset. An extension can
  own this asset.
- AIRI currently starts its Electron AUV sidecar only on macOS. Runner-owned
  inherited IPC also has no Windows named-pipe implementation. The Windows
  `OpenWindow` and UIA transport operations remain available through the typed
  CLI or library path, but not through this reflected daemon route. The inline
  `netease-runner-windows-ipc` deferral marks this boundary.
- The current annotations carry discovery and effect, but not user-facing
  titles/descriptions. AIRI derives a conservative description from the
  service and method name. An AIRI extension can replace the presentation
  metadata.
- AIRI's current MCP provider and web `ExecutableTool` contracts have no
  effect-aware approval hook. The adapters include the reflected effect in the
  tool description, but this text is not authorization. AIRI needs a host-owned
  confirmation contract before it can enforce `INPUT` approval at this layer.
- Those AIRI tool contracts also return one final value. The adapters therefore
  collect reflected server-stream packets into `{ events: [...] }`. The generic
  `@auv-js/sdk` discovery interface preserves the native `AsyncIterable`;
  AIRI can forward progress and cancellation after its host tool contract owns
  them.
- `PlaylistRef` is a semantic `section + label` value returned by
  `ListPlaylists` and consumed by playlist select, play, and song-list
  operations. Parse-scoped item, candidate, and anchor ids remain diagnostic
  evidence and are never replayed across scans. The reflected JSON Schema still
  describes wire shape rather than business-required values; the Runner rejects
  a missing reference, an unspecified section, or an empty label with
  `INVALID_ARGUMENT`.
- The JavaScript dynamic adapter intentionally implements unary and
  server-streaming business calls. Client-streaming and bidi business-tool
  projection need a concrete input and cancellation contract. Raw typed duplex
  invocation remains available.
- A hosted HTTPS AIRI page still depends on browser mixed-content/private-network
  policy when connecting to a local HTTP/WebSocket daemon. The real browser test
  proves the AUV protocol path in a development setup. It does not prove a
  hosted-web deployment. Production web use needs a secure endpoint and a
  credential delivery design.
- AIRI `stage-web` does not currently bootstrap its extension host, and the web
  plugin runtime still lacks a deployable entrypoint loader/transport. The
  current web integration is therefore an app bootstrap adapter that registers
  reflected definitions in the LLM tool store. It is not an installable
  `defineExtension` bundle. The projection can use `registerTools` after the web
  host adds an entrypoint loader and transport.
- AIRI currently consumes the published `@auv-js/sdk` release. Direct
  reflection reports a clear error until a release contains Runner Reflection.
  The Electron MCP provider keeps its static tools and omits reflected tools in
  that case. AIRI must then select the new release in its workspace catalog.

## Validation evidence

The integration has tests at these layers:

- descriptor, annotation, and schema projection with a fake transport.
- Node against a real daemon and daemon-spawned NetEase Runner.
- reflected server-streaming invocation against the real NetEase and local
  Runners, including NetEase source validation before UI work.
- a real Chromium browser against paired AUV HTTP/WebSocket listeners and the
  same registered Runner.
- AIRI's Electron AUV manager and MCP provider against the same real daemon and
  daemon-spawned Runner. This test finds all fourteen `netease_music_*` tools. It
  verifies the reflected Daily Recommended and `PlaylistRef` song-source
  schema, then calls `GetNowPlaying` without a generated NetEase client.
- AIRI's web adapter unit tests and stage-web typecheck.

The real integration probes use a nonexistent application bundle ID. A
successful call returns the empty ProtoJSON object `{}`. ProtoJSON omits the
protobuf default `present = false`. This tests routing and decoding without an
open NetEase Music process. It does not change user playback state.

The semantic ordinary-playlist path has Rust resolution and request-validation
coverage, but this validation session had no running NetEase Music process.
Selecting a real `PlaylistRef` and scanning that playlist's live song table
therefore remains a live-probe evidence gap; compilation and daemon E2E are not
being presented as proof of that UI behavior.

The existing Rust custom-runner test covers daemon supervision and generated
client invocation. It also covers inherited `AUV_CONTEXT` and the standalone
NetEase CLI frontend.
