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
  store: SessionRunStoreAuthority,
  registry: Mutex<SessionRegistry>,
}

enum SessionRunStoreAuthority {
  File(PathBuf),
  #[cfg(test)]
  Injected(Arc<dyn RunStore>),
}

impl SessionApiHandler {
  pub fn new(store_root: PathBuf) -> Self {
    Self {
      store: SessionRunStoreAuthority::File(store_root),
      registry: Mutex::new(SessionRegistry::new()),
    }
  }

  #[cfg(test)]
  fn with_store(store: Arc<dyn RunStore>) -> Self {
    Self {
      store: SessionRunStoreAuthority::Injected(store),
      registry: Mutex::new(SessionRegistry::new()),
    }
  }

  fn open_store(&self) -> Result<Arc<dyn RunStore>, SessionApiError> {
    match &self.store {
      SessionRunStoreAuthority::File(store_root) => FileRunStore::open(store_root)
        .map(|store| Arc::new(store) as Arc<dyn RunStore>)
        .map_err(|error| SessionApiError::Storage(error.to_string())),
      #[cfg(test)]
      SessionRunStoreAuthority::Injected(store) => Ok(store.clone()),
    }
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
    let mut recording_failures = Vec::new();
    if let Err(error) = dispatch.flush().await {
      recording_failures.push(error.to_string());
    }
    let artifacts = match store.load_snapshot(run_id).await {
      Ok(Some(snapshot)) => snapshot.artifacts().values().map(|published| published.metadata().clone()).collect(),
      Ok(None) => {
        recording_failures.push("recorded run snapshot is missing after execution".to_string());
        Vec::new()
      }
      Err(error) => {
        recording_failures.push(format!("failed to load recorded run snapshot after execution: {error}"));
        Vec::new()
      }
    };
    let result = InvokeResult::from_command_result(run_id.to_string(), &command, command_result).with_canonical_artifacts(artifacts);
    let recording_failure = (!recording_failures.is_empty()).then(|| recording_failures.join("; "));
    Ok(mapper::invoke_result_to_response(&result, recording_failure.as_deref()))
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
  use std::sync::Arc;

  use auv_api_proto::v1::session as proto;
  use auv_tracing::{
    ArtifactBody, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, CommitError, CommitResult, ErrorCode,
    IdempotencyKey, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision, RunSnapshot,
    RunStore, RunSubscription, StoreArtifactRequest,
  };

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
    assert_ne!(first.run_id, second.run_id);
  }

  #[tokio::test]
  async fn invoke_reports_missing_snapshot_without_changing_direct_status() {
    let store = Arc::new(SnapshotReadStore::new(SnapshotReadResult::Missing));
    let handler = SessionApiHandler::with_store(store);
    let response = invoke_dry_run(&handler).await;

    assert_eq!(response.status, "completed");
    assert!(response.failure_message.is_empty());
    assert_eq!(response.recording_failure, "recorded run snapshot is missing after execution");
    assert_eq!(response.known_limits, vec!["auv.api.session.recording_failed"]);
  }

  #[tokio::test]
  async fn invoke_reports_snapshot_read_error_without_changing_direct_status() {
    let store = Arc::new(SnapshotReadStore::new(SnapshotReadResult::Error));
    let handler = SessionApiHandler::with_store(store);
    let response = invoke_dry_run(&handler).await;

    assert_eq!(response.status, "completed");
    assert!(response.failure_message.is_empty());
    assert!(response.recording_failure.contains("auv.test.session_snapshot_read"));
    assert_eq!(response.known_limits, vec!["auv.api.session.recording_failed"]);
  }

  async fn invoke_dry_run(handler: &SessionApiHandler) -> proto::InvokeResponse {
    let session = handler
      .create_session(proto::CreateSessionRequest {
        client_label: String::new(),
      })
      .expect("create session")
      .session
      .expect("session ref");
    handler
      .invoke(proto::InvokeRequest {
        session: Some(session),
        command_id: "scan.coverage".to_string(),
        json_payload: br#"{"dry_run":true}"#.to_vec(),
      })
      .await
      .expect("direct invoke result")
  }

  enum SnapshotReadResult {
    Missing,
    Error,
  }

  struct SnapshotReadStore {
    inner: MemoryRunStore,
    result: SnapshotReadResult,
  }

  impl SnapshotReadStore {
    fn new(result: SnapshotReadResult) -> Self {
      Self {
        inner: MemoryRunStore::new(AuthorityId::new()),
        result,
      }
    }
  }

  impl RunStore for SnapshotReadStore {
    fn authority_id(&self) -> AuthorityId {
      self.inner.authority_id()
    }

    fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
      self.inner.commit(request)
    }

    fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
      self.inner.write_artifact(request, body)
    }

    fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
      self.inner.lookup_commit(run_id, key)
    }

    fn load_snapshot(&self, _run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>> {
      match self.result {
        SnapshotReadResult::Missing => Box::pin(async { Ok(None) }),
        SnapshotReadResult::Error => {
          Box::pin(async { Err(ReadError::Unavailable(ErrorCode::parse("auv.test.session_snapshot_read").expect("test error code"))) })
        }
      }
    }

    fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
      self.inner.commits_after(run_id, after, limit)
    }

    fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
      self.inner.subscribe(run_id, after)
    }

    fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
      self.inner.open_artifact(uri)
    }
  }
}
