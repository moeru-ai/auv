# auv-cli-invoke Traced Wrapper Runtime Exit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the invoke tracing wrapper out of `src/runtime.rs`, let CLI/MCP/app/scroll-scan call the shared `auv-cli-invoke` wrapper directly, and delete the migrated runtime invoke code.

**Architecture:** `InvokeCommand` handlers remain the semantic owners of `<domain>.<action>` execution. `auv-cli-invoke` gains shared request/result models plus a traced wrapper that resolves commands, opens command spans, calls handlers, records handler output through `auv-tracing-driver`, and returns `InvokeResult`. Root `Runtime` stops mediating invoke and is left only with non-invoke legacy facades that later cleanup can remove.

**Tech Stack:** Rust 2024, `auv-cli-invoke`, `auv-tracing-driver`, existing root CLI/MCP/app/scroll-scan code, `cargo test`.

---

## File Structure

- Create `crates/auv-cli-invoke/src/model.rs`
  - Own `ExecutionTarget`, `InvokeRequest`, `RunStatus`, and `InvokeResult`.
- Create `crates/auv-cli-invoke/src/recorded.rs`
  - Own the reusable traced wrapper APIs and helper functions copied from runtime invoke code.
- Modify `crates/auv-cli-invoke/src/lib.rs`
  - Export `model` and `recorded` APIs.
- Modify `src/model.rs`
  - Re-export invoke models from `auv-cli-invoke`.
  - Keep only root-local aliases such as `AuvResult` and `now_millis`.
- Modify `src/runtime.rs`
  - First delegate invoke methods to `auv-cli-invoke::recorded`.
  - After callers migrate, delete invoke methods and invoke-only tests/helpers.
- Modify `src/main.rs`
  - Build `RunRecordingBackend` directly for invoke paths.
  - Call `auv-cli-invoke` recorded wrapper instead of `Runtime::invoke` / `invoke_resolved`.
- Modify `src/mcp.rs`
  - Build store/recording directly for generic invoke.
  - Call `auv-cli-invoke` recorded wrapper.
- Modify `src/app/infra.rs`
  - Replace `Runtime::invoke_in_span` with `auv-cli-invoke::invoke_recorded_in_span`.
- Modify `src/scroll_scan/mod.rs`
  - Replace `Runtime::invoke_in_span` with `auv-cli-invoke::invoke_recorded_in_span`.
- Modify tests in `src/runtime.rs` and `crates/auv-cli-invoke`
  - Move invoke wrapper coverage to `auv-cli-invoke`.
  - Remove runtime tests that only cover moved invoke wrapper behavior.

---

### Task 1: Move Invoke Models Into auv-cli-invoke

**Files:**
- Create: `crates/auv-cli-invoke/src/model.rs`
- Modify: `crates/auv-cli-invoke/src/lib.rs`
- Modify: `src/model.rs`
- Test: `cargo test -p auv-cli-invoke`

- [ ] **Step 1: Write the model module in `auv-cli-invoke`**

Add this file:

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;

use auv_tracing_driver::trace::{ArtifactRecordV1Alpha1, SpanId};

#[derive(Clone, Debug, Default)]
pub struct ExecutionTarget {
  pub application_id: Option<String>,
  pub target_label: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct InvokeRequest {
  pub command_id: String,
  pub target: ExecutionTarget,
  pub inputs: BTreeMap<String, String>,
  pub dry_run: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunStatus {
  Completed,
  Failed,
}

impl RunStatus {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Completed => "completed",
      Self::Failed => "failed",
    }
  }
}

#[derive(Clone, Debug)]
pub struct InvokeResult {
  pub run_id: String,
  pub producer_span_id: SpanId,
  pub status: RunStatus,
  pub output_summary: String,
  pub signals: BTreeMap<String, String>,
  pub artifacts: Vec<ArtifactRecordV1Alpha1>,
  pub artifact_paths: Vec<PathBuf>,
  pub failure_message: Option<String>,
}
```

- [ ] **Step 2: Export the model types from `auv-cli-invoke`**

In `crates/auv-cli-invoke/src/lib.rs`, add:

```rust
pub mod model;
```

and add the model types to the public exports:

```rust
pub use model::{ExecutionTarget, InvokeRequest, InvokeResult, RunStatus};
```

- [ ] **Step 3: Re-export invoke models from root `src/model.rs`**

Replace the root-local definitions of `ExecutionTarget`, `InvokeRequest`, `RunStatus`, and `InvokeResult` with:

```rust
pub use auv_cli_invoke::{ExecutionTarget, InvokeRequest, InvokeResult, RunStatus};
pub use auv_tracing_driver::{AuvResult, now_millis};
```

Remove now-unused imports from `src/model.rs`:

```rust
use auv_tracing_driver::trace::{ArtifactRecordV1Alpha1, SpanId};
use std::collections::BTreeMap;
use std::path::PathBuf;
```

Also remove the root-local `new_run_id` function and its test if no call sites remain. Confirm with:

```bash
git grep -n "new_run_id" -- src crates
```

Expected after removal: no match in `src/model.rs`; `auv-tracing-driver::trace::new_run_id` may still match inside the tracing crate.

- [ ] **Step 4: Validate the model move**

Run:

```bash
cargo test -p auv-cli-invoke
```

Expected: pass. If root compile fails later because call sites still import through `auv_cli::model`, that is fine; the re-export keeps those paths valid.

- [ ] **Step 5: Commit**

```bash
git add crates/auv-cli-invoke/src/model.rs crates/auv-cli-invoke/src/lib.rs src/model.rs
git commit -m "refactor(auv-cli-invoke): own invoke request models"
```

---

### Task 2: Add the Traced Invoke Wrapper to auv-cli-invoke

**Files:**
- Create: `crates/auv-cli-invoke/src/recorded.rs`
- Modify: `crates/auv-cli-invoke/src/lib.rs`
- Test: `crates/auv-cli-invoke/src/recorded.rs` unit tests

- [ ] **Step 1: Create failing wrapper tests**

Create `crates/auv-cli-invoke/src/recorded.rs` with tests first. The test module should cover:

```rust
#[test]
fn invoke_recorded_records_successful_handler_output() {
  // Arrange a registry containing a fixture command whose handler returns:
  // summary "fixture observed", one signal, and no artifacts.
  //
  // Act by calling invoke_recorded(&recording, &registry, request).
  //
  // Assert:
  // - result.status == RunStatus::Completed
  // - result.output_summary == "fixture observed"
  // - result.producer_span_id points to the command span
  // - recording.read_run(result.run_id.as_str()) contains an "auv.command.invoke" span
  // - that span has a "command.resolved" event and a "run.completed" event
}

#[test]
fn invoke_recorded_records_handler_failure_as_failed_result() {
  // Arrange a registry command whose handler returns Err("boom").
  //
  // Act by calling invoke_recorded.
  //
  // Assert:
  // - result.status == RunStatus::Failed
  // - result.failure_message contains "handler failed: boom"
  // - the persisted run has an error command span
}

#[test]
fn invoke_recorded_rejects_unknown_command_and_finishes_failed_run() {
  // Arrange an empty registry.
  //
  // Act by calling invoke_recorded with command_id "missing.command".
  //
  // Assert:
  // - the function returns Err
  // - the error mentions `auv-cli invoke --help`
  // - a failed run snapshot exists when run creation succeeded
}
```

Use real assertions against `CanonicalRun` fields by following the existing runtime tests being moved from `src/runtime.rs`.

- [ ] **Step 2: Run the targeted test and verify failure**

Run:

```bash
cargo test -p auv-cli-invoke recorded -- --nocapture
```

Expected: fail because `invoke_recorded` and related wrapper functions do not exist.

- [ ] **Step 3: Implement wrapper APIs**

In `crates/auv-cli-invoke/src/recorded.rs`, implement these public functions:

```rust
use auv_tracing_driver::run_builder::{
  Attributes, RecordingRun, RunFinish, RunSpec, SpanFinish, SpanRef,
};
use auv_tracing_driver::trace::{RunType, SpanId, TraceStatusCode, string_attr};
use auv_tracing_driver::{AuvResult, RunRecordingBackend};

use crate::{
  InvokeCommand, InvokeCommandInput, InvokeRegistry, InvokeRequest, InvokeResult, RunStatus,
};

pub fn invoke_recorded(
  recording: &RunRecordingBackend,
  registry: &InvokeRegistry,
  request: InvokeRequest,
) -> AuvResult<InvokeResult> {
  let mut run = recording
    .handle()
    .start_run(RunSpec::new(RunType::Command, "auv.command"))?;
  let root = run.root_span();
  let result = match invoke_recorded_in_span(recording, registry, &mut run, &root, request) {
    Ok(result) => result,
    Err(error) => {
      recording.handle().finish_run(
        run,
        RunFinish {
          status_code: TraceStatusCode::Error,
          summary: Some(format!("Invocation failed. Inspect the run for details: {error}")),
          failure: Some(error.clone()),
        },
      )?;
      return Err(error);
    }
  };
  let status_code = if result.status == RunStatus::Completed {
    TraceStatusCode::Ok
  } else {
    TraceStatusCode::Error
  };
  recording.handle().finish_run(
    run,
    RunFinish {
      status_code,
      summary: Some(result.output_summary.clone()),
      failure: result.failure_message.clone(),
    },
  )?;
  Ok(result)
}

pub fn invoke_recorded_in_span(
  recording: &RunRecordingBackend,
  registry: &InvokeRegistry,
  run: &mut RecordingRun,
  parent: &SpanRef,
  request: InvokeRequest,
) -> AuvResult<InvokeResult> {
  let command_id = request.command_id.clone();
  let command = registry.resolve(&command_id).ok_or_else(|| {
    format!(
      "unknown command {command_id}; use `auv-cli invoke --help` to inspect available entries"
    )
  })?;
  invoke_resolved_recorded_in_span(recording, run, parent, command, request)
}

pub fn invoke_resolved_recorded_in_span(
  recording: &RunRecordingBackend,
  run: &mut RecordingRun,
  parent: &SpanRef,
  command: &InvokeCommand,
  request: InvokeRequest,
) -> AuvResult<InvokeResult> {
  let command_span = run.start_span(
    parent,
    auv_tracing_driver::run_builder::running_span_record(
      "auv.command.invoke",
      command_attributes(command.id, request.target.application_id.as_deref()),
    ),
  )?;
  record_event(run, command_span.id(), "command.resolved", Some(format!("resolved {}", command.id)));

  let output = match command.invoke(InvokeCommandInput {
    command_id: command.id,
    target_application_id: request.target.application_id.as_deref(),
    inputs: &request.inputs,
    dry_run: request.dry_run,
  }) {
    Ok(output) => output,
    Err(error) => {
      let failure_message = format!("command {} handler failed: {error}", command.id);
      let output_summary = format!(
        "Command invocation failed after run creation. Inspect {} for the recorded trace.",
        run.id()
      );
      record_event(run, command_span.id(), "command.failed", Some(failure_message.clone()));
      run.finish_span(
        &command_span,
        SpanFinish {
          status_code: TraceStatusCode::Error,
          summary: Some(output_summary.clone()),
          failure: Some(failure_message.clone()),
        },
      )?;
      return Ok(InvokeResult {
        run_id: run.id().to_string(),
        producer_span_id: command_span.id().clone(),
        status: RunStatus::Failed,
        output_summary,
        signals: Default::default(),
        artifacts: Vec::new(),
        artifact_paths: Vec::new(),
        failure_message: Some(failure_message),
      });
    }
  };

  if let Some(backend) = &output.backend {
    record_event(run, command_span.id(), "command.backend", Some(format!("backend={backend}")));
  }
  for note in &output.notes {
    record_event(run, command_span.id(), "command.note", Some(note.clone()));
  }
  if let Some(verification) = &output.verification {
    record_event(run, command_span.id(), "command.verification", Some(verification.clone()));
  }
  for known_limit in &output.known_limits {
    record_event(run, command_span.id(), "command.known_limit", Some(known_limit.clone()));
  }

  let artifact_result = recording.record_produced_artifacts(run, &command_span, output.artifacts);
  let (artifact_records, artifact_paths) = match artifact_result {
    Ok(recorded) => (recorded.records, recorded.paths),
    Err(failure) => {
      let failure_message = format!(
        "command {} artifact recording failed: {}",
        command.id, failure.message
      );
      let output_summary = format!(
        "Command invocation produced output, but artifact recording failed. Inspect {} for the recorded trace.",
        run.id()
      );
      run.finish_span(
        &command_span,
        SpanFinish {
          status_code: TraceStatusCode::Error,
          summary: Some(output_summary.clone()),
          failure: Some(failure_message.clone()),
        },
      )?;
      return Ok(InvokeResult {
        run_id: run.id().to_string(),
        producer_span_id: command_span.id().clone(),
        status: RunStatus::Failed,
        output_summary,
        signals: output.signals,
        artifacts: failure.recorded.records,
        artifact_paths: failure.recorded.paths,
        failure_message: Some(failure_message),
      });
    }
  };

  record_event(run, command_span.id(), "run.completed", Some(output.summary.clone()));
  run.finish_span(
    &command_span,
    SpanFinish {
      status_code: TraceStatusCode::Ok,
      summary: Some(output.summary.clone()),
      failure: None,
    },
  )?;

  Ok(InvokeResult {
    run_id: run.id().to_string(),
    producer_span_id: command_span.id().clone(),
    status: RunStatus::Completed,
    output_summary: output.summary,
    signals: output.signals,
    artifacts: artifact_records,
    artifact_paths,
    failure_message: None,
  })
}

fn command_attributes(command_id: &str, target_application_id: Option<&str>) -> Attributes {
  let mut attributes = Attributes::new();
  attributes.insert("command_id".to_string(), string_attr(command_id));
  attributes.insert("auv.command.id".to_string(), string_attr(command_id));
  if let Some(target_application_id) = target_application_id {
    attributes.insert("target_application_id".to_string(), string_attr(target_application_id));
    attributes.insert("auv.target.application_id".to_string(), string_attr(target_application_id));
  }
  attributes
}

fn record_event(run: &mut RecordingRun, span_id: &SpanId, name: &str, message: Option<String>) {
  run.record_event_in_span(span_id, name, message, Vec::new());
}
```

- [ ] **Step 4: Export wrapper APIs**

In `crates/auv-cli-invoke/src/lib.rs`, add:

```rust
pub mod recorded;
pub use recorded::{
  invoke_recorded, invoke_recorded_in_span, invoke_resolved_recorded_in_span,
};
```

- [ ] **Step 5: Run wrapper tests**

Run:

```bash
cargo test -p auv-cli-invoke recorded -- --nocapture
```

Expected: pass.

- [ ] **Step 6: Commit**

```bash
git add crates/auv-cli-invoke/src/recorded.rs crates/auv-cli-invoke/src/lib.rs
git commit -m "feat(auv-cli-invoke): add traced invoke wrapper"
```

---

### Task 3: Delegate Runtime Invoke Methods to the New Wrapper

**Files:**
- Modify: `src/runtime.rs`
- Test: `cargo test invoke_ -- --nocapture`

- [ ] **Step 1: Replace runtime invoke implementation with delegation**

In `src/runtime.rs`, replace the bodies of `invoke`, `invoke_resolved`, and `invoke_in_span` with:

```rust
pub fn invoke(&self, request: InvokeRequest) -> AuvResult<InvokeResult> {
  let registry = auv_cli_invoke::default_registry();
  auv_cli_invoke::invoke_recorded(&self.recording, &registry, request)
}

pub fn invoke_resolved(
  &self,
  request: InvokeRequest,
  command: &auv_cli_invoke::InvokeCommand,
) -> AuvResult<InvokeResult> {
  let mut run =
    self
      .recording
      .handle()
      .start_run(auv_tracing_driver::run_builder::RunSpec::new(
        RunType::Command,
        "auv.command",
      ))?;
  let root = run.root_span();
  let result = auv_cli_invoke::invoke_resolved_recorded_in_span(
    &self.recording,
    &mut run,
    &root,
    command,
    request,
  )?;
  let status_code = if result.status == RunStatus::Completed {
    TraceStatusCode::Ok
  } else {
    TraceStatusCode::Error
  };
  self.recording.handle().finish_run(
    run,
    auv_tracing_driver::run_builder::RunFinish {
      status_code,
      summary: Some(result.output_summary.clone()),
      failure: result.failure_message.clone(),
    },
  )?;
  Ok(result)
}

pub fn invoke_in_span(
  &self,
  run: &mut auv_tracing_driver::run_builder::RecordingRun,
  parent: &auv_tracing_driver::run_builder::SpanRef,
  request: InvokeRequest,
) -> AuvResult<InvokeResult> {
  let registry = auv_cli_invoke::default_registry();
  auv_cli_invoke::invoke_recorded_in_span(&self.recording, &registry, run, parent, request)
}
```

Remove the private runtime helpers:

```text
invoke_in_command_run
invoke_metadata_command_in_span
command_attributes
record_event
```

- [ ] **Step 2: Remove imports that are no longer needed**

Clean `src/runtime.rs` imports so it no longer imports `string_attr` or span attribute helpers for invoke. Keep `RunType` and `TraceStatusCode` only if still used by remaining runtime methods.

- [ ] **Step 3: Run existing runtime invoke tests**

Run:

```bash
cargo test invoke_ -- --nocapture
```

Expected: existing invoke tests pass through the delegated wrapper. If tests fail because assertions moved to `auv-cli-invoke`, remove only the duplicate runtime tests after Task 4 adds equivalent crate-level coverage.

- [ ] **Step 4: Commit**

```bash
git add src/runtime.rs
git commit -m "refactor(runtime): delegate invoke recording to auv-cli-invoke"
```

---

### Task 4: Migrate CLI and MCP Generic Invoke Off Runtime

**Files:**
- Modify: `src/main.rs`
- Modify: `src/mcp.rs`
- Test: `cargo test mcp -- --nocapture`
- Test: `cargo run --quiet -- invoke fixture.observe`

- [ ] **Step 1: Split `build_runtime_for_inspect` into recording construction**

In `src/main.rs`, replace:

```rust
fn build_runtime_for_inspect(
  project_root: &Path,
  inspect: &InspectClientOptions,
) -> auv_cli::model::AuvResult<auv_cli::runtime::Runtime>
```

with:

```rust
fn build_recording_for_inspect(
  project_root: &Path,
  inspect: &InspectClientOptions,
) -> auv_cli::model::AuvResult<auv_tracing_driver::RunRecordingBackend>
```

Keep the existing body through the `RunRecordingBackend::new(store, recorder)` construction, and return:

```rust
Ok(
  auv_tracing_driver::RunRecordingBackend::new(store, recorder)
    .with_local_snapshot_write_enabled(local_write_enabled)
    .with_temporary_store_cleanup(!local_write_enabled),
)
```

- [ ] **Step 2: Update `CliCommand::Invoke`**

Replace:

```rust
let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
let result = runtime.invoke(request)?;
```

with:

```rust
let recording = build_recording_for_inspect(&project_root, &inspect)?;
let registry = auv_cli_invoke::default_registry();
let result = auv_cli_invoke::invoke_recorded(&recording, &registry, request)?;
```

Keep the existing printing and error handling unchanged.

- [ ] **Step 3: Keep candidate-action runtime construction local**

If `CandidateActionRun` still needs `Runtime`, add a small helper for that path only:

```rust
fn build_runtime_for_inspect(
  project_root: &Path,
  inspect: &InspectClientOptions,
) -> auv_cli::model::AuvResult<auv_cli::runtime::Runtime> {
  let store_root = resolve_store_root(project_root, inspect.store_root.as_ref());
  let recording = build_recording_for_inspect(project_root, inspect)?;
  Ok(
    build_runtime_with_store_root(project_root.to_path_buf(), store_root)?
      .with_recording(recording),
  )
}
```

This keeps the archived candidate-action path compiling while generic invoke stops depending on runtime.

- [ ] **Step 4: Update MCP generic invoke**

In `src/mcp.rs`, change the `invoke` tool body from runtime construction to direct recording construction:

```rust
let store = self.store(req.inspect.store_root.clone())?;
let recording = auv_tracing_driver::RunRecordingBackend::new(
  store,
  Arc::new(auv_tracing_driver::MemoryRunRecorder::new()),
);
let registry = default_registry();
let result = auv_cli_invoke::invoke_recorded(
  &recording,
  &registry,
  InvokeRequest {
    command_id: req.command_id,
    target: ExecutionTarget {
      application_id: req.target.application_id,
      target_label: req.target.target_label,
    },
    inputs: req.inputs,
    dry_run: req.dry_run,
  },
)
.map_err(invalid_params)?;
```

Keep `McpServer::runtime` for `candidate_action_run` until that archived path is migrated separately.

- [ ] **Step 5: Run targeted checks**

Run:

```bash
cargo test mcp -- --nocapture
cargo run --quiet -- invoke fixture.observe
```

Expected:

- MCP tests pass.
- CLI prints `status: completed` and `output: fixture observed`.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/mcp.rs
git commit -m "refactor(auv): route generic invoke through auv-cli-invoke"
```

---

### Task 5: Migrate Existing-Span Invoke Callers

**Files:**
- Modify: `src/app/infra.rs`
- Modify: `src/scroll_scan/mod.rs`
- Modify: `src/main.rs`
- Test: `cargo test scroll_scan -- --nocapture`
- Test: `cargo test app -- --nocapture`

- [ ] **Step 1: Replace `app/infra` invoke-in-span call**

In `src/app/infra.rs`, replace:

```rust
let result = match runtime.invoke_in_span(run, &step_span, request) {
```

with:

```rust
let registry = auv_cli_invoke::default_registry();
let result = match auv_cli_invoke::invoke_recorded_in_span(
  runtime.recording(),
  &registry,
  run,
  &step_span,
  request,
) {
```

This preserves the existing `Runtime` parameter only as a temporary recording holder for app code.

- [ ] **Step 2: Replace scroll-scan invoke-in-span call**

In `src/scroll_scan/mod.rs`, replace:

```rust
let result = runtime.invoke_in_span(run, root, request)?;
```

with:

```rust
let registry = auv_cli_invoke::default_registry();
let result =
  auv_cli_invoke::invoke_recorded_in_span(runtime.recording(), &registry, run, root, request)?;
```

This keeps scroll-scan behavior unchanged while removing the `Runtime::invoke_in_span` dependency.

- [ ] **Step 3: Replace Minecraft resolved invoke in `src/main.rs`**

In the Minecraft live-click path, replace:

```rust
let invoke_result = runtime.invoke_resolved(
  InvokeRequest {
    command_id: "input.clickWindowPoint".to_string(),
    target: auv_cli::model::ExecutionTarget {
      application_id: Some(target_app.to_string()),
      target_label: None,
    },
    inputs,
    dry_run: false,
  },
  auv_cli_invoke::default_registry()
    .resolve("input.clickWindowPoint")
    .ok_or_else(|| "input.clickWindowPoint command is not registered".to_string())?,
)?;
```

with:

```rust
let registry = auv_cli_invoke::default_registry();
let command = registry
  .resolve("input.clickWindowPoint")
  .ok_or_else(|| "input.clickWindowPoint command is not registered".to_string())?;
let parent = context.current_span().clone();
let invoke_result = auv_cli_invoke::invoke_resolved_recorded_in_span(
  context.recording(),
  context.run_mut(),
  &parent,
  command,
  InvokeRequest {
    command_id: "input.clickWindowPoint".to_string(),
    target: auv_cli::model::ExecutionTarget {
      application_id: Some(target_app.to_string()),
      target_label: None,
    },
    inputs,
    dry_run: false,
  },
)?;
```

- [ ] **Step 4: Run targeted checks**

Run:

```bash
cargo test scroll_scan -- --nocapture
cargo test app -- --nocapture
```

Expected: pass. If live platform tests are skipped on non-macOS, note the skip in the implementation summary.

- [ ] **Step 5: Commit**

```bash
git add src/app/infra.rs src/scroll_scan/mod.rs src/main.rs
git commit -m "refactor(auv): migrate invoke-in-span callers"
```

---

### Task 6: Delete Migrated Runtime Invoke Code

**Files:**
- Modify: `src/runtime.rs`
- Test: `cargo test runtime -- --nocapture`
- Test: `cargo test -p auv-cli-invoke`

- [ ] **Step 1: Confirm no external call sites remain**

Run:

```bash
git grep -n -E "\\.invoke\\(|\\.invoke_resolved\\(|\\.invoke_in_span\\(" -- src crates ':!src/runtime.rs'
```

Expected: no calls to `Runtime::invoke`, `Runtime::invoke_resolved`, or `Runtime::invoke_in_span`. Calls to `InvokeCommand::invoke` inside `auv-cli-invoke` are allowed.

- [ ] **Step 2: Remove runtime invoke methods**

Delete these methods from `impl Runtime`:

```text
invoke
invoke_resolved
invoke_in_span
```

Also remove any invoke-only imports left in `src/runtime.rs`, including `InvokeRequest`, `InvokeResult`, `RunStatus`, and `TraceStatusCode` if they are no longer used.

- [ ] **Step 3: Remove runtime invoke tests that moved to auv-cli-invoke**

Delete runtime tests whose names start with:

```text
invoke_in_span_
invoke_resolved_
invoke_unknown_
invoke_succeeds_
invoke_aborts_
invoke_fails_
```

Keep runtime tests for non-invoke facades until those facades are migrated in a later task.

- [ ] **Step 4: Run targeted tests**

Run:

```bash
cargo test -p auv-cli-invoke
cargo test runtime -- --nocapture
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add src/runtime.rs
git commit -m "refactor(runtime): remove invoke wrapper"
```

---

### Task 7: Clean Directly Replaceable Runtime Facades

**Files:**
- Modify: `src/runtime.rs`
- Modify: `src/main.rs`
- Modify: affected tests from `git grep`
- Test: `cargo test`

- [ ] **Step 1: Move CLI inspect off runtime**

In `src/main.rs`, replace:

```rust
let runtime = build_default_runtime(project_root.clone())?;
print!("{}", runtime.inspect(&run_id)?);
```

with:

```rust
let store = auv_cli::build_default_store(project_root.clone())?;
print!("{}", auv_cli::inspect::inspect_run(&store, &run_id)?);
```

- [ ] **Step 2: Migrate runtime read/list test call sites**

Use:

```bash
git grep -n -E "\\.inspect\\(|\\.read_run\\(|list_verifications|list_observation_snapshots|list_.*lineage" -- src crates ':!src/runtime.rs'
```

Replace each migrated test call with the owning API:

```rust
let canonical = runtime.recording().read_run(output.run_id.as_str())?;
let inspect_text = crate::inspect::inspect_run(runtime.recording().store(), output.run_id.as_str())?;
let verifications = crate::run_read::list_verifications(runtime.recording().store(), output.run_id.as_str())?;
```

For cross-crate tests that import `auv_cli::build_runtime_with_store_root`, use:

```rust
let inspect_text = auv_cli::inspect::inspect_run(runtime.recording().store(), run_id)?;
```

- [ ] **Step 3: Delete migrated read/list methods**

From `src/runtime.rs`, delete any methods whose call sites are gone:

```text
inspect
list_verifications
list_observation_snapshots
list_detector_recognition_lineage
list_candidate_promotion_lineage
list_candidate_action_decision_lineage
list_candidate_action_execution_lineage
```

Keep `read_run` only if `src/app/tests.rs` or another intentionally untouched caller still uses it.

- [ ] **Step 4: Delete unused accessors**

Run:

```bash
git grep -n -E "\\.project_root\\(|\\.run_dir\\(|\\.recorder\\(" -- src crates ':!src/runtime.rs'
```

If there are no call sites, delete from `src/runtime.rs`:

```text
project_root
run_dir
recorder
```

- [ ] **Step 5: Delete business artifact facades with no callers**

Run:

```bash
git grep -n -E "record_telemetry_sample_artifact|record_minecraft_projection_artifact|record_candidate_action_decision|record_candidate_action_execution" -- src crates ':!src/runtime.rs'
```

If only runtime tests reference them, delete these runtime methods and their runtime tests:

```text
record_telemetry_sample_artifact
record_minecraft_projection_artifact
record_candidate_action_decision
record_candidate_action_execution
```

If a production caller remains, migrate it to the owning module plus `RecordingHandle::run_recorded_operation` before deleting the facade.

- [ ] **Step 6: Run full checks**

Run:

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add src/runtime.rs src/main.rs src crates
git commit -m "refactor(runtime): remove migrated facades"
```

---

## Self-Review Notes

- Spec coverage: The plan implements the accepted boundary in `2026-06-18-auv-cli-invoke-traced-wrapper-runtime-exit.md`: command handlers keep semantics, `auv-cli-invoke` gets the reusable traced wrapper, frontends call it directly, and migrated runtime invoke code is deleted.
- Type consistency: The plan resolves the open type-boundary decision by moving invoke request/result models into `auv-cli-invoke` and re-exporting from root `src/model.rs`.
- Scope: This plan intentionally does not delete all of `src/runtime.rs`. It removes invoke wrapper code and directly replaceable facades; remaining runtime cleanup can continue after candidate-action and other non-invoke callers are migrated.
- Validation: Each task has targeted tests; final validation runs `cargo fmt --check`, `cargo check`, `cargo test`, and `git diff --check`.
