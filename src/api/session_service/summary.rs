//! Two-source operation summary read path and join policy (API-P7/P12).
//!
//! API-P3 showed `GetOperation` is a two-source projection:
//! - `OperationResult` (persisted) owns `operation_id`, `status`,
//!   `known_limits`, and evidence artifact refs.
//! - the `InvokeResult`-sourced summary (via `OperationSummarySource`) owns
//!   `output_summary`, `signals`, and `failure_message`.
//!
//! This module joins them explicitly. Per API-P4 (`GetOperation` flow), the
//! persisted record is the required skeleton and the runtime summary is layered
//! on when available. When the runtime summary is absent, the join records it as
//! `None` rather than fabricating empty strings as authoritative data (API-P4:
//! "It must not silently fabricate empty strings").
//!
//! ## Runtime summary resolution
//!
//! [`load_joined_operation_summary`] picks the InvokeResult-sourced half in this
//! order (see also API-P11 handoff in
//! `docs/ai/references/2026-06-30-auv-api-p11-summary-durability-handoff.md`):
//!
//! 1. `process_local_runtime_override` — same-process cache hit from
//!    [`SessionApiHandler`](super::handler::SessionApiHandler) (API-P6).
//! 2. Persisted `operation-summary` artifact on the run (API-P11, store read).
//! 3. `None` — join leaves `runtime` absent; callers must not treat that as empty output.

use std::collections::BTreeMap;

use auv_cli_invoke::{OperationSummary, OperationSummarySource, RunStatus};
use auv_tracing_driver::store::LocalStore;

use crate::contract::{ArtifactRef, OperationResult, OperationStatus};
use crate::model::AuvResult;
use crate::run_read;

/// Known limit when wire `command_id` cannot be resolved (API-P12).
pub const COMMAND_ID_UNAVAILABLE_KNOWN_LIMIT: &str = "auv.api.session.command_id_unavailable";

/// Known limit when an evidence artifact has no catalog role entry (API-P12).
pub const ARTIFACT_ROLE_UNAVAILABLE_KNOWN_LIMIT: &str = "auv.api.session.artifact_role_unavailable";

/// Outcome of loading and joining a `GetOperation` summary for one run.
#[derive(Clone, Debug, PartialEq)]
pub enum JoinedOperationSummaryLoad {
  /// Persisted skeleton found and joined with the supplied runtime source.
  Found(JoinedOperationSummary),
  /// The run directory does not exist in the store.
  RunNotFound,
  /// The run exists but recorded no `operation-result` JSON artifact.
  NoPersistedOperationResult,
}

/// The `InvokeResult`-sourced half of the summary projection, captured as owned
/// data for one operation. This is a read-side view only, not durable session state.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeOperationSummary {
  pub output_summary: String,
  pub signals: BTreeMap<String, String>,
  pub failure_message: Option<String>,
}

impl RuntimeOperationSummary {
  fn from_source(source: &dyn OperationSummarySource) -> Self {
    Self {
      output_summary: source.output_summary().to_string(),
      signals: source.signals().clone(),
      failure_message: source.failure_message().map(str::to_string),
    }
  }
}

/// Explicit two-source join of a `GetOperation` summary view.
///
/// Projection only: durability lives in store artifacts, not in this struct.
/// `runtime` is `None` when no `InvokeResult`-sourced summary was available for
/// the run (for example, not cached and no store artifact). Callers must treat
/// that as "runtime summary unknown", not as empty output.
#[derive(Clone, Debug, PartialEq)]
pub struct JoinedOperationSummary {
  // OperationResult-sourced (persisted, required skeleton).
  pub run_id: String,
  /// Internal domain label from `OperationResult.operation_id` (not API wire).
  pub domain_operation_id: String,
  /// Invoke `command_id` for proto `OperationRef.operation_id` (API-P12).
  pub command_id: Option<String>,
  pub status: OperationStatus,
  pub known_limits: Vec<String>,
  pub artifacts: Vec<ArtifactRef>,
  /// Run artifact catalog `artifact_id` → `role` (API-P12).
  pub artifact_roles: BTreeMap<String, String>,
  // InvokeResult-sourced (runtime return value, may be absent).
  pub runtime: Option<RuntimeOperationSummary>,
}

fn runtime_status_matches_persisted(persisted: OperationStatus, runtime: RunStatus) -> bool {
  matches!((persisted, runtime), (OperationStatus::Completed, RunStatus::Completed) | (OperationStatus::Failed, RunStatus::Failed))
}

/// Join a persisted `OperationResult` with an optional runtime summary source.
///
/// Pure join policy: the persisted record provides the required fields; the
/// runtime summary is layered on when present, otherwise `runtime` is `None`.
/// When both halves are present but disagree on completion status, a
/// `auv.api.session.runtime_status_mismatch` known_limit is appended so callers
/// do not silently return contradictory status and failure_message fields.
pub fn join_operation_summary(
  operation: &OperationResult,
  runtime: Option<&dyn OperationSummarySource>,
  command_id: Option<String>,
  artifact_roles: BTreeMap<String, String>,
) -> JoinedOperationSummary {
  let mut known_limits = operation.known_limits.clone();
  if command_id.is_none() {
    known_limits.push(COMMAND_ID_UNAVAILABLE_KNOWN_LIMIT.to_string());
  }
  if let Some(source) = runtime {
    if !runtime_status_matches_persisted(operation.status, source.status()) {
      known_limits.push("auv.api.session.runtime_status_mismatch".to_string());
    }
  }
  JoinedOperationSummary {
    run_id: operation.run_id.as_str().to_string(),
    domain_operation_id: operation.operation_id.clone(),
    command_id,
    status: operation.status,
    known_limits,
    artifacts: operation.evidence_artifacts.clone(),
    artifact_roles,
    runtime: runtime.map(RuntimeOperationSummary::from_source),
  }
}

fn resolve_wire_command_id(
  store: &LocalStore,
  run_id: &str,
  local_summary: Option<&OperationSummary>,
  stored_summary: Option<&OperationSummary>,
) -> AuvResult<Option<String>> {
  for summary in [local_summary, stored_summary].into_iter().flatten() {
    if !summary.command_id().is_empty() {
      return Ok(Some(summary.command_id().to_string()));
    }
  }
  read_command_id_from_invoke_span(store, run_id)
}

fn read_command_id_from_invoke_span(store: &LocalStore, run_id: &str) -> AuvResult<Option<String>> {
  let run = store.read_run(run_id)?;
  for span in &run.spans {
    if span.name != "auv.command.invoke" {
      continue;
    }
    let Some(value) = span.attributes.get("auv.command.id") else {
      continue;
    };
    let Some(command_id) = value.as_str() else {
      continue;
    };
    if !command_id.is_empty() {
      return Ok(Some(command_id.to_string()));
    }
  }
  Ok(None)
}

fn artifact_role_catalog(store: &LocalStore, run_id: &str) -> AuvResult<BTreeMap<String, String>> {
  let run = store.read_run(run_id)?;
  let mut catalog = BTreeMap::new();
  for artifact in &run.artifacts {
    catalog.insert(artifact.artifact_id.as_str().to_string(), artifact.role.clone());
  }
  Ok(catalog)
}

/// Load and join the `GetOperation` summary for a run.
///
/// Reads the persisted `OperationResult` (storage-side read path via
/// [`run_read::read_operation_result`]) and joins it with the runtime summary
/// source. When `process_local_runtime_override` is absent, falls back to the
/// persisted `operation-summary` artifact (API-P11). Distinguishes a missing run
/// from a run that exists but recorded no `OperationResult`.
pub fn load_joined_operation_summary(
  store: &LocalStore,
  run_id: &str,
  process_local_runtime_override: Option<&OperationSummary>,
) -> AuvResult<JoinedOperationSummaryLoad> {
  let run_dir = store.run_dir(run_id)?;
  if !run_dir.join("run.json").exists() {
    return Ok(JoinedOperationSummaryLoad::RunNotFound);
  }
  let Some(operation) = run_read::read_operation_result(store, run_id)? else {
    return Ok(JoinedOperationSummaryLoad::NoPersistedOperationResult);
  };
  let stored_summary = if process_local_runtime_override.is_none() {
    run_read::read_operation_summary(store, run_id)?
  } else {
    None
  };
  let runtime: Option<&dyn OperationSummarySource> = match process_local_runtime_override {
    Some(summary) => Some(summary as &dyn OperationSummarySource),
    None => stored_summary.as_ref().map(|summary| summary as &dyn OperationSummarySource),
  };
  let command_id = resolve_wire_command_id(store, run_id, process_local_runtime_override, stored_summary.as_ref())?;
  let artifact_roles = artifact_role_catalog(store, run_id)?;
  Ok(JoinedOperationSummaryLoad::Found(join_operation_summary(&operation, runtime, command_id, artifact_roles)))
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::fs;

  use auv_cli_invoke::{InvokeResult, OperationSummary, OperationSummaryRecord, OperationSummarySource, RunStatus};
  use auv_tracing_driver::store::LocalStore;
  use auv_tracing_driver::trace::SpanId;

  use super::{JoinedOperationSummary, JoinedOperationSummaryLoad, join_operation_summary, load_joined_operation_summary};
  use crate::api::session_service::test_fixtures::{
    SessionRunFixture, music_runtime_summary, music_search_operation, persist_operation_result_and_summary_run,
    persist_operation_result_run, unique_temp_dir, write_minimal_run,
  };
  use crate::contract::{OPERATION_SUMMARY_API_VERSION, OperationStatus};

  fn sample_operation(run_id: &str) -> crate::contract::OperationResult {
    music_search_operation(run_id)
  }

  fn runtime_summary(run_id: &str) -> OperationSummary {
    music_runtime_summary(run_id)
  }

  fn join_args(command_id: &str) -> (Option<String>, BTreeMap<String, String>) {
    (Some(command_id.to_string()), BTreeMap::new())
  }

  #[test]
  fn join_includes_runtime_summary_when_present() {
    let operation = sample_operation("run-join");
    let summary = runtime_summary("run-join");
    let (command_id, roles) = join_args("music.search");

    let joined = join_operation_summary(&operation, Some(&summary), command_id, roles);

    assert_eq!(joined.run_id, "run-join");
    assert_eq!(joined.domain_operation_id, "music.search.results");
    assert_eq!(joined.command_id.as_deref(), Some("music.search"));
    assert_eq!(joined.status, OperationStatus::Completed);
    assert_eq!(joined.known_limits, vec!["semantic_shaping_synthetic"]);
    let runtime = joined.runtime.expect("runtime summary should be present");
    assert_eq!(runtime.output_summary, "did the thing");
    assert_eq!(runtime.signals.get("now_playing").map(String::as_str), Some("track-x"));
    assert_eq!(runtime.failure_message, None);
  }

  #[test]
  fn join_marks_runtime_absent_without_fabricating() {
    let operation = sample_operation("run-join-missing");
    let (command_id, roles) = join_args("music.search");

    let joined = join_operation_summary(&operation, None, command_id, roles);

    assert_eq!(joined.domain_operation_id, "music.search.results");
    assert_eq!(joined.known_limits, vec!["semantic_shaping_synthetic".to_string(),]);
    // Runtime summary explicitly absent, not fabricated as empty strings.
    assert!(joined.runtime.is_none());
  }

  #[test]
  fn join_preserves_persisted_known_limits_and_status_on_failure() {
    let mut operation = sample_operation("run-join-failed");
    operation.status = OperationStatus::Failed;
    operation.known_limits = vec!["dispatch_failed".to_string()];

    let joined = join_operation_summary(&operation, None, None, BTreeMap::new());

    assert_eq!(joined.status, OperationStatus::Failed);
    assert!(joined.known_limits.iter().any(|limit| { limit == "dispatch_failed" || limit == super::COMMAND_ID_UNAVAILABLE_KNOWN_LIMIT }));
  }

  #[test]
  fn join_flags_runtime_status_mismatch() {
    let operation = sample_operation("run-mismatch");
    let summary = OperationSummary::capture(&InvokeResult {
      run_id: "run-mismatch".to_string(),
      producer_span_id: SpanId::new("0000000000000001"),
      command_id: "fixture.observe".to_string(),
      command_summary: "Observe fixture.".to_string(),
      status: RunStatus::Failed,
      output_summary: "runtime failed".to_string(),
      backend: None,
      signals: BTreeMap::new(),
      notes: Vec::new(),
      known_limits: Vec::new(),
      verification: None,
      report: None,
      artifacts: Vec::new(),
      artifact_paths: Vec::new(),
      failure_message: Some("boom".to_string()),
    });
    let (command_id, roles) = join_args("fixture.observe");

    let joined = join_operation_summary(&operation, Some(&summary), command_id, roles);

    assert_eq!(joined.status, OperationStatus::Completed);
    assert!(joined.known_limits.iter().any(|limit| limit == "auv.api.session.runtime_status_mismatch"));
    let runtime = joined.runtime.expect("runtime summary should be present");
    assert_eq!(runtime.failure_message.as_deref(), Some("boom"));
  }

  #[test]
  fn load_joined_operation_summary_returns_run_not_found_for_missing_run() {
    let root = unique_temp_dir("session-summary-missing-run");
    let store = LocalStore::new(root.clone()).expect("store should initialize");

    let loaded = load_joined_operation_summary(&store, "missing-run", None).expect("load should succeed");
    assert_eq!(loaded, JoinedOperationSummaryLoad::RunNotFound);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn load_joined_operation_summary_returns_no_persisted_result_when_run_lacks_artifact() {
    let root = unique_temp_dir("session-summary-no-op-result");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    write_minimal_run(&store, "run-no-op-result");

    let loaded = load_joined_operation_summary(&store, "run-no-op-result", None).expect("load should succeed");
    assert_eq!(loaded, JoinedOperationSummaryLoad::NoPersistedOperationResult);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn load_joined_operation_summary_joins_persisted_and_runtime_halves() {
    let operation = sample_operation("run-happy");
    let SessionRunFixture { root, store } = persist_operation_result_run("session-summary-load", "run-happy", &operation);
    let summary = runtime_summary("run-happy");

    let loaded = load_joined_operation_summary(&store, "run-happy", Some(&summary)).expect("load should succeed");
    let JoinedOperationSummaryLoad::Found(joined) = loaded else {
      panic!("expected joined summary, got {loaded:?}");
    };

    assert_eq!(joined.run_id, "run-happy");
    assert_eq!(joined.domain_operation_id, "music.search.results");
    assert_eq!(joined.status, OperationStatus::Completed);
    let runtime = joined.runtime.expect("runtime summary should be present");
    assert_eq!(runtime.output_summary, "did the thing");
    assert_eq!(runtime.signals.get("now_playing").map(String::as_str), Some("track-x"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn load_joined_operation_summary_loads_persisted_runtime_when_cache_absent() {
    let operation = sample_operation("run-stored-runtime");
    let summary = runtime_summary("run-stored-runtime");
    let SessionRunFixture { root, store } =
      persist_operation_result_and_summary_run("session-summary-stored-runtime", "run-stored-runtime", &operation, &summary);

    let loaded = load_joined_operation_summary(&store, "run-stored-runtime", None).expect("load should succeed");
    let JoinedOperationSummaryLoad::Found(joined) = loaded else {
      panic!("expected joined summary, got {loaded:?}");
    };

    let runtime = joined.runtime.expect("runtime summary should be present");
    assert_eq!(runtime.output_summary, "did the thing");
    assert_eq!(runtime.signals.get("now_playing").map(String::as_str), Some("track-x"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn load_joined_operation_summary_prefers_process_local_override_over_stored_summary() {
    let operation = sample_operation("run-override");
    let stored_summary = runtime_summary("run-override");
    let SessionRunFixture { root, store } =
      persist_operation_result_and_summary_run("session-summary-override", "run-override", &operation, &stored_summary);
    let override_summary = OperationSummary::capture(&InvokeResult {
      run_id: "run-override".to_string(),
      producer_span_id: SpanId::new("0000000000000001"),
      command_id: "fixture.observe".to_string(),
      command_summary: "Observe fixture.".to_string(),
      status: RunStatus::Completed,
      output_summary: "process-local override wins".to_string(),
      backend: None,
      signals: BTreeMap::new(),
      notes: Vec::new(),
      known_limits: Vec::new(),
      verification: None,
      report: None,
      artifacts: Vec::new(),
      artifact_paths: Vec::new(),
      failure_message: None,
    });

    let loaded = load_joined_operation_summary(&store, "run-override", Some(&override_summary)).expect("load should succeed");
    let JoinedOperationSummaryLoad::Found(joined) = loaded else {
      panic!("expected joined summary, got {loaded:?}");
    };

    let runtime = joined.runtime.expect("runtime summary should be present");
    assert_eq!(runtime.output_summary, "process-local override wins");

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn operation_summary_record_deserializes_without_api_version_field() {
    let json = r#"{
      "run_id": "run-legacy",
      "status": "completed",
      "output_summary": "legacy",
      "signals": {},
      "failure_message": null
    }"#;
    let record: OperationSummaryRecord = serde_json::from_str(json).expect("deserialize");
    assert_eq!(record.api_version, OPERATION_SUMMARY_API_VERSION);
    assert_eq!(record.run_id, "run-legacy");
    assert_eq!(record.output_summary, "legacy");
    let restored = OperationSummary::from_record(record);
    assert_eq!(restored.output_summary(), "legacy");
  }

  fn _assert_send_sync<T: Send + Sync>() {}
  #[test]
  fn joined_summary_is_send_sync() {
    _assert_send_sync::<JoinedOperationSummary>();
  }
}
