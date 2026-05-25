# Driver Platform API Crates Design

Date: 2026-05-25

Status: proposed design, pending review

## Goal

Restructure AUV's driver layer into typed Rust crates without changing existing
command behavior. The new API should be shaped around explicit driver/platform
APIs, not around string command IDs, `BTreeMap<String, String>` inputs, or
module-level `pub fn` orchestration.

This phase creates:

- `crates/auv-driver`
- `crates/auv-driver-macos`
- `crates/auv-overlay-macos`

The existing command catalog and recipe runner remain usable through a small
compatibility adapter. They should lower old catalog operations into the new
typed macOS API instead of continuing to treat the old string dispatch as the
primary design.

The new driver crates must not depend on the current root crate. In particular,
they must not depend on the current `Runtime`, `DriverResponse`,
`ProducedArtifact`, run store, trace recorder, or command catalog types.

## Non-Goals

- Do not redesign recipe manifests in this phase.
- Do not implement the full surface reconstruction or virtual accessibility
  tree model in this phase.
- Do not add JS/RPC execution APIs in this phase.
- Do not split trace, runtime, store, recipe, or CLI into separate crates yet.
- Do not make overlay part of the driver API.
- Do not make driver crates write run artifacts, traces, or inspect records.

## Design Principles

The driver layer should follow this vocabulary:

- `Driver`: an automation backend implementation.
- `Platform`: the target platform exposed by a driver, such as macOS, Windows,
  Android, browser, fixture, or remote.
- `API`: a typed platform-specific capability namespace, such as app, display,
  window, screen, input, clipboard, vision, and macOS AX.

The code should prefer:

- typed structs, enums, and selectors over command strings and string maps
- `AppSelector` and `WindowSelector` over a single `TargetApp::bundle(...)`
- explicit macOS APIs such as `macos.window().resolve(...)`
- compatibility adapters for old commands, not old command semantics as the
  internal center
- moving complexity into deeper APIs so callers do not manually reproduce
  app/window resolution, coordinate projection, OCR matching, or click setup

The driver layer exposes platform automation facts and actions as Rust values.
Recording, artifact staging, inspect writes, and recipe compatibility remain in
the higher AUV runtime/root crate until those layers are split separately.

## External Naming Alignment

This design borrows names where they are already common in automation systems,
but it does not force AUV into one external project's model.

Useful precedent:

- Appium/WebDriver strongly supports `driver`, `session`, `capabilities`,
  `settings`, `context`, `window`, `click`, `screenshot`, `scroll`, `swipe`,
  `activate_app`, `terminate_app`, `get_clipboard`, and `set_clipboard`.
- Airtest strongly supports `snapshot`, `touch`, `swipe`, `text`, `wait`,
  `exists`, image `Template`, and direct image matching workflows.
- Poco strongly supports UI hierarchy selection through `poco(...)`, resolved
  UI objects with `.wait().click()`, `snapshot`, `get_screen_size`, `scroll`,
  `swipe`, and `set_text`.
- Maa strongly supports `Controller` as capture/input backend, `Tasker` as a
  bound execution instance, `recognition`, `action`, `anchor`, `roi`, `box`,
  `target`, `Click`, `LongPress`, `Swipe`, `Scroll`, `InputText`, and
  `Screencap`.

AUV should align with the broad vocabulary but keep AUV-specific boundaries:

- keep `Driver`, not Maa `Controller`
- keep `Session`, not Maa `Tasker`
- use `capture` or `screenshot` for bitmap capture; avoid `snapshot` for
  ordinary driver return types unless the type is explicitly a long-lived
  immutable observation contract
- use `region`/`bounds`/`action_target` instead of public `roi`/`box`/`target`
- use `recognition` for OCR/image/template match outputs
- use `selector` for unresolved queries and `element`, `node`, or `candidate`
  only for resolved objects
- use `ax_tree` for macOS accessibility-tree APIs, not a generic cross-platform
  `ax`

## Crate Boundaries

### `crates/auv-driver`

Owns shared driver API types and traits.

It should contain no macOS native calls, Swift bridge code, command catalog, run
storage, CLI rendering, or recipe execution.

Initial responsibilities:

- error and result types for driver APIs
- geometry types: `Point`, `Rect`, `RatioRect`, coordinate space markers
- app/window selectors:
  - `AppSelector`
  - `WindowSelector`
  - `TextMatcher`
- refs and snapshots:
  - `WindowRef`
  - `Window`
  - `ObservedWindows`
  - `Display`
  - `ObservedDisplays`
  - image/capture refs or lightweight capture result types
- operation options and action values:
  - `Click`
  - `PasteTextOptions`
  - `WaitOptions`
  - `CaptureOptions`
  - `Activation`
- driver traits and descriptors:
  - `Driver`
  - `DriverSession`

The traits in `auv-driver` should describe driver identity, platform ownership,
and lifecycle. They should not force every platform API into one fake universal
`invoke(String, Map)` shape. Platform-specific APIs are expected and should
remain concrete where that is clearer.

`auv-driver` should distinguish these data boundaries:

- observed platform data, such as display/window/app metadata
- in-memory media data, such as screenshots or image buffers
- recognition data, such as OCR matches or template matches
- action results, such as a click or paste result

It should not define persisted run artifacts. A later runtime or compatibility
layer can decide whether any returned image, OCR result, or action result should
be written to inspect storage.

`ObservedWindows` and `ObservedDisplays` are immutable observations of system
state at one point in time. They are not screenshot artifacts.

- `ObservedDisplays` is a timestamped display topology: display ids, logical
  bounds, visible bounds, scale factors, pixel dimensions, and combined desktop
  bounds.
- `ObservedWindows` is a timestamped window enumeration: window ids, owner
  app/process metadata, titles, bounds, layers, visibility/candidate metadata,
  and frontmost app/window metadata.

The public API should still read naturally as `session.display().list()` and
`session.window().list()`. These common methods should return ordinary Rust
collections:

```rust
let windows: Vec<Window> = session.window().list()?;
let displays: Vec<Display> = session.display().list()?;
```

The observed collection types exist for callers that need metadata and
same-moment consistency:

```rust
let observed: ObservedWindows = session.window().observe()?;
let frontmost = observed.frontmost_window();
let windows = observed.windows();
```

Selectors should live under a dedicated module:

```rust
use auv_driver::selector::{App, Window};

let app = App::bundle("com.netease.163music");
let selector = Window::main_visible().owned_by(app);
```

`App` and `Window` are constructor namespaces. The concrete returned types are
`AppSelector` and `WindowSelector`.

Operation configuration should follow Rust naming practice. Use `Options` for
optional behavior configuration, and use enums for compact action choices.
Avoid JS-style builder chains for simple action variants:

```rust
input.click_at(point, Click::Single)?;
input.click_at(
  point,
  Click::Double {
    interval: Duration::from_millis(80),
  },
)?;

window.capture(&window)?;
window.capture_with(
  &window,
  CaptureOptions {
    activation: Activation::ActivateFirst {
      settle: Duration::from_millis(200),
    },
  },
)?;
```

### `crates/auv-driver-macos`

Owns macOS automation.

This crate should move the current macOS driver implementation out of
`src/driver/macos` and reshape it behind typed APIs. The old root crate can
depend on this crate and expose a compatibility adapter for existing commands.
The old macOS implementation should not remain as the primary implementation in
`src/driver/macos`.

Initial public shape:

```rust
use auv_driver::selector::{App, Window};
use auv_driver_macos::MacosDriver;

let driver = MacosDriver::new();
let session = driver.open_local()?;

let app = App::bundle("com.netease.163music");
let window = session
  .window()
  .resolve(Window::main_visible().owned_by(app))?;
```

Suggested API namespaces:

- `session.app()`
- `session.display()`
- `session.window()`
- `session.screen()`
- `session.input()`
- `session.clipboard()`
- `session.vision()`
- `session.ax_tree()`

`ax_tree()` is macOS-specific in this phase. It should not be introduced as a
cross-platform `session.ax()` abstraction in the shared crate. If AUV later
adds a cross-platform accessibility abstraction, it should be separate from the
raw macOS AX tree API.

`auv-driver-macos` should return typed Rust values and in-memory media where
reasonable. It should not write screenshots, OCR JSON, action reports, run
artifacts, trace events, or inspect-server updates by itself. If compatibility
with existing commands requires files, the root compatibility layer should
perform that write/stage step from the typed result.

`DriverSession` in this crate is a driver-owned platform connection. It is not
the future high-level AUV session. For local macOS it may be lightweight, but it
still gives the driver a place for immutable capabilities, mutable settings,
permission/cache state, and native resources. A later AUV session can compose a
driver session with run recording, overlay visualization, resources, and
frontends.

### `crates/auv-overlay-macos`

Owns macOS overlay visualization.

Overlay is not driver automation. It visualizes automation process and results.
It may later be composed into a higher-level AUV session, but it must not be
part of `MacosDriverSession`.

Initial public shape:

```rust
use auv_overlay_macos::Overlay;

let overlay = Overlay::new()?;
overlay.show_cursor(point, "target")?;
overlay.hide_cursor()?;
```

Overlay is not debug-only. It is a visualization capability for automation
process, results, inspection, and human collaboration. `auv-driver-macos` should
not depend on `auv-overlay-macos` in this phase. A later AUV session layer can
compose driver automation, recording, and overlay visualization.

## Dependency Direction

This phase should keep dependency direction simple:

```text
auv-driver
  <- auv-driver-macos

auv-overlay-macos

root auv-cli crate
  -> auv-driver
  -> auv-driver-macos
  -> auv-overlay-macos
```

Rules:

- crates under `crates/*` must not depend on the current root crate
- `auv-driver-macos` depends on `auv-driver`
- `auv-overlay-macos` is independent from `auv-driver-macos`
- root compatibility code may depend on all three new crates
- run storage, trace recording, inspect-server writes, and command catalog
  compatibility remain above the driver crates

## Selector Model

Window resolution should be surface-first:

1. observe or list windows/pages/surfaces
2. filter by owner/context using `AppSelector`
3. filter by `WindowSelector`
4. return a concrete `WindowRef`

`AppSelector` describes an owner or context, not the primary operation target.
Examples:

```rust
App::bundle("com.netease.163music")
App::name("NetEaseMusic")
App::pid(1234)
App::frontmost()
```

`WindowSelector` describes the concrete surface:

```rust
Window::main_visible()
  .owned_by(App::bundle("com.netease.163music"))
  .title_contains("...")
```

Resolution should return structured errors:

- not found
- ambiguous
- stale ref
- unsupported platform capability
- permission denied

Existing macOS logic already mostly follows this model through window
snapshots, owner bundle IDs, app names, frontmost fallbacks, candidate ordering,
and display containment checks. The refactor should preserve those behaviors
while making the API explicit.

The selector module should be importable without importing all driver APIs:

```rust
use auv_driver::selector::{App, Window};

let app = App::bundle("com.netease.163music");
let window = Window::main_visible()
  .owned_by(app)
  .title_contains("NetEase");
```

This keeps selectors usable from Rust examples, compatibility adapters, future
RPC DTO conversion, and parser/matcher code.

## Interaction Layer Boundary

The driver crates only expose primitive platform APIs. They should not absorb
scroll scan, list scan, parser, matcher, or guard orchestration.

AUV still needs a higher interaction layer between driver primitives and
recipes/frontends. That layer should compose primitive operations into reusable
workflows such as:

- observe a collection page
- segment a list region
- build candidate contexts
- parse candidates
- match or select candidates
- act on a selected candidate
- verify the result
- scroll until top, bottom, stable state, or candidate match

This layer is not part of the first crate split, but the driver API must leave
room for it. Scroll scan should remain an orchestration workflow, not a driver
operation. Drivers provide capture, OCR, AX tree capture, pointer scroll,
keyboard input, and clipboard primitives; the interaction layer owns repeated
observation, candidate state, stop policy, parser/matcher decisions, and
structured candidate context.

The current `scroll_scan` module is the prototype of this missing layer. It
should eventually delegate candidate parsing, matching, and selection to a
typed interaction/candidate pipeline instead of encoding those contracts only
as scroll-scan hooks and exported scalar recipe variables.

Provisional future concepts:

- `InteractionCandidate`
- `CandidateContext`
- `ParsedCandidate`
- `MatchDecision`
- `SelectionDecision`
- `ListScan`
- `ScrollUntil`

These names are provisional and should be reconciled with
`docs/TERMS_AND_CONCEPTS.md` before implementation.

## Compatibility Layer

Existing command catalog behavior should remain stable.

The root crate can keep `CommandSpec` and old command IDs for now. Its macOS
driver compatibility layer should translate operations such as:

- `debug.captureWindow`
- `debug.clickWindowPoint`
- `debug.pasteTextPreserveClipboard`
- `debug.waitForWindowText`
- `debug.findImageText`
- `debug.clickWindowText`

into calls on `auv-driver-macos`.

This adapter is allowed to parse old string inputs because it is a boundary
layer. The typed driver crates should not use old command strings or maps as
their internal API.

The old `src/driver/macos` implementation should be migrated into
`crates/auv-driver-macos`. The root crate may keep a small compatibility module
that implements the existing root `Driver` trait and `DriverRegistry` contract,
but that module should delegate to `auv-driver-macos` and handle conversion to
the old `DriverResponse` and `ProducedArtifact` shapes.

## NetEase Validation Example

Add a Rust example that proves the typed API can perform the NetEase workflow.
The target behavior is no longer limited to copying the old recipe line by
line. The example should search NetEase Cloud Music and play the nth visible
matching result.

The example should be a Cargo example:

`examples/netease_play_visible_anchor.rs`

It is derived from:

`recipes/macos/netease-cloud-music/play-visible-anchor.v0.json`

The example should use the typed macOS API directly. It should not call the
recipe runner.

Approximate flow:

```rust
let app = App::bundle(inputs.app_id);
let window = session
  .window()
  .resolve(Window::main_visible().owned_by(app.clone()))?;

let before = session.window().capture_with(
  &window,
  CaptureOptions {
    activation: Activation::ActivateFirst {
      settle: Duration::from_millis(200),
    },
  },
)?;

let search_box = session
  .window()
  .find_text(&window, inputs.search_anchor, inputs.search_region, wait)?
  .best_match()?;
session.input().click_at(search_box.action_point(), Click::Single)?;

session.clipboard().paste_text(
  app,
  inputs.query,
  PasteTextOptions {
    replace_existing: true,
    submit: Submit::Return,
    settle: Duration::from_millis(1200),
  },
)?;

session.window().wait_text(&window, inputs.query, inputs.search_region, wait)?;

let after_search = session.window().capture(&window)?;
let results = session
  .window()
  .find_text(&window, inputs.result_title, inputs.result_region, wait)?;
let nth = results.nth_visible(inputs.result_index)?;
session
  .vision()
  .find_text(after_search.image_view(), inputs.result_artist, inputs.result_region)?;

session
  .input()
  .click_at(
    nth.action_point(),
    Click::Double {
      interval: Duration::from_millis(80),
    },
  )?;

let after_play = session.window().capture_with(
  &window,
  CaptureOptions {
    activation: Activation::ActivateFirst {
      settle: Duration::from_millis(200),
    },
  },
)?;
session
  .vision()
  .find_text(after_play.image_view(), inputs.result_title, inputs.player_region)?;
session
  .vision()
  .find_text(after_play.image_view(), inputs.result_artist, inputs.player_region)?;
```

The example is the ergonomics test. If this code reads like a catalog command
translation table, the API is not deep enough.

The example should read inputs from CLI arguments or a small typed config, not
hard-code query, artist, result index, or bundle id. Region ratios may have
defaults because they describe a constrained observation area. Fixed search box
points should be avoided; the example should prefer OCR/recognition or another
surface-derived target and only allow coordinate fallback as an explicit
debug/compatibility option.

The validation pass should also check that the existing scan hook recipe remains
compatible through the root command compatibility layer:

`recipes/scan/list-item-candidate-continue-hook.v0.json`

## Implementation Notes And Friction Log

During implementation, keep this section updated with concrete notes about
problems found while moving code and shaping the API. This is intentionally a
working log, not a polished final design.

Record:

- places where the spec feels over-abstracted, under-specified, or awkward in
  real Rust code
- places where old implementation details force a compatibility workaround
- code smells found during migration, especially hidden coupling, stringly
  typed boundaries, file writes inside driver code, global state, or pass-through
  wrappers
- API names that read poorly at call sites
- platform-specific behavior that does not fit the shared driver types
- temporary choices made to preserve existing behavior
- follow-up refactors that should happen after this crate split

Use short dated bullets while implementing. Prefer concrete file/module names
and the reason the issue matters. If a note introduces or changes a core term,
also update `docs/TERMS_AND_CONCEPTS.md`.

- 2026-05-25 Task 3: `src/driver/macos/observe.rs` could not move as a
  full implementation without pulling root `DriverCall`, `DriverResponse`, and
  `ProducedArtifact` into `auv-driver-macos`. The crate now owns the lower-level
  types/capture/support helpers, while root `observe.rs` remains a compatibility
  adapter until observation commands return typed driver-session outputs.
- 2026-05-25 Task 3: capture backend code needed local temp screenshot naming
  after moving to `crates/auv-driver-macos`, because the old helper lived beside
  root artifact construction. This is a sign that file naming/storage should be
  split from root artifact staging in a later runtime boundary pass.
- 2026-05-25 Task 3: pure capture geometry moved into
  `crates/auv-driver-macos/src/capture/geometry.rs`, while root
  `xcap_backend.rs` keeps the screenshot file write path. This preserves the
  current command/artifact behavior without making the driver crate own run
  storage.
- 2026-05-25 Task 3: the root macOS command adapter is now compiled and
  depended on only for `target_os = "macos"`. Non-mac default runtime currently
  registers only the fixture driver; full Windows/Linux driver registration is
  a later platform implementation task.
- 2026-05-25 Task 4: overlay native code moved into
  `crates/auv-overlay-macos`, with its Swift package under
  `crates/auv-overlay-macos/native/swift/`. Root
  `src/driver/macos/native::overlay` remains a compatibility delegation layer
  for existing command paths, but `auv-driver-macos` does not depend on the
  overlay crate. The bridge-generation xtask now emits IDE bridge files for
  both the driver and overlay Swift packages, which keeps the split boundary
  explicit while preserving SourceKit indexing.

## Migration Plan

1. Add workspace members for the three crates.
2. Create `auv-driver` shared types and selectors.
3. Move macOS driver implementation into `crates/auv-driver-macos`.
4. Move macOS overlay implementation into `crates/auv-overlay-macos`.
5. Replace root `src/driver/macos` with a compatibility module that calls
   `auv-driver-macos`.
6. Keep existing command catalog and recipe behavior passing.
7. Add the NetEase typed Rust example for searching and playing the nth visible
   matching result.
8. Verify the existing scan hook recipe remains compatible.
9. Run format, check, full tests, clippy, and the documented CLI smoke commands.

## Open Questions Resolved For This Phase

- Overlay is separate from driver automation.
- The first crate split is limited to driver and overlay crates.
- Old driver code may be moved aggressively as long as behavior stays stable.
- The command catalog remains as a compatibility boundary for now.
- New API design targets typed Rust first; RPC/JS comes later.
