# NetEase Cloud Music Domain API Design

Date: 2026-06-03

Status: proposed API design, docs-only. This records the agreed direction for a
future implementation slice; it does not approve implementing every API in this
document at once.

## Purpose

`auv-netease-music` currently exposes useful behavior through a product CLI, but
the CLI is only one frontend. The same NetEase Cloud Music operations should be
callable from CLI, MCP, recipes, tests, and future UI surfaces without copying
workflow logic into each frontend.

The shared layer should model NetEase Cloud Music as an app-specific domain API
that can observe the current UI, classify app state, perform typed actions, and
emit recorder-facing evidence. `inspect` remains a human-facing viewer over the
recorded data; the domain API should not depend on inspect server semantics.

## Existing Evidence

The current crate already has most of the product-specific recognition logic:

- sidebar region detection and playlist reconstruction in
  `crates/auv-netease-music/src/lib.rs`
- song-detail restore detection via the existing default-screen restore path
- bottom player control classification for daily recommended playback
- scroll motion evidence and AX scrollbar corroboration
- standalone screenshot, OCR, overlay, and interaction-event artifact writes

Root AUV already has run recording, spans, events, artifact staging,
`RunRecorder`, and inspect server streaming. The missing boundary is an app
domain API that uses those facilities instead of treating the CLI as the core
execution model.

## Core API Shape

The main app client should be named `NeteaseCloudMusic`.

```rust
pub struct NeteaseCloudMusic {
  // Driver session, resolved window, options, and recorder-facing sink.
}

pub struct NeteaseCloudMusicObservation {
  // App-specific read views derived from the same observation/reconstruction.
}
```

The intended common path:

```rust
let mut app = NeteaseCloudMusic::connect(options)?;

let observation = app.observe()?;
if observation.screen.is_playing_song_detail() {
  app.restore_default_screen()?;
}

if observation.sidebar.exists() {
  app.go_to_recommendation()?;
}
```

`NeteaseCloudMusicObservation` is one immutable observation of the app window at
a point in time. It is not a general evidence bag. It is an app-specific read
model derived from capture, OCR, AX, reconstruction, and domain projection data.
Callers should not need to understand those internals for common predicates such
as `screen().is_default()` or `sidebar().exists()`.

## Naming

Use Rust module names for boundaries and state/action names for types:

- `NeteaseCloudMusic`: executable app client/session.
- `NeteaseCloudMusicObservation`: read-only app view over one observation.
- `screen`, `sidebar`, `player`: Rust modules.
- `ScreenView`, `SidebarView`, `PlayerView`: domain read facades.
- `ScreenState`, `SidebarState`, `PlayerState`: compact state records used by
  those facades.

Do not name public types `ScreenModule` or `SidebarModule`. If action surfaces
become large enough to split from `NeteaseCloudMusic`, prefer handle names such
as `SidebarActions<'a>` or `PlayerActions<'a>`. v0 should keep action methods on
`NeteaseCloudMusic` until the method set proves that handles would hide real
complexity.

## API Categories

The API should make IO and UI mutation visible in method names and return types.

Pure observation reads:

```rust
observation.screen().is_default();
observation.screen().is_playing_song_detail();
observation.sidebar().exists();
observation.sidebar().find_playlist("daily");
observation.player().exists();
```

Fresh observation:

```rust
let observation = app.observe()?;
let refreshed = app.refresh()?;
```

`observe()` may reuse a valid cached observation within the current app/window
generation. `refresh()` is a shortcut for forcing a new capture/OCR/reconstruct
pass.

Actions:

```rust
app.restore_default_screen()?;
app.go_to_recommendation()?;
app.go_to_created_playlists()?;
app.play_daily_recommended()?;
```

Action methods must use `auv-driver` / `auv-driver-macos` for input delivery and
return typed operation evidence. They should not return only `bool`, because
callers need to inspect delivery path, fallback reason, verification result, and
evidence artifacts.

## Reconstruction And Cache Reuse

`NeteaseCloudMusicObservation` should be immutable. A new capture/OCR/AX pass
creates a new observation and, when needed, a new reconstruction. Existing
observations remain useful as historical evidence but must not be treated as the
current UI after an action mutates the app.

The app client may cache the latest observation to avoid repeated OCR and
reconstruction inside one orchestration step. Cache policy should be explicit:

```rust
pub struct ObserveOptions {
  pub reuse: ObserveReuseMode,
}

pub enum ObserveReuseMode {
  ReuseValidCache,
  ForceRefresh,
}
```

Default behavior may use `ReuseValidCache`. Mutating actions such as
`restore_default_screen()`, `go_to_recommendation()`, and
`play_daily_recommended()` must invalidate the cached observation unless they
return a fresh post-action observation as part of their result.

`SidebarView` should be derived from the reconstructed/projection data rather
than re-running a parallel OCR heuristic:

```text
ViewObservation
  -> ViewReconstruction
  -> PlaylistSidebarProjection
  -> SidebarView::exists / find_playlist / created_playlists
```

This keeps CLI, MCP, and inspect debugging aligned on one source of truth.
`inspect` can render the underlying reconstruction and artifacts; the domain API
can expose a smaller NetEase-specific read facade over that same data.

## Recording Boundary

The domain API should emit recorder-facing spans, events, and artifacts through
a generic recording sink. Inspect server reporting is one possible sink
configuration, not a dependency of the NetEase API.

```text
NeteaseCloudMusic observe/action
  -> recorder-facing spans/events/artifacts
  -> local store and/or inspect server delivery
  -> inspect viewer renders the recorded state
```

This keeps CLI, MCP, and future app surfaces on the same execution model:

```text
CLI args / MCP tool params / recipe step
  -> NeteaseCloudMusic options
  -> same observe/action API
  -> same OperationResult / VerificationResult / artifacts
```

## Inspect Relationship

`inspect` is a human-facing devtools and debugging surface. It should read and
render observations, reconstructions, projections, actions, verification
results, and artifacts produced by the domain API.

The domain API should not call inspect-specific endpoints or depend on inspect
viewer schema. If inspect needs more data, the domain API should record better
typed evidence; the viewer should then render that evidence.

## First Implementation Slice

The first approved implementation slice should be narrow:

```text
NeteaseCloudMusic observation + screen classifier v0
```

Scope:

- introduce `NeteaseCloudMusic` and `NeteaseCloudMusicObservation`
- introduce `ScreenState::{Default, PlayingSongDetail, BlockingModal, Unknown}`
- expose pure predicates on `ScreenState`
- define `ObserveReuseMode::{ReuseValidCache, ForceRefresh}` and cache
  invalidation rules for mutating actions
- adapt the existing default-screen/song-detail restore detection logic to
  consume the classifier
- preserve existing behavior and tests

Non-goals for the first slice:

- full sidebar/player module API
- MCP tools
- inspect viewer changes
- persistent view memory
- moving all playlist reconstruction code out of `lib.rs`

## Follow-Up Candidates

1. Add `SidebarState` with `exists()` and sidebar-region evidence.
2. Add `PlayerState` with bottom-player existence and playback-control state.
3. Move `playlist ls` and `daily-recommended` to call `NeteaseCloudMusic`.
4. Add recorder-facing view/parser artifacts for NetEase observations.
5. Add MCP tools that call the same domain API as the CLI.
6. Extend inspect viewer read-side rendering for NetEase observations and
   reconstruction artifacts.
