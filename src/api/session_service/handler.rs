//! Transport-independent session API frontend.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use auv_api_proto::v1::session as proto;
use auv_cli_invoke::{InvokeCancellation, InvokeCommandInput, InvokeResult, default_registry};
use auv_tracing::{Context, FileRunStore, RunId, RunStore, configure, dispatcher};

use crate::api::session_service::SessionApiError;
use crate::api::session_service::mapper;
use crate::api::session_service::registry::SessionRegistry;

#[derive(serde::Serialize)]
struct SessionFrontendLifecycle {
  frontend: &'static str,
}

impl auv_tracing::EventPayload for SessionFrontendLifecycle {
  const NAME: &'static str = "auv.frontend.lifecycle";
  const VERSION: u32 = 1;
}

pub struct SessionApiHandler {
  store_root: PathBuf,
  registry: Mutex<SessionRegistry>,
}

impl SessionApiHandler {
  pub fn new(store_root: PathBuf) -> Self {
    Self {
      store_root,
      registry: Mutex::new(SessionRegistry::new()),
    }
  }

  fn open_store(&self) -> Result<Arc<dyn RunStore>, SessionApiError> {
    FileRunStore::open(&self.store_root)
      .map(|store| Arc::new(store) as Arc<dyn RunStore>)
      .map_err(|error| SessionApiError::Storage(error.to_string()))
  }

  pub fn create_session(&self, _request: proto::CreateSessionRequest) -> Result<proto::CreateSessionResponse, SessionApiError> {
    let session_id = self.registry.lock().expect("session registry mutex poisoned").create();
    Ok(proto::CreateSessionResponse {
      session: Some(proto::SessionRef {
        session_id: session_id.as_str().to_string(),
      }),
    })
  }

  /// Executes one command under a frontend-owned root context. The direct
  /// command result is mapped before recording failures are reported, so
  /// instrumentation can never re-execute application work.
  pub async fn invoke(&self, request: proto::InvokeRequest) -> Result<proto::InvokeResponse, SessionApiError> {
    let session = request.session.ok_or(SessionApiError::MissingField("session"))?;
    if !self.registry.lock().expect("session registry mutex poisoned").contains(&session.session_id) {
      return Err(SessionApiError::UnknownSession(session.session_id));
    }

    let command_id = request.command_id;
    let host_request = mapper::decode_invoke_payload(command_id.clone(), &request.json_payload)?;
    let registry = default_registry();
    let command =
      registry.resolve(&command_id).cloned().ok_or_else(|| SessionApiError::InvokeExecution(format!("unknown command: {command_id}")))?;
    let input = InvokeCommandInput {
      command_id: command_id.clone(),
      target_application_id: host_request.target.application_id,
      inputs: host_request.inputs,
      dry_run: host_request.dry_run,
      cancellation: InvokeCancellation::new(),
    };

    let store = self.open_store()?;
    let dispatch = configure().run_store(store.clone()).build().map_err(|error| SessionApiError::Storage(error.to_string()))?;
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let future = root.in_scope(|| {
      auv_tracing::emit_event!(SessionFrontendLifecycle {
        frontend: "session-api"
      });
      command.invoke(input)
    });
    let command_result = root.instrument(future).await;
    let recording_failure = dispatch.flush().await.err().map(|error| error.to_string());
    let artifacts = match store.load_snapshot(run_id).await {
      Ok(Some(snapshot)) => snapshot.artifacts().values().map(|published| published.metadata().clone()).collect(),
      Ok(None) | Err(_) => Vec::new(),
    };
    let result = InvokeResult::from_command_result(run_id.to_string(), &command, command_result).with_canonical_artifacts(artifacts);
    Ok(mapper::invoke_result_to_response(&command_id, &result, recording_failure.as_deref()))
  }

  pub fn get_operation(&self, request: proto::GetOperationRequest) -> Result<proto::GetOperationResponse, SessionApiError> {
    let _operation = request.operation.ok_or(SessionApiError::MissingField("operation"))?;
    // TODO(session-get-operation-typed-projection): Wire this RPC to an owner-approved typed
    // domain projection. Canonical runs intentionally carry no generic status
    // or persisted invoke-result summary from which to fabricate a response.
    Err(SessionApiError::NotWired {
      gate: "typed GetOperation domain projection",
    })
  }

  pub fn stream_session_events(&self, request: proto::StreamSessionEventsRequest) -> Result<Vec<proto::SessionEvent>, SessionApiError> {
    let session = request.session.ok_or(SessionApiError::MissingField("session"))?;
    if !self.registry.lock().expect("session registry mutex poisoned").contains(&session.session_id) {
      return Err(SessionApiError::UnknownSession(session.session_id));
    }
    Err(SessionApiError::NotWired {
      gate: "session event projector",
    })
  }
}

#[cfg(test)]
mod tests {
  use auv_api_proto::v1::session as proto;

  use super::SessionApiHandler;
  use crate::api::session_service::SessionApiError;
  use crate::api::session_service::test_fixtures::session_api_temp_store_root;

  fn handler(label: &str) -> SessionApiHandler {
    SessionApiHandler::new(session_api_temp_store_root(label))
  }

  #[test]
  fn create_session_allocates_and_registers_session() {
    let handler = handler("create");
    let response = handler
      .create_session(proto::CreateSessionRequest {
        client_label: String::new(),
      })
      .expect("create session");
    assert!(!response.session.expect("session ref").session_id.is_empty());
  }

  #[tokio::test]
  async fn invoke_rejects_unknown_session() {
    let error = handler("unknown")
      .invoke(proto::InvokeRequest {
        session: Some(proto::SessionRef {
          session_id: "ghost".to_string(),
        }),
        command_id: "scan.coverage".to_string(),
        json_payload: br#"{"dry_run":true}"#.to_vec(),
      })
      .await
      .expect_err("unknown session");
    assert_eq!(error, SessionApiError::UnknownSession("ghost".to_string()));
  }

  #[tokio::test]
  async fn invoke_returns_direct_command_value_and_fresh_run_ids() {
    let handler = handler("invoke");
    let session = handler
      .create_session(proto::CreateSessionRequest {
        client_label: String::new(),
      })
      .expect("create session")
      .session
      .expect("session ref");
    let request = || proto::InvokeRequest {
      session: Some(session.clone()),
      command_id: "scan.coverage".to_string(),
      json_payload: br#"{"dry_run":true}"#.to_vec(),
    };
    let first = handler.invoke(request()).await.expect("first invoke");
    let second = handler.invoke(request()).await.expect("second invoke");
    assert_eq!(first.status, "completed");
    assert_eq!(second.status, "completed");
    assert_ne!(first.operation.expect("operation").run_id, second.operation.expect("operation").run_id);
  }

  #[test]
  fn get_operation_does_not_synthesize_a_generic_result() {
    let error = handler("get-operation")
      .get_operation(proto::GetOperationRequest {
        operation: Some(proto::OperationRef {
          run_id: RunIdForTest::value(),
          operation_id: "scan.coverage".to_string(),
        }),
      })
      .expect_err("projection is intentionally absent");
    assert!(matches!(error, SessionApiError::NotWired { .. }));
  }

  struct RunIdForTest;
  impl RunIdForTest {
    fn value() -> String {
      auv_tracing::RunId::new().to_string()
    }
  }
}
