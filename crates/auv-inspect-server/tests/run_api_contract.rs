use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use auv_inspect_server::{InspectServeConfig, router, serve};
use auv_tracing::{
  ArtifactBody, ArtifactReader, ArtifactWriteError, Attributes, AuthorityId, BoxFuture, CommitError, CommitResult, ErrorCode, EventId,
  EventName, EventOccurred, EventSchema, IdempotencyKey, JsonPayload, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage,
  RunCommitRequest, RunId, RunMutation, RunRevision, RunStore, RunSubscription, SpanName, SpanStarted, StoreArtifactRequest,
  SubscriptionError, Timestamp,
};
use auv_tracing_inspect::protocol::RUN_MEDIA_TYPE;
use axum::body::{Body, Bytes, to_bytes};
use axum::http::header::CONTENT_TYPE;
use axum::http::{Request, StatusCode};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const AUTHORITY: &str = "019f8b1e-4b2d-7a00-8f00-0000000000aa";
const OTHER_AUTHORITY: &str = "019f8b1e-4b2d-7a00-8f00-0000000000ab";
const RUN: &str = "019f8b1e-4b2d-7a00-8f00-000000000001";
const OTHER_RUN: &str = "019f8b1e-4b2d-7a00-8f00-000000000002";
const SPAN: &str = "019f8b1e-4b2d-7a00-8f00-000000000011";
const KEY_ONE: &str = "019f8b1e-4b2d-7a00-8f00-000000000031";
const KEY_TWO: &str = "019f8b1e-4b2d-7a00-8f00-000000000032";

#[derive(Clone)]
struct CommitProbe {
  inner: MemoryRunStore,
  commit_calls: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct FaultingRunStore {
  commit_error: Option<CommitError>,
  read_error: Option<ReadError>,
}

#[derive(Clone)]
struct RacingRunStore {
  inner: MemoryRunStore,
  lookup_barrier: Arc<tokio::sync::Barrier>,
  lookup_calls: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct CommitUnknownStore {
  resolved: Option<RunCommit>,
  lookup_calls: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct SubscriptionFaultStore {
  inner: MemoryRunStore,
  error: ReadError,
}

impl FaultingRunStore {
  fn commit(error: CommitError) -> Self {
    Self {
      commit_error: Some(error),
      read_error: None,
    }
  }

  fn read(error: ReadError) -> Self {
    Self {
      commit_error: None,
      read_error: Some(error),
    }
  }

  fn read_failure(&self) -> ReadError {
    self.read_error.clone().expect("read error")
  }
}

impl RunStore for FaultingRunStore {
  fn authority_id(&self) -> AuthorityId {
    authority_id()
  }

  fn commit(&self, _request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
    let error = self.commit_error.clone().expect("commit error");
    Box::pin(async move { Err(error) })
  }

  fn write_artifact(&self, _request: StoreArtifactRequest, _body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
    Box::pin(async { Err(ArtifactWriteError::Unavailable(error_code("auv.test.unavailable"))) })
  }

  fn lookup_commit(&self, _run_id: RunId, _key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    let error = self.read_error.clone();
    Box::pin(async move {
      match error {
        Some(error) => Err(error),
        None => Ok(None),
      }
    })
  }

  fn load_snapshot(&self, _run_id: RunId) -> BoxFuture<'_, Result<Option<auv_tracing::RunSnapshot>, ReadError>> {
    let error = self.read_failure();
    Box::pin(async move { Err(error) })
  }

  fn commits_after(&self, _run_id: RunId, _after: RunRevision, _limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    let error = self.read_failure();
    Box::pin(async move { Err(error) })
  }

  fn subscribe(&self, _run_id: RunId, _after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    let error = self.read_failure();
    Box::pin(async move { Err(error) })
  }

  fn open_artifact(&self, _uri: auv_tracing::ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    let error = self.read_failure();
    Box::pin(async move { Err(error) })
  }
}

impl CommitProbe {
  fn new() -> Self {
    Self {
      inner: MemoryRunStore::new(authority_id()),
      commit_calls: Arc::new(AtomicUsize::new(0)),
    }
  }

  fn calls(&self) -> usize {
    self.commit_calls.load(Ordering::SeqCst)
  }
}

impl RunStore for CommitProbe {
  fn authority_id(&self) -> AuthorityId {
    self.inner.authority_id()
  }

  fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
    self.commit_calls.fetch_add(1, Ordering::SeqCst);
    self.inner.commit(request)
  }

  fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
    self.inner.write_artifact(request, body)
  }

  fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    self.inner.lookup_commit(run_id, key)
  }

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<auv_tracing::RunSnapshot>, ReadError>> {
    self.inner.load_snapshot(run_id)
  }

  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    self.inner.commits_after(run_id, after, limit)
  }

  fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    self.inner.subscribe(run_id, after)
  }

  fn open_artifact(&self, uri: auv_tracing::ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    self.inner.open_artifact(uri)
  }
}

impl RacingRunStore {
  fn new() -> Self {
    Self {
      inner: MemoryRunStore::new(authority_id()),
      lookup_barrier: Arc::new(tokio::sync::Barrier::new(2)),
      lookup_calls: Arc::new(AtomicUsize::new(0)),
    }
  }

  fn lookup_calls(&self) -> usize {
    self.lookup_calls.load(Ordering::SeqCst)
  }
}

impl RunStore for RacingRunStore {
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
    let inner = self.inner.clone();
    let barrier = self.lookup_barrier.clone();
    let calls = self.lookup_calls.fetch_add(1, Ordering::SeqCst);
    Box::pin(async move {
      if calls < 2 {
        barrier.wait().await;
      }
      inner.lookup_commit(run_id, key).await
    })
  }

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<auv_tracing::RunSnapshot>, ReadError>> {
    self.inner.load_snapshot(run_id)
  }

  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    self.inner.commits_after(run_id, after, limit)
  }

  fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    self.inner.subscribe(run_id, after)
  }

  fn open_artifact(&self, uri: auv_tracing::ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    self.inner.open_artifact(uri)
  }
}

impl RunStore for CommitUnknownStore {
  fn authority_id(&self) -> AuthorityId {
    authority_id()
  }

  fn commit(&self, _request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
    Box::pin(async { Err(CommitError::CommitUnknown(error_code("auv.test.unknown"))) })
  }

  fn write_artifact(&self, _request: StoreArtifactRequest, _body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
    Box::pin(async { Err(ArtifactWriteError::Unavailable(error_code("auv.test.unavailable"))) })
  }

  fn lookup_commit(&self, _run_id: RunId, _key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    self.lookup_calls.fetch_add(1, Ordering::SeqCst);
    let resolved = self.resolved.clone();
    Box::pin(async move { Ok(resolved) })
  }

  fn load_snapshot(&self, _run_id: RunId) -> BoxFuture<'_, Result<Option<auv_tracing::RunSnapshot>, ReadError>> {
    Box::pin(async { Ok(None) })
  }

  fn commits_after(&self, _run_id: RunId, _after: RunRevision, _limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    Box::pin(async { Err(ReadError::NotFound) })
  }

  fn subscribe(&self, _run_id: RunId, _after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    Box::pin(async { Err(ReadError::NotFound) })
  }

  fn open_artifact(&self, _uri: auv_tracing::ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    Box::pin(async { Err(ReadError::NotFound) })
  }
}

impl RunStore for SubscriptionFaultStore {
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

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<auv_tracing::RunSnapshot>, ReadError>> {
    self.inner.load_snapshot(run_id)
  }

  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    self.inner.commits_after(run_id, after, limit)
  }

  fn subscribe(&self, _run_id: RunId, _after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    let error = self.error.clone();
    Box::pin(async move { Ok(Box::pin(futures_util::stream::once(async move { Err(SubscriptionError::Store(error)) })) as RunSubscription) })
  }

  fn open_artifact(&self, uri: auv_tracing::ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    self.inner.open_artifact(uri)
  }
}

fn authority_id() -> AuthorityId {
  AUTHORITY.parse().expect("authority id")
}

fn error_code(value: &str) -> ErrorCode {
  ErrorCode::parse(value).expect("error code")
}

fn run_id() -> RunId {
  RUN.parse().expect("run id")
}

fn start_span(name: &str) -> RunMutation {
  RunMutation::StartSpan(SpanStarted::new(
    SPAN.parse().expect("span id"),
    None,
    None,
    SpanName::parse(name).expect("span name"),
    Timestamp::new(1, 0).expect("timestamp"),
    Attributes::empty(),
  ))
}

fn event(index: u32) -> RunMutation {
  let event_id = format!("019f8b1e-4b2d-7a00-8f00-{index:012x}");
  RunMutation::EmitEvent(EventOccurred::new(
    EventId::from_str(&event_id).expect("event id"),
    None,
    Timestamp::new(i64::from(index) + 1, 0).expect("timestamp"),
    EventSchema::new(EventName::parse("auv.test.event").expect("event name"), 1).expect("event schema"),
    JsonPayload::from_str(&format!(r#"{{"index":{index}}}"#)).expect("payload"),
  ))
}

fn commit_body(authority: &str, mutation: &RunMutation) -> Vec<u8> {
  serde_json::to_vec(&json!({
    "authority_id": authority,
    "mutations": [mutation],
  }))
  .expect("commit body")
}

fn post_commit(run: &str, key: Option<&str>, body: Vec<u8>) -> Request<Body> {
  let mut request = Request::builder().method("POST").uri(format!("/v1/runs/{run}/commits")).header(CONTENT_TYPE, RUN_MEDIA_TYPE);
  if let Some(key) = key {
    request = request.header("Idempotency-Key", key);
  }
  request.body(Body::from(body)).expect("request")
}

async fn json_body(response: axum::response::Response) -> Value {
  let bytes = to_bytes(response.into_body(), 34 * 1024 * 1024).await.expect("response body");
  serde_json::from_slice(&bytes).expect("JSON response")
}

#[tokio::test]
async fn authority_endpoint_publishes_the_store_identity() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let response = app.oneshot(Request::builder().uri("/v1/authority").body(Body::empty()).expect("request")).await.expect("response");

  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), RUN_MEDIA_TYPE);
  assert_eq!(json_body(response).await, json!({"authority_id": AUTHORITY}));
}

#[tokio::test]
async fn commit_is_created_then_replayed_and_conflicting_replay_is_rejected() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let body = commit_body(AUTHORITY, &start_span("auv.test.root"));

  let created = app.clone().oneshot(post_commit(RUN, Some(KEY_ONE), body.clone())).await.expect("created response");
  assert_eq!(created.status(), StatusCode::CREATED);
  let original = json_body(created).await;
  assert_eq!(original["run_id"], RUN);
  assert_eq!(original["revision"], 1);

  let replayed = app.clone().oneshot(post_commit(RUN, Some(KEY_ONE), body)).await.expect("replay response");
  assert_eq!(replayed.status(), StatusCode::OK);
  assert_eq!(json_body(replayed).await, original);

  let conflict = app
    .oneshot(post_commit(RUN, Some(KEY_ONE), commit_body(AUTHORITY, &start_span("auv.test.different"))))
    .await
    .expect("conflict response");
  assert_eq!(conflict.status(), StatusCode::CONFLICT);
  assert_eq!(json_body(conflict).await, json!("idempotency_mismatch"));
}

#[tokio::test]
async fn shared_store_classifies_concurrent_cross_router_replay_atomically() {
  let store = RacingRunStore::new();
  let first = router(Arc::new(store.clone()));
  let second = router(Arc::new(store.clone()));
  let body = commit_body(AUTHORITY, &start_span("auv.test.root"));

  let (first, second) =
    tokio::join!(first.oneshot(post_commit(RUN, Some(KEY_ONE), body.clone())), second.oneshot(post_commit(RUN, Some(KEY_ONE), body)),);
  let mut statuses = [first.unwrap().status(), second.unwrap().status()];
  statuses.sort();

  assert_eq!(statuses, [StatusCode::OK, StatusCode::CREATED]);
  assert_eq!(store.lookup_calls(), 0, "successful commit classification must not pre-read idempotency state");
}

#[tokio::test]
async fn commit_requires_idempotency_key_and_takes_run_id_only_from_path() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let missing_key =
    app.clone().oneshot(post_commit(RUN, None, commit_body(AUTHORITY, &start_span("auv.test.root")))).await.expect("missing-key response");
  assert_eq!(missing_key.status(), StatusCode::BAD_REQUEST);

  let body_with_run_id = serde_json::to_vec(&json!({
    "authority_id": AUTHORITY,
    "run_id": OTHER_RUN,
    "mutations": [start_span("auv.test.root")],
  }))
  .expect("body");
  let response = app.oneshot(post_commit(RUN, Some(KEY_ONE), body_with_run_id)).await.expect("response");
  assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn unknown_and_duplicate_fields_are_rejected_before_store_commit() {
  let probe = CommitProbe::new();
  let app = router(Arc::new(probe.clone()));
  let mutation = serde_json::to_string(&start_span("auv.test.root")).expect("mutation");
  let unknown = format!(r#"{{"authority_id":"{AUTHORITY}","mutations":[{mutation}],"extra":true}}"#);
  let duplicate = format!(r#"{{"authority_id":"{AUTHORITY}","authority_id":"{AUTHORITY}","mutations":[{mutation}]}}"#);

  for body in [unknown, duplicate] {
    let response = app.clone().oneshot(post_commit(RUN, Some(KEY_ONE), body.into_bytes())).await.expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
  }
  assert_eq!(probe.calls(), 0);
}

#[tokio::test]
async fn run_json_body_over_32_mib_is_rejected_before_decode_or_store() {
  let probe = CommitProbe::new();
  let app = router(Arc::new(probe.clone()));
  let mut body = commit_body(AUTHORITY, &start_span("auv.test.root"));
  body.resize(32 * 1024 * 1024 + 1, b' ');

  let response = app.oneshot(post_commit(RUN, Some(KEY_ONE), body)).await.expect("response");

  assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
  assert_eq!(probe.calls(), 0);
}

#[tokio::test]
async fn failing_run_json_body_is_bad_request_before_store_commit() {
  let probe = CommitProbe::new();
  let app = router(Arc::new(probe.clone()));
  let body = Body::from_stream(futures_util::stream::iter([
    Ok::<_, std::io::Error>(Bytes::from_static(b"{")),
    Err(std::io::Error::other("request body failed")),
  ]));
  let request = Request::builder()
    .method("POST")
    .uri(format!("/v1/runs/{RUN}/commits"))
    .header(CONTENT_TYPE, RUN_MEDIA_TYPE)
    .header("Idempotency-Key", KEY_ONE)
    .body(body)
    .unwrap();

  let response = app.oneshot(request).await.unwrap();

  assert_eq!(response.status(), StatusCode::BAD_REQUEST);
  assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), RUN_MEDIA_TYPE);
  assert_eq!(json_body(response).await, json!({"invalid_reference":{"code":"auv.inspect.invalid_reference"}}));
  assert_eq!(probe.calls(), 0);
}

#[tokio::test]
async fn run_body_limit_is_not_installed_globally() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let response =
    app.oneshot(Request::builder().method("POST").uri("/").body(Body::from(vec![b' '; 32 * 1024 * 1024 + 1])).unwrap()).await.unwrap();

  assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn authority_mismatch_is_a_typed_conflict() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let response =
    app.oneshot(post_commit(RUN, Some(KEY_ONE), commit_body(OTHER_AUTHORITY, &start_span("auv.test.root")))).await.expect("response");

  assert_eq!(response.status(), StatusCode::CONFLICT);
  assert_eq!(json_body(response).await, json!({"authority_mismatch":{"expected":AUTHORITY,"received":OTHER_AUTHORITY}}),);
}

#[tokio::test]
async fn authority_mismatch_precedes_store_reads_and_commit() {
  let app = router(Arc::new(FaultingRunStore::read(ReadError::Unavailable(error_code("auv.test.lookup_unavailable")))));

  let response =
    app.oneshot(post_commit(RUN, Some(KEY_ONE), commit_body(OTHER_AUTHORITY, &start_span("auv.test.root")))).await.expect("response");

  assert_eq!(response.status(), StatusCode::CONFLICT);
  assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), RUN_MEDIA_TYPE);
  assert_eq!(json_body(response).await, json!({"authority_mismatch":{"expected":AUTHORITY,"received":OTHER_AUTHORITY}}));
}

#[tokio::test]
async fn unsupported_commit_content_type_returns_415_with_run_media_type() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let request = Request::builder()
    .method("POST")
    .uri(format!("/v1/runs/{RUN}/commits"))
    .header(CONTENT_TYPE, "application/json")
    .header("Idempotency-Key", KEY_ONE)
    .body(Body::from(commit_body(AUTHORITY, &start_span("auv.test.root"))))
    .unwrap();

  let response = app.oneshot(request).await.unwrap();

  assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
  assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), RUN_MEDIA_TYPE);
  assert_eq!(json_body(response).await, json!({"invalid_reference":{"code":"auv.inspect.unsupported_media_type"}}));
}

#[tokio::test]
async fn path_extractor_rejections_use_the_run_media_type() {
  let response = router(Arc::new(MemoryRunStore::new(authority_id())))
    .oneshot(Request::builder().uri("/v1/runs/%FF/snapshot").body(Body::empty()).unwrap())
    .await
    .unwrap();

  assert_eq!(response.status(), StatusCode::BAD_REQUEST);
  assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), RUN_MEDIA_TYPE);
  assert_eq!(json_body(response).await, json!({"invalid_reference":{"code":"auv.inspect.invalid_reference"}}));
}

#[tokio::test]
async fn standard_serve_rejects_non_loopback_before_writing_discovery() {
  let root = std::env::temp_dir().join(format!("auv-inspect-non-loopback-{}", std::process::id()));
  let session_path = root.join("session.json");
  let _ = std::fs::remove_dir_all(&root);
  unsafe {
    std::env::set_var("AUV_INSPECT_SESSION", &session_path);
  }

  let result = tokio::time::timeout(
    std::time::Duration::from_millis(250),
    serve(
      Arc::new(MemoryRunStore::new(authority_id())),
      InspectServeConfig {
        host: "0.0.0.0".to_string(),
        port: 0,
      },
    ),
  )
  .await;
  let session_written = session_path.exists();
  unsafe {
    std::env::remove_var("AUV_INSPECT_SESSION");
  }
  let _ = std::fs::remove_dir_all(root);

  let error = result.expect("non-loopback serve must reject instead of serving").expect_err("non-loopback serve must fail");
  assert!(error.contains("loopback"), "{error}");
  assert!(!session_written, "rejected serve wrote discovery state");
}

#[tokio::test]
async fn commit_failures_map_to_exact_typed_errors() {
  let cases = [
    (
      CommitError::Rejected(error_code("auv.test.rejected")),
      StatusCode::UNPROCESSABLE_ENTITY,
      json!({"rejected":{"code":"auv.test.rejected"}}),
    ),
    (
      CommitError::Unavailable(error_code("auv.test.unavailable")),
      StatusCode::SERVICE_UNAVAILABLE,
      json!({"unavailable":{"code":"auv.test.unavailable"}}),
    ),
    (
      CommitError::CommitUnknown(error_code("auv.test.unknown")),
      StatusCode::SERVICE_UNAVAILABLE,
      json!({"unavailable":{"code":"auv.test.unknown"}}),
    ),
  ];

  for (error, status, body) in cases {
    let response = router(Arc::new(FaultingRunStore::commit(error)))
      .oneshot(post_commit(RUN, Some(KEY_ONE), commit_body(AUTHORITY, &start_span("auv.test.root"))))
      .await
      .unwrap();
    assert_eq!(response.status(), status);
    assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), RUN_MEDIA_TYPE);
    assert_eq!(json_body(response).await, body);
  }
}

#[tokio::test]
async fn commit_unknown_resolves_once_by_lookup_without_resubmitting() {
  let memory = MemoryRunStore::new(authority_id());
  let committed = memory
    .commit(RunCommitRequest::new(authority_id(), run_id(), KEY_ONE.parse().unwrap(), vec![start_span("auv.test.root")]).unwrap())
    .await
    .unwrap()
    .into_commit();
  let lookup_calls = Arc::new(AtomicUsize::new(0));
  let store = CommitUnknownStore {
    resolved: Some(committed.clone()),
    lookup_calls: lookup_calls.clone(),
  };

  let response =
    router(Arc::new(store)).oneshot(post_commit(RUN, Some(KEY_ONE), commit_body(AUTHORITY, &start_span("auv.test.root")))).await.unwrap();

  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(json_body(response).await, serde_json::to_value(committed).unwrap());
  assert_eq!(lookup_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn read_failures_map_to_exact_typed_errors() {
  let cases = [
    (ReadError::NotFound, StatusCode::NOT_FOUND, json!("not_found")),
    (ReadError::Forbidden, StatusCode::FORBIDDEN, json!("forbidden")),
    (
      ReadError::InvalidReference(error_code("auv.test.invalid")),
      StatusCode::BAD_REQUEST,
      json!({"invalid_reference":{"code":"auv.test.invalid"}}),
    ),
    (
      ReadError::Integrity(error_code("auv.test.integrity")),
      StatusCode::INTERNAL_SERVER_ERROR,
      json!({"integrity":{"code":"auv.test.integrity"}}),
    ),
    (
      ReadError::Unavailable(error_code("auv.test.unavailable")),
      StatusCode::SERVICE_UNAVAILABLE,
      json!({"unavailable":{"code":"auv.test.unavailable"}}),
    ),
  ];

  for (error, status, body) in cases {
    let response = router(Arc::new(FaultingRunStore::read(error)))
      .oneshot(Request::builder().uri(format!("/v1/runs/{RUN}/snapshot")).body(Body::empty()).unwrap())
      .await
      .unwrap();
    assert_eq!(response.status(), status);
    assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), RUN_MEDIA_TYPE);
    assert_eq!(json_body(response).await, body);
  }
}

#[tokio::test]
async fn lookup_snapshot_and_page_expose_canonical_store_reads() {
  let store = MemoryRunStore::new(authority_id());
  let app = router(Arc::new(store));
  let first = commit_body(AUTHORITY, &start_span("auv.test.root"));
  let second = commit_body(AUTHORITY, &event(0x21));
  let first_response = app.clone().oneshot(post_commit(RUN, Some(KEY_ONE), first)).await.expect("first");
  assert_eq!(first_response.status(), StatusCode::CREATED);
  let first_commit = json_body(first_response).await;
  assert_eq!(app.clone().oneshot(post_commit(RUN, Some(KEY_TWO), second)).await.expect("second").status(), StatusCode::CREATED);

  let lookup = app
    .clone()
    .oneshot(
      Request::builder().uri(format!("/v1/runs/{RUN}/commits/by-idempotency-key/{KEY_ONE}")).body(Body::empty()).expect("lookup request"),
    )
    .await
    .expect("lookup");
  assert_eq!(lookup.status(), StatusCode::OK);
  assert_eq!(json_body(lookup).await["revision"], 1);

  let snapshot = app
    .clone()
    .oneshot(Request::builder().uri(format!("/v1/runs/{RUN}/snapshot")).body(Body::empty()).expect("snapshot request"))
    .await
    .expect("snapshot");
  assert_eq!(snapshot.status(), StatusCode::OK);
  let snapshot = json_body(snapshot).await;
  assert_eq!(snapshot["through_revision"], 2);
  assert_eq!(snapshot["events"][0]["schema"]["name"], "auv.test.event");
  assert_eq!(snapshot["events"][0]["payload"], json!({"index": 0x21}));

  let page = app
    .clone()
    .oneshot(Request::builder().uri(format!("/v1/runs/{RUN}/commits?after_revision=0&limit=1")).body(Body::empty()).expect("page request"))
    .await
    .expect("page");
  assert_eq!(page.status(), StatusCode::OK);
  assert_eq!(json_body(page).await, json!({"commits":[first_commit],"last_revision":1,"has_more":true}));
}

#[tokio::test]
async fn missing_lookup_and_snapshot_return_typed_not_found() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  for uri in [
    format!("/v1/runs/{RUN}/commits/by-idempotency-key/{KEY_ONE}"),
    format!("/v1/runs/{RUN}/snapshot"),
  ] {
    let response = app.clone().oneshot(Request::builder().uri(uri).body(Body::empty()).expect("request")).await.expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(response).await, json!("not_found"));
  }
}

#[tokio::test]
async fn history_gap_and_cursor_ahead_have_exact_typed_bodies() {
  let store = MemoryRunStore::with_history_limit(authority_id(), NonZeroUsize::new(2).unwrap());
  for index in 1..=10 {
    let request = RunCommitRequest::new(
      authority_id(),
      run_id(),
      format!("019f8b1e-4b2d-7a00-8f00-{index:012x}").parse().unwrap(),
      vec![event(0x100 + index)],
    )
    .unwrap();
    store.commit(request).await.unwrap();
  }
  let app = router(Arc::new(store));

  let response = app
    .clone()
    .oneshot(Request::builder().uri(format!("/v1/runs/{RUN}/commits?after_revision=4&limit=1")).body(Body::empty()).unwrap())
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::GONE);
  assert_eq!(json_body(response).await, serde_json::json!({"history_gap":{"requested_after":4,"earliest_available":9}}),);

  let response = app
    .oneshot(Request::builder().uri(format!("/v1/runs/{RUN}/commits?after_revision=11&limit=1")).body(Body::empty()).unwrap())
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::CONFLICT);
  assert_eq!(json_body(response).await, json!({"cursor_ahead":{"requested_after":11,"latest":10}}),);
}

#[tokio::test]
async fn sse_reconnect_uses_the_greater_valid_cursor() {
  let store = MemoryRunStore::new(authority_id());
  for index in 1..=3 {
    store
      .commit(
        RunCommitRequest::new(
          authority_id(),
          run_id(),
          format!("019f8b1e-4b2d-7a00-8f00-{index:012x}").parse().unwrap(),
          vec![event(0x200 + index)],
        )
        .unwrap(),
      )
      .await
      .unwrap();
  }
  let app = router(Arc::new(store));
  let response = app
    .oneshot(
      Request::builder()
        .uri(format!("/v1/runs/{RUN}/commits/stream?after_revision=1"))
        .header("Last-Event-ID", "2")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "text/event-stream");

  let mut chunks = response.into_body().into_data_stream();
  let chunk = tokio::time::timeout(std::time::Duration::from_secs(1), chunks.next())
    .await
    .expect("SSE data timeout")
    .expect("SSE data")
    .expect("SSE body");
  let event = std::str::from_utf8(&chunk).expect("UTF-8 SSE");
  assert!(event.contains("id: 3\n"), "{event}");
  assert!(event.contains("event: commit\n"), "{event}");
  assert!(event.contains("\"revision\":3"), "{event}");
}

#[tokio::test]
async fn sse_emits_gap_event_and_closes() {
  let store = MemoryRunStore::with_history_limit(authority_id(), NonZeroUsize::new(2).unwrap());
  for index in 1..=10 {
    store
      .commit(
        RunCommitRequest::new(
          authority_id(),
          run_id(),
          format!("019f8b1e-4b2d-7a00-8f00-{index:012x}").parse().unwrap(),
          vec![event(0x300 + index)],
        )
        .unwrap(),
      )
      .await
      .unwrap();
  }
  let response = router(Arc::new(store))
    .oneshot(Request::builder().uri(format!("/v1/runs/{RUN}/commits/stream?after_revision=4")).body(Body::empty()).unwrap())
    .await
    .unwrap();

  let bytes = tokio::time::timeout(std::time::Duration::from_secs(1), to_bytes(response.into_body(), 1024 * 1024))
    .await
    .expect("gap stream should close")
    .expect("gap body");
  let event = std::str::from_utf8(&bytes).unwrap();
  assert!(event.contains("event: gap\n"), "{event}");
  assert!(event.contains(r#"data: {"requested_after":4,"earliest_available":9}"#), "{event}");
}

#[tokio::test]
async fn sse_emits_typed_store_error_and_closes() {
  let store = SubscriptionFaultStore {
    inner: MemoryRunStore::new(authority_id()),
    error: ReadError::Integrity(error_code("auv.test.integrity")),
  };
  let response = router(Arc::new(store))
    .oneshot(Request::builder().uri(format!("/v1/runs/{RUN}/commits/stream?after_revision=0")).body(Body::empty()).unwrap())
    .await
    .unwrap();

  let bytes = tokio::time::timeout(std::time::Duration::from_secs(1), to_bytes(response.into_body(), 1024 * 1024))
    .await
    .expect("error stream should close")
    .expect("error body");
  let event = std::str::from_utf8(&bytes).unwrap();
  assert!(event.contains("event: error\n"), "{event}");
  assert!(event.contains(r#"data: {"integrity":{"code":"auv.test.integrity"}}"#), "{event}");
}

#[test]
fn protocol_error_codes_are_namespaced() {
  ErrorCode::parse("auv.inspect.invalid_request").expect("run API error code must remain valid");
}
