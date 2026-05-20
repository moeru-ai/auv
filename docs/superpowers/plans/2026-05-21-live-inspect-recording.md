# Live Inspect Recording Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build multi-target run recording with configurable local store roots and opt-in cross-process inspect server writes.

**Architecture:** Add `RunRecordingBackend` as the runtime recording dependency, backed by one local `RunStore` plus one or more `RunRecorder` targets. The inspect server gains a guarded write API that applies accepted updates to its configured store and rebroadcasts them to live viewers. CLI policy stays outside runtime: CLI options choose store roots, local write, server reporting, and write-required behavior.

**Tech Stack:** Rust 2024, `serde`/`serde_json`, `axum` HTTP/WebSocket server, `tokio`, `reqwest` blocking HTTP client for runtime-to-server reporting, existing `LocalStore` JSON/JSONL run format.

---

## File Structure

- Modify `Cargo.toml`: add `reqwest` for blocking JSON HTTP reporting.
- Modify `src/lib.rs`: add `run_recording` module and store-root-aware builders.
- Modify `src/trace.rs`: derive `PartialEq` for record structs so conflict checks can compare accepted records.
- Create `src/run_recording.rs`: define `RunRecordingBackend`, `RunRecorder`, `RunUpdate`, camelCase API DTOs, local/broadcast/composite/server recorder implementations, and inspect session discovery helpers.
- Modify `src/recording.rs`: make `RecordingRun` emit `RunUpdate` through `RunRecorder`; preserve compatibility type aliases where practical.
- Modify `src/runtime.rs`: replace direct `LocalStore` and event sink fields with `RunRecordingBackend`; route artifact staging, run persistence, and inspection through the backend.
- Modify `src/skill.rs`: add stable `auv.step.*` and `auv.recipe.id` attributes for recipe and case spans.
- Modify `src/app.rs`: add stable `auv.step.*` attributes for probe spans and preserve existing probe attributes.
- Modify `src/inspect_server.rs`: add write config, token checks, camelCase write DTO handling, conflict detection, store apply logic, and write-session descriptor output.
- Modify `src/cli.rs`: parse store root, inspect local/server write settings, require flag, server URL/token options, and short `inspect serve` write options.
- Modify `src/main.rs`: resolve CLI recording policy, build `RunRecordingBackend`, pass store root to runtime/server, and preserve existing command behavior by default.
- Modify `docs/TERMS_AND_CONCEPTS.md`: add a short note that inspect server write is opt-in and multi-write does not define one global source of truth.

---

### Task 1: Add Recording Update Types And Backend Boundary

**Files:**
- Modify: `src/trace.rs`
- Create: `src/run_recording.rs`
- Modify: `src/lib.rs`
- Modify: `src/recording.rs`
- Test: `src/run_recording.rs`
- Test: `src/recording.rs`

- [ ] **Step 1: Write tests for camelCase update serialization and composite fanout**

Add this test module to the new file `src/run_recording.rs`:

```rust
#[cfg(test)]
mod tests {
  use std::sync::{Arc, Mutex};

  use crate::trace::{
    RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SpanId, TraceId, TraceState,
    TraceStatusCode,
  };

  use super::{ApiRunUpdate, RunRecorder, RunUpdate};

  #[derive(Default)]
  struct CapturingRecorder {
    updates: Mutex<Vec<RunUpdate>>,
  }

  impl CapturingRecorder {
    fn updates(&self) -> Vec<RunUpdate> {
      self.updates.lock().expect("updates lock").clone()
    }
  }

  impl RunRecorder for CapturingRecorder {
    fn record(&self, update: RunUpdate) -> crate::model::AuvResult<()> {
      self.updates.lock().expect("updates lock").push(update);
      Ok(())
    }
  }

  fn test_run() -> RunRecordV1Alpha1 {
    RunRecordV1Alpha1 {
      api_version: RUN_API_VERSION.to_string(),
      run_id: RunId::new("run_update_test"),
      trace_id: TraceId::new("00000000000000000000000000000001"),
      run_type: RunType::Execute,
      state: TraceState::Running,
      status_code: TraceStatusCode::Unset,
      started_at_millis: 100,
      finished_at_millis: None,
      root_span_id: SpanId::new("0000000000000001"),
      attributes: Default::default(),
      summary: None,
      failure: None,
    }
  }

  #[test]
  fn run_update_serializes_public_shape_as_camel_case() {
    let update = RunUpdate::RunStarted {
      run_id: RunId::new("run_update_test"),
      run: test_run(),
    };

    let value = serde_json::to_value(ApiRunUpdate::from(update)).expect("update should serialize");
    assert_eq!(value["type"], "runStarted");
    assert_eq!(value["runId"], "run_update_test");
    assert_eq!(value["run"]["apiVersion"], "auv.run.v1alpha1");
    assert_eq!(value["run"]["rootSpanId"], "0000000000000001");
    assert!(value["run"].get("root_span_id").is_none());
  }

  #[test]
  fn composite_recorder_fans_out_to_every_target() {
    let first = Arc::new(CapturingRecorder::default());
    let second = Arc::new(CapturingRecorder::default());
    let recorder = super::CompositeRunRecorder::new(vec![first.clone(), second.clone()]);
    let update = RunUpdate::RunStarted {
      run_id: RunId::new("run_update_test"),
      run: test_run(),
    };

    recorder.record(update.clone()).expect("fanout should succeed");

    assert_eq!(first.updates(), vec![update.clone()]);
    assert_eq!(second.updates(), vec![update]);
  }
}
```

- [ ] **Step 2: Run the new tests and verify they fail**

Run:

```bash
cargo test run_recording --lib
```

Expected: FAIL because `src/run_recording.rs`, `RunUpdate`, `RunRecorder`, and `CompositeRunRecorder` do not exist.

- [ ] **Step 3: Create `src/run_recording.rs` with update types and recorders**

First update `src/trace.rs` record derives from:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
```

to:

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
```

for these structs:

- `TraceFailure`
- `RunRecordV1Alpha1`
- `SpanRecordV1Alpha1`
- `EventRecordV1Alpha1`
- `ArtifactRecordV1Alpha1`

Create `src/run_recording.rs` with this content:

```rust
use std::sync::Arc;

use crate::model::AuvResult;
use crate::store::{ArtifactFileSource, CanonicalRun, LocalStore};
use crate::trace::{
  ArtifactId, ArtifactRecordV1Alpha1, EventRecordV1Alpha1, RunId, RunRecordV1Alpha1, SpanId,
  SpanRecordV1Alpha1,
};

#[derive(Clone, Debug, PartialEq)]
pub enum RunUpdate {
  RunStarted {
    #[serde(rename = "runId")]
    run_id: RunId,
    run: RunRecordV1Alpha1,
  },
  SpanStarted {
    #[serde(rename = "runId")]
    run_id: RunId,
    span: SpanRecordV1Alpha1,
  },
  EventAppended {
    #[serde(rename = "runId")]
    run_id: RunId,
    event: EventRecordV1Alpha1,
  },
  ArtifactCreated {
    #[serde(rename = "runId")]
    run_id: RunId,
    artifact: ArtifactRecordV1Alpha1,
  },
  SpanFinished {
    #[serde(rename = "runId")]
    run_id: RunId,
    span: SpanRecordV1Alpha1,
  },
  RunFinished {
    #[serde(rename = "runId")]
    run_id: RunId,
    run: RunRecordV1Alpha1,
  },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiRunRecord {
  pub api_version: String,
  pub run_id: RunId,
  pub trace_id: crate::trace::TraceId,
  pub run_type: crate::trace::RunType,
  pub state: crate::trace::TraceState,
  pub status_code: crate::trace::TraceStatusCode,
  pub started_at_millis: u128,
  pub finished_at_millis: Option<u128>,
  pub root_span_id: SpanId,
  pub attributes: crate::recording::Attributes,
  pub summary: Option<String>,
  pub failure: Option<crate::trace::TraceFailure>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiSpanRecord {
  pub api_version: String,
  pub span_id: SpanId,
  pub parent_span_id: Option<SpanId>,
  pub name: String,
  pub state: crate::trace::TraceState,
  pub status_code: crate::trace::TraceStatusCode,
  pub started_at_millis: u128,
  pub finished_at_millis: Option<u128>,
  pub attributes: crate::recording::Attributes,
  pub summary: Option<String>,
  pub failure: Option<crate::trace::TraceFailure>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiEventRecord {
  pub api_version: String,
  pub event_id: crate::trace::EventId,
  pub span_id: SpanId,
  pub name: String,
  pub timestamp_millis: u128,
  pub attributes: crate::recording::Attributes,
  pub message: Option<String>,
  pub artifact_ids: Vec<ArtifactId>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiArtifactRecord {
  pub api_version: String,
  pub artifact_id: ArtifactId,
  pub span_id: SpanId,
  pub event_id: Option<crate::trace::EventId>,
  pub role: String,
  pub mime_type: String,
  pub path: String,
  pub sha256: Option<String>,
  pub attributes: crate::recording::Attributes,
  pub summary: Option<String>,
}

impl From<RunRecordV1Alpha1> for ApiRunRecord {
  fn from(record: RunRecordV1Alpha1) -> Self {
    Self {
      api_version: record.api_version,
      run_id: record.run_id,
      trace_id: record.trace_id,
      run_type: record.run_type,
      state: record.state,
      status_code: record.status_code,
      started_at_millis: record.started_at_millis,
      finished_at_millis: record.finished_at_millis,
      root_span_id: record.root_span_id,
      attributes: record.attributes,
      summary: record.summary,
      failure: record.failure,
    }
  }
}

impl From<ApiRunRecord> for RunRecordV1Alpha1 {
  fn from(record: ApiRunRecord) -> Self {
    Self {
      api_version: record.api_version,
      run_id: record.run_id,
      trace_id: record.trace_id,
      run_type: record.run_type,
      state: record.state,
      status_code: record.status_code,
      started_at_millis: record.started_at_millis,
      finished_at_millis: record.finished_at_millis,
      root_span_id: record.root_span_id,
      attributes: record.attributes,
      summary: record.summary,
      failure: record.failure,
    }
  }
}

impl From<SpanRecordV1Alpha1> for ApiSpanRecord {
  fn from(record: SpanRecordV1Alpha1) -> Self {
    Self {
      api_version: record.api_version,
      span_id: record.span_id,
      parent_span_id: record.parent_span_id,
      name: record.name,
      state: record.state,
      status_code: record.status_code,
      started_at_millis: record.started_at_millis,
      finished_at_millis: record.finished_at_millis,
      attributes: record.attributes,
      summary: record.summary,
      failure: record.failure,
    }
  }
}

impl From<ApiSpanRecord> for SpanRecordV1Alpha1 {
  fn from(record: ApiSpanRecord) -> Self {
    Self {
      api_version: record.api_version,
      span_id: record.span_id,
      parent_span_id: record.parent_span_id,
      name: record.name,
      state: record.state,
      status_code: record.status_code,
      started_at_millis: record.started_at_millis,
      finished_at_millis: record.finished_at_millis,
      attributes: record.attributes,
      summary: record.summary,
      failure: record.failure,
    }
  }
}

impl From<EventRecordV1Alpha1> for ApiEventRecord {
  fn from(record: EventRecordV1Alpha1) -> Self {
    Self {
      api_version: record.api_version,
      event_id: record.event_id,
      span_id: record.span_id,
      name: record.name,
      timestamp_millis: record.timestamp_millis,
      attributes: record.attributes,
      message: record.message,
      artifact_ids: record.artifact_ids,
    }
  }
}

impl From<ApiEventRecord> for EventRecordV1Alpha1 {
  fn from(record: ApiEventRecord) -> Self {
    Self {
      api_version: record.api_version,
      event_id: record.event_id,
      span_id: record.span_id,
      name: record.name,
      timestamp_millis: record.timestamp_millis,
      attributes: record.attributes,
      message: record.message,
      artifact_ids: record.artifact_ids,
    }
  }
}

impl From<ArtifactRecordV1Alpha1> for ApiArtifactRecord {
  fn from(record: ArtifactRecordV1Alpha1) -> Self {
    Self {
      api_version: record.api_version,
      artifact_id: record.artifact_id,
      span_id: record.span_id,
      event_id: record.event_id,
      role: record.role,
      mime_type: record.mime_type,
      path: record.path,
      sha256: record.sha256,
      attributes: record.attributes,
      summary: record.summary,
    }
  }
}

impl From<ApiArtifactRecord> for ArtifactRecordV1Alpha1 {
  fn from(record: ApiArtifactRecord) -> Self {
    Self {
      api_version: record.api_version,
      artifact_id: record.artifact_id,
      span_id: record.span_id,
      event_id: record.event_id,
      role: record.role,
      mime_type: record.mime_type,
      path: record.path,
      sha256: record.sha256,
      attributes: record.attributes,
      summary: record.summary,
    }
  }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ApiRunUpdate {
  RunStarted {
    #[serde(rename = "runId")]
    run_id: RunId,
    run: ApiRunRecord,
  },
  SpanStarted {
    #[serde(rename = "runId")]
    run_id: RunId,
    span: ApiSpanRecord,
  },
  EventAppended {
    #[serde(rename = "runId")]
    run_id: RunId,
    event: ApiEventRecord,
  },
  ArtifactCreated {
    #[serde(rename = "runId")]
    run_id: RunId,
    artifact: ApiArtifactRecord,
  },
  SpanFinished {
    #[serde(rename = "runId")]
    run_id: RunId,
    span: ApiSpanRecord,
  },
  RunFinished {
    #[serde(rename = "runId")]
    run_id: RunId,
    run: ApiRunRecord,
  },
}

impl From<RunUpdate> for ApiRunUpdate {
  fn from(update: RunUpdate) -> Self {
    match update {
      RunUpdate::RunStarted { run_id, run } => Self::RunStarted {
        run_id,
        run: run.into(),
      },
      RunUpdate::SpanStarted { run_id, span } => Self::SpanStarted {
        run_id,
        span: span.into(),
      },
      RunUpdate::EventAppended { run_id, event } => Self::EventAppended {
        run_id,
        event: event.into(),
      },
      RunUpdate::ArtifactCreated { run_id, artifact } => Self::ArtifactCreated {
        run_id,
        artifact: artifact.into(),
      },
      RunUpdate::SpanFinished { run_id, span } => Self::SpanFinished {
        run_id,
        span: span.into(),
      },
      RunUpdate::RunFinished { run_id, run } => Self::RunFinished {
        run_id,
        run: run.into(),
      },
    }
  }
}

impl From<ApiRunUpdate> for RunUpdate {
  fn from(update: ApiRunUpdate) -> Self {
    match update {
      ApiRunUpdate::RunStarted { run_id, run } => Self::RunStarted {
        run_id,
        run: run.into(),
      },
      ApiRunUpdate::SpanStarted { run_id, span } => Self::SpanStarted {
        run_id,
        span: span.into(),
      },
      ApiRunUpdate::EventAppended { run_id, event } => Self::EventAppended {
        run_id,
        event: event.into(),
      },
      ApiRunUpdate::ArtifactCreated { run_id, artifact } => Self::ArtifactCreated {
        run_id,
        artifact: artifact.into(),
      },
      ApiRunUpdate::SpanFinished { run_id, span } => Self::SpanFinished {
        run_id,
        span: span.into(),
      },
      ApiRunUpdate::RunFinished { run_id, run } => Self::RunFinished {
        run_id,
        run: run.into(),
      },
    }
  }
}

impl RunUpdate {
  pub fn run_id(&self) -> &RunId {
    match self {
      Self::RunStarted { run_id, .. }
      | Self::SpanStarted { run_id, .. }
      | Self::EventAppended { run_id, .. }
      | Self::ArtifactCreated { run_id, .. }
      | Self::SpanFinished { run_id, .. }
      | Self::RunFinished { run_id, .. } => run_id,
    }
  }
}

pub trait RunRecorder: Send + Sync {
  fn record(&self, update: RunUpdate) -> AuvResult<()>;
}

pub struct NoopRunRecorder;

impl RunRecorder for NoopRunRecorder {
  fn record(&self, _update: RunUpdate) -> AuvResult<()> {
    Ok(())
  }
}

pub struct CompositeRunRecorder {
  recorders: Vec<Arc<dyn RunRecorder>>,
}

impl CompositeRunRecorder {
  pub fn new(recorders: Vec<Arc<dyn RunRecorder>>) -> Self {
    Self { recorders }
  }
}

impl RunRecorder for CompositeRunRecorder {
  fn record(&self, update: RunUpdate) -> AuvResult<()> {
    let mut failures = Vec::new();
    for recorder in &self.recorders {
      if let Err(error) = recorder.record(update.clone()) {
        failures.push(error);
      }
    }
    if failures.is_empty() {
      Ok(())
    } else {
      Err(format!("{} recorder target(s) failed: {}", failures.len(), failures.join("; ")))
    }
  }
}

#[derive(Clone)]
pub struct RunRecordingBackend {
  store: LocalStore,
  recorder: Arc<dyn RunRecorder>,
}

impl RunRecordingBackend {
  pub fn new(store: LocalStore, recorder: Arc<dyn RunRecorder>) -> Self {
    Self { store, recorder }
  }

  pub fn local_only(store: LocalStore) -> Self {
    Self {
      store,
      recorder: Arc::new(NoopRunRecorder),
    }
  }

  pub fn store(&self) -> &LocalStore {
    &self.store
  }

  pub fn recorder(&self) -> Arc<dyn RunRecorder> {
    self.recorder.clone()
  }

  pub fn record(&self, update: RunUpdate) -> AuvResult<()> {
    self.recorder.record(update)
  }

  pub fn read_run(&self, run_id: &str) -> AuvResult<CanonicalRun> {
    self.store.read_run(run_id)
  }

  pub fn write_run_snapshot(&self, snapshot: &CanonicalRun) -> AuvResult<()> {
    self.store.write_run_snapshot(snapshot)
  }

  pub fn run_dir(&self, run_id: impl AsRef<str>) -> AuvResult<std::path::PathBuf> {
    self.store.run_dir(run_id)
  }

  pub fn stage_artifact(
    &self,
    run_id: &RunId,
    index: usize,
    artifact: crate::model::ProducedArtifact,
    span_id: &SpanId,
    event_id: Option<crate::trace::EventId>,
  ) -> AuvResult<ArtifactRecordV1Alpha1> {
    self.store.stage_artifact(run_id, index, artifact, span_id, event_id)
  }

  pub fn stage_artifact_file(
    &self,
    run_id: &RunId,
    index: usize,
    span_id: &SpanId,
    event_id: Option<crate::trace::EventId>,
    artifact: ArtifactFileSource,
  ) -> AuvResult<ArtifactRecordV1Alpha1> {
    self.store
      .stage_artifact_file(run_id, index, span_id, event_id, artifact)
  }
}
```

- [ ] **Step 4: Register the module in `src/lib.rs`**

Modify `src/lib.rs` to include:

```rust
pub mod run_recording;
```

Add it near the other module declarations.

- [ ] **Step 5: Update `src/recording.rs` to use `RunRecorder` and `RunUpdate`**

Replace the import area and old sink trait definitions so `RecordingRun` stores a recorder:

```rust
use crate::run_recording::{RunRecorder, RunUpdate};
```

Change `RecordingRun`:

```rust
pub struct RecordingRun {
  run: RunRecordV1Alpha1,
  spans: Vec<SpanRecordV1Alpha1>,
  events: Vec<EventRecordV1Alpha1>,
  artifacts: Vec<ArtifactRecordV1Alpha1>,
  recorder: Arc<dyn RunRecorder>,
}
```

Change `RecordingRun::new` signature and body:

```rust
pub fn new(
  run: RunRecordV1Alpha1,
  root_span: SpanRecordV1Alpha1,
  recorder: Arc<dyn RunRecorder>,
) -> Self {
  let _ = recorder.record(RunUpdate::RunStarted {
    run_id: run.run_id.clone(),
    run: run.clone(),
  });
  let _ = recorder.record(RunUpdate::SpanStarted {
    run_id: run.run_id.clone(),
    span: root_span.clone(),
  });
  Self {
    run,
    spans: vec![root_span],
    events: Vec::new(),
    artifacts: Vec::new(),
    recorder,
  }
}
```

Change each event emission:

```rust
let _ = self.recorder.record(RunUpdate::SpanStarted {
  run_id: self.run.run_id.clone(),
  span: span.clone(),
});
```

```rust
let _ = self.recorder.record(RunUpdate::SpanFinished {
  run_id: self.run.run_id.clone(),
  span: record.clone(),
});
```

```rust
let _ = self.recorder.record(RunUpdate::EventAppended {
  run_id: self.run.run_id.clone(),
  event: event.clone(),
});
```

```rust
let _ = self.recorder.record(RunUpdate::ArtifactCreated {
  run_id: self.run.run_id.clone(),
  artifact: artifact.clone(),
});
```

Delete the old `RunEventSink`, `MemoryRunEventSink`, and `BroadcastRunEventSink` definitions from `src/recording.rs` after their replacements are added in Task 2.

- [ ] **Step 6: Run tests and note remaining compile errors**

Run:

```bash
cargo test run_recording --lib
```

Expected: FAIL with compile errors in `runtime.rs`, `inspect_server.rs`, and tests that still reference `RunEventSink` or `BroadcastRunEventSink`. Those are resolved in Task 2.

- [ ] **Step 7: Commit Task 1**

After Task 2 also compiles, commit Tasks 1 and 2 together because Task 1 intentionally leaves transitional compile errors.

---

### Task 2: Replace Event Sinks With Recorders Without Changing Behavior

**Files:**
- Modify: `src/run_recording.rs`
- Modify: `src/runtime.rs`
- Modify: `src/inspect_server.rs`
- Modify: `src/recording.rs`
- Test: `src/runtime.rs`
- Test: `src/inspect_server.rs`

- [ ] **Step 1: Add memory and broadcast recorder compatibility implementations**

Append this to `src/run_recording.rs`:

```rust
use std::sync::Mutex;

use tokio::sync::broadcast;

#[derive(Clone)]
pub struct MemoryRunRecorder {
  updates: Arc<Mutex<Vec<RunUpdate>>>,
}

impl MemoryRunRecorder {
  pub fn new() -> Self {
    Self {
      updates: Arc::new(Mutex::new(Vec::new())),
    }
  }

  pub fn drain_for_test(&self) -> Vec<RunUpdate> {
    self
      .updates
      .lock()
      .map(|updates| updates.clone())
      .unwrap_or_default()
  }
}

impl Default for MemoryRunRecorder {
  fn default() -> Self {
    Self::new()
  }
}

impl RunRecorder for MemoryRunRecorder {
  fn record(&self, update: RunUpdate) -> AuvResult<()> {
    if let Ok(mut updates) = self.updates.lock() {
      updates.push(update);
    }
    Ok(())
  }
}

#[derive(Clone)]
pub struct BroadcastRunRecorder {
  sender: broadcast::Sender<RunUpdate>,
}

impl BroadcastRunRecorder {
  pub fn new(capacity: usize) -> Self {
    let (sender, _) = broadcast::channel(capacity);
    Self { sender }
  }

  pub fn subscribe(&self) -> broadcast::Receiver<RunUpdate> {
    self.sender.subscribe()
  }
}

impl RunRecorder for BroadcastRunRecorder {
  fn record(&self, update: RunUpdate) -> AuvResult<()> {
    let _ = self.sender.send(update);
    Ok(())
  }
}
```

- [ ] **Step 2: Update runtime imports and fields**

In `src/runtime.rs`, replace:

```rust
use crate::recording::{MemoryRunEventSink, RunEventSink};
use crate::store::{ArtifactFileSource, LocalStore};
```

with:

```rust
use crate::run_recording::{MemoryRunRecorder, RunRecorder, RunRecordingBackend, RunUpdate};
use crate::store::{ArtifactFileSource, LocalStore};
```

Change `Runtime` fields:

```rust
pub struct Runtime {
  project_root: PathBuf,
  commands: CommandCatalog,
  drivers: DriverRegistry,
  recording: RunRecordingBackend,
}
```

Change `Runtime::new`:

```rust
pub fn new(
  project_root: PathBuf,
  commands: CommandCatalog,
  drivers: DriverRegistry,
  store: LocalStore,
) -> Self {
  Self {
    project_root,
    commands,
    drivers,
    recording: RunRecordingBackend::new(store, Arc::new(MemoryRunRecorder::new())),
  }
}
```

Add:

```rust
pub fn with_recording(mut self, recording: RunRecordingBackend) -> Self {
  self.recording = recording;
  self
}

pub fn with_recorder(mut self, recorder: Arc<dyn RunRecorder>) -> Self {
  let store = self.recording.store().clone();
  self.recording = RunRecordingBackend::new(store, recorder);
  self
}
```

Keep `with_event_sink` temporarily as a compatibility wrapper if existing tests still call it, but rename call sites to `with_recorder` in this task.

- [ ] **Step 3: Route runtime store calls through `RunRecordingBackend`**

In `src/runtime.rs`, change these methods:

```rust
pub fn inspect(&self, run_id: &str) -> AuvResult<String> {
  let canonical = self.recording.read_run(run_id)?;
  Ok(crate::inspect::render_text(&canonical))
}

pub fn read_run(&self, run_id: &str) -> AuvResult<crate::store::CanonicalRun> {
  self.recording.read_run(run_id)
}
```

In `start_run`, pass `self.recording.recorder()` to `RecordingRun::new`.

In `finish_run`, replace `self.store.write_run_snapshot` and sink emission with:

```rust
self.recording.write_run_snapshot(&recorded.snapshot)?;
self.recording.record(RunUpdate::RunFinished {
  run_id: run_id.clone(),
  run: recorded.snapshot.run,
})?;
```

In `invoke_in_span` and `stage_artifact_file`, replace all `self.store` calls with `self.recording`.

- [ ] **Step 4: Update inspect server to use `BroadcastRunRecorder` and `RunUpdate`**

In `src/inspect_server.rs`, replace imports:

```rust
use crate::recording::BroadcastRunEventSink;
```

with:

```rust
use crate::run_recording::{BroadcastRunRecorder, RunUpdate};
```

Change state/config/router/serve signatures to use `Arc<BroadcastRunRecorder>`.

Change `next_stream_payload` receiver type:

```rust
async fn next_stream_payload(
  receiver: &mut broadcast::Receiver<RunUpdate>,
  run_id: &str,
) -> Option<String> {
  loop {
    match receiver.recv().await {
      Ok(update) if update.run_id().as_str() == run_id => match serde_json::to_string(&crate::run_recording::ApiRunUpdate::from(update)) {
        Ok(payload) => return Some(payload),
        Err(_) => continue,
      },
      Ok(_) => {}
      Err(broadcast::error::RecvError::Lagged(_)) => {}
      Err(broadcast::error::RecvError::Closed) => return None,
    }
  }
}
```

- [ ] **Step 5: Update tests to assert new update names**

In `src/inspect_server.rs` tests, replace `BroadcastRunEventSink` with `BroadcastRunRecorder`.

Replace `sink.on_event(RunStreamEvent::EventAppended { ... })` with:

```rust
sink.record(RunUpdate::EventAppended {
  run_id: run_b.clone(),
  event: test_event("event_stream_b"),
}).expect("record should publish");
```

Import `RunRecorder` so `.record(...)` is available.

In `src/runtime.rs` tests, replace `MemoryRunEventSink` with `MemoryRunRecorder` and `RunStreamEvent::RunFinished` matches with `RunUpdate::RunFinished`.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test --lib
```

Expected: PASS.

- [ ] **Step 7: Commit recording backend boundary**

Run:

```bash
git add src/lib.rs src/run_recording.rs src/recording.rs src/runtime.rs src/inspect_server.rs
git commit -m "refactor(runtime): introduce run recording backend"
```

---

### Task 3: Add Stable Viewer Attributes

**Files:**
- Modify: `src/runtime.rs`
- Modify: `src/skill.rs`
- Modify: `src/app.rs`
- Test: `src/skill.rs`
- Test: `src/app.rs`

- [ ] **Step 1: Write regression assertions for recipe step attributes**

In the existing `src/skill.rs` test `run_skill_manifest_records_one_execute_run`, after the assertion that counts `auv.recipe.step` spans, add:

```rust
let step_spans = canonical
  .spans
  .iter()
  .filter(|span| span.name == "auv.recipe.step")
  .collect::<Vec<_>>();
assert_eq!(step_spans[0].attributes["auv.step.id"], "first");
assert_eq!(step_spans[0].attributes["auv.step.index"], serde_json::json!(0));
assert_eq!(step_spans[0].attributes["auv.step.kind"], "recipe");
assert_eq!(step_spans[0].attributes["auv.recipe.id"], manifest.recipe_id);
```

- [ ] **Step 2: Write regression assertions for command attributes**

In `src/runtime.rs` test `invoke_in_span_adds_command_under_parent_span`, add:

```rust
let command_span = canonical
  .spans
  .iter()
  .find(|span| span.name == "auv.command.invoke")
  .expect("command span should exist");
assert_eq!(command_span.attributes["auv.command.id"], "test.invoke");
assert_eq!(command_span.attributes["auv.driver.id"], "test.driver");
assert_eq!(command_span.attributes["auv.driver.operation"], "test_operation");
```

- [ ] **Step 3: Run focused tests and verify they fail**

Run:

```bash
cargo test run_skill_manifest_records_one_execute_run --lib
cargo test invoke_in_span_adds_command_under_parent_span --lib
```

Expected: FAIL because the new stable attributes are missing.

- [ ] **Step 4: Add stable command attributes in `src/runtime.rs`**

Modify `command_attributes`:

```rust
fn command_attributes(
  command_id: &str,
  driver_id: &str,
  operation: &str,
  target_application_id: Option<&str>,
) -> crate::recording::Attributes {
  let mut attributes = crate::recording::Attributes::new();
  attributes.insert("command_id".to_string(), string_attr(command_id));
  attributes.insert("driver_id".to_string(), string_attr(driver_id));
  attributes.insert("operation".to_string(), string_attr(operation));
  attributes.insert("auv.command.id".to_string(), string_attr(command_id));
  attributes.insert("auv.driver.id".to_string(), string_attr(driver_id));
  attributes.insert("auv.driver.operation".to_string(), string_attr(operation));
  if let Some(target_application_id) = target_application_id {
    attributes.insert(
      "target_application_id".to_string(),
      string_attr(target_application_id),
    );
    attributes.insert(
      "auv.target.application_id".to_string(),
      string_attr(target_application_id),
    );
  }
  attributes
}
```

- [ ] **Step 5: Add stable recipe and case attributes in `src/skill.rs`**

In `run_skill_manifest_recorded`, add both keys:

```rust
attributes.insert(
  "auv.recipe.id".to_string(),
  string_attr(manifest.recipe_id.clone()),
);
attributes.insert(
  "recipe_id".to_string(),
  string_attr(manifest.recipe_id.clone()),
);
```

In `run_skill_manifest_into_run`, replace the step span attribute construction with:

```rust
let step_span = run.start_span(
  parent,
  span_record(
    "auv.recipe.step",
    BTreeMap::from([
      ("auv.recipe.step_id".to_string(), string_attr(&step_id)),
      ("auv.step.id".to_string(), string_attr(&step_id)),
      ("auv.step.index".to_string(), serde_json::json!(index)),
      ("auv.step.kind".to_string(), string_attr("recipe")),
      (
        "auv.recipe.id".to_string(),
        string_attr(manifest.recipe_id.clone()),
      ),
    ]),
  ),
)?;
```

In case matrix spans, preserve `auv.case.id` and add no duplicate unstable key unless needed.

- [ ] **Step 6: Add stable probe step attributes in `src/app.rs`**

In `invoke_probe_step`, replace the probe step span attributes with:

```rust
BTreeMap::from([
  ("auv.probe.step_id".to_string(), string_attr(step_id)),
  ("auv.step.id".to_string(), string_attr(step_id)),
  ("auv.step.kind".to_string(), string_attr("probe")),
])
```

Do not add `auv.step.index` for probe steps in this task because `invoke_probe_step` does not receive a stable index argument.

- [ ] **Step 7: Run focused tests**

Run:

```bash
cargo test --lib
```

Expected: PASS.

- [ ] **Step 8: Commit stable attributes**

Run:

```bash
git add src/runtime.rs src/skill.rs src/app.rs
git commit -m "feat(recording): add stable viewer attributes"
```

---

### Task 4: Add Store Root And Inspect Write CLI Settings

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`
- Test: `src/cli.rs`

- [ ] **Step 1: Add CLI parse tests**

In `src/cli.rs` tests, add:

```rust
#[test]
fn parse_inspect_serve_write_options() {
  let command = parse_cli(&[
    "inspect".to_string(),
    "serve".to_string(),
    "--store-root".to_string(),
    "/tmp/auv-store".to_string(),
    "--enable-write".to_string(),
    "--write-token".to_string(),
    "secret".to_string(),
  ])
  .expect("inspect serve options should parse");

  match command {
    CliCommand::InspectServe {
      host,
      port,
      store_root,
      write,
    } => {
      assert_eq!(host, auv_cli::inspect_server::DEFAULT_INSPECT_HOST);
      assert_eq!(port, auv_cli::inspect_server::DEFAULT_INSPECT_PORT);
      assert_eq!(store_root.as_deref(), Some("/tmp/auv-store"));
      assert!(write.enabled);
      assert_eq!(write.token.as_deref(), Some("secret"));
      assert!(!write.no_token);
    }
    other => panic!("unexpected command: {other:?}"),
  }
}

#[test]
fn parse_skill_run_inspect_write_options() {
  let command = parse_cli(&[
    "skill".to_string(),
    "run".to_string(),
    "recipe.id".to_string(),
    "--store-root".to_string(),
    "/tmp/auv-store".to_string(),
    "--inspect-local-write".to_string(),
    "false".to_string(),
    "--inspect-server-write".to_string(),
    "true".to_string(),
    "--require-inspect-server-write".to_string(),
    "--inspect-server-url".to_string(),
    "http://127.0.0.1:8765".to_string(),
    "--inspect-server-token".to_string(),
    "secret".to_string(),
  ])
  .expect("skill run inspect options should parse");

  match command {
    CliCommand::SkillRun { inspect, .. } => {
      assert_eq!(inspect.store_root.as_deref(), Some("/tmp/auv-store"));
      assert_eq!(inspect.local_write, InspectWriteSetting::Disabled);
      assert_eq!(inspect.server_write, InspectWriteSetting::Enabled);
      assert!(inspect.require_server_write);
      assert_eq!(inspect.server_url.as_deref(), Some("http://127.0.0.1:8765"));
      assert_eq!(inspect.server_token.as_deref(), Some("secret"));
    }
    other => panic!("unexpected command: {other:?}"),
  }
}
```

- [ ] **Step 2: Run CLI tests and verify they fail**

Run:

```bash
cargo test cli::tests::parse_inspect_serve_write_options cli::tests::parse_skill_run_inspect_write_options
```

Expected: FAIL because `InspectWriteSetting`, `InspectClientOptions`, and new enum fields do not exist.

- [ ] **Step 3: Add CLI option types**

In `src/cli.rs`, add:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectWriteSetting {
  Default,
  Enabled,
  Disabled,
}

impl InspectWriteSetting {
  fn parse(raw: &str) -> AuvResult<Self> {
    match raw {
      "default" => Ok(Self::Default),
      "true" => Ok(Self::Enabled),
      "false" => Ok(Self::Disabled),
      other => Err(format!(
        "invalid inspect write setting {other:?}; expected true, false, or default"
      )),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectClientOptions {
  pub store_root: Option<String>,
  pub local_write: InspectWriteSetting,
  pub server_write: InspectWriteSetting,
  pub require_server_write: bool,
  pub server_url: Option<String>,
  pub server_token: Option<String>,
  pub server_token_file: Option<String>,
}

impl Default for InspectClientOptions {
  fn default() -> Self {
    Self {
      store_root: None,
      local_write: InspectWriteSetting::Default,
      server_write: InspectWriteSetting::Default,
      require_server_write: false,
      server_url: None,
      server_token: None,
      server_token_file: None,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectServeWriteOptions {
  pub enabled: bool,
  pub token: Option<String>,
  pub token_file: Option<String>,
  pub no_token: bool,
}

impl Default for InspectServeWriteOptions {
  fn default() -> Self {
    Self {
      enabled: false,
      token: None,
      token_file: None,
      no_token: false,
    }
  }
}
```

- [ ] **Step 4: Add options to command variants**

Modify `CliCommand` variants:

```rust
Invoke {
  request: InvokeRequest,
  inspect: InspectClientOptions,
},
InspectServe {
  host: String,
  port: u16,
  store_root: Option<String>,
  write: InspectServeWriteOptions,
},
SkillCasesRun {
  query: String,
  dry_run: bool,
  max_disturbance: Option<DisturbanceClass>,
  only_case_ids: Vec<String>,
  include_nonvalidated: bool,
  inspect: InspectClientOptions,
},
SkillRun {
  query: String,
  dry_run: bool,
  max_disturbance: Option<DisturbanceClass>,
  overrides: BTreeMap<String, String>,
  inspect: InspectClientOptions,
},
```

Update match sites in `src/main.rs` after parsing changes.

- [ ] **Step 5: Add inspect option parser helpers**

Add to `src/cli.rs`:

```rust
fn parse_inspect_client_option(
  argument: &str,
  value: Option<&String>,
  inspect: &mut InspectClientOptions,
) -> AuvResult<Option<usize>> {
  match argument {
    "--store-root" => {
      let value = value.ok_or_else(|| "--store-root requires a value".to_string())?;
      inspect.store_root = Some(value.clone());
      Ok(Some(2))
    }
    "--inspect-local-write" => {
      let value = value.ok_or_else(|| "--inspect-local-write requires a value".to_string())?;
      inspect.local_write = InspectWriteSetting::parse(value)?;
      Ok(Some(2))
    }
    "--inspect-server-write" => {
      let value = value.ok_or_else(|| "--inspect-server-write requires a value".to_string())?;
      inspect.server_write = InspectWriteSetting::parse(value)?;
      Ok(Some(2))
    }
    "--require-inspect-server-write" => {
      inspect.require_server_write = true;
      Ok(Some(1))
    }
    "--inspect-server-url" => {
      let value = value.ok_or_else(|| "--inspect-server-url requires a value".to_string())?;
      inspect.server_url = Some(value.clone());
      Ok(Some(2))
    }
    "--inspect-server-token" => {
      let value = value.ok_or_else(|| "--inspect-server-token requires a value".to_string())?;
      inspect.server_token = Some(value.clone());
      Ok(Some(2))
    }
    "--inspect-server-token-file" => {
      let value =
        value.ok_or_else(|| "--inspect-server-token-file requires a value".to_string())?;
      inspect.server_token_file = Some(value.clone());
      Ok(Some(2))
    }
    _ => Ok(None),
  }
}
```

- [ ] **Step 6: Parse `inspect serve` write options**

Update `parse_inspect_serve` to parse:

```rust
let mut store_root = None;
let mut write = InspectServeWriteOptions::default();
```

Add match arms:

```rust
"--store-root" => {
  if index + 1 >= arguments.len() {
    return Err("--store-root requires a value".to_string());
  }
  store_root = Some(arguments[index + 1].clone());
  index += 2;
}
"--enable-write" => {
  write.enabled = true;
  index += 1;
}
"--write-token" => {
  if index + 1 >= arguments.len() {
    return Err("--write-token requires a value".to_string());
  }
  write.enabled = true;
  write.token = Some(arguments[index + 1].clone());
  index += 2;
}
"--write-token-file" => {
  if index + 1 >= arguments.len() {
    return Err("--write-token-file requires a value".to_string());
  }
  write.enabled = true;
  write.token_file = Some(arguments[index + 1].clone());
  index += 2;
}
"--no-write-token" => {
  write.no_token = true;
  index += 1;
}
```

Return `CliCommand::InspectServe { host, port, store_root, write }`.

- [ ] **Step 7: Parse run-side inspect options in `invoke`, `skill run`, and `skill cases run`**

In each loop, create `let mut inspect = InspectClientOptions::default();` and before command-specific handling call:

```rust
if let Some(consumed) = parse_inspect_client_option(
  arguments[index].as_str(),
  arguments.get(index + 1),
  &mut inspect,
)? {
  index += consumed;
  continue;
}
```

Return the updated command variant with `inspect`.

- [ ] **Step 8: Add store-root-aware builders**

In `src/lib.rs`, add:

```rust
pub fn build_runtime_with_store_root(
  project_root: PathBuf,
  store_root: PathBuf,
) -> AuvResult<Runtime> {
  let store = LocalStore::new(store_root)?;
  let commands = default_command_catalog();
  let drivers = default_driver_registry();
  Ok(Runtime::new(project_root, commands, drivers, store))
}

pub fn default_project_store_root(project_root: PathBuf) -> PathBuf {
  project_root.join(".auv")
}
```

Change `build_default_store`:

```rust
pub fn build_default_store(project_root: PathBuf) -> AuvResult<LocalStore> {
  LocalStore::new(default_project_store_root(project_root))
}
```

- [ ] **Step 9: Update `src/main.rs` to use store root options without server reporting yet**

Add helper:

```rust
fn resolve_store_root(project_root: &PathBuf, explicit: Option<&String>) -> PathBuf {
  explicit
    .map(PathBuf::from)
    .unwrap_or_else(|| auv_cli::default_project_store_root(project_root.clone()))
}
```

For `InspectServe`, use:

```rust
let store_root = resolve_store_root(&project_root, store_root.as_ref());
let store = auv_cli::store::LocalStore::new(store_root)?;
```

For run commands, build runtime with the command's `inspect.store_root`.

- [ ] **Step 10: Run CLI tests**

Run:

```bash
cargo test cli::tests
```

Expected: PASS.

- [ ] **Step 11: Commit CLI options**

Run:

```bash
git add src/cli.rs src/lib.rs src/main.rs
git commit -m "feat(cli): add inspect recording options"
```

---

### Task 5: Add Inspect Server Write Config, Token Validation, And Session Descriptor

**Files:**
- Modify: `src/inspect_server.rs`
- Modify: `src/run_recording.rs`
- Modify: `src/main.rs`
- Test: `src/inspect_server.rs`

- [ ] **Step 1: Write inspect server write config tests**

In `src/inspect_server.rs` tests, add:

```rust
#[test]
fn write_config_rejects_no_token_on_non_loopback() {
  let error = super::InspectServeConfig {
    host: "0.0.0.0".to_string(),
    port: 8765,
    store_root: None,
    write: super::InspectWriteConfig {
      enabled: true,
      token: None,
      no_token: true,
    },
  }
  .validate_write_security()
  .expect_err("non-loopback write without token should reject");

  assert!(error.contains("non-loopback"));
}

#[test]
fn write_config_allows_no_token_on_loopback() {
  super::InspectServeConfig {
    host: "127.0.0.1".to_string(),
    port: 8765,
    store_root: None,
    write: super::InspectWriteConfig {
      enabled: true,
      token: None,
      no_token: true,
    },
  }
  .validate_write_security()
  .expect("loopback write without token should be allowed");
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test inspect_server::tests::write_config --lib
```

Expected: FAIL because `InspectWriteConfig` and validation do not exist.

- [ ] **Step 3: Extend inspect server config**

In `src/inspect_server.rs`, add:

```rust
#[derive(Clone, Debug)]
pub struct InspectWriteConfig {
  pub enabled: bool,
  pub token: Option<String>,
  pub no_token: bool,
}

impl Default for InspectWriteConfig {
  fn default() -> Self {
    Self {
      enabled: false,
      token: None,
      no_token: false,
    }
  }
}
```

Extend `InspectServeConfig`:

```rust
pub struct InspectServeConfig {
  pub host: String,
  pub port: u16,
  pub store_root: Option<std::path::PathBuf>,
  pub write: InspectWriteConfig,
}
```

Update `Default`.

Add validation:

```rust
impl InspectServeConfig {
  pub fn validate_write_security(&self) -> AuvResult<()> {
    if !self.write.enabled {
      return Ok(());
    }
    if self.write.no_token && self.write.token.is_some() {
      return Err("--no-write-token cannot be combined with a write token".to_string());
    }
    if self.write.no_token && !is_loopback_host(&self.host) {
      return Err("non-loopback inspect server write requires a token".to_string());
    }
    if !is_loopback_host(&self.host) && self.write.token.is_none() {
      return Err("non-loopback inspect server write requires a token".to_string());
    }
    Ok(())
  }
}

fn is_loopback_host(host: &str) -> bool {
  matches!(host, "127.0.0.1" | "localhost" | "::1")
}
```

- [ ] **Step 4: Add session descriptor helpers**

In `src/run_recording.rs`, add:

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectServerSession {
  pub url: String,
  pub store_root: String,
  pub write_enabled: bool,
  pub write_token: Option<String>,
  pub pid: u32,
  pub started_at_millis: u128,
}

pub fn default_session_path() -> std::path::PathBuf {
  std::env::var_os("AUV_INSPECT_SESSION")
    .map(std::path::PathBuf::from)
    .unwrap_or_else(|| std::env::temp_dir().join("auv-inspect-session.json"))
}

pub fn write_inspect_session(session: &InspectServerSession) -> AuvResult<()> {
  let path = default_session_path();
  if let Some(parent) = path.parent() {
    std::fs::create_dir_all(parent)
      .map_err(|error| format!("failed to create inspect session directory: {error}"))?;
  }
  let bytes = serde_json::to_vec_pretty(session)
    .map_err(|error| format!("failed to encode inspect session: {error}"))?;
  std::fs::write(&path, bytes)
    .map_err(|error| format!("failed to write inspect session {}: {error}", path.display()))
}

pub fn read_inspect_session() -> AuvResult<Option<InspectServerSession>> {
  let path = default_session_path();
  if !path.exists() {
    return Ok(None);
  }
  let raw = std::fs::read_to_string(&path)
    .map_err(|error| format!("failed to read inspect session {}: {error}", path.display()))?;
  serde_json::from_str(&raw)
    .map(Some)
    .map_err(|error| format!("failed to parse inspect session {}: {error}", path.display()))
}
```

- [ ] **Step 5: Write session descriptor when server starts**

In `inspect_server::serve`, call `config.validate_write_security()?` before binding.

After `local_address` is known and before `axum::serve`, write a session when `config.write.enabled`:

```rust
if config.write.enabled {
  let session = crate::run_recording::InspectServerSession {
    url: format!("http://{local_address}"),
    store_root: store.root().display().to_string(),
    write_enabled: true,
    write_token: config.write.token.clone(),
    pid: std::process::id(),
    started_at_millis: crate::model::now_millis(),
  };
  crate::run_recording::write_inspect_session(&session)?;
}
```

- [ ] **Step 6: Wire `src/main.rs` serve options into config**

When handling `CliCommand::InspectServe`, convert CLI write options:

```rust
let token = if let Some(token) = &write.token {
  Some(token.clone())
} else if let Some(path) = &write.token_file {
  Some(std::fs::read_to_string(path).map_err(|error| {
    format!("failed to read write token file {path}: {error}")
  })?.trim().to_string())
} else if write.enabled && !write.no_token {
  Some(format!("session-{}-{}", std::process::id(), auv_cli::model::now_millis()))
} else {
  None
};

let config = auv_cli::inspect_server::InspectServeConfig {
  host: host.clone(),
  port: *port,
  store_root: Some(store_root.clone()),
  write: auv_cli::inspect_server::InspectWriteConfig {
    enabled: write.enabled || token.is_some(),
    token,
    no_token: write.no_token,
  },
};
```

- [ ] **Step 7: Run focused tests**

Run:

```bash
cargo test write_config --lib
cargo test run_recording --lib
```

Expected: PASS.

- [ ] **Step 8: Commit write config**

Run:

```bash
git add src/inspect_server.rs src/run_recording.rs src/main.rs
git commit -m "feat(inspect): add write configuration"
```

---

### Task 6: Implement Write Update Endpoint With Conflict Rejection

**Files:**
- Modify: `src/inspect_server.rs`
- Modify: `src/run_recording.rs`
- Test: `src/inspect_server.rs`

- [ ] **Step 1: Write write-endpoint tests**

In `src/inspect_server.rs` tests, add:

```rust
#[tokio::test]
async fn write_updates_rejects_when_write_disabled() {
  let root = temp_dir("inspect-write-disabled");
  let store = LocalStore::new(root.clone()).expect("store should initialize");
  let app = router_with_config(
    store,
    Arc::new(BroadcastRunRecorder::new(16)),
    InspectWriteConfig::default(),
  );

  let response = app
    .oneshot(
      Request::builder()
        .method("POST")
        .uri("/write/runs/run_write_test/updates")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"updates":[]}"#))
        .expect("request should build"),
    )
    .await
    .expect("route should respond");

  assert_eq!(response.status(), StatusCode::FORBIDDEN);
  let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn write_updates_accepts_run_started_and_persists_snapshot() {
  let root = temp_dir("inspect-write-accept");
  let store = LocalStore::new(root.clone()).expect("store should initialize");
  let app = router_with_config(
    store.clone(),
    Arc::new(BroadcastRunRecorder::new(16)),
    InspectWriteConfig {
      enabled: true,
      token: Some("secret".to_string()),
      no_token: false,
    },
  );
  let body = serde_json::json!({
    "updates": [{
      "type": "runStarted",
      "runId": "run_write_test",
      "run": test_run_json("run_write_test")
    }]
  });

  let response = app
    .oneshot(
      Request::builder()
        .method("POST")
        .uri("/write/runs/run_write_test/updates")
        .header("authorization", "Bearer secret")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request should build"),
    )
    .await
    .expect("route should respond");

  assert_eq!(response.status(), StatusCode::OK);
  assert!(store.read_run("run_write_test").is_ok());
  let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn write_updates_rejects_conflicting_run_metadata() {
  let root = temp_dir("inspect-write-conflict");
  let store = LocalStore::new(root.clone()).expect("store should initialize");
  write_test_run(&store, RunId::new("run_write_conflict"), None);
  let app = router_with_config(
    store,
    Arc::new(BroadcastRunRecorder::new(16)),
    InspectWriteConfig {
      enabled: true,
      token: None,
      no_token: true,
    },
  );
  let body = serde_json::json!({
    "updates": [{
      "type": "runStarted",
      "runId": "run_write_conflict",
      "run": test_run_json("run_write_conflict")
    }]
  });

  let response = app
    .oneshot(
      Request::builder()
        .method("POST")
        .uri("/write/runs/run_write_conflict/updates")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request should build"),
    )
    .await
    .expect("route should respond");

  assert_eq!(response.status(), StatusCode::CONFLICT);
  let body = to_bytes(response.into_body(), usize::MAX)
    .await
    .expect("body should read");
  let value: serde_json::Value = serde_json::from_slice(&body).expect("json error");
  assert_eq!(value["error"]["code"], "runConflict");
  assert_eq!(value["error"]["conflictKind"], "runMetadataMismatch");
  let _ = fs::remove_dir_all(root);
}
```

Add helper:

```rust
fn test_run_json(run_id: &str) -> serde_json::Value {
  serde_json::json!({
    "apiVersion": RUN_API_VERSION,
    "runId": run_id,
    "traceId": "00000000000000000000000000000001",
    "runType": "execute",
    "state": "running",
    "statusCode": "unset",
    "startedAtMillis": 100,
    "finishedAtMillis": null,
    "rootSpanId": "0000000000000001",
    "attributes": {},
    "summary": null,
    "failure": null
  })
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test inspect_server::tests::write_updates --lib
```

Expected: FAIL because write routes do not exist.

- [ ] **Step 3: Add router with write config**

In `src/inspect_server.rs`, add a new internal router constructor:

```rust
fn router_with_config(
  store: LocalStore,
  event_sink: Arc<BroadcastRunRecorder>,
  write: InspectWriteConfig,
) -> Router {
  let state = InspectServerState {
    store: Arc::new(store),
    event_sink,
    write,
  };
  Router::new()
    .route("/runs", get(list_runs))
    .route("/runs/{run_id}", get(get_run))
    .route("/runs/{run_id}/spans", get(get_spans))
    .route("/runs/{run_id}/events", get(get_events))
    .route("/runs/{run_id}/artifacts", get(get_artifacts))
    .route("/runs/{run_id}/artifacts/{artifact_id}", get(get_artifact))
    .route("/runs/{run_id}/stream", get(stream_run))
    .route("/write/runs/{run_id}/updates", axum::routing::post(write_updates))
    .with_state(state)
}
```

Make public `router` call `router_with_config(..., InspectWriteConfig::default())`.

- [ ] **Step 4: Add request/response structs**

In `src/inspect_server.rs`, add:

```rust
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteUpdatesRequest {
  updates: Vec<crate::run_recording::ApiRunUpdate>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct WriteUpdatesResponse {
  accepted: usize,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StructuredErrorBody {
  error: StructuredError,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StructuredError {
  code: String,
  message: String,
  run_id: Option<String>,
  conflict_kind: Option<String>,
  resolution: Option<String>,
  retryable: bool,
}
```

- [ ] **Step 5: Implement auth helper**

Add:

```rust
fn authorize_write(
  headers: &axum::http::HeaderMap,
  write: &InspectWriteConfig,
) -> Result<(), InspectHttpError> {
  if !write.enabled {
    return Err(InspectHttpError::forbidden("inspect server write is disabled".to_string()));
  }
  if write.no_token {
    return Ok(());
  }
  let Some(expected) = &write.token else {
    return Err(InspectHttpError::forbidden("inspect server write token is required".to_string()));
  };
  let actual = headers
    .get(axum::http::header::AUTHORIZATION)
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.strip_prefix("Bearer "));
  if actual == Some(expected.as_str()) {
    Ok(())
  } else {
    Err(InspectHttpError::forbidden("invalid inspect server write token".to_string()))
  }
}
```

Add `InspectHttpError::forbidden`.

- [ ] **Step 6: Implement update application**

Add:

```rust
async fn write_updates(
  State(state): State<InspectServerState>,
  Path(run_id): Path<String>,
  headers: axum::http::HeaderMap,
  Json(request): Json<WriteUpdatesRequest>,
) -> Result<Response, InspectHttpError> {
  authorize_write(&headers, &state.write)?;
  let mut snapshot = match state.store.read_run(&run_id) {
    Ok(snapshot) => Some(snapshot),
    Err(_) => None,
  };
  let updates = request
    .updates
    .into_iter()
    .map(RunUpdate::from)
    .collect::<Vec<_>>();
  for update in &updates {
    if update.run_id().as_str() != run_id {
      return Err(InspectHttpError::bad_request(format!(
        "update runId {} does not match request runId {run_id}",
        update.run_id()
      )));
    }
    apply_update(&mut snapshot, update).map_err(InspectHttpError::conflict)?;
  }
  let Some(snapshot) = snapshot else {
    return Err(InspectHttpError::bad_request(
      "first update for a run must be runStarted".to_string(),
    ));
  };
  state
    .store
    .write_run_snapshot(&snapshot)
    .map_err(InspectHttpError::from_store)?;
  let accepted = updates.len();
  for update in updates {
    state
      .event_sink
      .record(update)
      .map_err(InspectHttpError::from_store)?;
  }
  Ok(Json(WriteUpdatesResponse { accepted }).into_response())
}
```

Then implement `apply_update` with explicit conflict checks:

```rust
fn apply_update(
  snapshot: &mut Option<crate::store::CanonicalRun>,
  update: &RunUpdate,
) -> Result<(), RunConflict> {
  match update {
    RunUpdate::RunStarted { run, .. } => {
      if let Some(existing) = snapshot {
        if existing.run != *run {
          return Err(RunConflict::new(&run.run_id, "runMetadataMismatch"));
        }
        return Ok(());
      }
      *snapshot = Some(crate::store::CanonicalRun {
        run: run.clone(),
        spans: Vec::new(),
        events: Vec::new(),
        artifacts: Vec::new(),
      });
      Ok(())
    }
    RunUpdate::SpanStarted { run_id, span } => {
      let snapshot = snapshot
        .as_mut()
        .ok_or_else(|| RunConflict::new(run_id, "missingRunStarted"))?;
      if snapshot.run.state == TraceState::Ended {
        return Err(RunConflict::new(run_id, "runAlreadyFinished"));
      }
      if let Some(parent) = &span.parent_span_id
        && !snapshot.spans.iter().any(|existing| existing.span_id == *parent)
        && snapshot.run.root_span_id != *parent
      {
        return Err(RunConflict::new(run_id, "missingParentSpan"));
      }
      if let Some(existing) = snapshot.spans.iter().find(|existing| existing.span_id == span.span_id)
      {
        if existing != span {
          return Err(RunConflict::new(run_id, "duplicateSpanMismatch"));
        }
        return Ok(());
      }
      snapshot.spans.push(span.clone());
      Ok(())
    }
    RunUpdate::SpanFinished { run_id, span } => {
      let snapshot = snapshot
        .as_mut()
        .ok_or_else(|| RunConflict::new(run_id, "missingRunStarted"))?;
      let Some(existing) = snapshot.spans.iter_mut().find(|existing| existing.span_id == span.span_id)
      else {
        return Err(RunConflict::new(run_id, "missingParentSpan"));
      };
      if existing.state == TraceState::Ended && existing != span {
        return Err(RunConflict::new(run_id, "duplicateSpanMismatch"));
      }
      *existing = span.clone();
      Ok(())
    }
    RunUpdate::EventAppended { run_id, event } => {
      let snapshot = snapshot
        .as_mut()
        .ok_or_else(|| RunConflict::new(run_id, "missingRunStarted"))?;
      if let Some(existing) = snapshot.events.iter().find(|existing| existing.event_id == event.event_id)
      {
        if existing != event {
          return Err(RunConflict::new(run_id, "duplicateEventMismatch"));
        }
        return Ok(());
      }
      snapshot.events.push(event.clone());
      Ok(())
    }
    RunUpdate::ArtifactCreated { run_id, artifact } => {
      let snapshot = snapshot
        .as_mut()
        .ok_or_else(|| RunConflict::new(run_id, "missingRunStarted"))?;
      if let Some(existing) = snapshot
        .artifacts
        .iter()
        .find(|existing| existing.artifact_id == artifact.artifact_id)
      {
        if existing != artifact {
          return Err(RunConflict::new(run_id, "duplicateArtifactMismatch"));
        }
        return Ok(());
      }
      snapshot.artifacts.push(artifact.clone());
      Ok(())
    }
    RunUpdate::RunFinished { run_id, run } => {
      let snapshot = snapshot
        .as_mut()
        .ok_or_else(|| RunConflict::new(run_id, "missingRunStarted"))?;
      if snapshot.run.state == TraceState::Ended && snapshot.run != *run {
        return Err(RunConflict::new(run_id, "runAlreadyFinished"));
      }
      snapshot.run = run.clone();
      Ok(())
    }
  }
}
```

Add `RunConflict`:

```rust
#[derive(Debug)]
struct RunConflict {
  run_id: String,
  kind: String,
}

impl RunConflict {
  fn new(run_id: &RunId, kind: &str) -> Self {
    Self {
      run_id: run_id.to_string(),
      kind: kind.to_string(),
    }
  }
}
```

- [ ] **Step 7: Make conflict response structured**

Add:

```rust
fn conflict_response(conflict: RunConflict) -> Response {
  (
    StatusCode::CONFLICT,
    Json(StructuredErrorBody {
      error: StructuredError {
        code: "runConflict".to_string(),
        message: format!("run {} rejected update conflict {}", conflict.run_id, conflict.kind),
        run_id: Some(conflict.run_id),
        conflict_kind: Some(conflict.kind),
        resolution: Some("startNewRun".to_string()),
        retryable: false,
      },
    }),
  )
    .into_response()
}
```

Let `InspectHttpError::conflict` carry a prebuilt response or add a variant enum. Keep the first implementation simple and explicit.

- [ ] **Step 8: Run write endpoint tests**

Run:

```bash
cargo test inspect_server::tests::write_updates --lib
```

Expected: PASS.

- [ ] **Step 9: Commit write endpoint**

Run:

```bash
git add src/inspect_server.rs src/run_recording.rs
git commit -m "feat(inspect): accept run update writes"
```

---

### Task 7: Add Inspect Server Reporting Recorder

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/run_recording.rs`
- Modify: `src/main.rs`
- Test: `src/run_recording.rs`

- [ ] **Step 1: Add HTTP client dependency**

Modify `Cargo.toml`:

```toml
reqwest = { version = "0.12", default-features = false, features = ["blocking", "json", "rustls-tls"] }
```

- [ ] **Step 2: Write recorder tests with an axum route**

In `src/run_recording.rs` tests, add a test that posts to a local test server:

```rust
#[tokio::test]
async fn inspect_server_recorder_posts_update_batch() {
  use axum::routing::post;
  use axum::{Json, Router};
  use serde_json::Value;
  use std::sync::{Arc, Mutex};
  use tokio::net::TcpListener;

  let captured = Arc::new(Mutex::new(None::<Value>));
  let captured_route = captured.clone();
  let app = Router::new().route(
    "/write/runs/run_update_test/updates",
    post(move |Json(value): Json<Value>| {
      let captured = captured_route.clone();
      async move {
        *captured.lock().expect("capture lock") = Some(value);
        Json(serde_json::json!({ "accepted": 1 }))
      }
    }),
  );
  let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind test server");
  let address = listener.local_addr().expect("local addr");
  tokio::spawn(async move {
    axum::serve(listener, app).await.expect("test server");
  });

  let recorder = InspectServerRunRecorder::new(
    format!("http://{address}"),
    Some("secret".to_string()),
    false,
  );
  recorder
    .record(RunUpdate::RunStarted {
      run_id: RunId::new("run_update_test"),
      run: test_run(),
    })
    .expect("server record should succeed");

  let captured = captured.lock().expect("capture lock").clone().expect("captured body");
  assert_eq!(captured["updates"][0]["type"], "runStarted");
  assert_eq!(captured["updates"][0]["runId"], "run_update_test");
}
```

- [ ] **Step 3: Run test and verify it fails**

Run:

```bash
cargo test run_recording::tests::inspect_server_recorder_posts_update_batch --lib
```

Expected: FAIL because `InspectServerRunRecorder` does not exist.

- [ ] **Step 4: Implement `InspectServerRunRecorder`**

Add to `src/run_recording.rs`:

```rust
#[derive(Clone)]
pub struct InspectServerRunRecorder {
  base_url: String,
  token: Option<String>,
  required: bool,
  client: reqwest::blocking::Client,
}

impl InspectServerRunRecorder {
  pub fn new(base_url: String, token: Option<String>, required: bool) -> Self {
    Self {
      base_url: base_url.trim_end_matches('/').to_string(),
      token,
      required,
      client: reqwest::blocking::Client::new(),
    }
  }
}

impl RunRecorder for InspectServerRunRecorder {
  fn record(&self, update: RunUpdate) -> AuvResult<()> {
    let url = format!(
      "{}/write/runs/{}/updates",
      self.base_url,
      update.run_id().as_str()
    );
    let mut request = self
      .client
      .post(url)
      .json(&serde_json::json!({ "updates": [ApiRunUpdate::from(update)] }));
    if let Some(token) = &self.token {
      request = request.bearer_auth(token);
    }
    let response = request
      .send()
      .map_err(|error| format!("inspect server write failed: {error}"))?;
    if response.status().is_success() {
      return Ok(());
    }
    let status = response.status();
    let body = response.text().unwrap_or_else(|_| String::new());
    let message = format!("inspect server write rejected with {status}: {body}");
    if self.required {
      Err(message)
    } else {
      eprintln!("warning: {message}");
      Ok(())
    }
  }
}
```

- [ ] **Step 5: Add CLI recording builder in `src/main.rs`**

Add helper:

```rust
fn build_runtime_for_command(
  project_root: PathBuf,
  inspect: &cli::InspectClientOptions,
) -> Result<auv_cli::runtime::Runtime, String> {
  let store_root = resolve_store_root(&project_root, inspect.store_root.as_ref());
  let store = auv_cli::store::LocalStore::new(store_root)?;
  let mut recorders: Vec<std::sync::Arc<dyn auv_cli::run_recording::RunRecorder>> = Vec::new();
  if should_try_server_write(inspect) {
    if let Some((url, token)) = resolve_inspect_server_target(inspect)? {
      recorders.push(std::sync::Arc::new(
        auv_cli::run_recording::InspectServerRunRecorder::new(
          url,
          token,
          inspect.require_server_write,
        ),
      ));
    } else if inspect.require_server_write {
      return Err("inspect server write is required but no inspect server is configured".to_string());
    }
  }
  let recorder: std::sync::Arc<dyn auv_cli::run_recording::RunRecorder> =
    if recorders.is_empty() {
      std::sync::Arc::new(auv_cli::run_recording::NoopRunRecorder)
    } else {
      std::sync::Arc::new(auv_cli::run_recording::CompositeRunRecorder::new(recorders))
    };
  let recording = auv_cli::run_recording::RunRecordingBackend::new(store, recorder);
  Ok(auv_cli::build_default_runtime(project_root)?.with_recording(recording))
}
```

Add:

```rust
fn should_try_server_write(inspect: &cli::InspectClientOptions) -> bool {
  !matches!(inspect.server_write, cli::InspectWriteSetting::Disabled)
}

fn resolve_inspect_server_target(
  inspect: &cli::InspectClientOptions,
) -> Result<Option<(String, Option<String>)>, String> {
  if let Some(url) = &inspect.server_url {
    return Ok(Some((url.clone(), resolve_client_token(inspect)?)));
  }
  if let Some(session) = auv_cli::run_recording::read_inspect_session()? {
    if session.write_enabled {
      return Ok(Some((session.url, session.write_token)));
    }
  }
  Ok(None)
}

fn resolve_client_token(inspect: &cli::InspectClientOptions) -> Result<Option<String>, String> {
  if let Some(token) = &inspect.server_token {
    return Ok(Some(token.clone()));
  }
  if let Some(path) = &inspect.server_token_file {
    return Ok(Some(
      std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read inspect server token file {path}: {error}"))?
        .trim()
        .to_string(),
    ));
  }
  Ok(None)
}
```

Then use this helper for `Invoke`, `SkillRun`, and `SkillCasesRun` command arms. Keep app probe/analyze/distill/validate on default runtime in this task unless their CLI variants are extended with inspect options.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test inspect_server_recorder_posts_update_batch --lib
cargo test cli::tests
```

Expected: PASS.

- [ ] **Step 7: Commit server reporter**

Run:

```bash
git add Cargo.toml Cargo.lock src/run_recording.rs src/main.rs
git commit -m "feat(recording): report runs to inspect server"
```

---

### Task 8: Reserve Artifact Upload Route

**Files:**
- Modify: `src/inspect_server.rs`
- Test: `src/inspect_server.rs`

- [ ] **Step 1: Write route reservation test**

Add to `src/inspect_server.rs` tests:

```rust
#[tokio::test]
async fn artifact_write_route_is_reserved() {
  let root = temp_dir("inspect-artifact-write-reserved");
  let store = LocalStore::new(root.clone()).expect("store should initialize");
  let app = router_with_config(
    store,
    Arc::new(BroadcastRunRecorder::new(16)),
    InspectWriteConfig {
      enabled: true,
      token: None,
      no_token: true,
    },
  );

  let response = app
    .oneshot(
      Request::builder()
        .method("POST")
        .uri("/write/runs/run_artifact/artifacts/artifact_0001")
        .body(Body::from("artifact bytes"))
        .expect("request should build"),
    )
    .await
    .expect("route should respond");

  assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
  let _ = fs::remove_dir_all(root);
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
cargo test inspect_server::tests::artifact_write_route_is_reserved --lib
```

Expected: FAIL because the route is not registered.

- [ ] **Step 3: Add reserved route**

In `router_with_config`, add:

```rust
.route(
  "/write/runs/{run_id}/artifacts/{artifact_id}",
  axum::routing::post(write_artifact_reserved),
)
```

Add handler:

```rust
async fn write_artifact_reserved() -> Response {
  (
    StatusCode::NOT_IMPLEMENTED,
    Json(serde_json::json!({
      "error": {
        "code": "artifactUploadNotImplemented",
        "message": "artifact byte upload is reserved for a later implementation",
        "retryable": false
      }
    })),
  )
    .into_response()
}
```

- [ ] **Step 4: Run focused test**

Run:

```bash
cargo test inspect_server::tests::artifact_write_route_is_reserved --lib
```

Expected: PASS.

- [ ] **Step 5: Commit route reservation**

Run:

```bash
git add src/inspect_server.rs
git commit -m "feat(inspect): reserve artifact write route"
```

---

### Task 9: Documentation And Full Verification

**Files:**
- Modify: `docs/TERMS_AND_CONCEPTS.md`
- Modify: `docs/ai/references/2026-05-21-live-inspect-recording-design.md`

- [ ] **Step 1: Update shared terminology**

In `docs/TERMS_AND_CONCEPTS.md`, add this after the Inspect Server section:

```markdown
## Run Recording Backend

A run recording backend is the runtime dependency that receives run/span/event
updates and writes them to one or more configured targets. Targets may include a
local run store, an inspect server reporter, or a same-process broadcast bus.

When multiple targets are enabled, AUV treats recording as multi-write. There is
no universal single source of truth across targets; each target owns the records
it accepted.
```

- [ ] **Step 2: Mark the design reference as planned**

In `docs/ai/references/2026-05-21-live-inspect-recording-design.md`, change:

```markdown
Status: draft for review
```

to:

```markdown
Status: implementation planned
```

- [ ] **Step 3: Run formatting and tests**

Run:

```bash
cargo fmt --check
cargo test
git diff --check
cargo run --quiet -- list-commands
cargo run --quiet -- skill cases list
cargo run --quiet -- skill bundle list
```

Expected: all commands pass. The `cargo run` commands should print command, case-matrix, and bundle listings without errors.

- [ ] **Step 4: Commit docs and verification fixes**

Run:

```bash
git add docs/TERMS_AND_CONCEPTS.md docs/ai/references/2026-05-21-live-inspect-recording-design.md
git commit -m "docs: document run recording backend"
```

---

## Self-Review Notes

- Spec coverage:
  - `RunRecordingBackend`, multi-write, and stable attributes are covered in Tasks 1-3.
  - Store root and run-side/server-side CLI options are covered in Task 4.
  - Write token/session security is covered in Task 5.
  - Cross-process HTTP write and conflict rejection are covered in Task 6.
  - Client reporting is covered in Task 7.
  - Artifact upload is reserved in Task 8 and intentionally not fully implemented, matching the phase-1 design.
  - Shared terminology and full validation are covered in Task 9.
- Known staged limitation:
  - Phase 1 sends artifact metadata but not bytes. The reserved route returns `501 Not Implemented` until a later artifact-byte implementation plan.
