# auv-cli-invoke Catalog Removal Spec

Status: superseded by `docs/ai/references/archive/skill-bundle/2026-06-11-skill-recipe-removal-sequence-design.md`

Update 2026-06-11: the legacy JSON `skill`/recipe/case-matrix lane has been
removed by PR #35. Treat this document as historical design context only. A new
`auv-cli-invoke` design should not preserve JSON recipe compatibility or move
recipe step compatibility types forward; it should start from the post-removal
state where `src/skill/**` and `recipes/**` no longer exist.

Scope classification: approved feature slice

## Purpose

Move ad-hoc command invocation out of the root AUV runtime lane. The current
`src/catalog.rs` is not a core runtime contract; it is a compatibility registry
for CLI command ids such as `debug.captureWindow` and `music.result.play`.

This PR creates an `auv-cli-invoke` boundary that owns invoke command parsing,
legacy command lookup, and compatibility routing input while preserving the
existing CLI user experience.

For this PR, `auv-cli-invoke` is a frontend compatibility boundary, not a new
runtime. It resolves legacy command ids and normalizes invoke arguments. Run
lifecycle, artifact staging, and driver execution remain on the existing
compatibility path until the tracing-driver split lands.

## Current State

`src/catalog.rs` defines `CommandCatalog` and the default command table. It has
no direct dependency on `src/runtime.rs`, but `Runtime` depends on it to resolve
`InvokeRequest.command_id`.

The default catalog currently contains:

- 57 `macos.desktop` commands routed through the legacy `DriverCall` adapter.
- 1 `fixture.observe` command used by tests and fixture workflows.

The command table is grouped by provisional namespace metadata:

- observe commands
- action commands
- verify commands
- overlay commands
- domain music commands

That metadata is useful for migration, but the root crate should not continue
to present this table as a core runtime registry.

## Target Boundary

Introduce `auv-cli-invoke` as the owner of:

- invoke request and result types used by the CLI surface
- legacy command id compatibility
- command list rendering for invoke commands
- CLI argument normalization for legacy invoke handlers
- temporary routing to the existing legacy driver execution path where a command
  has not yet migrated

The root crate should not expose `src/catalog.rs` after this slice. If command
compatibility still needs a table, it lives under `auv-cli-invoke` with a name
such as `legacy_command_registry`.

Dependency direction for this PR:

- CLI presentation code calls `auv-cli-invoke`.
- `auv-cli-invoke` owns invoke request/result types and legacy command lookup.
- Any temporary legacy adapter lives at the boundary where `auv-cli-invoke`
  calls the existing root compatibility execution path.
- `Runtime` must not depend on `auv-cli-invoke` for command registry ownership.

The preferred target is a workspace crate named `auv-cli-invoke`. If the first
implementation uses a root module as a staging step, that module must be marked
temporary and carry a TODO with the crate extraction trigger and imports that
must disappear before extraction.

## Non-Goals

- Do not change command names, flags, output shape, or run recording behavior.
- Do not migrate command implementations to typed `auv-driver` APIs in this
  PR.
- Do not move run lifecycle, artifact staging, or driver execution ownership
  into `auv-cli-invoke`.
- Do not reintroduce JSON recipe execution. Bundle execution and JSON recipes
  have both been retired and must not be restored as compatibility.
- Do not introduce REPL behavior. `auv-cli-invoke` is a library boundary first.

## Proposed Steps

1. Add an `auv-cli-invoke` crate or an equivalent workspace boundary.
2. Move CLI invoke parsing, command metadata, and user-facing invoke result
   presentation into `auv-cli-invoke`.
3. Move the default command table from `src/catalog.rs` into
   `auv-cli-invoke` as a legacy registry.
4. Update the CLI `invoke` subcommand to call `auv-cli-invoke`.
5. Let `auv-cli-invoke` translate CLI input into the legacy compatibility
   request and call the current legacy `DriverCall` adapter for unmigrated
   commands without owning driver execution.
6. Delete root `src/catalog.rs` once no root module imports it.

## Compatibility Rule

The PR is successful only if existing CLI command behavior remains stable:

```text
cargo run --quiet -- list-commands
cargo run --quiet -- invoke debug.listDisplays
cargo run --quiet -- invoke debug.probePermissions
```

The command list may internally come from `auv-cli-invoke`, but user-facing
command ids must not change in this PR.

## Exit Criteria

- `src/catalog.rs` no longer exists.
- The root runtime no longer owns the command registry.
- `auv-cli-invoke` owns legacy command lookup.
- Existing invoke tests still pass with unchanged command ids.
- Any remaining legacy dispatch fallback is visibly marked as temporary.

## Deferrals

TODO(typed-invoke-handlers): broad direct typed `auv-driver` dispatch is
deferred until after artifact recording is split from `Runtime`. PR2 may add
only minimal typed coverage needed to prove the recorder boundary.

TODO(invoke-repl): REPL behavior is deferred until the invoke library boundary
is stable and can be embedded without CLI-only assumptions.
