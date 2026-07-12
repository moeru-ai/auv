# Recording Root Shim Removal Spec

## Status

Approved implementation slice.

## Classification

Narrow refactor.

## Context

`auv-tracing-driver` now owns durable run/span/event/artifact recording. The
root `auv-cli` crate still exposes `src/recording/mod.rs` as a compatibility
shim:

```rust
pub use auv_tracing_driver::recording::*;
```

That shim was introduced during the extraction documented in
`docs/ai/references/driver/2026-06-16-tracing-driver-extraction-implementation-plan.md`
so callers could continue importing `auv_cli::recording::*` while the
extraction landed.

The project is still pre-public, and the owner approved deleting the
compatibility path rather than preserving `auv_cli::recording::*`.

## Goal

Remove the root `src/recording` compatibility module and migrate all current
call sites to import recording primitives directly from `auv_tracing_driver`.

## Non-Goals

- Do not change `auv-tracing-driver` recording behavior.
- Do not change run record, span, event, artifact, or wire schemas.
- Do not remove other root compatibility shims such as `src/store.rs`,
  `src/run_builder.rs`, `src/trace.rs`, or `src/recorded_operation.rs`.
- Do not migrate unrelated recording facades or runtime methods.
- Do not clean up historical documentation references except where a touched
  implementation plan line would otherwise become actively misleading.

## Current Users To Migrate

Internal root crate users:

- `src/runtime.rs`
- `src/inspect_server/mod.rs`
- `src/main.rs`
- `src/scroll_scan/mod.rs`
- `src/app/mod.rs`
- `src/app/infra.rs`
- `src/osu.rs`

Root public module declaration:

- `src/lib.rs`

Workspace test user:

- `crates/auv-inference-ultralytics/tests/fixture_parity.rs`

## Design

Use direct owner-crate imports for recording primitives:

```rust
use auv_tracing_driver::{RecordingHandle, RunRecordingBackend};
```

or, where the module wants to emphasize the recording submodule:

```rust
use auv_tracing_driver::recording::{RunRecorder, RunUpdate};
```

Prefer the crate-level re-exports when they already exist in
`crates/auv-tracing-driver/src/lib.rs`, because those are the stable public
entrypoints of the owning crate.

Remove `pub mod recording;` from `src/lib.rs`, then delete
`src/recording/mod.rs`. This intentionally breaks `auv_cli::recording::*`.

For `crates/auv-inference-ultralytics/tests/fixture_parity.rs`, add
`auv-tracing-driver` as a `dev-dependency` and import `BroadcastRunRecorder`
and `RunRecordingBackend` from that crate directly.

## Error Handling

No runtime error behavior changes. This refactor only changes compile-time
module paths.

## Testing And Validation

Run focused compile validation first:

```bash
cargo check
```

Then run formatting and diff hygiene:

```bash
cargo fmt --check
git diff --check
```

If `cargo check` reports additional `auv_cli::recording` or
`crate::recording` references, migrate those to `auv_tracing_driver` as part of
this same slice.

## Expected Outcome

- `rg -n "pub mod recording|crate::recording|auv_cli::recording" -g '*.rs' .`
  has no matches.
- `src/recording/mod.rs` no longer exists.
- The recording owner is unambiguous: durable recording primitives come from
  `auv_tracing_driver`.

## Follow-Up Candidates

- Consider whether the remaining root compatibility shims for `trace`, `store`,
  `run_builder`, and `recorded_operation` should also be removed in separate
  owner-approved slices.
