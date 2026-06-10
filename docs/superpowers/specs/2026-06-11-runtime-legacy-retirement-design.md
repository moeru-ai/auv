# Runtime Legacy Retirement Design

Date: 2026-06-11

Status: approved design for planning

Scope classification: approved feature sequence

## Purpose

Remove the old JSON recipe, bundle, root catalog, legacy runtime, and root
`DriverCall` execution lane from AUV. The project has no public compatibility
contract for these surfaces, so the end state should not preserve executable
legacy compatibility.

The migration should keep the new app-local Rust command crates as the active
workflow model:

- `auv-apple-textedit`
- `auv-apple-notes`
- `auv-qqmusic`
- existing domain crates such as `auv-netease-music` and `auv-game-balatro`

The root crate should stop being the owner of workflow execution. It should keep
only command frontend code, shared storage/read APIs, inspect surfaces, and any
temporary bridge that is explicitly marked for deletion in this sequence.

## Current Problem

The old root execution path still exists:

```text
auv-cli skill/invoke
  -> src/runtime.rs
  -> src/catalog.rs
  -> DriverCall
  -> src/driver/**
  -> JSON recipes / bundles / case matrices
```

This path keeps several unrelated responsibilities tied together:

- recipe and case-matrix discovery under `recipes/`
- bundle discovery under `bundles/`
- root CLI `skill run`, `skill cases`, and `skill bundle`
- root command catalog ownership
- `Runtime`-owned run/span/event/artifact recording
- root `src/driver` legacy command handlers

The new app crates prove the target direction: app workflows can be expressed as
Rust domain commands over `auv-driver` and `auv-driver-macos` without depending
on root recipes, catalog command ids, or root `Runtime`.

## Non-Goals

- Do not preserve executable JSON recipe compatibility.
- Do not keep `skill run`, `skill cases`, or `skill bundle` as long-term CLI
  surfaces.
- Do not keep bundle-backed invoke behavior.
- Do not add a general recipe-to-Rust conversion tool.
- Do not rewrite every app domain command in this sequence. Migrate only what
  blocks deleting the old root lane.
- Do not expand archived AX copilot or demo verticals while removing legacy
  runtime pieces.

## Compatibility Policy

The sequence may briefly keep parser-level removal stubs if that makes review
and local debugging clearer. A removal stub may parse an old command only to
return an error such as:

```text
removed: JSON recipes and bundles are no longer executable; use app-local Rust commands
```

Removal stubs must not execute old behavior, read `recipes/`, read `bundles/`,
or route to `Runtime`.

Each stub must carry a `TODO:` marker naming the deletion trigger. The final
phase removes these stubs.

## Design Overview

Use five reviewable PR phases.

### PR1: Remove Bundle Execution And CLI Surface

Delete the active bundle lane first because it composes recipe execution and
keeps runtime tied to `SkillBundleCatalog`.

Planned changes:

- Remove `skill bundle list/show/coverage/verify/export/package verify` from
  the CLI.
- Remove bundle-backed command lookup from `Runtime`.
- Delete checked-in `bundles/` manifests or move them to archive only if the
  owner wants historical reference material.
- Delete `src/bundle/**` once no production code imports it.
- Update tests that validate bundle export/coverage to either disappear or move
  to archived fixture documentation tests if still useful.

Allowed temporary state:

- CLI may retain a removal stub for `skill bundle ...` for one phase.

Exit criteria:

- `SkillBundleCatalog` is not used by `src/main.rs`, `src/runtime.rs`, or root
  library initialization.
- No active command can execute a bundle-backed recipe.
- `bundles/` is no longer active source.

### PR2: Remove JSON Recipe And Case-Matrix Execution

Remove the recipe runner, case matrix runner, and checked-in JSON recipe tree.

Planned changes:

- Remove `skill run` and `skill cases` execution from the CLI.
- Delete `recipes/` as active source.
- Delete or shrink `src/skill/**` to only types still needed by archived docs or
  independent tests. Prefer full deletion if no production caller remains.
- Remove `SkillCatalog::discover` and `SkillCaseMatrixCatalog::discover` from
  root startup.
- Replace root tests that hard-read checked-in recipe JSON with tests against
  Rust app command crates or delete them when they only validate removed
  behavior.
- Remove scroll-scan recipe hook execution. If scroll scan still needs hooks,
  leave an explicit typed-hook TODO at the scroll-scan call site and require
  `auv-tracing-interaction` before reintroducing it.

Allowed temporary state:

- CLI may retain removal stubs for `skill run` and `skill cases`.
- Documentation may continue to mention historical recipes, but active docs
  must point to app-local Rust commands.

Exit criteria:

- No production code reads `recipes/`.
- No active CLI path executes a `SkillRecipe`.
- `cargo run --quiet -- skill run ...` no longer executes old behavior.

### PR3: Extract Invoke Registry To `auv-cli-invoke`

Move ad-hoc invoke command ownership out of root runtime.

Planned changes:

- Create `crates/auv-cli-invoke`.
- Move invoke command metadata and command-list rendering out of
  `src/catalog.rs`.
- Move CLI invoke parsing and argument normalization into `auv-cli-invoke`.
- Delete root `src/catalog.rs`.
- Decide command-by-command whether an invoke id maps to a new typed handler or
  is removed. Since there is no public compatibility contract, obsolete ids may
  be deleted instead of shimmed.
- Keep any still-needed temporary legacy adapter visibly inside
  `auv-cli-invoke` or a root compatibility module with deletion TODOs.

Allowed temporary state:

- `auv-cli-invoke` may call a small root compatibility adapter for commands not
  yet migrated, but this is a temporary bridge and not a new runtime.

Exit criteria:

- Root `Runtime` does not own or resolve command catalog entries.
- `src/catalog.rs` is deleted.
- `list-commands` and `invoke` are either backed by `auv-cli-invoke` typed
  handlers or report removed commands clearly.

### PR4: Extract Driver Recording To `auv-tracing-driver`

Move run/span/event/artifact recording out of `Runtime`.

Planned changes:

- Create `crates/auv-tracing-driver`.
- Move run lifecycle, span lifecycle, event recording, artifact staging, and
  recorder fan-out behind the new crate.
- Change `recorded_operation.rs` or its replacement to depend on
  `auv-tracing-driver`, not `Runtime`.
- Move read-only inspect helpers to store/read modules if they still live on
  `Runtime`.
- Prove the boundary with a small typed recorded operation test.

Allowed temporary state:

- `Runtime` may remain as a thin facade while final root driver deletion is in
  progress, but it must not own recording semantics.

Exit criteria:

- Typed Rust code can record a run and artifacts without constructing
  `Runtime`.
- `Runtime` no longer owns core recording behavior.
- Existing inspect/read behavior can load runs produced by the new recorder.

### PR5: Delete Runtime Facade And Root `src/driver`

Remove the remaining legacy `DriverCall` lane.

Planned changes:

- Delete `src/runtime.rs` or reduce it to a non-execution tombstone only if a
  short-lived import path requires it.
- Delete root `src/driver/**` after any still-needed platform capability has
  moved to `crates/auv-driver-macos` or an app/domain crate.
- Remove root `DriverCall`, `DriverResponse`, and related compatibility-only
  model types if no caller remains.
- Update `src/lib.rs` exports so root no longer exposes `catalog`, `bundle`,
  `skill`, or root `driver` modules.
- Remove CLI help text that points users to old skill/bundle/invoke ids.

Exit criteria:

- `src/runtime.rs`, `src/catalog.rs`, `src/bundle/**`, `src/skill/**`, and
  `src/driver/**` are deleted or reduced to explicit tombstones with no
  execution logic.
- No production code imports `DriverCall` or `DriverRegistry`.
- The active workflow model is app-local Rust commands plus typed tracing.

## Testing Strategy

Each PR should run:

```text
cargo fmt --check
cargo check
cargo test
git diff --check
```

When full `cargo fmt --check` fails due unrelated pre-existing formatting,
the PR should run package-scoped formatting checks for touched crates and note
the existing full-workspace formatter gap.

Deletion PRs should add focused negative tests when retaining temporary removal
stubs. The tests should assert that removed commands return the intended
message and do not attempt to discover `recipes/` or `bundles/`.

For final deletion PRs, prefer absence checks with `rg`:

```text
rg -n "SkillBundle|SkillCatalog|DriverCall|DriverRegistry|CommandCatalog|src/driver|recipes/" src crates
```

Expected remaining hits should be limited to archived docs or explicitly named
tombstones.

## Risks And Mitigations

Risk: deleting recipe/bundle surfaces also deletes useful validation examples.
Mitigation: keep historical references under docs/archive only when they still
teach something. Do not keep them executable.

Risk: root runtime deletion exposes hidden dependencies from app probe,
scroll scan, inspect, or candidate-action code.
Mitigation: each PR removes one owner boundary and uses `rg` gates to find
remaining imports before moving to the next phase.

Risk: command users lose discoverability.
Mitigation: app-local crates should have clear CLI help and docs. Temporary
removed-command stubs can point to app-local replacements.

Risk: trying to migrate every legacy command delays deletion.
Mitigation: delete obsolete command ids. Only migrate commands that are needed
by active app/domain crates or current owner-approved roadmap slices.

## Open Decisions Before Implementation

1. Whether PR1 should delete `bundles/` outright or move selected manifests to
   `docs/archive/verticals/`.
2. Whether PR2 should delete all checked-in `recipes/` outright or archive
   selected historical proofs.
3. Whether `invoke` itself remains as a root CLI command after PR3, backed by
   `auv-cli-invoke`, or whether only app-local CLIs remain active.
4. Which legacy macOS command ids are still active enough to migrate to typed
   handlers instead of deleting.

## Approval Summary

The owner selected the phased approach:

```text
skill/bundle removal -> recipe removal -> auv-cli-invoke -> auv-tracing-driver -> runtime/src-driver deletion
```

The owner also approved not preserving executable legacy compatibility because
the project does not yet have a public API contract.
