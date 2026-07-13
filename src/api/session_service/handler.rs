//! Session API application orchestration.
//!
//! This module implements no tonic service trait, but it accepts generated
//! protobuf request and response types, so it is tonic-independent rather than
//! transport-agnostic. It wires session validation, recorded invoke, operation
//! persistence, and the two-source `GetOperation` join.
//!
//! Invoke records a run, caches the runtime summary only after both durability
//! writes succeed, persists the runtime summary artifact, and writes a synthetic
//! `operation-result` so fresh `GetOperation` joins succeed. Typed producers may
//! still record richer domain operation identifiers outside this path.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use auv_api_proto::v1::session as proto;
use auv_cli_invoke::{OperationSummary, OperationSummaryCache, default_registry};
use auv_tracing_driver::store::LocalStore;
use auv_tracing_driver::{MemoryRunRecorder, RunRecordingBackend, SessionId};

use crate::api::session_service::SessionApiError;
use crate::api::session_service::durability::record_invoke_durability;
use crate::api::session_service::mapper;
use crate::api::session_service::registry::SessionRegistry;
use crate::api::session_service::summary::{JoinedOperationSummaryLoad, load_joined_operation_summary};

/// Process-local session API handler over one store path.
///
/// **Not** a long-lived `Runtime`, `RunRecordingBackend`, or shared invoke executor.
/// **Is** a process-local façade over:
/// - `store_root` — durable runs and artifacts live here
/// - `SessionRegistry` — lightweight session id registry
/// - `OperationSummaryCache` — invoke-result-sourced summary cache
///
/// Each `invoke` opens a fresh `LocalStore` + `RunRecordingBackend` (see
/// [`SessionApiHandler::invoke`]); recording is discarded when the call returns.
/// Durability is store-backed artifacts, not handler fields.
pub struct SessionApiHandler {
  store_root: PathBuf,
  registry: Mutex<SessionRegistry>,
  summaries: Mutex<OperationSummaryCache>,
}

impl SessionApiHandler {
  pub fn new(store_root: PathBuf) -> Self {
    Self {
      store_root,
      registry: Mutex::new(SessionRegistry::new()),
      summaries: Mutex::new(OperationSummaryCache::new()),
    }
  }

  fn open_store(&self) -> Result<LocalStore, SessionApiError> {
    LocalStore::new(self.store_root.clone()).map_err(SessionApiError::Storage)
  }

  /// `CreateSession`: allocate + register lightweight session metadata, return a
  /// `SessionRef`.
  pub fn create_session(&self, _request: proto::CreateSessionRequest) -> Result<proto::CreateSessionResponse, SessionApiError> {
    // TODO: Emit `session_created` after an application event source and event
    // projector are approved. No event bus exists, so creation is silent.
    let session_id = self.registry.lock().expect("session registry mutex poisoned").create();
    Ok(proto::CreateSessionResponse {
      session: Some(proto::SessionRef {
        session_id: session_id.as_str().to_string(),
      }),
    })
  }

  /// `Invoke`: validate the session, decode the payload, run the session-aware
  /// recorded invoke, record the summary, and map the result.
  ///
  /// Each call opens a new `LocalStore` and `RunRecordingBackend`; nothing
  /// session-scoped survives the return. This mirrors the MCP invoke surface
  /// and is not a session-bound runtime.
  pub fn invoke(&self, request: proto::InvokeRequest) -> Result<proto::InvokeResponse, SessionApiError> {
    let session = request.session.ok_or(SessionApiError::MissingField("session"))?;
    let session_id = session.session_id;
    if !self.registry.lock().expect("session registry mutex poisoned").contains(&session_id) {
      return Err(SessionApiError::UnknownSession(session_id));
    }

    let command_id = request.command_id;
    let host_request = mapper::decode_invoke_payload(command_id.clone(), &request.json_payload)?;

    let store = self.open_store()?;
    let recording = RunRecordingBackend::new(store, Arc::new(MemoryRunRecorder::new()));
    let registry = default_registry();
    let result = auv_cli_invoke::invoke_recorded_with_session(&recording, &registry, host_request, SessionId::new(session_id))
      .map_err(SessionApiError::InvokeExecution)?;

    Ok(self.finish_invoke_response(&command_id, &result, &recording))
  }

  fn finish_invoke_response(
    &self,
    command_id: &str,
    result: &auv_cli_invoke::InvokeResult,
    recording: &RunRecordingBackend,
  ) -> proto::InvokeResponse {
    let summary = OperationSummary::capture(result);
    // Invoke already finished, so persistence failure must not surface as an
    // invoke error that invites blind retry of a non-idempotent command.
    // Durability gaps are reported through known_limits.
    let durability = record_invoke_durability(recording.store(), command_id, result, &summary);
    if durability.cache_allowed() {
      self.summaries.lock().expect("summary cache mutex poisoned").record(summary.clone());
    }
    let known_limits = durability.known_limits();
    mapper::invoke_result_to_response(command_id, result, &known_limits)
  }

  /// `GetOperation`: read the persisted record + runtime summary and return the
  /// explicit two-source join.
  ///
  /// On this handler instance, a process-local cache hit becomes
  /// `process_local_runtime_override` for the join. A new handler or cache miss
  /// falls back to the persisted `operation-summary` artifact. A
  /// persisted `OperationResult` skeleton is still required.
  pub fn get_operation(&self, request: proto::GetOperationRequest) -> Result<proto::GetOperationResponse, SessionApiError> {
    let operation = request.operation.ok_or(SessionApiError::MissingField("operation"))?;
    let run_id = operation.run_id.clone();
    let requested_operation_id = operation.operation_id;

    let runtime_summary = {
      let summaries = self.summaries.lock().expect("summary cache mutex poisoned");
      summaries.get(&run_id).cloned()
    };

    let store = self.open_store()?;
    let local_override = runtime_summary.as_ref();
    match load_joined_operation_summary(&store, &run_id, local_override).map_err(SessionApiError::Storage)? {
      JoinedOperationSummaryLoad::Found(joined) => {
        if !requested_operation_id.is_empty() {
          match &joined.command_id {
            Some(resolved) if resolved != &requested_operation_id => {
              return Err(SessionApiError::OperationIdMismatch {
                run_id,
                requested: requested_operation_id,
                resolved: resolved.clone(),
              });
            }
            None => {
              return Err(SessionApiError::OperationIdMismatch {
                run_id,
                requested: requested_operation_id,
                resolved: String::new(),
              });
            }
            _ => {}
          }
        }
        Ok(mapper::joined_to_get_operation_response(&joined))
      }
      JoinedOperationSummaryLoad::RunNotFound => Err(SessionApiError::RunNotFound(run_id)),
      JoinedOperationSummaryLoad::NoPersistedOperationResult => Err(SessionApiError::PersistedOperationRequired(run_id)),
    }
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;
  use std::sync::atomic::{AtomicU64, Ordering};

  use auv_api_proto::v1::session as proto;
  use auv_cli_invoke::default_registry;
  use auv_tracing_driver::{MemoryRunRecorder, RunRecordingBackend, SessionId, now_millis};

  use super::SessionApiHandler;
  use crate::api::session_service::SessionApiError;
  use crate::api::session_service::durability::{
    INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT, OPERATION_RESULT_PERSIST_FAILED_KNOWN_LIMIT, OPERATION_SUMMARY_PERSIST_FAILED_KNOWN_LIMIT,
  };
  use crate::api::session_service::mapper;
  use crate::api::session_service::test_fixtures::unique_temp_dir;

  static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

  fn handler() -> SessionApiHandler {
    let unique = DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("auv-session-api-{}-{}", now_millis(), unique));
    SessionApiHandler::new(root)
  }

  fn handler_with_music_search_cached(label: &str, run_id: &str) -> (SessionApiHandler, std::path::PathBuf) {
    use crate::api::session_service::test_fixtures::{music_runtime_summary, music_search_operation_result_fixture};

    let fixture = music_search_operation_result_fixture(label, run_id);
    let root = fixture.root.clone();
    let handler = SessionApiHandler::new(root.clone());
    handler.summaries.lock().expect("summary cache mutex poisoned").record(music_runtime_summary(run_id));
    (handler, root)
  }

  #[test]
  fn create_session_allocates_and_registers_session() {
    let handler = handler();
    let response = handler
      .create_session(proto::CreateSessionRequest {
        client_label: String::new(),
      })
      .expect("create_session");
    let session = response.session.expect("session ref");
    assert!(!session.session_id.is_empty());
    assert!(handler.registry.lock().unwrap().contains(&session.session_id));
  }

  #[test]
  fn invoke_rejects_unknown_session() {
    let handler = handler();
    let error = handler
      .invoke(proto::InvokeRequest {
        session: Some(proto::SessionRef {
          session_id: "ghost".to_string(),
        }),
        command_id: "fixture.observe".to_string(),
        json_payload: Vec::new(),
      })
      .expect_err("unknown session should fail");
    assert_eq!(error, SessionApiError::UnknownSession("ghost".to_string()));
  }

  #[test]
  fn invoke_runs_fixture_command_for_known_session() {
    let handler = handler();
    let session = handler
      .create_session(proto::CreateSessionRequest {
        client_label: String::new(),
      })
      .expect("create_session")
      .session
      .expect("session ref");
    let response = handler
      .invoke(proto::InvokeRequest {
        session: Some(session),
        command_id: "fixture.observe".to_string(),
        json_payload: Vec::new(),
      })
      .expect("invoke fixture.observe");
    assert_eq!(response.status, "completed");
    let operation = response.operation.expect("operation ref");
    assert!(!operation.run_id.is_empty());
    assert_eq!(operation.operation_id, "fixture.observe");
  }

  #[test]
  fn invoke_then_get_operation_round_trips() {
    let handler = handler();
    let session = handler
      .create_session(proto::CreateSessionRequest {
        client_label: String::new(),
      })
      .expect("create_session")
      .session
      .expect("session ref");
    let invoked = handler
      .invoke(proto::InvokeRequest {
        session: Some(session),
        command_id: "fixture.observe".to_string(),
        json_payload: Vec::new(),
      })
      .expect("invoke fixture.observe");
    let run_id = invoked.operation.expect("operation ref").run_id;

    let response = handler
      .get_operation(proto::GetOperationRequest {
        operation: Some(proto::OperationRef {
          run_id: run_id.clone(),
          operation_id: String::new(),
        }),
      })
      .expect("get_operation should succeed after invoke");

    assert_eq!(response.status, "completed");
    assert_eq!(response.output_summary, "fixture observed");
    let operation_ref = response.operation.expect("operation ref");
    assert_eq!(operation_ref.run_id, run_id);
    assert_eq!(operation_ref.operation_id, "fixture.observe");
    assert!(response.known_limits.iter().any(|limit| limit == INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT));
  }

  #[test]
  fn get_operation_on_same_handler_uses_process_local_cache_path() {
    // Same-handler path: process-local cache supplies the runtime half of the join.
    // P12 wire operation_id (command_id, not domain label) covered here.
    let run_id = "run-get-op-happy";
    let (handler, root) = handler_with_music_search_cached("session-api-joined", run_id);

    let response = handler
      .get_operation(proto::GetOperationRequest {
        operation: Some(proto::OperationRef {
          run_id: run_id.to_string(),
          operation_id: String::new(),
        }),
      })
      .expect("get_operation should succeed");

    assert_eq!(response.status, "completed");
    assert_eq!(response.output_summary, "did the thing");
    assert_eq!(response.signals.get("now_playing").map(String::as_str), Some("track-x"));
    let operation_ref = response.operation.expect("operation ref");
    assert_eq!(operation_ref.run_id, run_id);
    assert_eq!(operation_ref.operation_id, "music.search");

    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn get_operation_after_new_handler_reads_store_not_cache() {
    // New-handler path: empty cache; runtime half comes from persisted operation-summary.
    let root = unique_temp_dir("session-api-restart");
    let handler1 = SessionApiHandler::new(root.clone());
    let session = handler1
      .create_session(proto::CreateSessionRequest {
        client_label: String::new(),
      })
      .expect("create_session")
      .session
      .expect("session ref");
    let invoked = handler1
      .invoke(proto::InvokeRequest {
        session: Some(session),
        command_id: "fixture.observe".to_string(),
        json_payload: Vec::new(),
      })
      .expect("invoke fixture.observe");
    let run_id = invoked.operation.expect("operation ref").run_id;

    let handler2 = SessionApiHandler::new(root.clone());
    assert!(handler2.summaries.lock().unwrap().is_empty());

    let response = handler2
      .get_operation(proto::GetOperationRequest {
        operation: Some(proto::OperationRef {
          run_id: run_id.clone(),
          operation_id: String::new(),
        }),
      })
      .expect("get_operation should succeed after restart");

    assert_eq!(response.status, "completed");
    assert_eq!(response.output_summary, "fixture observed");
    assert_eq!(response.operation.expect("operation ref").run_id, run_id);
    assert!(response.known_limits.iter().any(|limit| limit == INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT));

    let _ = std::fs::remove_dir_all(root);
  }

  #[cfg(unix)]
  #[test]
  fn get_operation_reads_preseeded_skeleton_when_invoke_durability_writes_fail() {
    use std::os::unix::fs::PermissionsExt;

    use crate::api::session_service::test_fixtures::{
      fixture_observe_invoke_result, music_search_operation, persist_operation_result_on_store,
    };

    let root = unique_temp_dir("session-api-persist-fail");
    let handler = SessionApiHandler::new(root.clone());
    let store = handler.open_store().expect("open store");
    let run_id = "run-summary-persist-fail";
    persist_operation_result_on_store(&store, &root, run_id, &music_search_operation(run_id));
    let run_dir = store.run_dir(run_id).expect("run dir");
    let mut permissions = std::fs::metadata(&run_dir).expect("run dir metadata").permissions();
    permissions.set_mode(0o500);
    std::fs::set_permissions(&run_dir, permissions).expect("run dir should be read-only");

    let recording = RunRecordingBackend::new(
      auv_tracing_driver::store::LocalStore::new(root.clone()).expect("recording store"),
      Arc::new(MemoryRunRecorder::new()),
    );
    let result = fixture_observe_invoke_result(run_id);
    let response = handler.finish_invoke_response("fixture.observe", &result, &recording);
    assert!(response.known_limits.iter().any(|limit| limit == OPERATION_SUMMARY_PERSIST_FAILED_KNOWN_LIMIT));
    assert!(response.known_limits.iter().any(|limit| limit == OPERATION_RESULT_PERSIST_FAILED_KNOWN_LIMIT));
    assert!(handler.summaries.lock().expect("summary cache mutex poisoned").get(run_id).is_none());

    let persisted_failure_response = handler
      .get_operation(proto::GetOperationRequest {
        operation: Some(proto::OperationRef {
          run_id: run_id.to_string(),
          operation_id: String::new(),
        }),
      })
      .expect("get_operation should succeed with persisted result");

    assert!(persisted_failure_response.output_summary.is_empty());
    assert!(persisted_failure_response.known_limits.iter().any(|limit| limit == "auv.api.session.runtime_summary_unavailable"));

    let mut cleanup_permissions = std::fs::metadata(&run_dir).expect("run dir metadata after test").permissions();
    cleanup_permissions.set_mode(0o700);
    let _ = std::fs::set_permissions(&run_dir, cleanup_permissions);
    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn finish_invoke_response_persists_failed_skeleton_and_get_operation_reads_it() {
    use crate::api::session_service::test_fixtures::{fixture_observe_invoke_result, write_minimal_run};
    use auv_cli_invoke::RunStatus;

    let root = unique_temp_dir("session-api-failed-invoke");
    let handler = SessionApiHandler::new(root.clone());
    let store = handler.open_store().expect("open store");
    write_minimal_run(&store, "run-failed-invoke");
    let recording = RunRecordingBackend::new(
      auv_tracing_driver::store::LocalStore::new(root.clone()).expect("recording store"),
      Arc::new(MemoryRunRecorder::new()),
    );
    let mut result = fixture_observe_invoke_result("run-failed-invoke");
    result.status = RunStatus::Failed;
    result.output_summary = "fixture failed".to_string();
    result.failure_message = Some("boom".to_string());

    let response = handler.finish_invoke_response("fixture.observe", &result, &recording);
    assert_eq!(response.status, "failed");
    assert!(handler.summaries.lock().expect("summary cache mutex poisoned").get("run-failed-invoke").is_some());

    let response = handler
      .get_operation(proto::GetOperationRequest {
        operation: Some(proto::OperationRef {
          run_id: "run-failed-invoke".to_string(),
          operation_id: String::new(),
        }),
      })
      .expect("get_operation should succeed for failed synthetic skeleton");

    assert_eq!(response.status, "failed");
    assert_eq!(response.output_summary, "fixture failed");
    assert_eq!(response.failure_message, "boom");
    assert!(response.known_limits.iter().any(|limit| limit == INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT));

    let _ = std::fs::remove_dir_all(root);
  }

  #[cfg(unix)]
  #[test]
  fn invoke_durability_failure_keeps_cache_empty_and_get_operation_preconditions() {
    use std::os::unix::fs::PermissionsExt;

    // TODO: A store fault-injection seam is needed to reproduce only the second
    // durability write failing after the first succeeds. Add that case when
    // durability writes gain an injectable storage boundary.
    let root = unique_temp_dir("session-api-op-result-persist-fail");
    let handler = SessionApiHandler::new(root.clone());
    let session = handler
      .create_session(proto::CreateSessionRequest {
        client_label: String::new(),
      })
      .expect("create_session")
      .session
      .expect("session ref");
    let run_id = {
      let store = handler.open_store().expect("open store");
      let recording = RunRecordingBackend::new(
        auv_tracing_driver::store::LocalStore::new(root.clone()).expect("recording store"),
        Arc::new(MemoryRunRecorder::new()),
      );
      let result = auv_cli_invoke::invoke_recorded_with_session(
        &recording,
        &default_registry(),
        mapper::decode_invoke_payload("fixture.observe".to_string(), &Vec::new()).expect("decode"),
        SessionId::new(session.session_id.clone()),
      )
      .expect("invoke fixture.observe");
      let run_dir = store.run_dir(result.run_id.as_str()).expect("run dir");
      let mut permissions = std::fs::metadata(&run_dir).expect("run dir metadata").permissions();
      permissions.set_mode(0o500);
      std::fs::set_permissions(&run_dir, permissions).expect("run dir should be read-only");
      let response = handler.finish_invoke_response("fixture.observe", &result, &recording);
      assert!(response.known_limits.iter().any(|limit| limit == OPERATION_SUMMARY_PERSIST_FAILED_KNOWN_LIMIT));
      assert!(response.known_limits.iter().any(|limit| limit == OPERATION_RESULT_PERSIST_FAILED_KNOWN_LIMIT));
      assert!(handler.summaries.lock().expect("summary cache mutex poisoned").get(result.run_id.as_str()).is_none());
      result.run_id
    };

    let error = handler
      .get_operation(proto::GetOperationRequest {
        operation: Some(proto::OperationRef {
          run_id: run_id.clone(),
          operation_id: String::new(),
        }),
      })
      .expect_err("missing synthetic operation-result should preserve precondition");
    assert!(matches!(error, SessionApiError::PersistedOperationRequired(_)));

    let run_dir = handler.open_store().expect("open store").run_dir(run_id.as_str()).expect("run dir");
    let mut cleanup_permissions = std::fs::metadata(&run_dir).expect("run dir metadata after test").permissions();
    cleanup_permissions.set_mode(0o700);
    let _ = std::fs::set_permissions(&run_dir, cleanup_permissions);
    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn get_operation_rejects_operation_id_mismatch() {
    let run_id = "run-p12-mismatch";
    let (handler, root) = handler_with_music_search_cached("session-operation-id-mismatch", run_id);

    let error = handler
      .get_operation(proto::GetOperationRequest {
        operation: Some(proto::OperationRef {
          run_id: run_id.to_string(),
          operation_id: "wrong.command".to_string(),
        }),
      })
      .expect_err("mismatch");

    assert!(matches!(error, SessionApiError::OperationIdMismatch { .. }));

    let _ = std::fs::remove_dir_all(root);
  }
}
