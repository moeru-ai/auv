# auv-cli-invoke Driver Console Design

Date: 2026-06-11

Status: implemented for the handler-first invoke command boundary

Scope classification: approved feature design

## Purpose

Redesign `auv-cli invoke` as the CLI console for AUV driver capabilities.
After PR #35, the old JSON `skill`/recipe lane is gone. The remaining root
legacy lane is the command catalog plus runtime-owned command lookup. The next
slice should move invoke command ownership out of root runtime and make the
user-facing command model match atomic driver capabilities, not historical
`debug.*`, `verify.*`, or app-workflow ids.

`invoke` should answer two user questions well:

- What driver capabilities are available?
- For one capability, what arguments, effects, artifacts, and verification
  semantics should I expect?

## Decision

`invoke` becomes a driver capability console.

The command namespace should be capability-oriented and camelCase:

- `display.*`
- `screen.*`
- `window.*`
- `input.*`
- `app.*`
- `overlay.*`
- `mediaControl.*`

Do not preserve old command id aliases. This project is not public, and the
current migration goal is to remove unnecessary legacy compatibility. Old ids
such as `debug.captureWindow`, `verify.musicNowPlaying`, and `music.result.play`
should disappear from the invoke registry instead of being carried as aliases.
When practical, an unknown-command error may suggest the new namespace family,
but it must not execute through an alias mapping.

## User Experience

Replace `list-commands` with `invoke --help` as the discovery surface.

`auv-cli invoke --help` should show:

- top-level usage
- grouped command index by namespace
- short summaries
- pointer to `auv-cli invoke <command> --help`

Example shape:

```text
USAGE
  auv-cli invoke <command> [options]

DISPLAY
  display.list
  display.capture
  display.identifyPoint
  display.projectScreenshotPoint

WINDOW
  window.list
  window.capture
  window.captureAxTree
  window.findText
  window.clickText

MEDIA CONTROL
  mediaControl.nowPlaying
  mediaControl.play
  mediaControl.pause
  mediaControl.togglePlayPause
  mediaControl.next
  mediaControl.previous

Use `auv-cli invoke <command> --help` for command-specific options.
```

`auv-cli invoke <command> --help` should show:

- command id
- summary
- backend driver and operation
- argument schema
- disturbance classes
- artifacts/signals produced when known
- verification semantics

Example shape:

```text
COMMAND
  mediaControl.pause

SUMMARY
  Pause the active system media session through the media control backend.

USAGE
  auv-cli invoke mediaControl.pause [--verify true|false|state]

VERIFY
  default: state
  success: playbackState == paused
  inconclusive: control sent but backend did not expose stable state
```

`auv-cli list-commands` should be removed as a normal command surface. If the
parser keeps a short human-facing tombstone during the transition, it may only
fail with guidance such as `use auv-cli invoke --help`; it must not own a
second registry path, print a separate index, or preserve old command behavior.

## Namespace Model

### Display

Display commands inspect or capture physical/logical display surfaces.

Candidate commands:

- `display.list`
- `display.capture`
- `display.identifyPoint`
- `display.projectScreenshotPoint`

### Screen

Screen commands operate on the user-facing desktop coordinate surface.

Candidate commands:

- `screen.capture`
- `screen.findText`
- `screen.waitForText`
- `screen.findRows`
- `screen.waitForRows`
- `screen.clickText`
- `screen.clickRow`

### Window

Window commands inspect, capture, recognize, verify, or act inside resolved
windows.

Candidate commands:

- `window.list`
- `window.capture`
- `window.captureAxTree`
- `window.findText`
- `window.waitForText`
- `window.findRows`
- `window.waitForRows`
- `window.observeRegion`
- `window.scrollRegion`
- `window.findIconMatch`
- `window.clickText`
- `window.clickRow`
- `window.verifyText`

`window.verifyText` is the honest replacement namespace for the old AX/window
text verification path when it remains useful. Do not call AX/window text
matching `mediaControl.*`.

### Input

Input commands deliver atomic input actions. They should describe their
disturbance and verification boundary clearly.

Candidate commands:

- `input.key`
- `input.typeText`
- `input.pasteText`
- `input.clickPoint`
- `input.clickWindowPoint`
- `input.scrollPoint`
- `input.focusText`
- `input.pressButton`
- `input.axFocusText`
- `input.axPressButton`
- `input.axClickWindowText`
- `input.smartPress`
- `input.teachClick`

### App

App commands handle application-level primitives, not app-specific workflows.

Candidate commands:

- `app.activate`
- `app.probePermissions`

Keep app-local workflows in app/domain crates such as `auv-qqmusic`,
`auv-apple-textedit`, and `auv-apple-notes`.

### Overlay

Overlay commands remain visual-only debug/trust presentation tools. They should
be clearly marked as not semantic verification.

Candidate commands:

- `overlay.showCursor`
- `overlay.showDualCursor`
- `overlay.applyCursorBatch`
- `overlay.setCursor`
- `overlay.moveCursor`
- `overlay.moveCursorById`
- `overlay.flashCursor`
- `overlay.flashCursorById`
- `overlay.hideCursor`
- `overlay.hideCursorId`
- `overlay.shutdown`

### Media Control

`mediaControl.*` is the cross-platform media session capability namespace. It
is the invoke/debug surface for `auv-media-*`, not a place for QQ Music,
NetEase, or other app-specific workflows.

Candidate commands:

- `mediaControl.nowPlaying`
- `mediaControl.play`
- `mediaControl.pause`
- `mediaControl.togglePlayPause`
- `mediaControl.next`
- `mediaControl.previous`

The first backend may be `auv-media-macos`, but the command namespace must stay
platform-neutral so future Windows/Linux backends can implement the same
capability contract.

Transport controls should be as verifiable as the backend allows:

- `mediaControl.nowPlaying` produces a structured artifact and signals for
  title, artist, album, bundle id, playback state, elapsed time, and duration
  when available.
- `mediaControl.play` / `mediaControl.pause` should re-read now-playing state
  by default and validate the expected playback state when the backend exposes
  one.
- `mediaControl.next` / `mediaControl.previous` should re-read now-playing
  state and validate a plausible track identity/title/elapsed transition when
  the backend exposes stable identity.
- If the backend accepts the control but cannot expose stable state, the
  command must report `verification=inconclusive` or an equivalent structured
  result, not silently claim semantic success.

## Explicit Exclusions

Do not carry app-specific music workflow commands into the new invoke registry:

- remove `music.search.results`
- remove `music.result.play`
- remove `music.validate.candidate.liveness`

Those belong in app/domain crates. For example, QQ Music result selection
belongs under `auv-qqmusic search results ...`, not `auv-cli invoke`.

Do not reintroduce JSON recipe, case-matrix, or bundle compatibility.

Do not design the REPL in this slice.

Do not move run recording into `auv-cli-invoke`; that belongs to the later
`auv-tracing-driver` split.

## Command Declaration Model

The first extraction introduced a command tree: each capability domain owns its
subtree and exposes a `group()` function that returns a `CommandGroup`. The
root registry composes domain groups, recursively flattens commands for lookup,
and traverses the same tree for help rendering.

The command tree is now handler-first. Each command is declared by annotating
the function that handles the command, and the attribute generates the
`*_invoke_command()` export used by the domain group.

Current shape:

```rust
pub fn group() -> CommandGroup {
  CommandGroup::new("screen", "SCREEN")
    .command(capture_region_invoke_command())
    .command(find_screen_text_invoke_command())
}

#[invoke_command(
  id = "screen.captureRegion",
  group = "screen",
  summary = "Capture one display-contained region and emit a coordinate contract.",
  driver = "macos.desktop",
  operation = "capture_region",
  args = REGION_ARGS,
  disturbance = NONE_OR_FOREGROUND,
  max_disturbance = OperationDisturbance::ForegroundApp,
  artifacts = ["region-capture", "capture-contract"],
  signals = [],
  verification = "capture-only; no semantic success claim",
)]
pub fn capture_region(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}
```

The attribute generates an `as_invoke_command`-style export as a free function
named from the handler, for example `capture_region_invoke_command()`. The
generated `InvokeCommand` carries both:

- `CommandGroup` and `CommandNode`
- `InvokeCommand`
- argument descriptors
- help rendering inputs
- disturbance metadata
- optional artifact/signal/verification notes
- a handler reference or equivalent resolved execution entrypoint

The purpose is direct traceability: reading one command module now shows the
CLI command id, argument contract, driver mapping, artifact/signals contract,
and the function that handles the command. The first macro slice deliberately
does not solve typed args comprehensively. Commands still use the existing
shared `ArgSpec` constants and default driver dispatch. Typed argument structs
can follow once the handler boundary is stable.

## Architecture

Introduce an `auv-cli-invoke` boundary as either a workspace crate or a
temporary root module with an explicit deferral marker at the module boundary.
The preferred target is a crate because the goal is to keep frontend command
metadata out of root runtime. If the first PR starts as a root module to reduce
churn, the marker must state that extraction reopens when the command registry
no longer depends on root runtime internals.

Responsibilities:

- own invoke command metadata
- render `invoke --help`
- render `invoke <command> --help`
- parse command-specific invoke arguments into the existing request shape for
  commands still using the temporary driver adapter
- own invoke command handler registration and dispatch once the handler-first
  boundary lands
- expose command lookup for CLI and MCP frontend use

Non-responsibilities:

- run lifecycle
- artifact staging
- inspect server write behavior
- app workflow orchestration
- JSON recipe execution
- REPL state

The root runtime should stop owning command registry semantics. If runtime still
executes the temporary adapter path, it should receive an already-resolved
command descriptor or execution request from `auv-cli-invoke`.

TODO(invoke-boundary): passing an already-resolved descriptor into runtime is
deferred until CLI, MCP, app-probe, and scroll-scan share the next typed invoke
request. The current extraction may still call the `auv-cli-invoke` registry
from runtime as a temporary adapter, but no legacy command ids or alias tables
should be reintroduced there.

## Migration Strategy

This is a breaking rename by design.

The old ids should be removed from the invoke registry:

- `debug.*`
- `verify.*`
- `music.*`

Tests should assert that old ids no longer resolve. If an error hint is added,
it must be non-executable and should not create an alias table that keeps old
ids alive.

Suggested first implementation slice:

1. Add the new `CommandGroup` / `CommandNode` / `InvokeCommand` metadata model.
2. Add help rendering for `invoke --help` and `invoke <command> --help`.
3. Rebuild the current root catalog as the new capability registry with renamed
   ids, excluding app-specific `music.*`. Each command domain should register
   its own group instead of adding commands to one flat root table.
4. Remove `list-commands` as a first-class command. A parser tombstone is
   acceptable only if it fails and points to `invoke --help`.
5. Update CLI/MCP/runtime tests to use new ids.
6. Delete `src/catalog.rs` after callers move to the new boundary.
7. In a follow-up slice, replace runtime-side string lookup with an
   already-resolved invoke command descriptor.
8. Replace string-only `spec(...)` declarations with handler-owned
   `#[invoke_command]` declarations that generate `as_invoke_command` exports.

Exit criteria:

- `src/catalog.rs` is deleted.
- Root runtime no longer owns `CommandCatalog` or command id discovery. Any
  remaining runtime call into `auv-cli-invoke::default_registry()` must carry a
  `TODO(invoke-boundary)` marker and be removed by the resolved-descriptor
  follow-up.
- `auv-cli invoke --help` is the command index.
- `auv-cli invoke <command> --help` renders command-specific arguments,
  disturbance, artifacts/signals, and verification notes.
- No `debug.*`, `verify.*`, or `music.*` command ids appear in the new registry,
  help output, or positive tests.
- `mediaControl.*` contains only generic media session controls and reports
  verified or inconclusive outcomes honestly.
- A follow-up handler-first PR should make each invoke command traceable from
  command id to handler function without searching for string operation names
  through runtime dispatch.

## Verification

Required checks:

```text
cargo fmt --check
cargo check
cargo test
git diff --check
cargo run --quiet -- invoke --help
cargo run --quiet -- invoke window.capture --help
cargo run --quiet -- invoke mediaControl.nowPlaying --help
cargo run --quiet -- invoke display.list
```

Negative checks:

```text
cargo run --quiet -- invoke debug.listDisplays
cargo run --quiet -- invoke verify.musicNowPlaying
cargo run --quiet -- invoke music.result.play
```

The negative checks should fail without executing legacy aliases.

## Open Risks

- Some existing root tests and fixtures still use old ids such as
  `debug.captureDisplay`. They should be updated to the new command ids in the
  same PR rather than preserved through aliases.
- `mediaControl.*` may require a new root driver adapter or a direct bridge to
  `auv-media-macos`. Register only commands backed by real executable code.
- `list-commands` output will change. This is acceptable, but the deprecation
  path, if retained at all, should be a failing parser tombstone so humans know
  to use `invoke --help`.
- The first metadata macro should stay small. If macro implementation becomes
  distracting, a plain data builder is acceptable for the first PR as long as
  the spec shape can support a macro later.
