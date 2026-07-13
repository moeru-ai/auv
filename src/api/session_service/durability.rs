//! Invoke durability policy for the session API.
//!
//! A completed invoke writes two independent read-side artifacts in order:
//! `operation-summary`, then synthetic `operation-result`. Failure of either
//! write does not skip the other and does not turn an already executed command
//! into an RPC error. Instead, [`InvokeDurabilityOutcome`] reports the precise
//! durability gaps for `InvokeResponse.known_limits`.
//!
//! The process-local summary cache is safe to populate only when both artifacts
//! persisted. This module owns that gate so write order, partial success, and
//! known-limit reporting cannot drift across call sites.

use serde::Serialize;

use auv_cli_invoke::{InvokeResult, OperationSummary, RunStatus};
use auv_tracing_driver::artifact::ArtifactBytesSource;
use auv_tracing_driver::store::LocalStore;
use auv_tracing_driver::trace::RunId;

use super::SessionApiError;
use crate::contract::{
  ArtifactRef, OPERATION_RESULT_API_VERSION, OPERATION_SUMMARY_API_VERSION, OPERATION_SUMMARY_ARTIFACT_ROLE, OperationOutput,
  OperationResult, OperationStatus,
};

/// Honesty marker on persisted session-invoke synthetic `OperationResult` records.
pub(crate) const INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT: &str = "auv.api.session.invoke_synthetic_operation_result";

/// Known limit when the operation-summary artifact could not be persisted.
pub(crate) const OPERATION_SUMMARY_PERSIST_FAILED_KNOWN_LIMIT: &str = "auv.api.session.operation_summary_persist_failed";

/// Known limit when the operation-result artifact could not be persisted.
pub(crate) const OPERATION_RESULT_PERSIST_FAILED_KNOWN_LIMIT: &str = "auv.api.session.operation_result_persist_failed";

const OPERATION_RESULT_ARTIFACT_ROLE: &str = "operation-result";

/// Result of both post-invoke durability writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InvokeDurabilityOutcome {
  summary_persisted: bool,
  operation_result_persisted: bool,
}

impl InvokeDurabilityOutcome {
  /// Both read-side artifacts are durable, so a process-local cache entry may
  /// safely override the stored summary on the same handler instance.
  pub(crate) fn cache_allowed(self) -> bool {
    self.summary_persisted && self.operation_result_persisted
  }

  /// Durability-only limits in write order.
  pub(crate) fn known_limits(self) -> Vec<&'static str> {
    let mut limits = Vec::with_capacity(2);
    if !self.summary_persisted {
      limits.push(OPERATION_SUMMARY_PERSIST_FAILED_KNOWN_LIMIT);
    }
    if !self.operation_result_persisted {
      limits.push(OPERATION_RESULT_PERSIST_FAILED_KNOWN_LIMIT);
    }
    limits
  }
}

fn persist_json_artifact<T: Serialize>(
  store: &LocalStore,
  result: &InvokeResult,
  role: &str,
  preferred_name: &str,
  value: &T,
) -> Result<(), SessionApiError> {
  let run_id = result.run_id.as_str();
  let mut canonical = store.read_run(run_id).map_err(SessionApiError::Storage)?;
  let rendered = serde_json::to_string_pretty(value).map_err(|error| SessionApiError::Storage(error.to_string()))? + "\n";
  let artifact = store
    .stage_artifact_bytes(
      &RunId::new(run_id),
      canonical.artifacts.len(),
      &result.producer_span_id,
      None,
      ArtifactBytesSource {
        role: role.to_string(),
        bytes: rendered.into_bytes(),
        preferred_name: preferred_name.to_string(),
        summary: None,
      },
    )
    .map_err(SessionApiError::Storage)?;
  canonical.artifacts.push(artifact);
  store.replace_run_snapshot(&canonical).map_err(SessionApiError::Storage)
}

fn persist_operation_summary(store: &LocalStore, result: &InvokeResult, summary: &OperationSummary) -> Result<(), SessionApiError> {
  let record = summary.to_record(OPERATION_SUMMARY_API_VERSION);
  persist_json_artifact(store, result, OPERATION_SUMMARY_ARTIFACT_ROLE, "operation-summary.json", &record)
}

#[cfg(test)]
pub(crate) fn persist_operation_summary_fixture(
  store: &LocalStore,
  result: &InvokeResult,
  summary: &OperationSummary,
) -> Result<(), SessionApiError> {
  persist_operation_summary(store, result, summary)
}

fn synthetic_operation_result(command_id: &str, result: &InvokeResult) -> OperationResult {
  let status = match result.status {
    RunStatus::Completed => OperationStatus::Completed,
    RunStatus::Failed => OperationStatus::Failed,
  };
  let run_id = RunId::new(result.run_id.as_str());
  let evidence_artifacts = result
    .artifacts
    .iter()
    .map(|artifact| ArtifactRef {
      run_id: run_id.clone(),
      artifact_id: artifact.artifact_id.clone(),
      span_id: artifact.span_id.clone(),
      captured_event_id: artifact.event_id.clone(),
    })
    .collect();
  OperationResult {
    api_version: OPERATION_RESULT_API_VERSION.to_string(),
    run_id,
    operation_id: command_id.to_string(),
    status,
    output: OperationOutput::Acknowledged {
      message: Some(result.output_summary.clone()),
    },
    verifications: Vec::new(),
    evidence_artifacts,
    freshness_basis: None,
    known_limits: vec![INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT.to_string()],
  }
}

fn persist_operation_result(store: &LocalStore, command_id: &str, result: &InvokeResult) -> Result<(), SessionApiError> {
  let operation = synthetic_operation_result(command_id, result);
  persist_json_artifact(store, result, OPERATION_RESULT_ARTIFACT_ROLE, "operation-result.json", &operation)
}

fn report_write(run_id: &str, label: &str, result: Result<(), SessionApiError>) -> bool {
  match result {
    Ok(()) => true,
    Err(error) => {
      eprintln!("warning: failed to persist {label} for run {run_id}: {error}");
      false
    }
  }
}

/// Persist the runtime summary and synthetic operation result after invoke.
///
/// The command has already executed. Both writes are always attempted, and any
/// failure is represented in the returned outcome rather than propagated as an
/// invoke error. Repeated calls append another pair; read paths intentionally
/// continue to resolve the first artifact for each role.
pub(crate) fn record_invoke_durability(
  store: &LocalStore,
  command_id: &str,
  result: &InvokeResult,
  summary: &OperationSummary,
) -> InvokeDurabilityOutcome {
  let run_id = result.run_id.as_str();
  let summary_persisted = report_write(run_id, "operation-summary", persist_operation_summary(store, result, summary));
  let operation_result_persisted = report_write(run_id, "operation-result", persist_operation_result(store, command_id, result));
  InvokeDurabilityOutcome {
    summary_persisted,
    operation_result_persisted,
  }
}

#[cfg(test)]
mod tests {
  use std::fs;

  use auv_cli_invoke::{OperationSummary, OperationSummaryRecord, OperationSummarySource, RunStatus};

  use super::*;
  use crate::api::session_service::test_fixtures::{fixture_observe_invoke_result, unique_temp_dir, write_minimal_run};
  use crate::run_read;

  fn store_with_run(label: &str, run_id: &str) -> (std::path::PathBuf, LocalStore) {
    let root = unique_temp_dir(label);
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    write_minimal_run(&store, run_id);
    (root, store)
  }

  fn block_artifact_destination(store: &LocalStore, run_id: &str, artifact_id: u32, preferred_name: &str) {
    let path = store.run_dir(run_id).expect("run dir").join("artifacts").join(format!("artifact_{artifact_id:04}_{preferred_name}"));
    fs::create_dir(path).expect("blocking directory should be created");
  }

  #[test]
  fn successful_writes_persist_summary_then_operation_result() {
    let (root, store) = store_with_run("session-durability-success", "run-durability-success");
    let result = fixture_observe_invoke_result("run-durability-success");
    let summary = OperationSummary::capture(&result);

    let outcome = record_invoke_durability(&store, "fixture.observe", &result, &summary);

    assert!(outcome.cache_allowed());
    assert!(outcome.known_limits().is_empty());
    let run = store.read_run("run-durability-success").expect("run should load");
    assert_eq!(
      run.artifacts.iter().map(|artifact| artifact.role.as_str()).collect::<Vec<_>>(),
      vec![
        OPERATION_SUMMARY_ARTIFACT_ROLE,
        OPERATION_RESULT_ARTIFACT_ROLE
      ]
    );
    assert_eq!(
      run_read::read_operation_summary(&store, "run-durability-success").expect("summary read").expect("summary").output_summary(),
      "fixture observed"
    );
    let operation = run_read::read_operation_result(&store, "run-durability-success").expect("operation read").expect("operation result");
    assert_eq!(operation.operation_id, "fixture.observe");
    assert!(operation.known_limits.iter().any(|limit| limit == INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn summary_failure_still_attempts_and_persists_operation_result() {
    let run_id = "run-summary-failure";
    let (root, store) = store_with_run("session-durability-summary-failure", run_id);
    block_artifact_destination(&store, run_id, 1, "operation-summary.json");
    let result = fixture_observe_invoke_result(run_id);
    let summary = OperationSummary::capture(&result);

    let outcome = record_invoke_durability(&store, "fixture.observe", &result, &summary);

    assert!(!outcome.cache_allowed());
    assert_eq!(outcome.known_limits(), vec![OPERATION_SUMMARY_PERSIST_FAILED_KNOWN_LIMIT]);
    assert!(run_read::read_operation_summary(&store, run_id).expect("summary read").is_none());
    assert!(run_read::read_operation_result(&store, run_id).expect("operation read").is_some());

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn operation_result_failure_preserves_summary_and_reports_partial_success() {
    let run_id = "run-operation-result-failure";
    let (root, store) = store_with_run("session-durability-operation-result-failure", run_id);
    block_artifact_destination(&store, run_id, 2, "operation-result.json");
    let result = fixture_observe_invoke_result(run_id);
    let summary = OperationSummary::capture(&result);

    let outcome = record_invoke_durability(&store, "fixture.observe", &result, &summary);

    assert!(!outcome.cache_allowed());
    assert_eq!(outcome.known_limits(), vec![OPERATION_RESULT_PERSIST_FAILED_KNOWN_LIMIT]);
    assert!(run_read::read_operation_summary(&store, run_id).expect("summary read").is_some());
    assert!(run_read::read_operation_result(&store, run_id).expect("operation read").is_none());

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn dual_failure_reports_both_limits_in_write_order() {
    let run_id = "run-dual-failure";
    let (root, store) = store_with_run("session-durability-dual-failure", run_id);
    block_artifact_destination(&store, run_id, 1, "operation-summary.json");
    block_artifact_destination(&store, run_id, 1, "operation-result.json");
    let result = fixture_observe_invoke_result(run_id);
    let summary = OperationSummary::capture(&result);

    let outcome = record_invoke_durability(&store, "fixture.observe", &result, &summary);

    assert!(!outcome.cache_allowed());
    assert_eq!(
      outcome.known_limits(),
      vec![
        OPERATION_SUMMARY_PERSIST_FAILED_KNOWN_LIMIT,
        OPERATION_RESULT_PERSIST_FAILED_KNOWN_LIMIT
      ]
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn repeated_writes_append_pairs_while_readers_keep_first_pair() {
    let run_id = "run-repeated-durability";
    let (root, store) = store_with_run("session-durability-repeated", run_id);
    let first_result = fixture_observe_invoke_result(run_id);
    let first_summary = OperationSummary::capture(&first_result);
    assert!(record_invoke_durability(&store, "fixture.observe", &first_result, &first_summary).cache_allowed());

    let mut second_result = fixture_observe_invoke_result(run_id);
    second_result.output_summary = "second summary".to_string();
    let second_summary = OperationSummary::capture(&second_result);
    assert!(record_invoke_durability(&store, "fixture.second", &second_result, &second_summary).cache_allowed());

    let run = store.read_run(run_id).expect("run should load");
    assert_eq!(
      run.artifacts.iter().map(|artifact| artifact.role.as_str()).collect::<Vec<_>>(),
      vec![
        OPERATION_SUMMARY_ARTIFACT_ROLE,
        OPERATION_RESULT_ARTIFACT_ROLE,
        OPERATION_SUMMARY_ARTIFACT_ROLE,
        OPERATION_RESULT_ARTIFACT_ROLE,
      ]
    );
    let summary = run_read::read_operation_summary(&store, run_id).expect("summary read").expect("summary");
    assert_eq!(summary.output_summary(), "fixture observed");
    let operation = run_read::read_operation_result(&store, run_id).expect("operation read").expect("operation");
    assert_eq!(operation.operation_id, "fixture.observe");

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn failed_invoke_maps_to_failed_synthetic_operation_result() {
    let mut result = fixture_observe_invoke_result("run-failed-invoke");
    result.status = RunStatus::Failed;
    result.failure_message = Some("boom".to_string());

    let operation = synthetic_operation_result("fixture.observe", &result);

    assert_eq!(operation.status, OperationStatus::Failed);
  }

  #[test]
  fn operation_summary_record_roundtrips_through_serde() {
    let mut signals = std::collections::BTreeMap::new();
    signals.insert("k".to_string(), "v".to_string());
    let record = OperationSummaryRecord {
      api_version: OPERATION_SUMMARY_API_VERSION.to_string(),
      run_id: "run-roundtrip".to_string(),
      command_id: "fixture.observe".to_string(),
      status: RunStatus::Failed,
      output_summary: "failed".to_string(),
      signals,
      failure_message: Some("boom".to_string()),
    };
    let rendered = serde_json::to_string(&record).expect("serialize");
    let decoded: OperationSummaryRecord = serde_json::from_str(&rendered).expect("deserialize");
    assert_eq!(decoded, record);
    let restored = OperationSummary::from_record(decoded);
    assert_eq!(restored.run_id(), "run-roundtrip");
    assert_eq!(restored.failure_message(), Some("boom"));
  }
}
