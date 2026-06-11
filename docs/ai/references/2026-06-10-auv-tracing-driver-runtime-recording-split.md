# auv-tracing-driver Runtime Recording Split Spec

Status: proposed

Scope classification: approved feature slice

## Purpose

Extract run/span/event/artifact recording from `Runtime` so typed driver calls
and Rust orchestration code can record inspectable evidence without depending
on the legacy command runtime.

This PR creates the recording substrate needed for direct typed driver
invocation. It should remove most of the practical reason for `runtime.rs` to
exist after `auv-cli-invoke` has taken command compatibility.

The PR should prove the new recording boundary with the smallest useful typed
operation coverage. It is not the broad command migration PR.

## Current State

`src/runtime.rs` currently owns several unrelated responsibilities:

- starts and finishes runs
- records spans and events
- stages file artifacts and artifact refs
- resolves command ids through the catalog
- invokes legacy drivers through `DriverCall`
- previously invoked bundle-era recipe commands, retired 2026-06-11
- exposes read/inspect helper methods
- hosts recorded operation helpers through `recorded_operation.rs`

`recorded_operation.rs` already describes itself as the bridge between typed
Rust driver APIs and AUV run recording, but it still depends on `Runtime`.

## Target Boundary

Create an `auv-tracing-driver` boundary for driver-level recording:

- run lifecycle
- span lifecycle
- event recording
- artifact staging
- artifact refs
- local store persistence
- recorder fan-out to local snapshots and inspect server write mode

This boundary records evidence for atomic driver operations. It should not own
command compatibility, JSON recipe execution, retired bundle lookup, or UI-specific
interaction loops.

## API Shape

The exact type names are provisional, but the split should expose concepts
equivalent to:

```rust
let mut run = recorder.start_run(spec)?;
let span = run.start_span("auv.driver.invoke", attributes)?;
let artifact_ref = run.stage_artifact_file(&span, artifact)?;
run.finish_span(span, status)?;
recorder.finish_run(run, finish)?;
```

The API should be shaped so future consumers can use it, but PR2 should wire
only the recorder extraction path and the selected proof for this slice. The
selected proof is the existing `recorded_operation.rs` direct recorded-operation
path. Existing candidate-action, detector, and AX recognition callers may be
updated only as needed to keep that path compiling and recording. Do not wire
app probe/analyze/distill/validate flows or migrate command families in this
PR.

Future consumers include:

- `auv-cli-invoke`
- candidate action recording
- detector/AX recognition recording
- app probe/analyze/distill/validate flows
- tests that need inspectable artifacts

## Relationship To auv-tracing-interaction

`auv-tracing-driver` records atomic driver operations.

`auv-tracing-interaction` records macro interactions that compose multiple
driver operations, such as scroll scan. Interaction recording may create
higher-level spans and artifacts, but it should call into driver-level
recording for atomic observations and input actions.

## Non-Goals

- Do not rewrite all command implementations in this PR.
- Do not remove JSON recipes in this PR. Bundles have already been retired and
  must not be restored.
- Do not change persisted run record wire shapes unless a compatibility
  boundary is documented and tested.
- Do not make the inspect server an execution dependency.
- Do not redesign viewer APIs, inspect-server protocol, replay, or mutation
  semantics; reuse existing inspect write behavior where needed.
- Do not migrate, polish, or expand archived AX copilot behavior. This boundary
  may become usable by candidate-action code, but that is not approval to grow
  the archived vertical.

## Proposed Steps

1. Move run lifecycle and artifact staging APIs from `Runtime` into the new
   tracing-driver boundary.
2. Change `recorded_operation.rs` so its context depends on the tracing
   recorder, not on `Runtime`.
3. Move read-only inspect helpers toward store/read modules rather than
   keeping them on `Runtime`.
4. Update only the existing direct recorded operation path needed to prove the
   new recording context.
5. Preserve existing CLI invoke behavior through the compatibility path. Wire
   `auv-cli-invoke` to the new recorder only to keep current recording behavior
   working, and add at most one minimal typed proof handler with no
   command-family migration.
6. Shrink `Runtime` to a temporary facade for JSON recipe paths that have not
   yet migrated.

## Exit Criteria

- Typed Rust code can create a recorded run and stage artifacts without
  constructing `Runtime`.
- `recorded_operation.rs` no longer imports `crate::runtime::Runtime`.
- `Runtime` no longer owns core recording semantics.
- Existing inspect/read behavior still loads persisted runs.
- Remaining `Runtime` methods are limited to temporary compatibility paths and
  carry clear TODO markers.

## Verification

Required checks for the PR:

```text
cargo fmt --check
cargo check
cargo test
git diff --check
```

Focused tests should cover:

- successful recorded operation with artifacts
- failed recorded operation still persists a failed run
- artifact refs include run id, span id, artifact id, and capture event id
- inspect/read helpers can load runs produced by the new recorder

## Deferrals

TODO(runtime-delete): full `runtime.rs` deletion is deferred until JSON recipe
execution no longer depends on the compatibility facade.

TODO(tracing-interaction): scroll scan and other macro-operation recording are
deferred to `auv-tracing-interaction` after driver-level recording is stable.

TODO(command-migration): broad invoke command rewrites are deferred until the
recording boundary is stable and the owner approves a typed migration slice.
