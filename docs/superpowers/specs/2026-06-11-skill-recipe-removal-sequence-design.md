# Skill And Recipe Removal Sequence Design

Date: 2026-06-11

Status: proposed owner-update spec

Scope classification: approved feature sequence adjustment

## Purpose

Update the runtime legacy retirement order after bundle removal. The project is
not public, and the owner has approved deleting legacy `skill` and JSON recipe
execution instead of preserving a compatibility registry or long-lived fallback
path.

The next sequence should remove active `skill`/recipe/case-matrix behavior
before designing the replacement `invoke` surface. New command discovery and
`list-commands` should describe the new Rust command model, not the old
catalog ids.

## Decision

Do not build a legacy `auv-cli-invoke` registry around the old `src/catalog.rs`
command table.

Instead:

- Remove active `skill` CLI and MCP surfaces.
- Remove JSON recipe and case-matrix execution.
- Remove `recipes/` as active source.
- Delete `src/skill/**` as far as production callers allow in the first PR.
- Keep only short-lived tombstones or neutral schema fragments when another
  active module cannot be converted inside the same reviewable PR.
- Redesign `invoke` and `list-commands` around the new Rust command model after
  the legacy skill lane is gone.

This supersedes the older compatibility wording in:

- `docs/ai/references/2026-06-10-auv-cli-invoke-catalog-removal.md`
- `docs/ai/references/2026-06-10-rust-orchestration-recipes-bundles-retirement.md`
- `docs/ai/references/2026-06-10-recipe-bundle-retirement-inventory.md`

Those references remain useful historical context, but their JSON recipe
fallback strategy is no longer the preferred next implementation order.

## Current Skill Links

The active `skill` chain currently includes:

- CLI parser and dispatch:
  - `auv-cli skill list`
  - `auv-cli skill show`
  - `auv-cli skill run`
  - `auv-cli skill cases list/show/report/run`
- MCP tools:
  - `skill_list`
  - `skill_show`
- JSON execution engine:
  - `src/skill/mod.rs`
  - `src/skill/recipe.rs`
  - `src/skill/case_matrix.rs`
  - `src/skill/recipe_observer.rs`
  - `src/skill/validate/**`
- Active source data:
  - `recipes/**.json`
  - `recipes/**/README.md` where it documents executable recipes
- Production callers outside `main`:
  - `src/scroll_scan/mod.rs` uses `SkillManifest` for inline recipe hooks.
  - `src/app/mod.rs` and `src/app/analysis.rs` use `SkillManifest` and
    `SkillCaseMatrix` for app distillation/validation output.

The first removal PR should try to remove all of these. If one caller cannot be
converted safely inside the PR, it must be marked with a narrow `TODO:` or
`NOTICE:` at the call site explaining the temporary residual dependency and the
exact deletion trigger.

## Target Order

### PR2: Remove Active Skill And Recipe Chain

Goal: delete active `skill` execution and discovery in one reviewable PR.

Planned changes:

- Remove `CliCommand::Skill*` variants and parser branches.
- Remove `skill` help text except for an optional parser-level removal stub.
- Remove `src/main.rs` dispatch for skill list/show/run/cases.
- Remove MCP `skill_list` and `skill_show`.
- Delete JSON recipe execution:
  - `run_skill`
  - `run_skill_manifest_*`
  - `SkillRecipeRunner`
  - `run_skill_case_matrix*`
  - recipe/case discovery.
- Delete `recipes/` as active source. Move files to an archive path only if
  the owner requests historical recovery in this PR; otherwise delete them.
- Remove checked-in tests that only validate removed manifests, recipe
  execution, or case matrices.
- Remove scroll-scan recipe hook execution. If typed hook replacement is not
  included, leave a clear `TODO(tracing-interaction-hooks)` at the hook
  boundary and make the old manifest hook path unavailable.
- Remove app distillation/validation emission of `SkillManifest` and
  `SkillCaseMatrix`, or replace it with a neutral app-analysis artifact shape.
  If that conversion is too large for PR2, keep only private schema fragments
  with a deletion TODO and no JSON execution path.

Allowed temporary state:

- A parser-level `skill ...` removal error is allowed for one follow-up phase.
- Private schema fragments may remain only if needed by app/scroll-scan code
  that is explicitly marked for immediate deletion or replacement.
- No temporary state may execute JSON recipes or discover `recipes/`.

Exit criteria:

- No active CLI or MCP path lists, shows, validates, or executes recipes.
- No production code discovers `recipes/`.
- No production code can execute `SkillRecipe` or `SkillCaseMatrix`.
- `src/skill/**` is deleted, or reduced to private non-executing tombstone
  schema with deletion TODOs.
- `cargo run --quiet -- skill run ...` cannot execute legacy behavior.

### PR3: Redesign Invoke And Command Listing

Goal: replace the old command catalog model with the new Rust command model.

Planned changes:

- Design `list-commands` as a view over app-local Rust command crates and
  selected typed driver capabilities, not over legacy recipe/catalog ids.
- Decide whether `invoke` remains a root CLI entrypoint or moves into an
  `auv-cli-invoke` crate as a new dispatcher.
- Delete `src/catalog.rs`.
- Stop constructing `CommandCatalog` in root runtime setup.
- Remove or rewrite old catalog ids such as `debug.*` and `music.*` when they
  do not map to the new command model.
- If a small bridge is needed for one or two still-used commands, place it
  behind an explicit temporary compatibility module with deletion TODOs. Do not
  call it a legacy registry.

Exit criteria:

- `list-commands` no longer reflects `src/catalog.rs`.
- `Runtime` no longer owns command lookup.
- `src/catalog.rs` is deleted.
- New command listing points users to Rust app/domain commands.

### PR4: Extract Recording To `auv-tracing-driver`

Goal: make recorded typed operations independent from `Runtime`.

Planned changes:

- Create `crates/auv-tracing-driver`.
- Move run/span/event/artifact staging and recorder fan-out out of
  `Runtime`.
- Change `recorded_operation.rs` or its replacement to depend on the tracing
  boundary.
- Prove the boundary with direct recorded-operation tests.

Exit criteria:

- Typed Rust operations can record runs and artifacts without constructing
  `Runtime`.
- `Runtime` no longer owns core recording semantics.

### PR5: Remove Remaining Runtime And Root Driver Compatibility

Goal: delete the old execution lane.

Planned changes:

- Delete or tombstone `src/runtime.rs`.
- Delete root `src/driver/**` after needed capabilities have moved to typed
  crates.
- Remove `DriverCall` compatibility model types when no caller remains.
- Update root exports and docs.

Exit criteria:

- Root no longer exposes `skill`, `catalog`, or legacy `driver` execution
  modules.
- No active frontend can route through the old JSON recipe/catalog/runtime
  chain.

## Non-Goals

- Do not preserve JSON recipe compatibility.
- Do not build a general recipe-to-Rust converter.
- Do not migrate every app workflow in this PR sequence unless it blocks
  deleting the old root lane.
- Do not expand archived AX copilot or demo flows.
- Do not design the final REPL in PR2.

## Open Implementation Risks

- `src/app/**` may still use skill schema as an output format. If removing that
  schema widens PR2 too much, PR2 should keep only private non-executing schema
  fragments with deletion TODOs.
- `src/scroll_scan/mod.rs` currently uses inline recipe hooks. Removing the
  old hook path may temporarily reduce hook functionality until
  `auv-tracing-interaction` provides typed hooks.
- Some tests currently hard-read checked-in recipe files. These tests should be
  deleted when they only cover removed behavior, or rewritten around Rust
  command crates if they still cover active behavior.

## Verification For PR2

Required checks:

```text
cargo fmt --check
cargo check
cargo test
git diff --check
cargo run --quiet -- skill run recipes/macos/textedit/create-and-verify-text.v0.json
cargo run --quiet -- skill cases list
```

The two `skill` commands should fail with the intentional removal behavior and
must not read or execute JSON recipes.
