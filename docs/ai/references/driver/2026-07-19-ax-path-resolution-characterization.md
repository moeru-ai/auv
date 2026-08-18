# AX Path Resolution Characterization

Date: 2026-07-19
Responsibility: driver (macOS AX observed-path resolution)
Type: reference / characterization

## What this locks

The macOS Swift AX layer resolves a stored *observed path* (a dotted string
like `0.1.2`) back to a live `AXUIElement` for three operations that share one
resolver (extracted in #111): `perform_ax_action`, `set_ax_focused`, and
`inspect_ax_node`. This note records the resolver's current behavior so a later
typed-error slice (milestone Workstream 2 / PR 5) can change error
*classification* against a known baseline.

The resolver has two layers with very different testability:

| Layer | Symbol | Pure? | How it is characterized |
|---|---|---|---|
| Parse | `axObservedPathIndices` (`AxPath.swift`) | Yes — only `String`/`Int`/`Result`, no AX, no Rust bridge | **Automated** via `swift test`, CI-gated (see below) |
| Tree walk | `axResolveObservedPath` (`AxTree.swift`) | No — calls `AXUIElementCreateApplication`, `axChildren`, `axStringAttribute` | **Documented only** (needs a live AX tree) |

## Automated parse-layer characterization

`crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/AxPath.swift`
holds the pure parse layer. `swift test` over that package runs the Swift Testing
suite at
`crates/auv-driver-macos/native/swift/Tests/AuvAxPathTests/AxPathTests.swift`,
which asserts:

- root-only path `0` → no child indices;
- valid multi-segment `0.1.2` → `[1, 2]`; `0.0.0` → `[0, 0]`;
- leading, repeated, and trailing empty segments are currently omitted by
  Swift `String.split`, so `.0` → `[]`, `0..1` → `[1]`, and `0.` → `[]`;
- non-`0` first segment → `AX <op> path must begin with 0; got <path>`;
- empty path → same root-marker failure;
- non-integer segment → `AX <op> path segment <seg> at offset <n> is not a non-negative integer`;
- negative segment → same, with the correct zero-based `offset`;
- `<op>` verb (`action` / `focus` / `inspection`) and `retry` phrase are
  interpolated per operation into message and recovery hint.

### Why the parse layer is a separate SwiftPM target

Three repo facts shape the split, documented so the next reader does not
"simplify" it by folding the test target onto `AuvMacosNative`:

1. A test target that depends on `AuvMacosNative` cannot link: its generated
   `Generated/SwiftBridgeCore.swift` references `__swift_bridge__$…` C symbols
   that only exist in the cargo-built Rust static library, so `swift test`
   fails at `ld`. The static-library product itself still builds, because
   archiving never resolves those symbols. `AuvAxPath` links nothing, so
   `AuvAxPathTests` depends on it alone.
2. `crates/auv-driver-macos/build.rs` globs `Sources/AuvMacosNative/*.swift`
   (non-recursive) into a single flat `swiftc -emit-library
   -module-name AuvMacosNative` call. `AxPath.swift` therefore stays in that
   directory: `Package.swift` points both targets at the same `path:` and
   partitions them with `sources:` / `exclude:`, so no file moves and no
   `build.rs` change are needed.
3. CI's `macos-14` runner has no Accessibility (TCC) grant, so the tree-walk
   layer cannot run there regardless.

Two consequences of the shared directory: the parse symbols are `public` for the
cross-module `import AuvAxPath` (inert in `build.rs`'s flat single-module
compile, where `axResolveObservedPath` sees them as same-module symbols), and
`AxTree.swift` guards that import with `#if SWIFT_PACKAGE` because SwiftPM
defines it and bare `swiftc` does not.

## Documented tree-walk contract (live AX, not automated)

`axResolveObservedPath` walks from the app's first window (or the app element
if there is no window) through the parsed indices, then checks the resolved
node's role. Current behavior, recorded for the baseline:

- **Out of range**: `AX <op> path index <i> is out of range at offset <n>;
  element has <k> child(ren)`; recovery: "the AX tree likely shifted since
  observation; capture a fresh tree and retry <retry>".
- **Empty children**: a node with zero children makes any index out of range at
  that offset (same message, `<k>` = 0).
- **First window vs app root**: resolution starts at `axFirstWindow(appElement)`
  and falls back to the app element when no window is present.
- **Role mismatch**: when `expectedRole` is non-empty and the resolved node's
  role differs → `AX <op> expected role <role> at path <path>, got <actual>`;
  same "tree likely shifted" recovery.
- **Stale tree**: manifests as either out-of-range or role-mismatch above,
  because a shifted tree changes child counts or roles at the recorded path.

These require a real running app with an AX tree; they are exercised in live
runs (e.g. the TextEdit reference integration and the Apple Music AX probe),
not in headless CI.

## Non-goals

- No change to resolver behavior, error strings, or error *type* (that is the
  later typed-error vertical slice).
- No tightening of empty-segment validation. `AxPath.swift` carries
  `TODO(ax-path-empty-segments)` because rejecting the currently accepted forms
  is a behavior change that needs an owner-approved bug-fix slice.
- No `build.rs` change, no file moves, and no test coverage of the FFI-linked
  part of `AuvMacosNative` (see reasons above).
- No automation of the live-AX tree-walk cases.
