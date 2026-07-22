//! Persisted operation-summary artifact write path (API-P11).
//!
//! Write-through companion to the in-memory [`OperationSummaryCache`]: after
//! invoke, the runtime half of the `GetOperation` projection is staged as an
//! `operation-summary` JSON artifact on the recorded run.

use auv_cli_invoke::{InvokeResult, OperationSummary};
use auv_tracing_driver::artifact::ArtifactBytesSource;
use auv_tracing_driver::store::LocalStore;
use auv_tracing_driver::trace::RunId;

use crate::api::session_service::SessionApiError;
use crate::contract::{OPERATION_SUMMARY_API_VERSION, OPERATION_SUMMARY_ARTIFACT_ROLE};

/// Known limit surfaced when invoke succeeded but the operation-summary artifact
/// could not be persisted (API-P11). The command already executed; callers must
/// not treat this as a failed invoke suitable for blind retry.
pub const OPERATION_SUMMARY_PERSIST_FAILED_KNOWN_LIMIT: &str = "auv.api.session.operation_summary_persist_failed";

/// Stage and append an `operation-summary` artifact onto an existing run.
///
/// Reads the canonical run snapshot, stages the JSON record under
/// [`OPERATION_SUMMARY_ARTIFACT_ROLE`], and replaces the run snapshot with the
/// new artifact list. Uses [`InvokeResult::producer_span_id`] as the artifact
/// span.
///
/// NOTICE(api-p11-duplicate-artifacts): invoke retries may append multiple
/// `operation-summary` artifacts; the read path takes the **first** match,
/// mirroring [`crate::run_read::read_operation_result`].
pub fn persist_operation_summary(store: &LocalStore, result: &InvokeResult, summary: &OperationSummary) -> Result<(), SessionApiError> {
  let run_id = result.run_id.as_str();
  let producer_span_id = result
    .producer_span_id
    .as_ref()
    .ok_or_else(|| SessionApiError::Storage(format!("cannot persist operation-summary for direct run {run_id} without a recorded span")))?;
  let mut canonical = store.read_run(run_id).map_err(SessionApiError::Storage)?;

  let record = summary.to_record(OPERATION_SUMMARY_API_VERSION);
  let rendered = serde_json::to_string_pretty(&record).map_err(|error| SessionApiError::Storage(error.to_string()))? + "\n";

  let artifact = store
    .stage_artifact_bytes(
      &RunId::new(run_id),
      canonical.artifacts.len(),
      producer_span_id,
      None,
      ArtifactBytesSource {
        role: OPERATION_SUMMARY_ARTIFACT_ROLE.to_string(),
        bytes: rendered.into_bytes(),
        preferred_name: "operation-summary.json".to_string(),
        summary: None,
      },
    )
    .map_err(SessionApiError::Storage)?;

  canonical.artifacts.push(artifact);
  store.replace_run_snapshot(&canonical).map_err(SessionApiError::Storage)?;
  Ok(())
}

/// Write-through summary artifact after a successful invoke.
///
/// Returns durability `known_limits` when persistence fails. The invoke command
/// has already executed; failures here must not be propagated as invoke errors.
pub fn record_invoke_summary(store: &LocalStore, result: &InvokeResult, summary: &OperationSummary) -> Vec<String> {
  match persist_operation_summary(store, result, summary) {
    Ok(()) => Vec::new(),
    Err(error) => {
      eprintln!("warning: failed to persist operation-summary for run {}: {error}", result.run_id);
      vec![OPERATION_SUMMARY_PERSIST_FAILED_KNOWN_LIMIT.to_string()]
    }
  }
}

#[cfg(test)]
mod tests {
  use std::fs;

  use auv_cli_invoke::{OperationSummary, OperationSummaryRecord, OperationSummarySource, RunStatus};

  use super::{persist_operation_summary, record_invoke_summary};
  use crate::api::session_service::test_fixtures::{SessionRunFixture, fixture_observe_invoke_result, unique_temp_dir, write_minimal_run};
  use crate::contract::{OPERATION_SUMMARY_API_VERSION, OPERATION_SUMMARY_ARTIFACT_ROLE};
  use crate::run_read;

  #[test]
  fn persist_operation_summary_stages_operation_summary_artifact() {
    let root = unique_temp_dir("summary-store-persist");
    let store = auv_tracing_driver::store::LocalStore::new(root.clone()).expect("store should initialize");
    write_minimal_run(&store, "run-summary-persist");

    let result = fixture_observe_invoke_result("run-summary-persist");
    let summary = OperationSummary::capture(&result);
    persist_operation_summary(&store, &result, &summary).expect("persist should succeed");

    let loaded = store.read_run("run-summary-persist").expect("run should load");
    assert_eq!(loaded.artifacts.len(), 1);
    assert_eq!(loaded.artifacts[0].role, OPERATION_SUMMARY_ARTIFACT_ROLE);

    let read_back =
      run_read::read_operation_summary(&store, "run-summary-persist").expect("read should succeed").expect("summary artifact should exist");
    assert_eq!(read_back.run_id(), "run-summary-persist");
    assert_eq!(read_back.output_summary(), "fixture observed");
    assert_eq!(read_back.signals().get("fixture.observe").map(String::as_str), Some("records deterministic fixture output only."));

    let _ = fs::remove_dir_all(root);
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

  #[cfg(unix)]
  #[test]
  fn record_invoke_summary_surfaces_persist_failure_as_known_limit() {
    use std::os::unix::fs::PermissionsExt;

    let SessionRunFixture { root, store } = {
      let root = unique_temp_dir("summary-store-persist-fail");
      let store = auv_tracing_driver::store::LocalStore::new(root.clone()).expect("store should initialize");
      write_minimal_run(&store, "run-summary-persist-fail");
      SessionRunFixture { root, store }
    };

    let run_dir = store.run_dir("run-summary-persist-fail").expect("run dir should resolve");
    let mut permissions = fs::metadata(&run_dir).expect("run dir metadata").permissions();
    permissions.set_mode(0o500);
    fs::set_permissions(&run_dir, permissions).expect("run dir should be read-only");

    let result = fixture_observe_invoke_result("run-summary-persist-fail");
    let summary = OperationSummary::capture(&result);
    let limits = record_invoke_summary(&store, &result, &summary);
    assert_eq!(limits, vec![super::OPERATION_SUMMARY_PERSIST_FAILED_KNOWN_LIMIT.to_string()]);

    let _ = fs::remove_dir_all(root);
  }
}
