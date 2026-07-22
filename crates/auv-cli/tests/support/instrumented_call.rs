use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use auv_tracing::{
  ArtifactBody, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, CommitError, CommitResult, Context, ErrorCode,
  EventPayload, IdempotencyKey, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision,
  RunSnapshot, RunStore, RunSubscription, StoreArtifactRequest, TelemetryError, TelemetryItem, TelemetryProjector, TelemetryRoutePolicy,
  configure, dispatcher,
};

#[derive(Clone, Default)]
pub struct CountingCall {
  calls: Arc<AtomicUsize>,
}

impl CountingCall {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn call_count(&self) -> usize {
    self.calls.load(Ordering::SeqCst)
  }

  pub fn call(&self) -> impl Future<Output = Result<u32, String>> + Send + 'static {
    auv_tracing::emit_event!(CallEvent {
      phase: "constructed"
    });
    let calls = self.calls.clone();
    async move {
      calls.fetch_add(1, Ordering::SeqCst);
      auv_tracing::emit_event!(CallEvent { phase: "polled" });
      Ok(7)
    }
  }
}

#[derive(serde::Serialize)]
struct CallEvent {
  phase: &'static str,
}

impl EventPayload for CallEvent {
  const NAME: &'static str = "auv.test.frontend_call";
  const VERSION: u32 = 1;
}

pub struct FrontendCall {
  pub value: u32,
  pub run_id: RunId,
  pub stored_event_run_ids: Vec<RunId>,
  pub stored_event_count: usize,
  pub tracing_error: Option<String>,
}

pub struct FailedTracingCall {
  pub value: u32,
  pub tracing_error: Option<String>,
  pub canonical_facts: String,
}

pub async fn call_as_library(call: &CountingCall) -> Result<u32, String> {
  call.call().await
}

pub async fn call_as_cli(call: &CountingCall) -> Result<FrontendCall, String> {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let dispatch = configure().run_store(store.clone()).build().map_err(|error| error.to_string())?;
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
  let future = root.in_scope(|| call.call());
  let value = root.instrument(future).await?;
  let tracing_error = dispatch.flush().await.err().map(|error| error.to_string());
  let snapshot = store.load_snapshot(run_id).await.map_err(|error| error.to_string())?;
  let stored_event_count = snapshot.as_ref().map(|snapshot| snapshot.events().len()).unwrap_or_default();
  let stored_event_run_ids =
    snapshot.filter(|snapshot| !snapshot.events().is_empty()).map(|snapshot| vec![snapshot.run_id()]).unwrap_or_default();
  Ok(FrontendCall {
    value,
    run_id,
    stored_event_run_ids,
    stored_event_count,
    tracing_error,
  })
}

pub async fn call_as_mcp(call: &CountingCall) -> Result<FrontendCall, String> {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let dispatch = configure().run_store(store.clone()).build().map_err(|error| error.to_string())?;
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
  let future = root.in_scope(|| call.call());
  let value = root.instrument(future).await?;
  let tracing_error = dispatch.flush().await.err().map(|error| error.to_string());
  let snapshot = store.load_snapshot(run_id).await.map_err(|error| error.to_string())?;
  let stored_event_count = snapshot.as_ref().map(|snapshot| snapshot.events().len()).unwrap_or_default();
  let stored_event_run_ids =
    snapshot.filter(|snapshot| !snapshot.events().is_empty()).map(|snapshot| vec![snapshot.run_id()]).unwrap_or_default();
  Ok(FrontendCall {
    value,
    run_id,
    stored_event_run_ids,
    stored_event_count,
    tracing_error,
  })
}

pub async fn call_as_cli_with_commit_unknown(call: &CountingCall) -> Result<FailedTracingCall, String> {
  let store = Arc::new(CommitUnknownStore::new());
  let dispatch = configure().run_store(store.clone()).build().map_err(|error| error.to_string())?;
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
  let future = root.in_scope(|| call.call());
  let value = root.instrument(future).await?;
  let tracing_error = dispatch.flush().await.err().map(|error| error.to_string());
  let canonical_facts =
    store.load_snapshot(run_id).await.map_err(|error| error.to_string())?.map(|snapshot| format!("{snapshot:?}")).unwrap_or_default();
  Ok(FailedTracingCall {
    value,
    tracing_error,
    canonical_facts,
  })
}

pub async fn call_as_mcp_with_telemetry_error(call: &CountingCall) -> Result<FailedTracingCall, String> {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let dispatch = configure()
    .run_store(store.clone())
    .project_telemetry(Arc::new(FailingProjector), TelemetryRoutePolicy::fixed_fields_only())
    .build()
    .map_err(|error| error.to_string())?;
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
  let future = root.in_scope(|| call.call());
  let value = root.instrument(future).await?;
  let tracing_error = dispatch.flush().await.err().map(|error| error.to_string());
  let canonical_facts =
    store.load_snapshot(run_id).await.map_err(|error| error.to_string())?.map(|snapshot| format!("{snapshot:?}")).unwrap_or_default();
  Ok(FailedTracingCall {
    value,
    tracing_error,
    canonical_facts,
  })
}

struct CommitUnknownStore {
  inner: MemoryRunStore,
}

impl CommitUnknownStore {
  fn new() -> Self {
    Self {
      inner: MemoryRunStore::new(AuthorityId::new()),
    }
  }
}

impl RunStore for CommitUnknownStore {
  fn authority_id(&self) -> AuthorityId {
    self.inner.authority_id()
  }

  fn commit(&self, _request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
    Box::pin(async { Err(CommitError::CommitUnknown(ErrorCode::parse("auv.test.commit_unknown").unwrap())) })
  }

  fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
    self.inner.write_artifact(request, body)
  }

  fn lookup_commit(&self, _run_id: RunId, _key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    Box::pin(async { Ok(None) })
  }

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>> {
    self.inner.load_snapshot(run_id)
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

#[derive(Clone, Copy)]
struct FailingProjector;

impl TelemetryProjector for FailingProjector {
  fn project(&self, _item: TelemetryItem) -> BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async { Err(TelemetryError::new(ErrorCode::parse("auv.test.telemetry_error").unwrap())) })
  }

  fn flush(&self) -> BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async { Ok(()) })
  }
}
