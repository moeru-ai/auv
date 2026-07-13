//! Persisted operation-result artifact write path.
//!
//! Write-through companion after invoke: the persisted skeleton required by
//! `GetOperation` is staged as an `operation-result` JSON artifact on the
//! recorded run. Session invoke records are synthetic (honesty marker in
//! `known_limits`); typed producers use richer domain labels elsewhere.

use auv_cli_invoke::{InvokeResult, RunStatus};
use auv_tracing_driver::artifact::ArtifactBytesSource;
use auv_tracing_driver::store::LocalStore;
use auv_tracing_driver::trace::RunId;

use crate::api::session_service::SessionApiError;
use crate::contract::{ArtifactRef, OPERATION_RESULT_API_VERSION, OperationOutput, OperationResult, OperationStatus};

/// Honesty marker on persisted session-invoke synthetic `OperationResult` records.
pub const INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT: &str = "auv.api.session.invoke_synthetic_operation_result";

/// Known limit when invoke succeeded but the operation-result artifact could not
/// be persisted. The command already executed; callers must not treat this as a
/// failed invoke suitable for blind retry.
pub const OPERATION_RESULT_PERSIST_FAILED_KNOWN_LIMIT: &str = "auv.api.session.operation_result_persist_failed";

const OPERATION_RESULT_ARTIFACT_ROLE: &str = "operation-result";

fn synthetic_operation_result_from_invoke(command_id: &str, result: &InvokeResult) -> OperationResult {
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

/// Stage and append an `operation-result` artifact onto an existing run.
///
/// Reads the canonical run snapshot, stages the JSON record under
/// `operation-result`, and replaces the run snapshot with the new artifact
/// list. Uses [`InvokeResult::producer_span_id`] as the artifact span.
///
/// NOTICE: Invoke retries may append multiple `operation-result` artifacts.
/// The read path takes the first match, mirroring
/// [`crate::run_read::read_operation_result`]. Remove this behavior only with an
/// explicit idempotency and artifact-replacement policy.
pub fn persist_operation_result(store: &LocalStore, result: &InvokeResult, operation: &OperationResult) -> Result<(), SessionApiError> {
  let run_id = result.run_id.as_str();
  let mut canonical = store.read_run(run_id).map_err(SessionApiError::Storage)?;

  let rendered = serde_json::to_string_pretty(operation).map_err(|error| SessionApiError::Storage(error.to_string()))? + "\n";

  let artifact = store
    .stage_artifact_bytes(
      &RunId::new(run_id),
      canonical.artifacts.len(),
      &result.producer_span_id,
      None,
      ArtifactBytesSource {
        role: OPERATION_RESULT_ARTIFACT_ROLE.to_string(),
        bytes: rendered.into_bytes(),
        preferred_name: "operation-result.json".to_string(),
        summary: None,
      },
    )
    .map_err(SessionApiError::Storage)?;

  canonical.artifacts.push(artifact);
  store.replace_run_snapshot(&canonical).map_err(SessionApiError::Storage)?;
  Ok(())
}

/// Write-through operation-result artifact after invoke (completed or failed).
///
/// Returns durability `known_limits` when persistence fails. The invoke command
/// has already executed; failures here must not be propagated as invoke errors.
pub fn record_invoke_operation_result(store: &LocalStore, command_id: &str, result: &InvokeResult) -> Vec<String> {
  let operation = synthetic_operation_result_from_invoke(command_id, result);
  match persist_operation_result(store, result, &operation) {
    Ok(()) => Vec::new(),
    Err(error) => {
      eprintln!("warning: failed to persist operation-result for run {}: {error}", result.run_id);
      vec![OPERATION_RESULT_PERSIST_FAILED_KNOWN_LIMIT.to_string()]
    }
  }
}

#[cfg(test)]
mod tests {
  use std::fs;

  use auv_cli_invoke::RunStatus;

  use super::{
    INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT, OPERATION_RESULT_PERSIST_FAILED_KNOWN_LIMIT, persist_operation_result,
    record_invoke_operation_result, synthetic_operation_result_from_invoke,
  };
  use crate::api::session_service::test_fixtures::{SessionRunFixture, fixture_observe_invoke_result, unique_temp_dir, write_minimal_run};
  use crate::contract::OperationStatus;
  use crate::run_read;

  #[test]
  fn persist_operation_result_appends_synthetic_artifact() {
    let root = unique_temp_dir("op-result-store-persist");
    let store = auv_tracing_driver::store::LocalStore::new(root.clone()).expect("store should initialize");
    write_minimal_run(&store, "run-op-result-persist");

    let result = fixture_observe_invoke_result("run-op-result-persist");
    let operation = synthetic_operation_result_from_invoke("fixture.observe", &result);
    persist_operation_result(&store, &result, &operation).expect("persist should succeed");

    let loaded = store.read_run("run-op-result-persist").expect("run should load");
    assert_eq!(loaded.artifacts.len(), 1);
    assert_eq!(loaded.artifacts[0].role, "operation-result");

    let read_back = run_read::read_operation_result(&store, "run-op-result-persist")
      .expect("read should succeed")
      .expect("operation-result artifact should exist");
    assert_eq!(read_back.run_id.as_str(), "run-op-result-persist");
    assert_eq!(read_back.operation_id, "fixture.observe");
    assert!(read_back.known_limits.iter().any(|limit| limit == INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn synthetic_operation_result_maps_failed_invoke_status() {
    let mut result = fixture_observe_invoke_result("run-op-result-failed");
    result.status = RunStatus::Failed;
    result.output_summary = "failed".to_string();
    result.failure_message = Some("boom".to_string());

    let operation = synthetic_operation_result_from_invoke("fixture.observe", &result);
    assert_eq!(operation.status, OperationStatus::Failed);
    assert!(operation.known_limits.contains(&INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT.to_string()));
  }

  #[cfg(unix)]
  #[test]
  fn record_invoke_operation_result_surfaces_persist_failure() {
    use std::os::unix::fs::PermissionsExt;

    let SessionRunFixture { root, store } = {
      let root = unique_temp_dir("op-result-store-persist-fail");
      let store = auv_tracing_driver::store::LocalStore::new(root.clone()).expect("store should initialize");
      write_minimal_run(&store, "run-op-result-persist-fail");
      SessionRunFixture { root, store }
    };

    let run_dir = store.run_dir("run-op-result-persist-fail").expect("run dir should resolve");
    let mut permissions = fs::metadata(&run_dir).expect("run dir metadata").permissions();
    permissions.set_mode(0o500);
    fs::set_permissions(&run_dir, permissions).expect("run dir should be read-only");

    let result = fixture_observe_invoke_result("run-op-result-persist-fail");
    let limits = record_invoke_operation_result(&store, "fixture.observe", &result);
    assert_eq!(limits, vec![OPERATION_RESULT_PERSIST_FAILED_KNOWN_LIMIT.to_string()]);

    let _ = fs::remove_dir_all(root);
  }
}
