# Trace Run Inspect Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

Implementation status: completed on 2026-05-19. The resulting implementation is
the source of truth where this plan's scaffolding snippets differ from code.

**Goal:** Implement the `v1alpha1` trace/run recording format, canonical run store, formatter-based inspect output, workflow-level run aggregation, and read-only inspect server described in `docs/ai/references/2026-05-19-trace-run-inspect-design.md`.

**Architecture:** Add a focused trace model and recording state, then migrate current command recording to the new canonical `.auv/runs/{run_id}` layout. Keep CLI-facing APIs stable where practical, but make `run.json`, `spans.jsonl`, `events.jsonl`, `artifacts.jsonl`, and `artifacts/` the only persisted run data. Build the inspect server as a read-only same-process access layer over the new run reader and a broadcast stream.

**Tech Stack:** Rust 2024, `serde`, `serde_json`, existing `LocalStore`, new `axum`/`tokio` for inspect server.

---

## Scope Notes

This plan intentionally excludes replay, mutation APIs, legacy run conversion, native GUI, and OTLP export. It implements the local shape needed for future viewer work.

The plan uses `docs/ai/references` instead of `docs/superpowers/plans` because repository guidance already treats `docs/ai/references` as durable reference material, and the user requested not to keep the spec under `docs/superpowers`.

Implementation order note: execute Task 5 before Task 4. Task 4 is numbered
earlier because it describes the current ad-hoc command behavior being
migrated, but it depends on the recording context introduced in Task 5:
`RecordingRun`, `SpanRef`, `RunSpec`, `RunFinish`, and `SpanFinish`.

Rust tracing alignment:

- Follow `tracing`'s split between spans as timed scopes and events as point-in-time records.
- Keep persistence/export code separate from instrumentation state, similar to how `tracing-subscriber` composes observing `Layer`s around a subscriber.
- Keep AUV events in `events.jsonl` for local append-friendly writes, then group them back under spans for future OTLP export, matching `tracing-opentelemetry`'s conversion model.
- Do not adopt the `tracing` crate as the internal storage format in this phase; use AUV records that map cleanly to Rust tracing and OpenTelemetry concepts.
- Treat span lifecycle as first-class. A child span should be able to end before
  the run ends; do not rely on run finalization to close every span with one
  inherited status.

## File Structure

- Create `src/trace.rs`: canonical `v1alpha1` run/span/event/artifact types, API-version constants, ID helpers, and status enums.
- Create `src/recording.rs`: runtime recording state, typed span handles, and same-process event sinks for live inspect streaming.
- Create `src/inspect.rs`: formatter-based human-readable run rendering from canonical records.
- Create `src/inspect_server.rs`: read-only HTTP/WebSocket inspect server routes and response wrappers.
- Modify `src/lib.rs`: export the new modules.
- Modify `src/model.rs`: keep public command/driver types; reduce old run-rendering responsibility after migration.
- Modify `src/store.rs`: replace old text-snapshot persistence with canonical run snapshot writer/reader and run-local artifact persistence.
- Modify `src/runtime.rs`: record ad-hoc command runs through the new recorder, expose helpers for workflow-level recording, and publish live stream updates to the event bus.
- Modify `src/skill.rs`: aggregate recipe and case-matrix execution under parent runs/spans.
- Modify `src/app.rs`: aggregate app probe/analyze/distill/validate under parent runs/spans and remove `inspect_path` from new probe step data.
- Modify `src/cli.rs`: add `inspect serve` parsing.
- Modify `src/main.rs`: render inspect output dynamically and start the inspect server command.
- Modify `Cargo.toml`: add server dependencies only when the server task begins.

## Task 1: Canonical Trace Types

**Files:**
- Create: `src/trace.rs`
- Modify: `src/lib.rs`
- Test: `src/trace.rs`

- [ ] **Step 1: Write failing tests for API versions, state/status strings, and ID shape**

Add this test module to the new `src/trace.rs` file before implementation:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn api_versions_are_v1alpha1() {
    assert_eq!(RUN_API_VERSION, "auv.run.v1alpha1");
    assert_eq!(SPAN_API_VERSION, "auv.span.v1alpha1");
    assert_eq!(EVENT_API_VERSION, "auv.event.v1alpha1");
    assert_eq!(ARTIFACT_API_VERSION, "auv.artifact.v1alpha1");
  }

  #[test]
  fn generated_ids_are_prefixed_and_distinct() {
    let first_run = new_run_id();
    let second_run = new_run_id();
    let trace_id = new_trace_id();
    let span_id = new_span_id();
    let event_id = new_event_id();

    assert!(first_run.as_str().starts_with("run_"));
    assert_ne!(first_run, second_run);
    assert_eq!(trace_id.as_str().len(), 32);
    assert_eq!(span_id.as_str().len(), 16);
    assert!(event_id.as_str().starts_with("event_"));
  }

  #[test]
  fn status_codes_match_otel_words() {
    assert_eq!(TraceStatusCode::Unset.as_str(), "unset");
    assert_eq!(TraceStatusCode::Ok.as_str(), "ok");
    assert_eq!(TraceStatusCode::Error.as_str(), "error");
  }
}
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run:

```bash
cargo test trace::tests::api_versions_are_v1alpha1 trace::tests::generated_ids_are_prefixed_and_distinct trace::tests::status_codes_match_otel_words
```

Expected: compile failure because `src/trace.rs` is not implemented and not exported.

- [ ] **Step 3: Implement trace constants, enums, record structs, and ID helpers**

Create `src/trace.rs` with this implementation:

```rust
use std::collections::BTreeMap;
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::model::now_millis;

pub const RUN_API_VERSION: &str = "auv.run.v1alpha1";
pub const SPAN_API_VERSION: &str = "auv.span.v1alpha1";
pub const EVENT_API_VERSION: &str = "auv.event.v1alpha1";
pub const ARTIFACT_API_VERSION: &str = "auv.artifact.v1alpha1";

static TRACE_COUNTER: AtomicU64 = AtomicU64::new(0);
static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);
static SPAN_COUNTER: AtomicU64 = AtomicU64::new(0);
static EVENT_COUNTER: AtomicU64 = AtomicU64::new(0);

macro_rules! id_type {
  ($name:ident) => {
    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct $name(String);

    impl $name {
      pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
      }

      pub fn as_str(&self) -> &str {
        &self.0
      }
    }

    impl std::fmt::Display for $name {
      fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
      }
    }

    impl AsRef<str> for $name {
      fn as_ref(&self) -> &str {
        self.as_str()
      }
    }
  };
}

id_type!(RunId);
id_type!(TraceId);
id_type!(SpanId);
id_type!(EventId);
id_type!(ArtifactId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunType {
  Command,
  Execute,
  Probe,
  Analyze,
  Distill,
  Validate,
}

impl RunType {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Command => "command",
      Self::Execute => "execute",
      Self::Probe => "probe",
      Self::Analyze => "analyze",
      Self::Distill => "distill",
      Self::Validate => "validate",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceState {
  Running,
  Ended,
}

impl TraceState {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Running => "running",
      Self::Ended => "ended",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatusCode {
  Unset,
  Ok,
  Error,
}

impl TraceStatusCode {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Unset => "unset",
      Self::Ok => "ok",
      Self::Error => "error",
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TraceFailure {
  pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRecordV1Alpha1 {
  pub api_version: String,
  pub run_id: RunId,
  pub trace_id: TraceId,
  pub run_type: RunType,
  pub state: TraceState,
  pub status_code: TraceStatusCode,
  pub started_at_millis: u128,
  pub finished_at_millis: Option<u128>,
  pub root_span_id: SpanId,
  pub attributes: BTreeMap<String, serde_json::Value>,
  pub summary: Option<String>,
  pub failure: Option<TraceFailure>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpanRecordV1Alpha1 {
  pub api_version: String,
  pub span_id: SpanId,
  pub parent_span_id: Option<SpanId>,
  pub name: String,
  pub state: TraceState,
  pub status_code: TraceStatusCode,
  pub started_at_millis: u128,
  pub finished_at_millis: Option<u128>,
  pub attributes: BTreeMap<String, serde_json::Value>,
  pub summary: Option<String>,
  pub failure: Option<TraceFailure>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventRecordV1Alpha1 {
  pub api_version: String,
  pub event_id: EventId,
  pub span_id: SpanId,
  pub name: String,
  pub timestamp_millis: u128,
  pub attributes: BTreeMap<String, serde_json::Value>,
  pub message: Option<String>,
  pub artifact_ids: Vec<ArtifactId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArtifactRecordV1Alpha1 {
  pub api_version: String,
  pub artifact_id: ArtifactId,
  pub span_id: SpanId,
  pub event_id: Option<EventId>,
  pub role: String,
  pub mime_type: String,
  pub path: String,
  pub sha256: Option<String>,
  pub attributes: BTreeMap<String, serde_json::Value>,
  pub summary: Option<String>,
}

pub fn new_run_id() -> RunId {
  let sequence = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
  RunId::new(format!("run_{}_{}_{}", now_millis(), process::id(), sequence))
}

pub fn new_trace_id() -> TraceId {
  let sequence = TRACE_COUNTER.fetch_add(1, Ordering::Relaxed);
  TraceId::new(format!("{:016x}{:016x}", now_millis() as u64, sequence))
}

pub fn new_span_id() -> SpanId {
  let sequence = SPAN_COUNTER.fetch_add(1, Ordering::Relaxed);
  SpanId::new(format!("{:016x}", sequence + 1))
}

pub fn new_event_id() -> EventId {
  let sequence = EVENT_COUNTER.fetch_add(1, Ordering::Relaxed);
  EventId::new(format!("event_{}_{}", now_millis(), sequence))
}

pub fn string_attr(value: impl Into<String>) -> serde_json::Value {
  serde_json::Value::String(value.into())
}
```

- [ ] **Step 4: Export the trace module**

Modify `src/lib.rs` so it includes:

```rust
pub mod app;
pub mod bundle;
pub mod catalog;
pub mod driver;
pub mod model;
pub mod recording;
pub mod runtime;
pub mod skill;
pub mod store;
pub mod trace;
```

If `recording` does not exist yet, add only `pub mod trace;` in this task. Add `recording`, `inspect`, and `inspect_server` in their own tasks.

- [ ] **Step 5: Run the focused test and verify it passes**

Run:

```bash
cargo test trace::tests
```

Expected: all `trace::tests` pass.

- [ ] **Step 6: Commit**

```bash
git add src/trace.rs src/lib.rs
git commit -m "feat: add trace record types"
```

## Task 2: Canonical Run Store

**Files:**
- Modify: `src/store.rs`
- Test: `src/store.rs`

- [ ] **Step 1: Replace store tests with canonical layout tests**

In `src/store.rs`, replace the existing tests with tests that assert the new layout:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::trace::{
    ArtifactRecordV1Alpha1, EventRecordV1Alpha1, RunRecordV1Alpha1, RunType, SpanRecordV1Alpha1,
    TraceState, TraceStatusCode, RUN_API_VERSION,
  };
  use std::collections::BTreeMap;
  use std::env;

  #[test]
  fn local_store_persists_canonical_run_files() {
    let root = temp_dir("store-canonical");
    let store = LocalStore::new(root.clone()).expect("should initialize");
    let run = dummy_run("run_store_test");
    let span = dummy_span(&run.root_span_id);
    let event = dummy_event(&span.span_id);
    let artifact = dummy_artifact(&span.span_id);

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: vec![event],
        artifacts: vec![artifact],
      })
      .expect("should persist canonical run");

    let run_dir = root.join("runs").join("run_store_test");
    assert!(run_dir.join("run.json").exists());
    assert!(run_dir.join("spans.jsonl").exists());
    assert!(run_dir.join("events.jsonl").exists());
    assert!(run_dir.join("artifacts.jsonl").exists());
    assert!(!run_dir.join("inspect.txt").exists());
    assert!(!run_dir.join("meta.txt").exists());

    let loaded = store.read_run("run_store_test").expect("run should read");
    assert_eq!(loaded.run.api_version, RUN_API_VERSION);
    assert_eq!(loaded.spans.len(), 1);
    assert_eq!(loaded.events.len(), 1);
    assert_eq!(loaded.artifacts.len(), 1);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn local_store_ignores_directories_without_run_json() {
    let root = temp_dir("store-list");
    let store = LocalStore::new(root.clone()).expect("should initialize");
    fs::create_dir_all(root.join("runs").join("old_run_without_run_json"))
      .expect("old run dir");

    let runs = store.list_runs().expect("runs should list");
    assert!(runs.is_empty());

    let _ = fs::remove_dir_all(root);
  }

  fn temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("auv-{}-{}", label, crate::model::now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }

  fn dummy_run(run_id: &str) -> RunRecordV1Alpha1 {
    let root_span_id = crate::trace::SpanId::new("0000000000000001");
    RunRecordV1Alpha1 {
      api_version: RUN_API_VERSION.to_string(),
      run_id: crate::trace::RunId::new(run_id),
      trace_id: crate::trace::TraceId::new("00000000000000000000000000000001"),
      run_type: RunType::Command,
      state: TraceState::Ended,
      status_code: TraceStatusCode::Ok,
      started_at_millis: 100,
      finished_at_millis: Some(200),
      root_span_id,
      attributes: BTreeMap::new(),
      summary: Some("ok".to_string()),
      failure: None,
    }
  }

  fn dummy_span(span_id: &crate::trace::SpanId) -> SpanRecordV1Alpha1 {
    SpanRecordV1Alpha1 {
      api_version: crate::trace::SPAN_API_VERSION.to_string(),
      span_id: span_id.clone(),
      parent_span_id: None,
      name: "auv.command".to_string(),
      state: TraceState::Ended,
      status_code: TraceStatusCode::Ok,
      started_at_millis: 100,
      finished_at_millis: Some(200),
      attributes: BTreeMap::new(),
      summary: Some("ok".to_string()),
      failure: None,
    }
  }

  fn dummy_event(span_id: &crate::trace::SpanId) -> EventRecordV1Alpha1 {
    EventRecordV1Alpha1 {
      api_version: crate::trace::EVENT_API_VERSION.to_string(),
      event_id: crate::trace::EventId::new("event_1"),
      span_id: span_id.clone(),
      name: "command.resolved".to_string(),
      timestamp_millis: 100,
      attributes: BTreeMap::new(),
      message: Some("resolved".to_string()),
      artifact_ids: Vec::new(),
    }
  }

  fn dummy_artifact(span_id: &crate::trace::SpanId) -> ArtifactRecordV1Alpha1 {
    ArtifactRecordV1Alpha1 {
      api_version: crate::trace::ARTIFACT_API_VERSION.to_string(),
      artifact_id: crate::trace::ArtifactId::new("artifact_0001"),
      span_id: span_id.clone(),
      event_id: None,
      role: "driver.output".to_string(),
      mime_type: "text/plain".to_string(),
      path: "artifacts/artifact_0001_output.txt".to_string(),
      sha256: None,
      attributes: BTreeMap::new(),
      summary: Some("output".to_string()),
    }
  }
}
```

- [ ] **Step 2: Run store tests and verify they fail**

Run:

```bash
cargo test store::tests
```

Expected: compile failure for missing `write_run_snapshot`, `read_run`, `list_runs`, and `CanonicalRun`.

- [ ] **Step 3: Implement canonical read/write methods**

In `src/store.rs`, replace old text-snapshot write logic with these public types and methods:

```rust
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::driver::{copy_file, sanitized_artifact_name};
use crate::model::{AuvResult, ProducedArtifact};
use crate::trace::{
  ArtifactId, ArtifactRecordV1Alpha1, EventId, EventRecordV1Alpha1, RunId,
  RunRecordV1Alpha1, SpanId, SpanRecordV1Alpha1,
  ARTIFACT_API_VERSION, EVENT_API_VERSION, RUN_API_VERSION, SPAN_API_VERSION,
};

pub struct CanonicalRun {
  pub run: RunRecordV1Alpha1,
  pub spans: Vec<SpanRecordV1Alpha1>,
  pub events: Vec<EventRecordV1Alpha1>,
  pub artifacts: Vec<ArtifactRecordV1Alpha1>,
}

pub struct LocalStore {
  root: PathBuf,
}

impl LocalStore {
  pub fn new(root: PathBuf) -> AuvResult<Self> {
    fs::create_dir_all(root.join("runs"))
      .map_err(|error| format!("failed to create run store root: {error}"))?;
    Ok(Self { root })
  }

  pub fn root(&self) -> &Path {
    &self.root
  }

  pub fn run_dir(&self, run_id: impl AsRef<str>) -> PathBuf {
    self.root.join("runs").join(run_id.as_ref())
  }

  pub fn write_run_snapshot(&self, snapshot: &CanonicalRun) -> AuvResult<()> {
    let run_directory = self.run_dir(&snapshot.run.run_id);
    fs::create_dir_all(run_directory.join("artifacts")).map_err(|error| {
      format!(
        "failed to create canonical run directory {}: {error}",
        run_directory.display()
      )
    })?;
    write_json_atomic(&run_directory.join("run.json"), &snapshot.run, "run metadata")?;
    write_jsonl_atomic(&run_directory.join("spans.jsonl"), &snapshot.spans, "span records")?;
    write_jsonl_atomic(&run_directory.join("events.jsonl"), &snapshot.events, "event records")?;
    write_jsonl_atomic(
      &run_directory.join("artifacts.jsonl"),
      &snapshot.artifacts,
      "artifact records",
    )?;
    Ok(())
  }

  pub fn stage_artifact(
    &self,
    run_id: &RunId,
    index: usize,
    artifact: ProducedArtifact,
    span_id: &SpanId,
    event_id: Option<EventId>,
  ) -> AuvResult<ArtifactRecordV1Alpha1> {
    self.stage_artifact_file(
      run_id,
      index,
      span_id,
      event_id,
      artifact.kind,
      artifact.source_path,
      artifact.preferred_name,
      artifact.note,
    )
  }

  pub fn stage_artifact_file(
    &self,
    run_id: &RunId,
    index: usize,
    span_id: &SpanId,
    event_id: Option<EventId>,
    role: String,
    source_path: PathBuf,
    preferred_name: String,
    summary: Option<String>,
  ) -> AuvResult<ArtifactRecordV1Alpha1> {
    let artifact_id = ArtifactId::new(format!("artifact_{:04}", index + 1));
    let extension = source_path
      .extension()
      .and_then(|extension| extension.to_str())
      .unwrap_or("bin");
    let base_name =
      sanitized_artifact_name(preferred_name.trim_end_matches(&format!(".{extension}")));
    let relative_path = PathBuf::from("artifacts").join(format!(
      "{}_{base_name}.{extension}",
      artifact_id.as_str()
    ));
    let destination = self.run_dir(run_id).join(&relative_path);

    copy_file(&source_path, &destination)?;
    if source_path != destination {
      let _ = fs::remove_file(&source_path);
    }

    Ok(ArtifactRecordV1Alpha1 {
      api_version: ARTIFACT_API_VERSION.to_string(),
      artifact_id,
      span_id: span_id.clone(),
      event_id,
      role,
      mime_type: mime_type_for_extension(extension).to_string(),
      path: relative_path.to_string_lossy().into_owned(),
      sha256: None,
      attributes: Default::default(),
      summary,
    })
  }

  pub fn read_run(&self, run_id: &str) -> AuvResult<CanonicalRun> {
    let run_directory = self.run_dir(run_id);
    let run: RunRecordV1Alpha1 = read_json(&run_directory.join("run.json"))?;
    if run.api_version != RUN_API_VERSION {
      return Err(format!("unsupported_run_format: {}", run.api_version));
    }
    let spans: Vec<SpanRecordV1Alpha1> = read_jsonl(&run_directory.join("spans.jsonl"))?;
    let events: Vec<EventRecordV1Alpha1> = read_jsonl(&run_directory.join("events.jsonl"))?;
    let artifacts: Vec<ArtifactRecordV1Alpha1> =
      read_jsonl(&run_directory.join("artifacts.jsonl"))?;

    for span in &spans {
      if span.api_version != SPAN_API_VERSION {
        return Err(format!("invalid_run_format: {}", span.api_version));
      }
    }
    for event in &events {
      if event.api_version != EVENT_API_VERSION {
        return Err(format!("invalid_run_format: {}", event.api_version));
      }
    }
    for artifact in &artifacts {
      if artifact.api_version != ARTIFACT_API_VERSION {
        return Err(format!("invalid_run_format: {}", artifact.api_version));
      }
    }

    Ok(CanonicalRun {
      run,
      spans,
      events,
      artifacts,
    })
  }

  pub fn list_runs(&self) -> AuvResult<Vec<RunRecordV1Alpha1>> {
    let runs_root = self.root.join("runs");
    let mut runs = Vec::new();
    for entry in fs::read_dir(&runs_root)
      .map_err(|error| format!("failed to read runs root {}: {error}", runs_root.display()))?
    {
      let entry = entry.map_err(|error| format!("failed to enumerate runs: {error}"))?;
      if !entry.path().is_dir() {
        continue;
      }
      let run_path = entry.path().join("run.json");
      if !run_path.exists() {
        continue;
      }
      let run: RunRecordV1Alpha1 = read_json(&run_path)?;
      if run.api_version == RUN_API_VERSION {
        runs.push(run);
      }
    }
    runs.sort_by_key(|run| run.started_at_millis);
    Ok(runs)
  }
}
```

Also add private helpers in `src/store.rs`:

```rust
fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T, label: &str) -> AuvResult<()> {
  let tmp = path.with_extension("tmp");
  let bytes = serde_json::to_vec_pretty(value)
    .map_err(|error| format!("failed to encode {label} {}: {error}", path.display()))?;
  fs::write(&tmp, bytes)
    .map_err(|error| format!("failed to write {label} {}: {error}", tmp.display()))?;
  fs::rename(&tmp, path)
    .map_err(|error| format!("failed to publish {label} {}: {error}", path.display()))
}

fn write_jsonl_atomic<T: serde::Serialize>(path: &Path, values: &[T], label: &str) -> AuvResult<()> {
  let tmp = path.with_extension("tmp");
  let mut file = fs::File::create(&tmp)
    .map_err(|error| format!("failed to create {label} {}: {error}", tmp.display()))?;
  for value in values {
    serde_json::to_writer(&mut file, value)
      .map_err(|error| format!("failed to encode {label} {}: {error}", tmp.display()))?;
    file
      .write_all(b"\n")
      .map_err(|error| format!("failed to write {label} {}: {error}", tmp.display()))?;
  }
  drop(file);
  fs::rename(&tmp, path)
    .map_err(|error| format!("failed to publish {label} {}: {error}", path.display()))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> AuvResult<T> {
  let raw = fs::read_to_string(path)
    .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
  serde_json::from_str(&raw).map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn read_jsonl<T: serde::de::DeserializeOwned>(path: &Path) -> AuvResult<Vec<T>> {
  let raw = fs::read_to_string(path)
    .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
  let mut records = Vec::new();
  for (index, line) in raw.lines().enumerate() {
    if line.trim().is_empty() {
      continue;
    }
    let record = serde_json::from_str(line).map_err(|error| {
      format!(
        "failed to parse {} line {}: {error}",
        path.display(),
        index + 1
      )
    })?;
    records.push(record);
  }
  Ok(records)
}

fn mime_type_for_extension(extension: &str) -> &'static str {
  match extension {
    "json" => "application/json",
    "png" => "image/png",
    "txt" | "log" | "md" => "text/plain",
    _ => "application/octet-stream",
  }
}
```

- [ ] **Step 4: Run store tests and verify they pass**

Run:

```bash
cargo test store::tests
```

Expected: all store tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/store.rs
git commit -m "feat: persist canonical run store"
```

## Task 3: Dynamic Inspect Formatter

**Files:**
- Create: `src/inspect.rs`
- Modify: `src/lib.rs`
- Modify: `src/runtime.rs`
- Test: `src/inspect.rs`

- [ ] **Step 1: Write failing formatter tests**

Create `src/inspect.rs` with:

```rust
use crate::store::CanonicalRun;

pub fn render_text(_run: &CanonicalRun) -> String {
  String::new()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::store::CanonicalRun;
  use crate::trace::{
    ArtifactRecordV1Alpha1, EventRecordV1Alpha1, RunRecordV1Alpha1, RunType, SpanRecordV1Alpha1,
    TraceState, TraceStatusCode,
  };
  use std::collections::BTreeMap;

  #[test]
  fn render_text_includes_run_span_event_and_artifact() {
    let canonical = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: crate::trace::RUN_API_VERSION.to_string(),
        run_id: crate::trace::RunId::new("run_inspect"),
        trace_id: crate::trace::TraceId::new("00000000000000000000000000000001"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 100,
        finished_at_millis: Some(200),
        root_span_id: crate::trace::SpanId::new("0000000000000001"),
        attributes: BTreeMap::new(),
        summary: Some("typed text".to_string()),
        failure: None,
      },
      spans: vec![SpanRecordV1Alpha1 {
        api_version: crate::trace::SPAN_API_VERSION.to_string(),
        span_id: crate::trace::SpanId::new("0000000000000001"),
        parent_span_id: None,
        name: "auv.command".to_string(),
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 100,
        finished_at_millis: Some(200),
        attributes: BTreeMap::new(),
        summary: Some("done".to_string()),
        failure: None,
      }],
      events: vec![EventRecordV1Alpha1 {
        api_version: crate::trace::EVENT_API_VERSION.to_string(),
        event_id: crate::trace::EventId::new("event_1"),
        span_id: crate::trace::SpanId::new("0000000000000001"),
        name: "command.resolved".to_string(),
        timestamp_millis: 110,
        attributes: BTreeMap::new(),
        message: Some("resolved".to_string()),
        artifact_ids: Vec::new(),
      }],
      artifacts: vec![ArtifactRecordV1Alpha1 {
        api_version: crate::trace::ARTIFACT_API_VERSION.to_string(),
        artifact_id: crate::trace::ArtifactId::new("artifact_0001"),
        span_id: crate::trace::SpanId::new("0000000000000001"),
        event_id: None,
        role: "driver.output".to_string(),
        mime_type: "text/plain".to_string(),
        path: "artifacts/artifact_0001_output.txt".to_string(),
        sha256: None,
        attributes: BTreeMap::new(),
        summary: Some("output".to_string()),
      }],
    };

    let rendered = render_text(&canonical);

    assert!(rendered.contains("Run run_inspect"));
    assert!(rendered.contains("Type: command"));
    assert!(rendered.contains("Status: ok"));
    assert!(rendered.contains("auv.command"));
    assert!(rendered.contains("command.resolved"));
    assert!(rendered.contains("artifact_0001"));
  }
}
```

- [ ] **Step 2: Run formatter test and verify it fails**

Run:

```bash
cargo test inspect::tests::render_text_includes_run_span_event_and_artifact
```

Expected: test failure because `render_text` returns an empty string.

- [ ] **Step 3: Implement formatter**

Replace `render_text` in `src/inspect.rs` with:

```rust
use crate::store::CanonicalRun;

pub fn render_text(run: &CanonicalRun) -> String {
  let mut lines = Vec::new();
  lines.push(format!("Run {}", run.run.run_id));
  lines.push(format!("Type: {}", run.run.run_type.as_str()));
  lines.push(format!("State: {}", run.run.state.as_str()));
  lines.push(format!("Status: {}", run.run.status_code.as_str()));
  lines.push(format!("Trace: {}", run.run.trace_id));
  lines.push(format!("Started At (ms): {}", run.run.started_at_millis));
  lines.push(format!(
    "Finished At (ms): {}",
    run
      .run
      .finished_at_millis
      .map(|value| value.to_string())
      .unwrap_or_else(|| "n/a".to_string())
  ));
  if let Some(summary) = &run.run.summary {
    lines.push(format!("Summary: {summary}"));
  }
  if let Some(failure) = &run.run.failure {
    lines.push(format!("Failure: {}", failure.message));
  }

  lines.push(String::new());
  lines.push("Spans".to_string());
  for span in &run.spans {
    let parent = span
      .parent_span_id
      .as_ref()
      .map(|span_id| span_id.as_str())
      .unwrap_or("root");
    lines.push(format!(
      "  {} parent={} status={} name={}",
      span.span_id,
      parent,
      span.status_code.as_str(),
      span.name
    ));
  }

  lines.push(String::new());
  lines.push("Events".to_string());
  for event in &run.events {
    let message = event.message.as_deref().unwrap_or("");
    lines.push(format!(
      "  {} span={} name={} {}",
      event.event_id, event.span_id, event.name, message
    ));
  }

  lines.push(String::new());
  lines.push("Artifacts".to_string());
  for artifact in &run.artifacts {
    lines.push(format!(
      "  {} span={} role={} mime={} path={}",
      artifact.artifact_id, artifact.span_id, artifact.role, artifact.mime_type, artifact.path
    ));
  }

  lines.join("\n") + "\n"
}
```

- [ ] **Step 4: Export inspect module and wire runtime.inspect**

In `src/lib.rs`, add:

```rust
pub mod inspect;
```

In `src/runtime.rs`, replace `inspect` with:

```rust
pub fn inspect(&self, run_id: &str) -> AuvResult<String> {
  let run = self.store.read_run(run_id)?;
  Ok(crate::inspect::render_text(&run))
}
```

- [ ] **Step 5: Run formatter tests**

Run:

```bash
cargo test inspect::tests
```

Expected: all inspect tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/inspect.rs src/lib.rs src/runtime.rs
git commit -m "feat: render inspect output from canonical runs"
```

## Task 4: Ad-hoc Command Recording

Dependency: complete Task 5 first. This task migrates the existing
`Runtime::invoke` path onto the recording context created there.

**Files:**
- Modify: `src/runtime.rs`
- Modify: `src/model.rs`
- Test: `src/runtime.rs`

- [ ] **Step 1: Rewrite runtime tests for canonical command runs**

Update the runtime artifact failure test to read canonical files instead of `inspect.txt`:

```rust
#[test]
fn invoke_persists_failed_canonical_run_when_artifact_staging_breaks() {
  let project_root = temp_dir("runtime-tests-project");
  let store_root = temp_dir("runtime-tests-store");
  let runtime = Runtime::new(
    project_root.clone(),
    CommandCatalog::new(vec![CommandSpec {
      id: "test.invoke",
      summary: "Test invoke",
      driver_id: "test.driver",
      operation: "test_operation",
      disturbance_classes: &[crate::model::DisturbanceClass::None],
      max_disturbance: crate::model::DisturbanceClass::None,
    }]),
    DriverRegistry::new(vec![Box::new(ArtifactFailureDriver)]),
    LocalStore::new(store_root.clone()).expect("store should initialize"),
  );

  let result = runtime
    .invoke(InvokeRequest {
      command_id: "test.invoke".to_string(),
      target: ExecutionTarget::default(),
      inputs: BTreeMap::new(),
    })
    .expect("artifact staging failures should still return an inspectable run");

  assert_eq!(result.status, RunStatus::Failed);
  assert!(result.failure_message.is_some());

  let canonical = runtime
    .read_run(&result.run_id)
    .expect("canonical run should read");
  assert_eq!(canonical.run.run_type, crate::trace::RunType::Command);
  assert_eq!(canonical.run.status_code, crate::trace::TraceStatusCode::Error);
  assert!(canonical.events.iter().any(|event| event.name == "artifact.failed"));

  let inspection = runtime
    .inspect(&result.run_id)
    .expect("failed run should render");
  assert!(inspection.contains("Status: error"));
  assert!(inspection.contains("artifact.failed"));

  let run_dir = store_root.join("runs").join(&result.run_id);
  assert!(run_dir.join("run.json").exists());
  assert!(!run_dir.join("inspect.txt").exists());

  let _ = fs::remove_dir_all(project_root);
  let _ = fs::remove_dir_all(store_root);
}
```

- [ ] **Step 2: Run runtime tests and verify they fail**

Run:

```bash
cargo test runtime::tests::invoke_persists_failed_canonical_run_when_artifact_staging_breaks
```

Expected: compile failure for missing `Runtime::read_run`, `Runtime::start_run`,
`Runtime::invoke_in_span`, `Runtime::finish_run`, and old `RunRecord` usage.

- [ ] **Step 3: Add runtime read helper**

In `src/runtime.rs`, add:

```rust
pub fn read_run(&self, run_id: &str) -> AuvResult<crate::store::CanonicalRun> {
  self.store.read_run(run_id)
}
```

- [ ] **Step 4: Use the Rust-style recording API from Task 5**

Do not duplicate recording types in `src/runtime.rs`. Use the API from
`src/recording.rs`:

```rust
pub type Attributes = std::collections::BTreeMap<String, serde_json::Value>;

pub struct RunSpec {
  pub run_type: crate::trace::RunType,
  pub root_span_name: String,
  pub attributes: Attributes,
}

impl RunSpec {
  pub fn new(run_type: crate::trace::RunType, root_span_name: impl Into<String>) -> Self {
    Self {
      run_type,
      root_span_name: root_span_name.into(),
      attributes: Attributes::new(),
    }
  }

  pub fn with_attributes(mut self, attributes: Attributes) -> Self {
    self.attributes = attributes;
    self
  }
}

#[derive(Clone, Debug)]
pub struct SpanRef {
  span_id: crate::trace::SpanId,
}

impl SpanRef {
  pub fn new(span_id: crate::trace::SpanId) -> Self {
    Self { span_id }
  }

  pub fn id(&self) -> &crate::trace::SpanId {
    &self.span_id
  }
}

pub struct RunFinish {
  pub status_code: crate::trace::TraceStatusCode,
  pub summary: Option<String>,
  pub failure: Option<String>,
}
```

Runtime should expose:

```rust
pub fn start_run(&self, spec: crate::recording::RunSpec) -> AuvResult<crate::recording::RecordingRun>;

pub fn invoke_in_span(
  &self,
  run: &mut crate::recording::RecordingRun,
  parent: &crate::recording::SpanRef,
  request: InvokeRequest,
) -> AuvResult<InvokeResult>;

pub fn finish_run(
  &self,
  run: crate::recording::RecordingRun,
  finish: crate::recording::RunFinish,
) -> AuvResult<crate::trace::RunId>;
```

The ad-hoc `Runtime::invoke` wrapper should become:

```rust
pub fn invoke(&self, request: InvokeRequest) -> AuvResult<InvokeResult> {
  let mut run = self.start_run(crate::recording::RunSpec::new(
    crate::trace::RunType::Command,
    "auv.command",
  ))?;
  let root = run.root_span();
  let result = self.invoke_in_span(&mut run, &root, request)?;
  let status_code = if result.status == RunStatus::Completed {
    crate::trace::TraceStatusCode::Ok
  } else {
    crate::trace::TraceStatusCode::Error
  };
  self.finish_run(
    run,
    crate::recording::RunFinish {
      status_code,
      summary: Some(result.output_summary.clone()),
      failure: result.failure_message.clone(),
    },
  )?;
  Ok(result)
}
```

- [ ] **Step 5: Keep `RunStatus` and `InvokeResult` stable**

Do not remove `RunStatus` or `InvokeResult` from `src/model.rs` in this task. Existing CLI and skill code still use them. Remove only `EventRecord`, `ArtifactRecord`, `RunRecord`, and render methods after no files import them.

Run:

```bash
rg -n "RunRecord|EventRecord|ArtifactRecord|render_inspection|render_meta|render_inputs|render_events|render_artifacts" src
```

Expected after cleanup: no matches except old names if they are still inside removed diff context.

- [ ] **Step 6: Run runtime tests**

Run:

```bash
cargo test runtime::tests
```

Expected: runtime tests pass.

- [ ] **Step 7: Run store and inspect tests**

Run:

```bash
cargo test store::tests inspect::tests
```

Expected: store and inspect tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/runtime.rs src/model.rs src/store.rs
git commit -m "feat: record command runs in canonical format"
```

## Task 5: Workflow Recording Context

**Files:**
- Create: `src/recording.rs`
- Modify: `src/lib.rs`
- Modify: `src/runtime.rs`
- Test: `src/runtime.rs`

- [ ] **Step 1: Add failing test for child command spans inside a parent run**

Add this runtime test:

```rust
#[test]
fn invoke_in_span_adds_command_under_parent_span() {
  let project_root = temp_dir("runtime-recorded-project");
  let store_root = temp_dir("runtime-recorded-store");
  let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());

  let mut run = runtime
    .start_run(crate::recording::RunSpec::new(
      crate::trace::RunType::Execute,
      "auv.execute",
    ))
    .expect("run should start");
  let parent = run.root_span();
  let result = runtime
    .invoke_in_span(
      &mut run,
      &parent,
      InvokeRequest {
        command_id: "test.invoke".to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
      },
    )
    .expect("recorded invoke should succeed");
  let run_id = runtime
    .finish_run(
      run,
      crate::recording::RunFinish {
        status_code: crate::trace::TraceStatusCode::Ok,
        summary: Some("done".to_string()),
        failure: None,
      },
    )
    .expect("run should finish");

  let canonical = runtime.read_run(run_id.as_str()).expect("run should read");
  assert_eq!(canonical.run.run_type, crate::trace::RunType::Execute);
  assert!(canonical.spans.iter().any(|span| span.name == "auv.command.invoke"));
  assert!(canonical.spans.iter().any(|span| span.parent_span_id.as_ref() == Some(parent.id())));

  let _ = fs::remove_dir_all(project_root);
  let _ = fs::remove_dir_all(store_root);
}
```

Add a success driver helper in the test module:

```rust
struct SuccessDriver;

impl Driver for SuccessDriver {
  fn descriptor(&self) -> DriverDescriptor {
    DriverDescriptor {
      id: "test.driver",
      summary: "Test driver",
      capabilities: &["test.success"],
      donor_boundary: "test-only",
    }
  }

  fn invoke(&self, _call: &DriverCall) -> AuvResult<DriverResponse> {
    Ok(DriverResponse {
      summary: "driver ok".to_string(),
      backend: Some("test.backend".to_string()),
      notes: vec![],
      artifacts: vec![],
    })
  }
}

fn runtime_with_success_driver(project_root: PathBuf, store_root: PathBuf) -> Runtime {
  Runtime::new(
    project_root,
    CommandCatalog::new(vec![CommandSpec {
      id: "test.invoke",
      summary: "Test invoke",
      driver_id: "test.driver",
      operation: "test_operation",
      disturbance_classes: &[crate::model::DisturbanceClass::None],
      max_disturbance: crate::model::DisturbanceClass::None,
    }]),
    DriverRegistry::new(vec![Box::new(SuccessDriver)]),
    LocalStore::new(store_root).expect("store should initialize"),
  )
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
cargo test runtime::tests::invoke_in_span_adds_command_under_parent_span
```

Expected: compile failure for missing `start_run`, `invoke_in_span`, `finish_run`, `RecordingRun`, `SpanRef`, and `RecordedRun`.

- [ ] **Step 3: Introduce `RecordingRun`, `SpanRef`, `RecordedRun`, and `RunEventSink`**

Create `src/recording.rs`. Keep runtime recording state out of `src/store.rs`; the store should only persist and read snapshots.

```rust
use std::sync::{Arc, Mutex};

use crate::store::CanonicalRun;
use crate::trace::{
  ArtifactId, ArtifactRecordV1Alpha1, EventId, EventRecordV1Alpha1, RunId,
  RunRecordV1Alpha1, RunType, SpanId, SpanRecordV1Alpha1, TraceFailure,
  TraceState, TraceStatusCode,
};

pub type Attributes = std::collections::BTreeMap<String, serde_json::Value>;

pub struct RunSpec {
  pub run_type: RunType,
  pub root_span_name: String,
  pub attributes: Attributes,
}

impl RunSpec {
  pub fn new(run_type: RunType, root_span_name: impl Into<String>) -> Self {
    Self {
      run_type,
      root_span_name: root_span_name.into(),
      attributes: Attributes::new(),
    }
  }

  pub fn with_attributes(mut self, attributes: Attributes) -> Self {
    self.attributes = attributes;
    self
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpanRef {
  span_id: SpanId,
}

impl SpanRef {
  pub fn new(span_id: SpanId) -> Self {
    Self { span_id }
  }

  pub fn id(&self) -> &SpanId {
    &self.span_id
  }
}

pub struct RunFinish {
  pub status_code: TraceStatusCode,
  pub summary: Option<String>,
  pub failure: Option<String>,
}

pub struct SpanFinish {
  pub status_code: TraceStatusCode,
  pub summary: Option<String>,
  pub failure: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunStreamEvent {
  SpanStarted {
    run_id: RunId,
    span: SpanRecordV1Alpha1,
  },
  SpanFinished {
    run_id: RunId,
    span: SpanRecordV1Alpha1,
  },
  EventAppended {
    run_id: RunId,
    event: EventRecordV1Alpha1,
  },
  ArtifactCreated {
    run_id: RunId,
    artifact: ArtifactRecordV1Alpha1,
  },
  RunFinished {
    run_id: RunId,
    run: RunRecordV1Alpha1,
  },
}

impl RunStreamEvent {
  pub fn run_id(&self) -> &RunId {
    match self {
      Self::SpanStarted { run_id, .. }
      | Self::SpanFinished { run_id, .. }
      | Self::EventAppended { run_id, .. }
      | Self::ArtifactCreated { run_id, .. }
      | Self::RunFinished { run_id, .. } => run_id,
    }
  }
}

pub trait RunEventSink: Send + Sync {
  fn on_event(&self, event: RunStreamEvent);
}

#[derive(Clone)]
pub struct MemoryRunEventSink {
  events: Arc<Mutex<Vec<RunStreamEvent>>>,
}

impl MemoryRunEventSink {
  pub fn new() -> Self {
    Self {
      events: Arc::new(Mutex::new(Vec::new())),
    }
  }

  pub fn drain_for_test(&self) -> Vec<RunStreamEvent> {
    self.events.lock().map(|events| events.clone()).unwrap_or_default()
  }
}

impl RunEventSink for MemoryRunEventSink {
  fn on_event(&self, event: RunStreamEvent) {
    if let Ok(mut events) = self.events.lock() {
      events.push(event);
    }
  }
}

pub struct RecordingRun {
  run: RunRecordV1Alpha1,
  spans: Vec<SpanRecordV1Alpha1>,
  events: Vec<EventRecordV1Alpha1>,
  artifacts: Vec<ArtifactRecordV1Alpha1>,
  event_sink: Arc<dyn RunEventSink>,
}

pub struct RecordedRun {
  pub snapshot: CanonicalRun,
}

impl RecordingRun {
  pub fn new(
    run: RunRecordV1Alpha1,
    root_span: SpanRecordV1Alpha1,
    event_sink: Arc<dyn RunEventSink>,
  ) -> Self {
    event_sink.on_event(RunStreamEvent::SpanStarted {
      run_id: run.run_id.clone(),
      span: root_span.clone(),
    });
    Self {
      run,
      spans: vec![root_span],
      events: Vec::new(),
      artifacts: Vec::new(),
      event_sink,
    }
  }

  pub fn id(&self) -> &RunId {
    &self.run.run_id
  }

  pub fn root_span(&self) -> SpanRef {
    SpanRef::new(self.run.root_span_id.clone())
  }

  pub fn start_span(&mut self, parent: &SpanRef, mut span: SpanRecordV1Alpha1) -> SpanRef {
    span.parent_span_id = Some(parent.id().clone());
    let span_ref = SpanRef::new(span.span_id.clone());
    self.event_sink.on_event(RunStreamEvent::SpanStarted {
      run_id: self.run.run_id.clone(),
      span: span.clone(),
    });
    self.spans.push(span);
    span_ref
  }

  pub fn finish_span(&mut self, span: &SpanRef, finish: SpanFinish) {
    if let Some(record) = self.spans.iter_mut().find(|record| record.span_id == *span.id()) {
      if record.state == TraceState::Ended {
        return;
      }
      record.state = TraceState::Ended;
      record.status_code = finish.status_code;
      record.finished_at_millis = Some(crate::model::now_millis());
      record.summary = finish.summary;
      record.failure = finish.failure.map(|message| TraceFailure { message });
      self.event_sink.on_event(RunStreamEvent::SpanFinished {
        run_id: self.run.run_id.clone(),
        span: record.clone(),
      });
    }
  }

  pub fn record_event(&mut self, event: EventRecordV1Alpha1) -> EventId {
    let event_id = event.event_id.clone();
    self.event_sink.on_event(RunStreamEvent::EventAppended {
      run_id: self.run.run_id.clone(),
      event: event.clone(),
    });
    self.events.push(event);
    event_id
  }

  pub fn record_artifact(&mut self, artifact: ArtifactRecordV1Alpha1) -> ArtifactId {
    let artifact_id = artifact.artifact_id.clone();
    self.event_sink.on_event(RunStreamEvent::ArtifactCreated {
      run_id: self.run.run_id.clone(),
      artifact: artifact.clone(),
    });
    self.artifacts.push(artifact);
    artifact_id
  }

  pub fn finish(
    mut self,
    status_code: TraceStatusCode,
    summary: Option<String>,
    failure: Option<TraceFailure>,
  ) -> RecordedRun {
    self.run.state = TraceState::Ended;
    self.run.status_code = status_code;
    self.run.finished_at_millis = Some(crate::model::now_millis());
    self.run.summary = summary;
    self.run.failure = failure;
    for span in &mut self.spans {
      if span.state == TraceState::Running {
        span.state = TraceState::Ended;
        span.status_code = status_code;
        span.finished_at_millis = self.run.finished_at_millis;
        self.event_sink.on_event(RunStreamEvent::SpanFinished {
          run_id: self.run.run_id.clone(),
          span: span.clone(),
        });
      }
    }
    RecordedRun {
      snapshot: CanonicalRun {
        run: self.run,
        spans: self.spans,
        events: self.events,
        artifacts: self.artifacts,
      },
    }
  }
}
```

- [ ] **Step 4: Export recording module and add runtime recording methods**

In `src/lib.rs`, add:

```rust
pub mod recording;
```

In `src/runtime.rs`, add `event_sink: std::sync::Arc<dyn crate::recording::RunEventSink>` to `Runtime`.
Initialize it in `Runtime::new` with `Arc::new(crate::recording::MemoryRunEventSink::new())`.
Expose a clone and a builder-style override for same-process inspect server wiring:

```rust
pub fn event_sink(&self) -> std::sync::Arc<dyn crate::recording::RunEventSink> {
  self.event_sink.clone()
}

pub fn with_event_sink(
  mut self,
  event_sink: std::sync::Arc<dyn crate::recording::RunEventSink>,
) -> Self {
  self.event_sink = event_sink;
  self
}
```

In `src/runtime.rs`, add:

```rust
pub fn start_run(
  &self,
  spec: crate::recording::RunSpec,
) -> AuvResult<crate::recording::RecordingRun> {
  let run_id = new_run_id();
  let root_span_id = new_span_id();
  let started = now_millis();
  let run = RunRecordV1Alpha1 {
    api_version: RUN_API_VERSION.to_string(),
    run_id: run_id.clone(),
    trace_id: new_trace_id(),
    run_type: spec.run_type,
    state: TraceState::Running,
    status_code: TraceStatusCode::Unset,
    started_at_millis: started,
    finished_at_millis: None,
    root_span_id: root_span_id.clone(),
    attributes: spec.attributes.clone(),
    summary: None,
    failure: None,
  };
  let root_span = SpanRecordV1Alpha1 {
    api_version: SPAN_API_VERSION.to_string(),
    span_id: root_span_id,
    parent_span_id: None,
    name: spec.root_span_name,
    state: TraceState::Running,
    status_code: TraceStatusCode::Unset,
    started_at_millis: started,
    finished_at_millis: None,
    attributes: spec.attributes,
    summary: None,
    failure: None,
  };
  Ok(crate::recording::RecordingRun::new(
    run,
    root_span,
    self.event_sink.clone(),
  ))
}
```

Add `finish_run` in `src/runtime.rs`:

```rust
pub fn finish_run(
  &self,
  run: crate::recording::RecordingRun,
  finish: crate::recording::RunFinish,
) -> AuvResult<crate::trace::RunId> {
  let failure = finish.failure.map(|message| TraceFailure { message });
  let recorded = run.finish(finish.status_code, finish.summary, failure);
  let run_id = recorded.snapshot.run.run_id.clone();
  self.store.write_run_snapshot(&recorded.snapshot)?;
  self.event_sink.on_event(crate::recording::RunStreamEvent::RunFinished {
    run_id: run_id.clone(),
    run: recorded.snapshot.run,
  });
  Ok(run_id)
}
```

- [ ] **Step 5: Refactor command invocation body into `invoke_in_span`**

Move the command/driver invocation logic from `Runtime::invoke` into:

```rust
pub fn invoke_in_span(
  &self,
  run: &mut crate::recording::RecordingRun,
  parent: &crate::recording::SpanRef,
  request: InvokeRequest,
) -> AuvResult<InvokeResult> {
  // Resolve command, create auv.command.invoke and auv.driver.invoke child spans.
  // Run the driver.
  // Stage artifacts into run.id().
  // Push canonical events/artifacts into run so RecordingRun publishes them.
  // Finish child spans with their own status before returning.
  // End child spans.
  // Return InvokeResult using run.id().
}
```

Keep `Runtime::invoke` as the ad-hoc wrapper:

```rust
pub fn invoke(&self, request: InvokeRequest) -> AuvResult<InvokeResult> {
  let mut run = self.start_run(crate::recording::RunSpec::new(RunType::Command, "auv.command"))?;
  let root = run.root_span();
  let result = self.invoke_in_span(&mut run, &root, request)?;
  let status_code = if result.status == RunStatus::Completed {
    TraceStatusCode::Ok
  } else {
    TraceStatusCode::Error
  };
  self.finish_run(
    run,
    crate::recording::RunFinish {
      status_code,
      summary: Some(result.output_summary.clone()),
      failure: result.failure_message.clone(),
    },
  )?;
  Ok(result)
}
```

- [ ] **Step 6: Run runtime tests**

Run:

```bash
cargo test runtime::tests
```

Expected: runtime tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/recording.rs src/lib.rs src/runtime.rs
git commit -m "feat: add workflow run recording"
```

## Task 6: Recipe And Case-Matrix Aggregation

**Files:**
- Modify: `src/skill.rs`
- Test: `src/skill.rs`

- [ ] **Step 1: Add tests for one recipe run and one case-matrix run**

Add a test that runs a two-step manifest with a test runtime and asserts one `execute` run contains two `auv.recipe.step` spans. Use the existing manifest construction style in `src/skill.rs` tests. The assertion should read the run id returned by the new `run_skill_manifest_recorded` helper.

Test body to add:

```rust
#[test]
fn run_skill_manifest_records_one_execute_run() {
  let project_root = temp_dir("skill-recording-project");
  let store_root = temp_dir("skill-recording-store");
  let runtime = test_runtime(project_root.clone(), store_root.clone());
  let manifest = two_step_test_manifest();

  let run_id = run_skill_manifest_recorded(
    &runtime,
    &manifest,
    SkillRunOptions {
      dry_run: false,
      max_disturbance: None,
      overrides: BTreeMap::new(),
    },
  )
  .expect("skill should run");

  let canonical = runtime.read_run(run_id.as_str()).expect("run should read");
  assert_eq!(canonical.run.run_type, crate::trace::RunType::Execute);
  assert_eq!(
    canonical
      .spans
      .iter()
      .filter(|span| span.name == "auv.recipe.step")
      .count(),
    2
  );

  let _ = fs::remove_dir_all(project_root);
  let _ = fs::remove_dir_all(store_root);
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run:

```bash
cargo test skill::tests::run_skill_manifest_records_one_execute_run
```

Expected: compile failure because `run_skill_manifest_recorded` does not exist.

- [ ] **Step 3: Add recorded skill runner plus attach helper**

In `src/skill.rs`, add `run_skill_manifest_recorded` as the top-level convenience wrapper and `run_skill_manifest_into_run` as the reusable implementation. Case-matrix validation must call the attach helper so recipe execution is nested under the validate run instead of creating unrelated execute runs.

```rust
pub(crate) fn run_skill_manifest_recorded(
  runtime: &Runtime,
  manifest: &SkillManifest,
  options: SkillRunOptions,
) -> AuvResult<crate::trace::RunId> {
  let mut attrs = BTreeMap::new();
  attrs.insert(
    "auv.recipe.id".to_string(),
    serde_json::Value::String(manifest.recipe_id.clone()),
  );
  let mut run = runtime.start_run(
    crate::recording::RunSpec::new(crate::trace::RunType::Execute, "auv.execute")
      .with_attributes(attrs),
  )?;
  let root = run.root_span();
  run_skill_manifest_into_run(runtime, &mut run, &root, manifest, options)?;
  runtime.finish_run(
    run,
    crate::recording::RunFinish {
      status_code: crate::trace::TraceStatusCode::Ok,
      summary: Some(format!("Recipe {} completed", manifest.recipe_id)),
      failure: None,
    },
  )
}

fn run_skill_manifest_into_run(
  runtime: &Runtime,
  run: &mut crate::recording::RecordingRun,
  parent: &crate::recording::SpanRef,
  manifest: &SkillManifest,
  options: SkillRunOptions,
) -> AuvResult<()> {
  validate_skill_manifest_with_commands(manifest, runtime.list_commands())?;
  let mut variables = default_inputs(manifest)?;
  for (key, value) in options.overrides {
    variables.insert(key, value);
  }

  for (index, step) in manifest.steps.iter().enumerate() {
    let step_id = if step.id.is_empty() {
      format!("step-{}", index + 1)
    } else {
      step.id.clone()
    };
    let step_span = run.start_span(parent, crate::trace::SpanRecordV1Alpha1 {
      api_version: crate::trace::SPAN_API_VERSION.to_string(),
      span_id: crate::trace::new_span_id(),
      parent_span_id: None,
      name: "auv.recipe.step".to_string(),
      state: crate::trace::TraceState::Running,
      status_code: crate::trace::TraceStatusCode::Unset,
      started_at_millis: crate::model::now_millis(),
      finished_at_millis: None,
      attributes: BTreeMap::from([(
        "auv.recipe.step_id".to_string(),
        serde_json::Value::String(step_id.clone()),
      )]),
      summary: None,
      failure: None,
    });

    if options.dry_run {
      run.finish_span(
        &step_span,
        crate::recording::SpanFinish {
          status_code: crate::trace::TraceStatusCode::Ok,
          summary: Some("dry run".to_string()),
          failure: None,
        },
      );
      continue;
    }
    let request = build_invoke_request(step, &variables)?;
    let result = runtime.invoke_in_span(run, &step_span, request)?;
    enforce_step_expectations(&step_id, step, &result, &variables)?;
    export_step_variables(&step_id, &result, &mut variables);
    enforce_invoke_success(&result)?;
    run.finish_span(
      &step_span,
      crate::recording::SpanFinish {
        status_code: crate::trace::TraceStatusCode::Ok,
        summary: Some(format!("Step {step_id} completed")),
        failure: None,
      },
    );
  }

  Ok(())
}
```

When implementing this helper, wrap each step body so failures call
`run.finish_span(&step_span, SpanFinish { status_code: Error, ... })` before
returning the error. The snippet above shows the success path; the implementation
must not leave failed step spans running until final run close.

- [ ] **Step 4: Route product-facing `run_skill_manifest` through recorded runner**

Keep the public return type stable by changing `run_skill_manifest` to call `run_skill_manifest_recorded` and discard the run id:

```rust
pub(crate) fn run_skill_manifest(
  runtime: &Runtime,
  manifest: &SkillManifest,
  options: SkillRunOptions,
) -> AuvResult<()> {
  run_skill_manifest_recorded(runtime, manifest, options).map(|_| ())
}
```

- [ ] **Step 5: Add recorded case-matrix runner**

Create `run_skill_case_matrix_recorded` with this signature:

```rust
pub(crate) fn run_skill_case_matrix_recorded(
  runtime: &Runtime,
  manifest: &SkillManifest,
  matrix: &SkillCaseMatrix,
  options: SkillCaseRunOptions,
) -> AuvResult<crate::trace::RunId>
```

Use this structure:

```rust
let mut attrs = BTreeMap::new();
attrs.insert(
  "auv.case_matrix.skill_id".to_string(),
  serde_json::Value::String(matrix.skill_id.clone()),
);
let mut run = runtime.start_run(
  crate::recording::RunSpec::new(crate::trace::RunType::Validate, "auv.validate")
    .with_attributes(attrs),
)?;
let root = run.root_span();

for case in select_cases(matrix, &options)? {
  let case_span = run.start_span(&root, crate::trace::SpanRecordV1Alpha1 {
    api_version: crate::trace::SPAN_API_VERSION.to_string(),
    span_id: crate::trace::new_span_id(),
    parent_span_id: None,
    name: "auv.case".to_string(),
    state: crate::trace::TraceState::Running,
    status_code: crate::trace::TraceStatusCode::Unset,
    started_at_millis: crate::model::now_millis(),
    finished_at_millis: None,
    attributes: BTreeMap::from([(
      "auv.case.id".to_string(),
      serde_json::Value::String(case.case_id.clone()),
    )]),
    summary: None,
    failure: None,
  });

  let execute_span = run.start_span(&case_span, crate::trace::SpanRecordV1Alpha1 {
    api_version: crate::trace::SPAN_API_VERSION.to_string(),
    span_id: crate::trace::new_span_id(),
    parent_span_id: None,
    name: "auv.execute".to_string(),
    state: crate::trace::TraceState::Running,
    status_code: crate::trace::TraceStatusCode::Unset,
    started_at_millis: crate::model::now_millis(),
    finished_at_millis: None,
    attributes: BTreeMap::from([(
      "auv.recipe.id".to_string(),
      serde_json::Value::String(manifest.recipe_id.clone()),
    )]),
    summary: None,
    failure: None,
  });

  let execute_options = SkillRunOptions {
    dry_run: options.dry_run,
    max_disturbance: options.max_disturbance,
    overrides: case.inputs.clone(),
  };
  run_skill_manifest_into_run(runtime, &mut run, &execute_span, manifest, execute_options)?;
}

runtime.finish_run(
  run,
  crate::recording::RunFinish {
    status_code: crate::trace::TraceStatusCode::Ok,
    summary: Some(format!("Case matrix {} completed", matrix.skill_id)),
    failure: None,
  },
)
```

Keep `run_skill_case_matrix_inline` as a wrapper that calls `run_skill_case_matrix_recorded(...).map(|_| ())`.

- [ ] **Step 6: Run skill tests**

Run:

```bash
cargo test skill::tests
```

Expected: skill tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/skill.rs
git commit -m "feat: aggregate skill execution runs"
```

## Task 7: App Workflow Aggregation

**Files:**
- Modify: `src/app.rs`
- Test: `src/app.rs`

- [ ] **Step 1: Remove new writes of `inspect_path`**

Change `AppProbeStep` by removing:

```rust
pub inspect_path: PathBuf,
```

Update construction in `invoke_probe_step` so it no longer builds `.auv/runs/{run_id}/inspect.txt`.

- [ ] **Step 2: Add probe run aggregation**

Change `probe_app` so it creates one `RunType::Probe` run with root span `auv.probe`. Each existing probe step should create an `auv.probe.step` span and call `runtime.invoke_in_span`.

Preserve `AppProbe.steps[].run_id` during this transition by setting it to the parent probe `run_id` for each step. This keeps serialized probe files easy to tie back to the full run.

- [ ] **Step 3: Add analyze/distill/validate runs**

Change app workflow signatures so they can record through the shared runtime:

```rust
pub fn analyze_app_probe(runtime: &Runtime, query: &Path) -> AuvResult<AppAnalyzeOutput>

pub fn distill_app_analysis(
  runtime: &Runtime,
  query: &Path,
  output_dir: Option<PathBuf>,
) -> AuvResult<AppDistillOutput>

pub fn validate_app_distillation(runtime: &Runtime, query: &Path) -> AuvResult<AppValidateOutput>
```

Update `src/main.rs` call sites to pass `&runtime`.

Wrap each workflow in one recorder:

- `analyze_app_probe`: `RunType::Analyze`, root span `auv.analyze`.
- `distill_app_analysis`: `RunType::Distill`, root span `auv.distill`.
- `validate_app_distillation`: `RunType::Validate`, root span `auv.validate`.

Persist generated JSON and Markdown reports as artifacts by copying each output file into the run's `artifacts/` directory through `LocalStore::stage_artifact_file`. Use these roles:

```text
analysis.output
analysis.report
distillation.output
distillation.report
validation.output
validation.report
```

- [ ] **Step 4: Run app tests or compile check**

Run:

```bash
cargo test app::
```

Expected: app module tests pass if present; otherwise cargo reports no matching tests.

Run:

```bash
cargo check
```

Expected: compile succeeds.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat: aggregate app workflow runs"
```

## Task 8: Inspect Server

**Files:**
- Modify: `Cargo.toml`
- Create: `src/inspect_server.rs`
- Modify: `src/lib.rs`
- Modify: `src/cli.rs`
- Modify: `src/main.rs`
- Test: `src/inspect_server.rs`, `src/cli.rs`

- [ ] **Step 1: Add dependencies**

Modify `Cargo.toml`:

```toml
tokio = { version = "1", features = ["macros", "rt-multi-thread", "sync"] }
axum = { version = "0.8", features = ["ws"] }
tower-http = { version = "0.6", features = ["cors", "fs"] }
```

Add a broadcast-backed sink in `src/recording.rs`. It should implement the same `RunEventSink` trait as `MemoryRunEventSink`, so runtime code publishes to a sink and the inspect server decides whether that sink is memory-only or WebSocket-capable.

```rust
#[derive(Clone)]
pub struct BroadcastRunEventSink {
  sender: tokio::sync::broadcast::Sender<RunStreamEvent>,
}

impl BroadcastRunEventSink {
  pub fn new() -> Self {
    let (sender, _) = tokio::sync::broadcast::channel(256);
    Self { sender }
  }

  pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<RunStreamEvent> {
    self.sender.subscribe()
  }
}

impl RunEventSink for BroadcastRunEventSink {
  fn on_event(&self, event: RunStreamEvent) {
    let _ = self.sender.send(event);
  }
}
```

Derive `serde::Serialize` for `RunStreamEvent` so WebSocket streaming can serialize the same event values used internally.

- [ ] **Step 2: Add CLI parse test for inspect serve**

In `src/cli.rs`, add a variant:

```rust
InspectServe {
  host: String,
  port: u16,
},
```

Add parser test:

```rust
#[test]
fn inspect_serve_parses_host_and_port() {
  let parsed = parse_cli(&[
    "inspect".to_string(),
    "serve".to_string(),
    "--host".to_string(),
    "127.0.0.1".to_string(),
    "--port".to_string(),
    "9090".to_string(),
  ])
  .expect("inspect serve should parse");

  match parsed {
    CliCommand::InspectServe { host, port } => {
      assert_eq!(host, "127.0.0.1");
      assert_eq!(port, 9090);
    }
    other => panic!("unexpected command: {other:?}"),
  }
}
```

- [ ] **Step 3: Implement inspect serve parsing**

Update `parse_inspect`:

```rust
fn parse_inspect(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.get(1).map(String::as_str) == Some("serve") {
    let mut host = "127.0.0.1".to_string();
    let mut port = 8765u16;
    let mut index = 2;
    while index < arguments.len() {
      match arguments[index].as_str() {
        "--host" => {
          index += 1;
          host = arguments
            .get(index)
            .ok_or_else(|| "--host requires a value".to_string())?
            .clone();
        }
        "--port" => {
          index += 1;
          let raw = arguments
            .get(index)
            .ok_or_else(|| "--port requires a value".to_string())?;
          port = raw
            .parse::<u16>()
            .map_err(|error| format!("invalid --port {raw}: {error}"))?;
        }
        other => return Err(format!("unknown inspect serve option {other}")),
      }
      index += 1;
    }
    return Ok(CliCommand::InspectServe { host, port });
  }
  if arguments.len() != 2 {
    return Err("usage: auv-cli inspect <run-id> | auv-cli inspect serve [--host <host>] [--port <port>]".to_string());
  }
  Ok(CliCommand::Inspect {
    run_id: arguments[1].clone(),
  })
}
```

- [ ] **Step 4: Create inspect server state and router**

Create `src/inspect_server.rs`:

```rust
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Path, State, WebSocketUpgrade};
use axum::extract::ws::{Message, WebSocket};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use tower_http::cors::CorsLayer;

use crate::model::AuvResult;
use crate::recording::BroadcastRunEventSink;
use crate::store::LocalStore;

#[derive(Clone)]
pub struct InspectServerState {
  store: Arc<LocalStore>,
  event_sink: Arc<BroadcastRunEventSink>,
}

impl InspectServerState {
  pub fn new(store: LocalStore, event_sink: Arc<BroadcastRunEventSink>) -> Self {
    Self {
      store: Arc::new(store),
      event_sink,
    }
  }
}

pub fn router(state: InspectServerState) -> Router {
  Router::new()
    .route("/runs", get(list_runs))
    .route("/runs/{run_id}", get(get_run))
    .route("/runs/{run_id}/spans", get(get_spans))
    .route("/runs/{run_id}/events", get(get_events))
    .route("/runs/{run_id}/artifacts", get(get_artifacts))
    .route("/runs/{run_id}/artifacts/{artifact_id}", get(get_artifact_file))
    .route("/runs/{run_id}/stream", get(stream_run))
    .layer(CorsLayer::permissive())
    .with_state(state)
}

pub async fn serve(
  store: LocalStore,
  event_sink: Arc<BroadcastRunEventSink>,
  host: String,
  port: u16,
) -> AuvResult<()> {
  let state = InspectServerState::new(store, event_sink);
  let app = router(state);
  let address: SocketAddr = format!("{host}:{port}")
    .parse()
    .map_err(|error| format!("invalid inspect server address {host}:{port}: {error}"))?;
  let listener = tokio::net::TcpListener::bind(address)
    .await
    .map_err(|error| format!("failed to bind inspect server {address}: {error}"))?;
  axum::serve(listener, app)
    .await
    .map_err(|error| format!("inspect server failed: {error}"))
}
```

Implement handlers in the same file:

```rust
async fn list_runs(State(state): State<InspectServerState>) -> Response {
  match state.store.list_runs() {
    Ok(runs) => Json(runs).into_response(),
    Err(error) => api_error(StatusCode::INTERNAL_SERVER_ERROR, error),
  }
}

async fn get_run(State(state): State<InspectServerState>, Path(run_id): Path<String>) -> Response {
  match state.store.read_run(&run_id) {
    Ok(run) => Json(run.run).into_response(),
    Err(error) => api_error(StatusCode::NOT_FOUND, error),
  }
}

async fn get_spans(State(state): State<InspectServerState>, Path(run_id): Path<String>) -> Response {
  match state.store.read_run(&run_id) {
    Ok(run) => Json(run.spans).into_response(),
    Err(error) => api_error(StatusCode::NOT_FOUND, error),
  }
}

async fn get_events(State(state): State<InspectServerState>, Path(run_id): Path<String>) -> Response {
  match state.store.read_run(&run_id) {
    Ok(run) => Json(run.events).into_response(),
    Err(error) => api_error(StatusCode::NOT_FOUND, error),
  }
}

async fn get_artifacts(State(state): State<InspectServerState>, Path(run_id): Path<String>) -> Response {
  match state.store.read_run(&run_id) {
    Ok(run) => Json(run.artifacts).into_response(),
    Err(error) => api_error(StatusCode::NOT_FOUND, error),
  }
}
```

Implement `get_artifact_file` with this code:

```rust
async fn get_artifact_file(
  State(state): State<InspectServerState>,
  Path((run_id, artifact_id)): Path<(String, String)>,
) -> Response {
  let run = match state.store.read_run(&run_id) {
    Ok(run) => run,
    Err(error) => return api_error(StatusCode::NOT_FOUND, error),
  };
  let artifact = match run
    .artifacts
    .iter()
    .find(|artifact| artifact.artifact_id.as_str() == artifact_id)
  {
    Some(artifact) => artifact,
    None => return api_error(StatusCode::NOT_FOUND, "artifact not found".to_string()),
  };
  let path = state.store.run_dir(&run_id).join(&artifact.path);
  match std::fs::read(&path) {
    Ok(bytes) => {
      let content_type = artifact.mime_type.parse().unwrap_or(
        axum::http::HeaderValue::from_static("application/octet-stream"),
      );
      ([(axum::http::header::CONTENT_TYPE, content_type)], bytes).into_response()
    }
    Err(error) => api_error(
      StatusCode::NOT_FOUND,
      format!("failed to read artifact {}: {error}", path.display()),
    ),
  }
}
```

For `stream_run`, subscribe to `state.event_sink`. This endpoint streams same-process live messages only:

```rust
async fn stream_run(
  State(state): State<InspectServerState>,
  Path(run_id): Path<String>,
  upgrade: WebSocketUpgrade,
) -> Response {
  let receiver = state.event_sink.subscribe();
  upgrade.on_upgrade(move |socket| stream_socket(socket, receiver, run_id))
}

async fn stream_socket(
  mut socket: WebSocket,
  mut receiver: tokio::sync::broadcast::Receiver<crate::recording::RunStreamEvent>,
  run_id: String,
) {
  while let Ok(event) = receiver.recv().await {
    if event.run_id().as_str() != run_id {
      continue;
    }
    let value = match serde_json::to_string(&event) {
      Ok(value) => value,
      Err(_) => continue,
    };
    if socket
      .send(Message::Text(value.into()))
      .await
      .is_err()
    {
      break;
    }
  }
}
```

Add error helper:

```rust
fn api_error(status: StatusCode, message: String) -> Response {
  (
    status,
    Json(serde_json::json!({
      "error": message,
    })),
  )
    .into_response()
}
```

- [ ] **Step 5: Export module and wire main**

In `src/lib.rs`:

```rust
pub mod inspect_server;
```

In `src/main.rs`, change `fn main` to a tokio main:

```rust
#[tokio::main]
async fn main() {
  if let Err(error) = run().await {
    eprintln!("error: {error}");
    process::exit(1);
  }
}

async fn run() -> Result<(), String> {
  // existing body
}
```

In the match:

```rust
CliCommand::InspectServe { host, port } => {
  let event_sink = std::sync::Arc::new(auv_cli::recording::BroadcastRunEventSink::new());
  auv_cli::inspect_server::serve(
    auv_cli::build_default_store(project_root.clone())?,
    event_sink,
    host,
    port,
  )
  .await?;
}
```

The CLI `inspect serve` command is a history viewer process, so its broadcast
sink starts empty. Same-process live inspection is enabled by constructing a
`BroadcastRunEventSink`, passing a clone into `Runtime::with_event_sink`, and
passing the same sink to `inspect_server::serve` from an embedded runtime/UI
entrypoint when that entrypoint is added.

Add `build_default_store` in `src/lib.rs` next to `build_default_runtime`:

```rust
pub fn build_default_store(project_root: PathBuf) -> AuvResult<store::LocalStore> {
  store::LocalStore::new(project_root.join(".auv"))
}
```

- [ ] **Step 6: Run server and CLI tests**

Run:

```bash
cargo test cli::tests::inspect_serve_parses_host_and_port
cargo test inspect_server::
cargo check
```

Expected: tests pass and project compiles.

Add an inspect-server unit test that publishes events for two different run ids
into one `BroadcastRunEventSink` and asserts `/runs/{run_id}/stream` only emits
events whose `RunStreamEvent::run_id()` matches the requested run. This prevents
same-process live viewers from seeing unrelated runs.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/inspect_server.rs src/lib.rs src/cli.rs src/main.rs
git commit -m "feat: add read-only inspect server"
```

## Task 9: End-to-End Verification And Documentation Update

**Files:**
- Modify: `docs/ai/references/2026-05-19-trace-run-inspect-design.md`
- Modify: `docs/TERMS_AND_CONCEPTS.md` if implementation names diverged

- [ ] **Step 1: Run full validation**

Run:

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
cargo run --quiet -- list-commands
cargo run --quiet -- skill cases list
```

Expected: every command exits successfully.

Note: the former bundle-list validation command was retired on 2026-06-11.

- [ ] **Step 2: Verify new ad-hoc run layout manually**

Run the read-only display listing command:

```bash
cargo run --quiet -- invoke debug.listDisplays
```

Then inspect the generated run directory:

```bash
find .auv/runs -maxdepth 2 -name run.json -print | tail -1
find .auv/runs -maxdepth 2 -name inspect.txt -print
```

Expected: newest run has `run.json`; there are no new `inspect.txt` files from the migrated code path.

- [ ] **Step 3: Verify inspect command renders dynamically**

Use the newest run id:

```bash
cargo run --quiet -- inspect <run-id>
```

Expected: output includes `Run <run-id>`, `Type:`, `Status:`, `Spans`, `Events`, and `Artifacts`.

- [ ] **Step 4: Verify inspect server history endpoints**

Start the server:

```bash
cargo run --quiet -- inspect serve --host 127.0.0.1 --port 8765
```

From another shell:

```bash
curl -sS http://127.0.0.1:8765/runs
curl -sS http://127.0.0.1:8765/runs/<run-id>
curl -sS http://127.0.0.1:8765/runs/<run-id>/spans
curl -sS http://127.0.0.1:8765/runs/<run-id>/events
curl -sS http://127.0.0.1:8765/runs/<run-id>/artifacts
```

Expected: each endpoint returns JSON and no endpoint exposes checkpoints or replay.

- [ ] **Step 5: Update reference docs if names changed**

If implementation uses names different from the design, update
`docs/ai/references/2026-05-19-trace-run-inspect-design.md` to match. Keep
`docs/TERMS_AND_CONCEPTS.md` aligned with implemented terms.

- [ ] **Step 6: Commit verification doc updates**

```bash
git add docs/ai/references/2026-05-19-trace-run-inspect-design.md docs/TERMS_AND_CONCEPTS.md
git commit -m "docs: align trace inspect references"
```

If neither file changed, skip this commit and record the verification commands in the final implementation response.
