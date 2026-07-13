# auv-tracing-driver Extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract AUV durable run/span/event/artifact recording into a dedicated `auv-tracing-driver` crate while keeping root CLI invoke behavior unchanged.

**Architecture:** Move the existing recording substrate (`trace`, `store`, `run_builder`, `recording`, and `recorded_operation`) into a workspace crate that owns durable recording and emits optional Rust `tracing` instrumentation. The root `auv-cli` crate keeps compatibility shims during this PR, then `Runtime` delegates recording calls through `RecordingHandle` instead of owning duplicate lifecycle/staging logic.

**Tech Stack:** Rust 2024, Cargo workspace crates, `serde`, `serde_json`, `tokio::sync::broadcast`, `reqwest` blocking client for inspect-server write mode, `tracing` for structured observability events, existing `LocalStore` persisted run format.

---

## Source Specs

Implement against:

- `docs/ai/references/inspect/2026-06-10-tracing-driver-runtime-recording-split.md`
- `docs/TERMS_AND_CONCEPTS.md` sections "Driver Tracing Boundary" and "Interaction Tracing Boundary"

Reference projects cloned locally for ecosystem guidance:

- `/Users/neko/Git/github.com/tokio-rs/tracing`
- `/Users/neko/Git/github.com/tokio-rs/console`
- `/Users/neko/Git/github.com/getsentry/sentry-rust`
- `/Users/neko/Git/github.com/open-telemetry/opentelemetry-rust`

## File Structure

- Create `crates/auv-tracing-driver/Cargo.toml`
  - Owns dependencies needed by durable recording and optional telemetry emission.
- Create `crates/auv-tracing-driver/src/lib.rs`
  - Public crate entrypoint; re-exports durable recording primitives.
- Create `crates/auv-tracing-driver/src/error.rs`
  - Defines `AuvResult<T> = Result<T, String>`.
- Create `crates/auv-tracing-driver/src/time.rs`
  - Defines `now_millis()`.
- Create `crates/auv-tracing-driver/src/artifact.rs`
  - Defines `ArtifactRef`, `ArtifactFileSource`, and `ProducedArtifact`.
- Move `src/trace.rs` to `crates/auv-tracing-driver/src/trace.rs`
  - Owns persisted run/span/event/artifact wire records and ids.
- Move `src/store.rs` to `crates/auv-tracing-driver/src/store.rs`
  - Owns local run store, artifact copy/write/read behavior, and persisted snapshots.
- Move `src/run_builder.rs` to `crates/auv-tracing-driver/src/run_builder.rs`
  - Owns in-memory canonical run construction and recorder update fan-out.
- Move `src/recording/` to `crates/auv-tracing-driver/src/recording/`
  - Owns recorder trait, backends, update enum, and inspect-server wire adapter.
- Move `src/recorded_operation.rs` to `crates/auv-tracing-driver/src/recorded_operation.rs`
  - Owns typed recorded-operation context and artifact staging helper.
- Replace root `src/trace.rs`, `src/store.rs`, `src/run_builder.rs`, `src/recorded_operation.rs`, and `src/recording/mod.rs` with compatibility re-export shims.
- Modify `src/contract.rs`
  - Re-export `ArtifactRef` from `auv-tracing-driver` instead of defining a duplicate struct.
- Modify `src/model.rs`
  - Re-export `AuvResult`, `ProducedArtifact`, and `now_millis` from `auv-tracing-driver`.
- Modify `src/runtime.rs`
  - Delegate recording lifecycle and artifact staging to `RecordingHandle`.
- Modify root `Cargo.toml`
  - Add the workspace member and root dependency.

## Boundary Decisions

- `auv-tracing-driver` is AUV-first durable recording. It must not initialize global tracing subscribers or OTel exporters.
- Rust `tracing` spans/events are an observability side channel. Missing subscriber setup must not change whether AUV run artifacts persist.
- Root compatibility modules are allowed in this PR so downstream code can continue importing `auv_cli::trace`, `auv_cli::store`, `auv_cli::recording`, `auv_cli::run_builder`, and `auv_cli::recorded_operation`.
- `Runtime` remains the invoke execution facade in this PR. Full `runtime.rs` deletion belongs to the later root driver compatibility removal slice.

---

### Task 1: Scaffold `auv-tracing-driver` With Local Support Types

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/auv-tracing-driver/Cargo.toml`
- Create: `crates/auv-tracing-driver/src/lib.rs`
- Create: `crates/auv-tracing-driver/src/error.rs`
- Create: `crates/auv-tracing-driver/src/time.rs`
- Create: `crates/auv-tracing-driver/src/artifact.rs`

- [ ] **Step 1: Add the workspace member and root dependency**

Edit root `Cargo.toml`.

Add this to `[workspace] members` after `crates/auv-steam`:

```toml
  "crates/auv-tracing-driver",
```

Add this to root `[dependencies]` after `auv-steam`:

```toml
auv-tracing-driver = { path = "crates/auv-tracing-driver" }
```

Move the existing root package `reqwest` dependency shape into `[workspace.dependencies]`, then keep the root package on the centralized dependency:

```toml
reqwest.workspace = true
```

- [ ] **Step 2: Create the new crate manifest**

Create `crates/auv-tracing-driver/Cargo.toml`:

```toml
[package]
name = "auv-tracing-driver"
version.workspace = true
edition.workspace = true
publish.workspace = true
readme.workspace = true
license.workspace = true
authors.workspace = true
description.workspace = true
documentation.workspace = true
homepage.workspace = true
repository.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
tokio = { version = "1", features = ["sync"] }
reqwest.workspace = true
tracing = "0.1"
```

- [ ] **Step 3: Create the crate entrypoint**

Create `crates/auv-tracing-driver/src/lib.rs`:

```rust
//! Durable AUV driver-level run recording.
//!
//! This crate owns AUV's persisted run/span/event/artifact model and recorder
//! fan-out. It emits ordinary Rust `tracing` events for observability, but does
//! not install subscribers or OpenTelemetry exporters.

pub mod artifact;
pub mod error;
pub mod time;

pub use artifact::{ArtifactFileSource, ArtifactRef, ProducedArtifact};
pub use error::AuvResult;
pub use time::now_millis;
```

- [ ] **Step 4: Add the local result alias**

Create `crates/auv-tracing-driver/src/error.rs`:

```rust
pub type AuvResult<T> = Result<T, String>;
```

- [ ] **Step 5: Add the timestamp helper**

Create `crates/auv-tracing-driver/src/time.rs`:

```rust
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_millis() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_millis() as u64)
    .unwrap_or(0)
}
```

- [ ] **Step 6: Add artifact support types**

Create `crates/auv-tracing-driver/src/artifact.rs`:

```rust
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ArtifactRef {
  // TODO(auv-tracing-driver-task2): replace string IDs with trace ID newtypes
  // after `trace` moves into this crate; Task 1 must build without placeholder
  // future modules.
  pub run_id: String,
  pub artifact_id: String,
  pub span_id: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub captured_event_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProducedArtifact {
  pub kind: String,
  pub source_path: PathBuf,
  pub preferred_name: String,
  pub note: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ArtifactFileSource {
  pub role: String,
  pub source_path: PathBuf,
  pub preferred_name: String,
  pub summary: Option<String>,
}
```

- [ ] **Step 7: Check formatting for the new crate skeleton**

Run:

```bash
cargo fmt --check
```

Expected: pass. Task 1 exposes only scaffold modules that already exist; Task 2 adds the moved recording/trace/store/run_builder module declarations and reexports after the files are moved.

- [ ] **Step 8: Commit the scaffold**

```bash
git add Cargo.toml crates/auv-tracing-driver
git commit -m "feat(auv-tracing-driver): scaffold recording crate"
```

---

### Task 2: Move Trace, Store, Run Builder, Recorder, And Recorded Operation Modules

**Files:**
- Move: `src/trace.rs` -> `crates/auv-tracing-driver/src/trace.rs`
- Move: `src/store.rs` -> `crates/auv-tracing-driver/src/store.rs`
- Move: `src/run_builder.rs` -> `crates/auv-tracing-driver/src/run_builder.rs`
- Move: `src/recorded_operation.rs` -> `crates/auv-tracing-driver/src/recorded_operation.rs`
- Move: `src/recording/**` -> `crates/auv-tracing-driver/src/recording/**`
- Create: `src/trace.rs`
- Create: `src/store.rs`
- Create: `src/run_builder.rs`
- Create: `src/recorded_operation.rs`
- Create: `src/recording/mod.rs`

- [ ] **Step 1: Move files with git**

Run:

```bash
git mv src/trace.rs crates/auv-tracing-driver/src/trace.rs
git mv src/store.rs crates/auv-tracing-driver/src/store.rs
git mv src/run_builder.rs crates/auv-tracing-driver/src/run_builder.rs
git mv src/recorded_operation.rs crates/auv-tracing-driver/src/recorded_operation.rs
git mv src/recording crates/auv-tracing-driver/src/recording
mkdir -p src/recording
```

Expected: the moved files are staged as renames and `src/recording` exists again for the compatibility shim.

After moving the files, update `crates/auv-tracing-driver/src/lib.rs` to declare `recorded_operation`, `recording`, `run_builder`, `store`, and `trace`, and restore the full public reexports for those moved modules. Also update `artifact.rs` to use `crate::trace::{ArtifactId, EventId, RunId, SpanId}` instead of Task 1 string IDs.

- [ ] **Step 2: Rewrite moved-module imports to local crate paths**

In `crates/auv-tracing-driver/src/run_builder.rs`, replace imports:

```rust
use crate::model::{AuvResult, now_millis};
use crate::recording::{RunRecorder, RunUpdate};
use crate::store::CanonicalRun;
```

with:

```rust
use crate::error::AuvResult;
use crate::recording::{RunRecorder, RunUpdate};
use crate::store::CanonicalRun;
use crate::time::now_millis;
```

Also replace `crate::trace::...` references only when the imported type is already in scope. Keep explicit `crate::trace::DeviceId`, `crate::trace::SessionId`, and `crate::trace::RUN_ATTR_DEVICE_ID` paths readable.

- [ ] **Step 3: Rewrite `store.rs` imports**

In `crates/auv-tracing-driver/src/store.rs`, replace:

```rust
use crate::model::{AuvResult, ProducedArtifact, now_millis};
```

with:

```rust
use crate::artifact::{ArtifactFileSource, ProducedArtifact};
use crate::error::AuvResult;
use crate::time::now_millis;
```

Remove any duplicate local `ArtifactFileSource` definition from the moved file if present. The public source type must be `crate::artifact::ArtifactFileSource`.

- [ ] **Step 4: Rewrite `recording/backend.rs` imports**

In `crates/auv-tracing-driver/src/recording/backend.rs`, replace:

```rust
use crate::contract::ArtifactRef;
use crate::model::{AuvResult, now_millis};
use crate::store::{ArtifactFileSource, CanonicalRun, LocalStore};
```

with:

```rust
use crate::artifact::{ArtifactFileSource, ArtifactRef};
use crate::error::AuvResult;
use crate::store::{CanonicalRun, LocalStore};
use crate::time::now_millis;
```

Replace `crate::model::ProducedArtifact` in `stage_artifact` with `crate::artifact::ProducedArtifact`.

- [ ] **Step 5: Rewrite `recording/recorder.rs` imports**

In `crates/auv-tracing-driver/src/recording/recorder.rs`, replace:

```rust
use crate::model::AuvResult;
```

with:

```rust
use crate::error::AuvResult;
```

- [ ] **Step 6: Rewrite `recorded_operation.rs` imports**

In `crates/auv-tracing-driver/src/recorded_operation.rs`, replace:

```rust
use crate::contract::ArtifactRef;
use crate::model::{AuvResult, now_millis};
use crate::store::ArtifactFileSource;
```

with:

```rust
use crate::artifact::{ArtifactFileSource, ArtifactRef};
use crate::error::AuvResult;
use crate::time::now_millis;
```

If the moved file still refers to `crate::store::ArtifactFileSource` inside methods, replace those paths with `ArtifactFileSource`.

- [ ] **Step 7: Replace root modules with compatibility shims**

Create `src/trace.rs`:

```rust
pub use auv_tracing_driver::trace::*;
```

Create `src/store.rs`:

```rust
pub use auv_tracing_driver::store::*;
pub use auv_tracing_driver::{ArtifactFileSource, ProducedArtifact};
```

Create `src/run_builder.rs`:

```rust
pub use auv_tracing_driver::run_builder::*;
```

Create `src/recorded_operation.rs`:

```rust
pub use auv_tracing_driver::recorded_operation::*;
```

Create `src/recording/mod.rs`:

```rust
pub use auv_tracing_driver::recording::*;
```

- [ ] **Step 8: Run the moved crate tests**

Run:

```bash
cargo test -p auv-tracing-driver
```

Expected: tests from moved modules compile and pass. If the command fails with unresolved `crate::model`, `crate::contract`, or `crate::store::ArtifactFileSource`, fix the imports listed in Steps 2-6 and rerun.

- [ ] **Step 9: Run root compatibility tests**

Run:

```bash
cargo test recorded_operation::tests
cargo test recording::backend::tests
cargo test run_builder::tests
cargo test store::tests
cargo test trace::tests
```

Expected: tests compile through root re-export modules and pass.

- [ ] **Step 10: Commit the module extraction**

```bash
git add Cargo.toml Cargo.lock crates/auv-tracing-driver src/trace.rs src/store.rs src/run_builder.rs src/recorded_operation.rs src/recording
git commit -m "refactor(auv-tracing-driver): move durable recording modules"
```

> Implementation note: during execution, Task 3 was folded into the Task 2
> migration commit because moving `ArtifactRef` and `ProducedArtifact` without
> immediately unifying root `contract`/`model` re-exports made the root crate
> compile against two incompatible contract types. The separate Task 3 checklist
> below remains as the design boundary for that type-unification work.

---

### Task 3: Re-export Shared Types From Root Compatibility Modules

**Files:**
- Modify: `src/contract.rs`
- Modify: `src/model.rs`
- Modify: root modules that fail after type unification

- [ ] **Step 1: Move `ArtifactRef` ownership to `auv-tracing-driver`**

In `src/contract.rs`, find the existing `ArtifactRef` struct definition and remove the full struct block.

Add this near the top-level contract imports:

```rust
pub use auv_tracing_driver::ArtifactRef;
```

Expected: all existing `crate::contract::ArtifactRef` call sites continue compiling and use the same type owned by `auv-tracing-driver`.

- [ ] **Step 2: Re-export root model compatibility types**

In `src/model.rs`, replace the local `AuvResult` type alias and `now_millis()` function with:

```rust
pub use auv_tracing_driver::{AuvResult, ProducedArtifact, now_millis};
```

Remove the local `ProducedArtifact` struct definition if it still exists in `src/model.rs`.

- [ ] **Step 3: Remove unused imports introduced by re-exports**

Run:

```bash
cargo check
```

Expected: compile may fail with unused imports or duplicate definitions. For each duplicate `ArtifactRef`, `AuvResult`, `ProducedArtifact`, or `now_millis` error, keep the `auv_tracing_driver` definition and remove the root duplicate.

- [ ] **Step 4: Verify root and crate type identity through tests**

Add this test to `src/contract.rs` under the existing `#[cfg(test)]` module, or create one if no test module exists:

```rust
#[test]
fn artifact_ref_is_owned_by_tracing_driver_boundary() {
  fn accepts_driver_ref(_value: auv_tracing_driver::ArtifactRef) {}

  let artifact_ref = ArtifactRef {
    run_id: crate::trace::RunId::new("run_type_identity"),
    artifact_id: crate::trace::ArtifactId::new("artifact_type_identity"),
    span_id: crate::trace::SpanId::new("span_type_identity"),
    captured_event_id: Some(crate::trace::EventId::new("event_type_identity")),
  };

  accepts_driver_ref(artifact_ref);
}
```

- [ ] **Step 5: Run the type identity test**

Run:

```bash
cargo test contract::tests::artifact_ref_is_owned_by_tracing_driver_boundary
```

Expected: pass.

- [ ] **Step 6: Commit the compatibility type re-exports**

```bash
git add src/contract.rs src/model.rs
git commit -m "refactor(recording): re-export tracing driver contracts"
```

---

### Task 4: Add Explicit `tracing` Instrumentation Without Subscriber Setup

**Files:**
- Modify: `crates/auv-tracing-driver/src/recording/backend.rs`
- Modify: `crates/auv-tracing-driver/src/recorded_operation.rs`

- [ ] **Step 1: Emit structured events from run lifecycle**

In `crates/auv-tracing-driver/src/recording/backend.rs`, inside `RecordingHandle::start_run`, after `let run_id = new_run_id();` and `let root_span_id = new_span_id();`, add:

```rust
tracing::info!(
  target: "auv.tracing_driver",
  auv.run_id = %run_id,
  auv.root_span_id = %root_span_id,
  auv.run_type = ?spec.run_type,
  "AUV run started"
);
```

Inside `RecordingHandle::finish_run`, after `let run_id = recorded.snapshot.run.run_id.clone();`, add:

```rust
tracing::info!(
  target: "auv.tracing_driver",
  auv.run_id = %run_id,
  auv.status = ?recorded.snapshot.run.status_code,
  "AUV run finished"
);
```

- [ ] **Step 2: Emit structured events from artifact staging**

In `RecordingHandle::stage_artifact_file`, after `let artifact = self.recording.stage_artifact_file(...) ?;`, add:

```rust
tracing::info!(
  target: "auv.tracing_driver",
  auv.run_id = %run.id(),
  auv.span_id = %span.id(),
  auv.artifact_id = %artifact.artifact_id,
  auv.artifact_role = %artifact.role,
  "AUV artifact staged"
);
```

In `RecordingHandle::stage_artifact_file_with_ref`, add the same `tracing::info!` block after the artifact is staged and before `record_event_with_id`.

- [ ] **Step 3: Wrap recorded operations in a Rust tracing span**

In `crates/auv-tracing-driver/src/recorded_operation.rs`, inside `run_recorded_operation`, after:

```rust
let run_id = run.id().clone();
let run_dir = (services.run_dir)(run_id.as_str())?;
```

add:

```rust
let operation_span = tracing::info_span!(
  target: "auv.tracing_driver",
  "auv.recorded_operation",
  auv.run_id = %run_id,
  auv.root_span_id = %root.id(),
  auv.operation_label = %operation_label,
);
let _operation_span_guard = operation_span.enter();
```

Keep the guard scoped over the synchronous operation. Do not use this guard across an async `.await` point; this helper is currently synchronous.

- [ ] **Step 4: Add a source-level guard against subscriber setup**

Add this test to `crates/auv-tracing-driver/src/lib.rs` under a `#[cfg(test)]` module:

```rust
#[cfg(test)]
mod tests {
  #[test]
  fn crate_does_not_initialize_global_tracing_subscriber() {
    let source_files = [
      include_str!("recording/backend.rs"),
      include_str!("recording/recorder.rs"),
      include_str!("recorded_operation.rs"),
      include_str!("run_builder.rs"),
      include_str!("store.rs"),
      include_str!("trace.rs"),
    ];

    for source in source_files {
      assert!(
        !source.contains("set_global_default")
          && !source.contains(".init()")
          && !source.contains("tracing_subscriber::"),
        "auv-tracing-driver must emit tracing data without installing subscribers"
      );
    }
  }
}
```

- [ ] **Step 5: Run focused tracing-driver tests**

Run:

```bash
cargo test -p auv-tracing-driver
```

Expected: pass. The new instrumentation should not require any subscriber in tests.

- [ ] **Step 6: Commit the instrumentation**

```bash
git add crates/auv-tracing-driver
git commit -m "feat(auv-tracing-driver): emit structured tracing events"
```

---

### Task 5: Shrink `Runtime` To Recording Facade Delegation

**Files:**
- Modify: `src/runtime.rs`

- [ ] **Step 1: Replace `Runtime::run_recorded_operation` with handle delegation**

In `src/runtime.rs`, replace the body of `Runtime::run_recorded_operation` with:

```rust
  {
    self
      .recording
      .handle()
      .run_recorded_operation(spec, operation_label, operation)
  }
```

The full method should remain:

```rust
  pub fn run_recorded_operation<T, E, F>(
    &self,
    spec: crate::run_builder::RunSpec,
    operation_label: impl Into<String>,
    operation: F,
  ) -> AuvResult<crate::recorded_operation::RecordedOperationOutput<T>>
  where
    E: std::fmt::Display,
    F: FnOnce(&mut crate::recorded_operation::RecordedOperationContext<'_>) -> Result<T, E>,
  {
    self
      .recording
      .handle()
      .run_recorded_operation(spec, operation_label, operation)
  }
```

- [ ] **Step 2: Delegate `start_run` and `finish_run`**

Replace `Runtime::start_run` body with:

```rust
  {
    self.recording.handle().start_run(spec)
  }
```

Replace `Runtime::finish_run` body with:

```rust
  {
    self.recording.handle().finish_run(run, finish)
  }
```

Keep the methods public for compatibility in this PR.

- [ ] **Step 3: Delegate artifact staging helpers**

Replace `Runtime::stage_artifact_file` body with:

```rust
  {
    self
      .recording
      .handle()
      .stage_artifact_file(run, span, role, source_path, preferred_name, summary)
  }
```

Replace `Runtime::stage_artifact_file_with_ref` body with:

```rust
  {
    self
      .recording
      .handle()
      .stage_artifact_file_with_ref(run, span, role, source_path, preferred_name, summary)
  }
```

- [ ] **Step 4: Remove runtime-local recording helpers and imports**

From `src/runtime.rs`, remove imports that are only used by deleted local recording logic:

```rust
use crate::store::ArtifactFileSource;
use crate::trace::{
  EVENT_API_VERSION, EventRecordV1Alpha1, RUN_API_VERSION, RunRecordV1Alpha1,
  SPAN_API_VERSION, SpanRecordV1Alpha1, TraceFailure, new_event_id, new_run_id,
  new_span_id, new_trace_id,
};
```

Keep imports still used by invoke execution:

```rust
use crate::trace::{RunId, RunType, TraceStatusCode, string_attr};
```

Delete runtime-local helper functions that became unused:

```rust
fn record_event_with_id(...)
fn render_artifact_event(...)
```

- [ ] **Step 5: Add a deferral marker on runtime compatibility methods**

Above `Runtime::start_run`, add:

```rust
  // TODO(runtime-facade-delete): recording lifecycle methods remain here only
  // for callers that still construct Runtime; delete these facades once invoke
  // and typed workflows depend directly on auv-tracing-driver RecordingHandle.
```

- [ ] **Step 6: Run runtime and recorded operation tests**

Run:

```bash
cargo test runtime::tests
cargo test recorded_operation::tests
cargo test candidate_action_decision::tests::recorded_operation_persists_decide_only_action_decision_artifact
cargo test candidate_promotion_recording::tests::recorded_operation_persists_candidate_promotion_artifact
```

Expected: pass.

- [ ] **Step 7: Commit runtime delegation**

```bash
git add src/runtime.rs
git commit -m "refactor(runtime): delegate recording to tracing driver"
```

---

### Task 6: Move Direct Typed Callers To RecordingHandle Where It Reduces Runtime Coupling

**Files:**
- Modify: `src/osu.rs`
- Modify: `crates/auv-inference-ultralytics/tests/fixture_parity.rs`
- Modify: `crates/auv-inference-ultralytics/tests/slay_the_spire_observe_only_boundary.rs`
- Modify: any root tests that only build `Runtime` to call `run_recorded_operation`

- [ ] **Step 1: Update `src/osu.rs` function signatures**

In `src/osu.rs`, replace imports:

```rust
use crate::recorded_operation::RecordedOperationOutput;
use crate::runtime::Runtime;
```

with:

```rust
use crate::recorded_operation::RecordedOperationOutput;
use crate::recording::RecordingHandle;
```

For each function that accepts `runtime: &Runtime` only to call `run_recorded_operation`, change the parameter to:

```rust
recording: &RecordingHandle
```

and replace:

```rust
runtime.run_recorded_operation(
```

with:

```rust
recording.run_recorded_operation(
```

- [ ] **Step 2: Update root call sites for osu commands**

In `src/main.rs`, find calls into `src/osu.rs` functions changed in Step 1. Pass:

```rust
runtime.recording().handle()
```

or, if a reference is needed:

```rust
&runtime.recording().handle()
```

Expected: `src/osu.rs` no longer imports `Runtime`.

- [ ] **Step 3: Update inference tests to use a recording handle**

In `crates/auv-inference-ultralytics/tests/fixture_parity.rs`, replace:

```rust
let runtime = auv_cli::build_runtime_with_store_root(project_root.clone(), store_root.clone())
  .expect("runtime should build");
let recorded = runtime.run_recorded_operation(
```

with:

```rust
let store = auv_cli::store::LocalStore::new(store_root.clone()).expect("store should build");
let recording = auv_cli::recording::RunRecordingBackend::local_only(store).handle();
let recorded = recording.run_recorded_operation(
```

Apply the same replacement in `crates/auv-inference-ultralytics/tests/slay_the_spire_observe_only_boundary.rs`.

- [ ] **Step 4: Run typed caller tests**

Run:

```bash
cargo test -p auv-game-osu
cargo test -p auv-inference-ultralytics
```

Expected: pass. If `auv-game-osu` has no tests affected by root `src/osu.rs`, run:

```bash
cargo test osu::tests
```

Expected: pass or "0 tests" with successful compilation.

- [ ] **Step 5: Search for unnecessary Runtime recorded-operation callers**

Run:

```bash
rg "run_recorded_operation\\(" src crates -n
```

Expected: callers that need full `Runtime` for invoke may remain; typed-only callers should use `RecordingHandle`.

- [ ] **Step 6: Commit typed caller decoupling**

```bash
git add src/osu.rs src/main.rs crates/auv-inference-ultralytics/tests/fixture_parity.rs crates/auv-inference-ultralytics/tests/slay_the_spire_observe_only_boundary.rs
git commit -m "refactor(recording): call tracing driver from typed workflows"
```

---

### Task 7: Update Documentation And Run Full Verification

**Files:**
- Modify: `docs/TERMS_AND_CONCEPTS.md`
- Modify: `docs/ai/references/inspect/2026-06-10-tracing-driver-runtime-recording-split.md`
- Modify: this plan file if implementation discovers a narrower approved boundary

- [ ] **Step 1: Update terms for completed boundary**

In `docs/TERMS_AND_CONCEPTS.md`, update the "Driver Tracing Boundary" section so it states:

```markdown
The driver tracing boundary is implemented by `auv-tracing-driver`. It owns
durable AUV run/span/event/artifact recording and may emit Rust `tracing`
spans/events for observability. It does not install global subscribers or
OpenTelemetry exporters; binaries and servers configure those layers.
```

- [ ] **Step 2: Update the tracing-driver reference status**

In `docs/ai/references/inspect/2026-06-10-tracing-driver-runtime-recording-split.md`, change the status line to:

```markdown
Status: implementation landed for durable recording extraction; interaction tracing remains deferred
```

Add this under "Deferrals":

```markdown
NOTICE(runtime-facade): `Runtime` still exposes recording facade methods for
remaining invoke and historical callers. New typed workflows should use
`auv_tracing_driver::RecordingHandle` directly.
```

- [ ] **Step 3: Run formatting**

Run:

```bash
cargo fmt --check
```

Expected: pass.

- [ ] **Step 4: Run full check**

Run:

```bash
cargo check
```

Expected: pass.

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test -p auv-tracing-driver
cargo test recorded_operation::tests
cargo test runtime::tests
cargo test mcp::tests::mcp_server_lists_and_invokes_shared_runtime
```

Expected: pass.

- [ ] **Step 6: Run full tests if focused tests pass**

Run:

```bash
cargo test
```

Expected: pass. If full tests expose unrelated platform or long-running failures, record the exact failing test names and keep the focused verification output in the final handoff.

- [ ] **Step 7: Check whitespace**

Run:

```bash
git diff --check
```

Expected: no output.

- [ ] **Step 8: Commit documentation and final verification updates**

```bash
git add docs/TERMS_AND_CONCEPTS.md docs/ai/references/inspect/2026-06-10-tracing-driver-runtime-recording-split.md docs/ai/references/driver/2026-06-16-tracing-driver-extraction-implementation-plan.md
git commit -m "docs(tracing-driver): record extraction completion"
```

---

## Self-Review

Spec coverage:

- Durable run/span/event/artifact recording moves behind `auv-tracing-driver`: Tasks 1-3.
- Rust ecosystem split between library instrumentation and binary subscriber setup: Task 4.
- Runtime no longer owns core recording logic: Task 5.
- Typed workflows can record without constructing `Runtime`: Task 6.
- Docs and terms reflect the new boundary: Task 7.

Placeholder scan:

- No placeholder wording or unspecified edge handling remains.
- The only deferral marker is an explicit project marker with a removal condition.

Type consistency:

- `AuvResult`, `ArtifactRef`, `ProducedArtifact`, `ArtifactFileSource`, `RunRecordingBackend`, `RecordingHandle`, `RecordingRun`, and `RunSpec` are owned by `auv-tracing-driver` and re-exported by root compatibility modules.
- `Runtime` methods remain facades over `RecordingHandle`, so existing call sites can compile while typed callers migrate directly.
