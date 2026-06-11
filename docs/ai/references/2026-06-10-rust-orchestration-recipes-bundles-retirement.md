# Rust Orchestration Recipe Migration And Bundle Retirement Spec

Status: proposed, corrected after owner clarification; implementation started

Scope classification: approved feature slice

## Purpose

Migrate the executable workflows currently described under `recipes/` from JSON
manifests into Rust orchestration. The replacement workflows should be built
from atomic `auv-driver` API calls plus `auv-tracing-driver` and, where the
workflow is a macro interaction such as scroll scan, `auv-tracing-interaction`
recording.

JSON recipes and case matrices remain available during migration. They should
not be disabled before their Rust equivalents exist and are wired into the
current user-facing entrypoints. Bundles are retired after their referenced
workflows have either moved to Rust or been explicitly archived.

This is the recipe migration lane in the runtime removal sequence. It can start
before `runtime.rs` is fully removed, but each migrated workflow should reduce
the amount of active behavior that depends on JSON recipe execution.

## Current State

Recipes and bundles currently provide reusable workflow manifests and case
matrices. They execute by calling `Runtime::invoke_in_span`, which routes
through catalog command ids and legacy `DriverCall` dispatch.

That model was useful for proving replayable UI workflow structure, but it now
keeps active work tied to:

- scalar template inputs
- catalog command ids
- retired bundle-era command lookup
- `Runtime` as a central execution object

The desired lane is direct Rust composition over typed driver APIs. The Rust
version of each workflow should make control flow, branching, typed values,
artifact expectations, and verification logic explicit in Rust rather than
encoding them as manifest steps and scalar templates.

In this spec, "required workflows" means only entries listed in the checked-in
inventory with owner-approved disposition `migrate`. Entries without
owner-approved disposition must remain untouched except for documenting them in
the inventory.

## Target Boundary

Rust orchestration functions should own workflow logic. They may live in domain
crates or focused modules, but they should compose:

- typed `auv-driver` and platform driver session APIs
- `auv-tracing-driver` for atomic operation evidence
- `auv-tracing-interaction` for macro interactions such as scroll scan
- typed domain contracts such as `RecognitionResult`, `CandidateRef`,
  `OperationResult`, and `VerificationResult`

Each migrated recipe should have a Rust owner and a stable Rust entrypoint.
During the transition, CLI commands such as `skill run` and `skill cases run`
may resolve a known recipe id or manifest path to the Rust implementation after
the app-local operation can execute all required steps through typed APIs. JSON
execution remains a compatibility fallback for entries that have not been
migrated or whose required typed driver surface is still incomplete.

Update, 2026-06-11: the active bundle surface was retired before JSON recipe
execution. The former bundle CLI, bundle export/verification, checked-in bundle
manifests, and bundle-era invoke resolution are no longer compatibility paths.
Fallback now refers only to JSON recipe and case-matrix execution that remains
temporarily available through `skill run` and `skill cases`.

This spec does not require every migrated workflow to wait for a finished
`auv-tracing-driver` crate. A first Rust operation may land its app-local
workflow contract and driver boundary before it is wired into CLI. It should
not call root `runtime.rs`, catalog command ids, or root legacy command modules
just to appear complete; missing typed driver APIs should be marked as explicit
deferrals at the adapter boundary.

## App-Local CLI Shape

App-local crates should expose commands in the target app's domain language,
not in legacy recipe names or catalog command ids. Recipe ids such as
`create-and-verify-text`, `open-search-submit-query`, or
`play-visible-anchor` are migration references only. They may appear in
`legacy_recipe.rs` parity tests, but they should not become the durable CLI
surface.

For the first migrated app crates, the intended command shapes are:

```text
auv-apple-textedit document write <content> [--replace] [--verify]
auv-apple-textedit document compare <content> [--role AXTextArea]
auv-apple-textedit document focus [--query "First Text View"]
```

```text
auv-apple-notes note new
auv-apple-notes note write <content> [--new] [--replace] [--verify]
auv-apple-notes note compare <content> [--role AXTextArea]
auv-apple-notes note focus [--query <text>]
```

```text
auv-qqmusic search <query>
auv-qqmusic search results select <query> --anchor <text>
auv-qqmusic search results click <query> --anchor <text>
auv-qqmusic search results click <query> --row <index>
auv-qqmusic search results click --candidate-ref <json>
```

The `search <query>` command owns the full search flow for QQMusic: reveal or
focus search, submit the query, scan visible results, and emit result
candidates. `search results ...` commands operate on that result set. They may
perform the search phase when a query is supplied or consume a structured
candidate ref when `--candidate-ref` is supplied. Do not add a durable
`search submit` command unless a future app-specific need requires exposing
that lower-level step.

The TextEdit and Notes `focus` commands are accepted as app-local document/note
commands because they are useful for debugging and composition while typed AX
focus support is still being moved into `auv-driver-macos`. They should remain
domain-scoped (`document focus`, `note focus`) rather than leaking catalog names
such as `debug.focusTextInput`.

## Migration Strategy

1. Inventory active recipe, case matrix, and bundle entry points.
2. Classify each entry as:
   - migrate to Rust orchestration
   - archive as historical proof
   - delete as obsolete
   - fallback during migration
3. Check in the inventory with owner-reviewed disposition for each
   recipe and bundle entry.
4. Choose one small recipe as the Rust orchestration exemplar.
5. Implement its Rust workflow and Rust case data/tests behind an app-local CLI
   shape that uses app-domain names rather than recipe ids.
6. Wire the existing CLI entrypoint for that recipe id or path to the Rust
   workflow without changing user-facing command syntax once the app-local
   operation can execute all required steps through typed driver APIs.
7. Keep JSON execution as fallback for unmigrated entries.
8. Repeat by recipe family until the approved inventory no longer needs JSON
   execution.
9. Retire bundles once the owner approves removing bundle execution/export
   compatibility. This was completed on 2026-06-11; do not reintroduce bundle
   compatibility while migrating recipes.
10. Remove JSON recipe and case matrix execution after no active entrypoint
    depends on it.

The inventory must be checked in before removal work, preferably under
`docs/ai/references/`, and must include:

- path
- kind (`recipe`, `case_matrix`, or `bundle`)
- current CLI/runtime entrypoint
- disposition (`migrate`, `fallback`, `archive`, `delete`, or `hold`)
- replacement Rust owner when applicable
- approval source/date

`fallback` means a JSON recipe or case-matrix manifest stays executable until
its replacement is implemented or the owner approves archival. It no longer
applies to bundle manifests after the 2026-06-11 bundle retirement slice.
`hold` means the entry needs owner review before migration or removal. Neither
`fallback` nor `hold` is a long-term target state.

## Non-Goals

- Do not disable JSON recipe execution before a replacement Rust workflow is
  available for the affected entrypoint.
- Do not preserve JSON recipes as a second long-term execution engine after
  migration completes.
- Do not add compatibility shims for arbitrary old manifests unless a specific
  migration boundary is owner-approved.
- Do not treat archived AX copilot verticals as active AUV roadmap work.
- Do not add new high-level workflow surface area while retiring the old one.

## Scroll Scan Placement

Scroll scan is not a driver primitive and should not be modeled as a catalog
command. It is a macro interaction:

```text
observe window/region
  -> scroll
  -> observe again
  -> merge evidence
  -> record interaction artifacts
```

It belongs behind `auv-tracing-interaction`, which composes atomic driver
operations and records higher-level interaction artifacts.

Path-based scroll-scan recipe hooks should migrate to typed Rust hook
functions. Do not remove the existing hook manifest path until the replacement
hook has equivalent tests or the inventory marks that hook as archive/delete.

## Exit Criteria

For the first implementation slice:

- At least one owner-approved `recipes/` workflow has a Rust-owned operation
  equivalent and app-local driver boundary.
- The existing recipe CLI entrypoint keeps working through the compatibility
  path until the operation's required typed driver APIs exist.
- The workflow's case matrix is represented by Rust test data or Rust-driven
  cases.
- The original JSON manifest remains readable for comparison or archival until
  the owner approves deletion.
- Unmigrated recipes continue to work through the existing JSON compatibility
  path. Bundles do not; the active bundle surface has been removed.

For the full retirement:

- Every active recipe entry is migrated, archived, or deleted according to the
  inventory.
- No active CLI path depends on JSON recipe execution.
- No active runtime path invokes bundle-era commands.
- `recipes/` contain only archived reference material, tombstones, or are
  removed according to the approved inventory.
- Documentation points users and contributors to Rust orchestration and typed
  driver APIs.

## Verification

Required checks for each migration slice:

```text
cargo fmt --check
cargo check
cargo test
git diff --check
cargo run --quiet -- skill run <migrated-recipe-id-or-path>
cargo run --quiet -- skill cases run <migrated-case-matrix-id-or-path>
```

If the migrated workflow touches the live desktop, the PR may replace the CLI
smoke command with a deterministic fixture or dry-run check, but it must state
why the live command was not run.

## Deferrals

TODO(replay-v2): durable replay semantics for Rust orchestration are deferred
until the typed recording model defines replayable inputs independently from
legacy recipe manifests.

TODO(manifest-import): one-time import or conversion tools for old recipes are
deferred until an owner names specific manifests that must survive as active
workflows.

TODO(runtime-delete): deleting `runtime.rs` is deferred until migrated Rust
workflows, `auv-cli-invoke`, and tracing boundaries no longer need the
compatibility facade.
