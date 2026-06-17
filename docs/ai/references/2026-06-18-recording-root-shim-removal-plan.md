# Recording Root Shim Removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete the root `auv_cli::recording::*` compatibility shim and migrate all current Rust call sites to import durable recording primitives directly from `auv_tracing_driver`.

**Architecture:** `auv-tracing-driver` remains the sole owner of durable run/span/event/artifact recording. The root `auv-cli` crate keeps using those types, but no longer republishes them through `src/recording/mod.rs`. This is a compile-time module path refactor only; schemas and runtime recording behavior must stay unchanged.

**Tech Stack:** Rust 2024, Cargo workspace, `auv-tracing-driver`, root `auv-cli` crate, `auv-inference-ultralytics` integration tests.

---

## Scope And Guardrails

This plan implements
`docs/ai/references/2026-06-18-recording-root-shim-removal-spec.md`.

The repository may already have unrelated edits. Before changing a file listed
below, inspect its current diff and preserve unrelated user work. Do not revert
or clean up any modified/deleted files outside this slice.

## File Structure

- Modify `src/lib.rs`: remove the public root `recording` module declaration.
- Delete `src/recording/mod.rs`: remove the re-export shim.
- Modify `src/runtime.rs`: import `MemoryRunRecorder`, `RunRecorder`, `RunRecordingBackend`, and test-only `RunUpdate` from `auv_tracing_driver`.
- Modify `src/inspect_server/mod.rs`: import `BroadcastRunRecorder`, `RunRecorder`, `RunUpdate`, and `WireUpdate` from `auv_tracing_driver`.
- Modify `src/main.rs`: construct recorder types through `auv_tracing_driver` instead of `auv_cli::recording`.
- Modify `src/scroll_scan/mod.rs`: import `RecordingHandle` from `auv_tracing_driver`.
- Modify `src/app/mod.rs`: import `RecordingHandle` from `auv_tracing_driver`.
- Modify `src/app/infra.rs`: import `RecordingHandle` from `auv_tracing_driver`.
- Modify `src/osu.rs`: import `RecordingHandle` from `auv_tracing_driver`.
- Modify `crates/auv-inference-ultralytics/Cargo.toml`: add an `auv-tracing-driver` dev-dependency for tests that previously imported through `auv-cli`.
- Modify `crates/auv-inference-ultralytics/tests/fixture_parity.rs`: import `BroadcastRunRecorder` and `RunRecordingBackend` from `auv_tracing_driver`.

## Task 1: Establish The Compile-Fail Baseline

**Files:**
- Inspect: `src/lib.rs`
- Inspect: `src/recording/mod.rs`
- Inspect: `src/runtime.rs`
- Inspect: `src/inspect_server/mod.rs`
- Inspect: `src/main.rs`
- Inspect: `src/scroll_scan/mod.rs`
- Inspect: `src/app/mod.rs`
- Inspect: `src/app/infra.rs`
- Inspect: `src/osu.rs`
- Inspect: `crates/auv-inference-ultralytics/Cargo.toml`
- Inspect: `crates/auv-inference-ultralytics/tests/fixture_parity.rs`

- [ ] **Step 1: Check existing user edits before touching target files**

Run:

```bash
git status --short
git diff -- src/lib.rs src/runtime.rs src/inspect_server/mod.rs src/main.rs src/scroll_scan/mod.rs src/app/mod.rs src/app/infra.rs src/osu.rs crates/auv-inference-ultralytics/Cargo.toml crates/auv-inference-ultralytics/tests/fixture_parity.rs
```

Expected: any unrelated changes are noted and preserved. If a target file has
unrelated edits, apply only the import/module-path changes from this plan.

- [ ] **Step 2: Confirm current shim users**

Run:

```bash
rg -n "pub mod recording|crate::recording|auv_cli::recording" -g '*.rs' .
```

Expected: matches include `src/lib.rs`, root call sites, and the
`auv-inference-ultralytics` fixture parity test.

- [ ] **Step 3: Create the RED compile failure by removing only the root module declaration**

In `src/lib.rs`, remove only this line:

```rust
pub mod recording;
```

Do not delete `src/recording/mod.rs` yet. This isolates the expected compiler
failure to callers still using the root module path.

- [ ] **Step 4: Run compile check and verify the expected failure**

Run:

```bash
cargo check
```

Expected: FAIL. The important failures are unresolved imports or paths for
`crate::recording` or `auv_cli::recording`. If Cargo fails first because of an
unrelated dirty-worktree change, stop and report the blocker instead of folding
that fix into this slice.

## Task 2: Migrate Root Crate Internal Imports

**Files:**
- Modify: `src/runtime.rs`
- Modify: `src/inspect_server/mod.rs`
- Modify: `src/scroll_scan/mod.rs`
- Modify: `src/app/mod.rs`
- Modify: `src/app/infra.rs`
- Modify: `src/osu.rs`

- [ ] **Step 1: Update `src/runtime.rs` imports**

Replace the production import:

```rust
use crate::recording::{MemoryRunRecorder, RunRecorder, RunRecordingBackend};
```

with:

```rust
use auv_tracing_driver::{MemoryRunRecorder, RunRecorder, RunRecordingBackend};
```

Inside the test module in the same file, replace:

```rust
use crate::recording::{MemoryRunRecorder, RunRecorder, RunUpdate};
```

with:

```rust
use auv_tracing_driver::{MemoryRunRecorder, RunRecorder, RunUpdate};
```

- [ ] **Step 2: Update `src/inspect_server/mod.rs` imports**

Near the top-level imports, replace:

```rust
use crate::recording::{BroadcastRunRecorder, RunRecorder, RunUpdate, WireUpdate};
```

with:

```rust
use auv_tracing_driver::{BroadcastRunRecorder, RunRecorder, RunUpdate, WireUpdate};
```

Inside the test module in the same file, replace:

```rust
use crate::recording::{BroadcastRunRecorder, RunRecorder, RunUpdate};
```

with:

```rust
use auv_tracing_driver::{BroadcastRunRecorder, RunRecorder, RunUpdate};
```

- [ ] **Step 3: Update `RecordingHandle` users**

In each file below, replace:

```rust
use crate::recording::RecordingHandle;
```

with:

```rust
use auv_tracing_driver::RecordingHandle;
```

Files:

```text
src/scroll_scan/mod.rs
src/app/mod.rs
src/app/infra.rs
src/osu.rs
```

- [ ] **Step 4: Run focused search**

Run:

```bash
rg -n "crate::recording" -g '*.rs' src
```

Expected: no matches. If matches remain, migrate only those recording imports
to `auv_tracing_driver`.

## Task 3: Migrate Binary And Workspace Test Imports

**Files:**
- Modify: `src/main.rs`
- Modify: `crates/auv-inference-ultralytics/Cargo.toml`
- Modify: `crates/auv-inference-ultralytics/tests/fixture_parity.rs`

- [ ] **Step 1: Update recorder construction in `src/main.rs`**

Replace root-public recording paths with owner-crate paths:

```rust
auv_cli::recording::BroadcastRunRecorder
auv_cli::recording::RunRecorder
auv_cli::recording::InspectServerRunRecorder
auv_cli::recording::NoopRunRecorder
auv_cli::recording::CompositeRunRecorder
auv_cli::recording::RunRecordingBackend
```

becomes:

```rust
auv_tracing_driver::BroadcastRunRecorder
auv_tracing_driver::RunRecorder
auv_tracing_driver::InspectServerRunRecorder
auv_tracing_driver::NoopRunRecorder
auv_tracing_driver::CompositeRunRecorder
auv_tracing_driver::RunRecordingBackend
```

For example, this code:

```rust
let mut recorders: Vec<Arc<dyn auv_cli::recording::RunRecorder>> = Vec::new();
```

becomes:

```rust
let mut recorders: Vec<Arc<dyn auv_tracing_driver::RunRecorder>> = Vec::new();
```

- [ ] **Step 2: Add the direct dev-dependency for the ultralytics test crate**

In `crates/auv-inference-ultralytics/Cargo.toml`, under `[dev-dependencies]`,
ensure these entries exist:

```toml
auv-cli = { path = "../.." }
auv-tracing-driver = { path = "../auv-tracing-driver" }
```

Do not move unrelated dependency entries.

- [ ] **Step 3: Update `fixture_parity.rs` imports**

In `crates/auv-inference-ultralytics/tests/fixture_parity.rs`, replace:

```rust
use auv_cli::recording::{BroadcastRunRecorder, RunRecordingBackend};
use auv_cli::{inspect_server, store::LocalStore};
```

with:

```rust
use auv_cli::{inspect_server, store::LocalStore};
use auv_tracing_driver::{BroadcastRunRecorder, RunRecordingBackend};
```

- [ ] **Step 4: Run focused search**

Run:

```bash
rg -n "auv_cli::recording" -g '*.rs' .
```

Expected: no matches. If matches remain, migrate them to `auv_tracing_driver`.

## Task 4: Delete The Shim And Validate

**Files:**
- Delete: `src/recording/mod.rs`
- Validate: all Rust files touched above

- [ ] **Step 1: Delete the shim file**

Delete:

```text
src/recording/mod.rs
```

If the now-empty `src/recording/` directory remains locally, it does not need
to be tracked by Git.

- [ ] **Step 2: Verify no root recording module users remain**

Run:

```bash
rg -n "pub mod recording|crate::recording|auv_cli::recording" -g '*.rs' .
```

Expected: no matches.

- [ ] **Step 3: Run compile validation**

Run:

```bash
cargo check
```

Expected: PASS. If it fails because of unrelated existing workspace changes,
report the first unrelated failure clearly and keep this slice limited to the
recording import migration.

- [ ] **Step 4: Run formatting and diff hygiene**

Run:

```bash
cargo fmt --check
git diff --check
```

Expected: both PASS. If `cargo fmt --check` reports formatting only in touched
files, run `cargo fmt` and repeat this step. If formatting failures are only in
unrelated dirty files, report them instead of folding them into this slice.

- [ ] **Step 5: Review final diff scope**

Run:

```bash
git diff -- src/lib.rs src/runtime.rs src/inspect_server/mod.rs src/main.rs src/scroll_scan/mod.rs src/app/mod.rs src/app/infra.rs src/osu.rs crates/auv-inference-ultralytics/Cargo.toml crates/auv-inference-ultralytics/tests/fixture_parity.rs src/recording/mod.rs
```

Expected: the diff only removes the root `recording` module, rewrites recording
imports/paths to `auv_tracing_driver`, and adds the ultralytics dev-dependency.

## Task 5: Commit The Slice When The Worktree Is Ready

**Files:**
- Commit: all files changed by Tasks 1-4

- [ ] **Step 1: Check for unrelated dirty work**

Run:

```bash
git status --short
```

Expected: recording-shim-removal files may be dirty. If unrelated files are
also dirty, do not stage them.

- [ ] **Step 2: Stage only this slice**

Run:

```bash
git add src/lib.rs src/runtime.rs src/inspect_server/mod.rs src/main.rs src/scroll_scan/mod.rs src/app/mod.rs src/app/infra.rs src/osu.rs crates/auv-inference-ultralytics/Cargo.toml crates/auv-inference-ultralytics/tests/fixture_parity.rs
git add -u src/recording/mod.rs
```

Expected: only this slice is staged.

- [ ] **Step 3: Commit**

Run:

```bash
git commit -m "refactor(recording): remove root compatibility shim"
```

Expected: commit succeeds. If the owner wants to batch this with other active
workspace changes, skip this step and report the exact staged/unstaged state.

## Self-Review Notes

- Spec coverage: the plan deletes `src/recording/mod.rs`, removes
  `pub mod recording`, migrates every listed current user, adds the required
  ultralytics dev-dependency, and validates with the spec's search and Cargo
  commands.
- Placeholder scan: no unfinished placeholder markers or vague deferred-work
  tasks are present.
- Type consistency: all migrated symbols are already crate-level re-exports in
  `crates/auv-tracing-driver/src/lib.rs`.
