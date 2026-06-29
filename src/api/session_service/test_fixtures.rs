//! Shared hermetic fixtures for `session_service` unit tests.
//!
//! Centralizes run/artifact staging so operation-result and operation-summary
//! shape changes touch one place. Callers: `summary`, `summary_store`,
//! `handler`, `transport`, and `client_smoke` test modules.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use auv_cli_invoke::{InvokeResult, OperationSummary, OperationSummarySource, RunStatus};
use auv_tracing_driver::artifact::ArtifactFileSource;
use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_tracing_driver::trace::{
  ArtifactRecordV1Alpha1, RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SPAN_API_VERSION,
  SpanId, SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
};
use serde::Serialize;

use crate::contract::{
  OPERATION_RESULT_API_VERSION, OperationOutput, OperationResult, OperationStatus,
};
use crate::model::now_millis;

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Temp store root plus an open [`LocalStore`].
pub struct SessionRunFixture {
  pub root: PathBuf,
  pub store: LocalStore,
}

impl SessionRunFixture {
  pub fn cleanup(self) {
    let _ = fs::remove_dir_all(self.root);
  }
}

pub fn unique_temp_dir(label: &str) -> PathBuf {
  let unique = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
  let path = std::env::temp_dir().join(format!("auv-{label}-{}-{unique}", now_millis()));
  let _ = fs::remove_dir_all(&path);
  fs::create_dir_all(&path).expect("temp dir should be creatable");
  path
}

/// Temp store root for session API gRPC tests (`transport`, `client_smoke`).
pub fn session_api_temp_store_root(label: &str) -> PathBuf {
  unique_temp_dir(&format!("session-api-{label}"))
}

pub fn dummy_run(run_id: &str) -> RunRecordV1Alpha1 {
  let root_span_id = SpanId::new("0000000000000001");
  RunRecordV1Alpha1 {
    api_version: RUN_API_VERSION.to_string(),
    run_id: RunId::new(run_id),
    trace_id: TraceId::new("00000000000000000000000000000001"),
    run_type: RunType::Execute,
    state: TraceState::Ended,
    status_code: TraceStatusCode::Ok,
    started_at_millis: 100,
    finished_at_millis: Some(200),
    root_span_id,
    attributes: Default::default(),
    summary: Some("done".to_string()),
    failure: None,
  }
}

pub fn dummy_command_span(span_id: &SpanId) -> SpanRecordV1Alpha1 {
  SpanRecordV1Alpha1 {
    api_version: SPAN_API_VERSION.to_string(),
    span_id: span_id.clone(),
    parent_span_id: None,
    name: "auv.command".to_string(),
    state: TraceState::Ended,
    status_code: TraceStatusCode::Ok,
    started_at_millis: 100,
    finished_at_millis: Some(200),
    attributes: Default::default(),
    summary: None,
    failure: None,
  }
}

pub fn dummy_read_span(span_id: &SpanId) -> SpanRecordV1Alpha1 {
  SpanRecordV1Alpha1 {
    api_version: SPAN_API_VERSION.to_string(),
    span_id: span_id.clone(),
    parent_span_id: None,
    name: "auv.run.read".to_string(),
    state: TraceState::Ended,
    status_code: TraceStatusCode::Ok,
    started_at_millis: 100,
    finished_at_millis: Some(200),
    attributes: Default::default(),
    summary: None,
    failure: None,
  }
}

pub fn stage_json_artifact<T: Serialize>(
  store: &LocalStore,
  root: &Path,
  run_id: &RunId,
  span_id: &SpanId,
  index: usize,
  role: &str,
  preferred_name: &str,
  value: &T,
) -> ArtifactRecordV1Alpha1 {
  let source_path = root.join(format!("source-{index}-{preferred_name}"));
  let rendered =
    serde_json::to_string_pretty(value).expect("artifact json should serialize") + "\n";
  fs::write(&source_path, rendered).expect("artifact source should write");
  store
    .stage_artifact_file(
      run_id,
      index,
      span_id,
      None,
      ArtifactFileSource {
        role: role.to_string(),
        source_path,
        preferred_name: preferred_name.to_string(),
        summary: None,
      },
    )
    .expect("artifact should stage")
}

pub fn music_search_operation(run_id: &str) -> OperationResult {
  OperationResult {
    api_version: OPERATION_RESULT_API_VERSION.to_string(),
    run_id: RunId::new(run_id),
    status: OperationStatus::Completed,
    operation_id: "music.search.results".to_string(),
    evidence_artifacts: Vec::new(),
    output: OperationOutput::Acknowledged { message: None },
    verifications: Vec::new(),
    freshness_basis: None,
    known_limits: vec!["semantic_shaping_synthetic".to_string()],
  }
}

pub fn fixture_observe_operation(run_id: &str) -> OperationResult {
  OperationResult {
    api_version: OPERATION_RESULT_API_VERSION.to_string(),
    run_id: RunId::new(run_id),
    status: OperationStatus::Completed,
    operation_id: "fixture.observe".to_string(),
    evidence_artifacts: Vec::new(),
    output: OperationOutput::Acknowledged { message: None },
    verifications: Vec::new(),
    freshness_basis: None,
    known_limits: Vec::new(),
  }
}

pub fn music_runtime_summary(run_id: &str) -> OperationSummary {
  let mut signals = std::collections::BTreeMap::new();
  signals.insert("now_playing".to_string(), "track-x".to_string());
  OperationSummary::capture(
    &InvokeResult {
      run_id: run_id.to_string(),
      producer_span_id: SpanId::new("0000000000000001"),
      status: RunStatus::Completed,
      output_summary: "did the thing".to_string(),
      signals,
      artifacts: Vec::new(),
      artifact_paths: Vec::new(),
      failure_message: None,
    },
    "music.search",
  )
}

pub fn fixture_observe_invoke_result(run_id: &str) -> InvokeResult {
  let mut signals = std::collections::BTreeMap::new();
  signals.insert(
    "fixture.observe".to_string(),
    "records deterministic fixture output only.".to_string(),
  );
  InvokeResult {
    run_id: run_id.to_string(),
    producer_span_id: SpanId::new("0000000000000001"),
    status: RunStatus::Completed,
    output_summary: "fixture observed".to_string(),
    signals,
    artifacts: Vec::new(),
    artifact_paths: Vec::new(),
    failure_message: None,
  }
}

pub fn invoke_result_matching_summary(run_id: &str, summary: &OperationSummary) -> InvokeResult {
  InvokeResult {
    run_id: run_id.to_string(),
    producer_span_id: SpanId::new("0000000000000001"),
    status: summary.status(),
    output_summary: summary.output_summary().to_string(),
    signals: summary.signals().clone(),
    artifacts: Vec::new(),
    artifact_paths: Vec::new(),
    failure_message: summary.failure_message().map(str::to_string),
  }
}

pub fn write_minimal_run(store: &LocalStore, run_id: &str) {
  let run = dummy_run(run_id);
  let span = dummy_command_span(&run.root_span_id);
  store
    .write_run_snapshot(&CanonicalRun {
      run,
      spans: vec![span],
      events: Vec::new(),
      artifacts: Vec::new(),
    })
    .expect("run snapshot should persist");
}

pub fn persist_operation_result_on_store(
  store: &LocalStore,
  root: &Path,
  run_id: &str,
  operation: &OperationResult,
) {
  let run = dummy_run(run_id);
  let span = dummy_read_span(&run.root_span_id);
  let artifact = stage_json_artifact(
    store,
    root,
    &run.run_id,
    &span.span_id,
    0,
    "operation-result",
    "operation-result.json",
    operation,
  );
  store
    .write_run_snapshot(&CanonicalRun {
      run,
      spans: vec![span],
      events: Vec::new(),
      artifacts: vec![artifact],
    })
    .expect("run snapshot should persist");
}

pub fn append_operation_result_artifact(
  store: &LocalStore,
  root: &Path,
  run_id: &str,
  operation: &OperationResult,
) {
  let mut canonical = store.read_run(run_id).expect("run should exist");
  let span_id = canonical.run.root_span_id.clone();
  let artifact = stage_json_artifact(
    store,
    root,
    &canonical.run.run_id,
    &span_id,
    canonical.artifacts.len(),
    "operation-result",
    "operation-result.json",
    operation,
  );
  canonical.artifacts.push(artifact);
  store
    .replace_run_snapshot(&canonical)
    .expect("run snapshot should update");
}

pub fn persist_operation_result_run(
  label: &str,
  run_id: &str,
  operation: &OperationResult,
) -> SessionRunFixture {
  let root = unique_temp_dir(label);
  let store = LocalStore::new(root.clone()).expect("store should initialize");
  persist_operation_result_on_store(&store, &root, run_id, operation);
  SessionRunFixture { root, store }
}

/// Store with a `music.search` domain `operation-result` staged on disk.
pub fn music_search_operation_result_fixture(label: &str, run_id: &str) -> SessionRunFixture {
  persist_operation_result_run(label, run_id, &music_search_operation(run_id))
}

pub fn persist_operation_result_and_summary_run(
  label: &str,
  run_id: &str,
  operation: &OperationResult,
  summary: &OperationSummary,
) -> SessionRunFixture {
  let fixture = persist_operation_result_run(label, run_id, operation);
  let result = invoke_result_matching_summary(run_id, summary);
  crate::api::session_service::summary_store::persist_operation_summary(
    &fixture.store,
    &result,
    summary,
  )
  .expect("summary artifact should persist");
  fixture
}
