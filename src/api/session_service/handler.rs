//! Transport-independent session API frontend.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use auv_api_proto::v1::session as proto;
use auv_cli_invoke::{InvokeCancellation, InvokeCommandInput, InvokeResult, default_registry};
use auv_tracing::{Context, FileTracingStore, RunId, TracingStore, configure, dispatcher};

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

  fn open_store(&self) -> Result<Arc<dyn TracingStore>, SessionApiError> {
    FileTracingStore::open(&self.store_root)
      .map(|store| Arc::new(store) as Arc<dyn TracingStore>)
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
    let dispatch = configure().tracing_store(store).build().map_err(|error| SessionApiError::Storage(error.to_string()))?;
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let future = root.in_scope(|| {
      auv_tracing::emit_event!(SessionFrontendLifecycle {
        frontend: "session-api"
      });
      command.invoke(input)
    });
    let command_result = root.instrument(future).await;
    let result = InvokeResult::from_command_result(run_id, &command, command_result);
    let recording_failure = dispatch.flush().await.err().map(|error| error.to_string());
    Ok(mapper::invoke_result_to_response(&result, recording_failure.as_deref()))
  }
}
