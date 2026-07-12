# macOS Driver Namespace Refactor Design

> Implementation scope chosen on 2026-05-21: first migrate public command and
> driver naming (`macos.desktop`, `debug.captureAxTree`) and split AX tree
> capture into its own module. Full screen/window directory reshaping remains
> deferred until more non-macOS backend work exists.

## Context

The current macOS driver grew from PoC command paths. Public command names,
driver operation names, and Rust modules still use broad terms such as
`observe`, even when the implementation is really listing windows, capturing a
screen source, running OCR, dumping an AX tree, or sending input events.

`docs/ai/references/driver/2026-05-20-window-screen-ocr-click-design.md` defines the
window/screen OCR click model that should land first. This namespace refactor
should happen after that design is implemented, so it can organize the settled
screen, display, window, region, resolver, OCR, and click behavior instead of
renaming moving targets.

This design is PoC-scoped and does not preserve backward compatibility.

## Goals

- Remove `observe` as the dominant macOS driver namespace.
- Split macOS driver modules by capability: screen, window, AX tree, pointer,
  keyboard, clipboard, app, and permission.
- Align public command names with concrete actions such as list, capture, find,
  wait, click, verify, and probe.
- Keep one coherent macOS desktop driver surface for the runtime while making
  implementation modules small and inspectable.
- Prepare the structure for future non-macOS backends without leaking macOS
  implementation names into shared concepts.

## Non-Goals

- Do not change the window/screen OCR click semantics defined in the dependency
  design.
- Do not design Linux, Windows, Android, iOS, or Web namespaces in this phase.
- Do not introduce `swift_bridge` or `osascript` as public command namespaces.
- Do not keep compatibility aliases. This repository is still PoC-stage, so old
  command ids and driver ids should be removed when their replacements land.

## Dependency

This refactor should happen after the window/screen OCR click command family is
implemented. The dependency design establishes:

- `debug.listWindows`
- shared window resolver behavior
- explicit screen source selection behavior
- window text and row commands
- screen text and row command semantics
- AX tree rename away from `observeAxTree`

The namespace refactor should not start while those command semantics are still
being actively redesigned.

## Public Naming Model

Command verbs should describe the operation:

```text
list      enumerate current candidates
capture   produce a point-in-time artifact or coordinate contract
find      run one search over a selected source
wait      poll until a condition appears or times out
click     send pointer input
type      send text input
press     send key/button input
verify    assert that current or captured state contains expected evidence
probe     inspect environment readiness or permissions
```

`observe` should not be used for window listing, OCR lookup, AX tree capture,
or generic macOS driver identity.

Preferred command examples:

```text
debug.listDisplays
debug.listWindows
debug.captureDisplay
debug.captureRegion
debug.captureWindow
debug.captureAxTree
debug.findScreenText
debug.waitForScreenText
debug.clickScreenText
debug.findWindowText
debug.waitForWindowText
debug.clickWindowText
debug.verifyAxText
debug.probePermissions
```

`debug.observeWindows` should be replaced by `debug.listWindows`.
`debug.observeAxTree` should be replaced by `debug.captureAxTree`.
No compatibility aliases should remain after the migration.

## Driver Identity

The current `macos.observe` driver id is too narrow and historically loaded.
The preferred code-level macOS implementation name is:

```text
macos.desktop
```

However, `driver_id` should remain a runtime registry concern, not the main
public architecture vocabulary. A command catalog that wants to be portable
across platforms should eventually be able to route through a generic desktop
capability driver and let the registry select the current platform
implementation.

The immediate refactor may still use `macos.desktop` as the concrete registered
driver id if that fits the current runtime. The important rule is that durable
command names and shared concepts should not encode `macos.observe`, and the
code-level capability modules should not depend on `observe` as a namespace.

## Rust Module Layout

The target layout is:

```text
src/driver/macos/
  mod.rs
  descriptor.rs
  dispatch.rs
  native/
    ffi.rs
  screen/
    mod.rs
    capture.rs
    ocr.rs
    rows.rs
    projection.rs
  window/
    mod.rs
    list.rs
    resolver.rs
    capture.rs
    ocr.rs
    rows.rs
  ax_tree/
    mod.rs
    capture.rs
    query.rs
    verify.rs
    action.rs
  pointer/
    mod.rs
    click.rs
    scroll.rs
  keyboard/
    mod.rs
    type_text.rs
    press_key.rs
    shortcut.rs
  clipboard/
    mod.rs
    snapshot.rs
    lock.rs
  app/
    mod.rs
    activate.rs
    resolve.rs
  permission/
    mod.rs
    probe.rs
  artifact/
    mod.rs
  common/
    mod.rs
    geometry.rs
    parse.rs
```

Capability modules should expose narrow command handlers or shared helpers.
`dispatch.rs` should route operation names to the capability modules, but should
not accumulate operation logic.

## Internal Boundaries

`screen` owns display-backed observation sources, screen OCR, screen row
detection, and screen coordinate projection.

`window` owns CoreGraphics window candidates, window resolver behavior, window
capture, and window-local OCR/row operations.

`ax_tree` owns accessibility tree capture, AX text verification, and AX actions.
It should not own CoreGraphics window listing.

`pointer` owns global logical click and scroll actions.

`keyboard` owns text typing, key presses, and shortcut parsing.

`clipboard` owns clipboard snapshot, mutation, restore, and locking.

`app` owns activation and app identity resolution.

`permission` owns readiness and permission probes.

Implementation backends such as Swift bridge, xcap, osascript, and Rust-native
input crates should remain behind these capability modules.

## Documentation Updates

`docs/TERMS_AND_CONCEPTS.md` should define or update terms that become durable:

- screen
- display
- window
- region
- window candidate
- window resolver
- AX tree
- capture contract

The docs should distinguish observation concepts from command verbs. AUV can
have an observation model without exposing `observe` as the main command name.

## Testing

Unit tests should cover:

- command catalog registration for renamed commands
- absence of old PoC command ids when compatibility is intentionally dropped
- dispatch from command operation names into capability modules
- window resolver behavior remains unchanged after file movement
- screen OCR and window OCR share the intended helpers
- AX tree capture is distinct from window listing

Validation commands should include:

- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `cargo run --quiet -- list-commands`
