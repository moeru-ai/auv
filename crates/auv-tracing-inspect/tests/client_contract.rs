use std::io;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use auv_inspect_server::{router, router_with_artifact_origin};
use auv_tracing::{
  ArtifactBody, ArtifactId, ArtifactPurpose, ArtifactReader, ArtifactUri, ArtifactWriteError, Attributes, AuthorityId, BoxFuture,
  ByteLength, CommitError, CommitResult, ContentType, ErrorCode, EventId, EventName, EventOccurred, EventSchema, IdempotencyKey,
  JsonPayload, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunFact, RunId, RunMutation, RunRevision,
  RunStore, RunSubscription, Sha256Digest, SpanName, SpanStarted, StoreArtifactRequest, Timestamp,
};
use auv_tracing_inspect::protocol::{
  ARTIFACT_UPLOAD_ADMISSION_HEADER, ARTIFACT_UPLOAD_MEDIA_TYPE, ArtifactApiError, ArtifactUploadDraft, ArtifactUploadId, AuthorityResponse,
  RUN_MEDIA_TYPE, ResolveArtifactsResponse, ResolvedArtifact, RunApiError, RunStreamGap,
};
use auv_tracing_inspect::{InspectRunStore, TokioTaskSpawner};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, Query, Request as AxumRequest, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post, put};
use futures_util::StreamExt;
use futures_util::io::{AsyncRead, Cursor};
use serde::Deserialize;
use tokio::net::TcpListener;
use url::Url;

const AUTHORITY: &str = "019f8b1e-4b2d-7a00-8f00-0000000000aa";
const OTHER_AUTHORITY: &str = "019f8b1e-4b2d-7a00-8f00-0000000000ab";
const RUN: &str = "019f8b1e-4b2d-7a00-8f00-000000000001";
const SPAN: &str = "019f8b1e-4b2d-7a00-8f00-000000000011";
const KEY: &str = "019f8b1e-4b2d-7a00-8f00-000000000031";
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

struct TestServer {
  base_url: Url,
  task: tokio::task::JoinHandle<()>,
}

impl TestServer {
  async fn start(app: Router) -> Self {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind test server");
    let address = listener.local_addr().expect("test server address");
    let task = tokio::spawn(async move {
      axum::serve(listener, app).await.expect("test server");
    });
    Self {
      base_url: Url::parse(&format!("http://{address}/")).expect("base URL"),
      task,
    }
  }

  async fn start_store(store: Arc<dyn RunStore>) -> Self {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind test server");
    let address = listener.local_addr().expect("test server address");
    let base_url = Url::parse(&format!("http://{address}/")).expect("base URL");
    let app = router_with_artifact_origin(store, base_url.clone()).expect("trusted test origin");
    let task = tokio::spawn(async move {
      axum::serve(listener, app).await.expect("test server");
    });
    Self { base_url, task }
  }
}

impl Drop for TestServer {
  fn drop(&mut self) {
    self.task.abort();
  }
}

#[derive(Clone)]
struct FaultStore {
  commit_error: Option<CommitError>,
  artifact_error: Option<ArtifactWriteError>,
  read_error: Option<ReadError>,
}

#[derive(Clone)]
struct BlockingArtifactStore {
  inner: MemoryRunStore,
  entered: Arc<tokio::sync::Barrier>,
  release: Arc<tokio::sync::Notify>,
  write_calls: Arc<AtomicUsize>,
  lookup_calls: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct WriteRedirectState {
  attacker_base_url: Url,
  draft: ArtifactUploadDraft,
  draft_calls: Arc<AtomicUsize>,
}

impl BlockingArtifactStore {
  fn new() -> Self {
    Self {
      inner: MemoryRunStore::new(authority_id()),
      entered: Arc::new(tokio::sync::Barrier::new(2)),
      release: Arc::new(tokio::sync::Notify::new()),
      write_calls: Arc::new(AtomicUsize::new(0)),
      lookup_calls: Arc::new(AtomicUsize::new(0)),
    }
  }
}

impl FaultStore {
  fn commit(error: CommitError) -> Self {
    Self {
      commit_error: Some(error),
      artifact_error: None,
      read_error: None,
    }
  }

  fn artifact(error: ArtifactWriteError) -> Self {
    Self {
      commit_error: None,
      artifact_error: Some(error),
      read_error: None,
    }
  }

  fn read(error: ReadError) -> Self {
    Self {
      commit_error: None,
      artifact_error: None,
      read_error: Some(error),
    }
  }

  fn read_failure(&self) -> ReadError {
    self.read_error.clone().expect("read error")
  }
}

impl RunStore for FaultStore {
  fn authority_id(&self) -> AuthorityId {
    authority_id()
  }

  fn commit(&self, _request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
    let error = self.commit_error.clone().expect("commit error");
    Box::pin(async move { Err(error) })
  }

  fn write_artifact(&self, _request: StoreArtifactRequest, _body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
    let error = self.artifact_error.clone().unwrap_or_else(|| ArtifactWriteError::Unavailable(error_code("auv.test.unavailable")));
    Box::pin(async move { Err(error) })
  }

  fn lookup_commit(&self, _run_id: RunId, _key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    let error = self.read_error.clone();
    Box::pin(async move { error.map_or(Ok(None), Err) })
  }

  fn load_snapshot(&self, _run_id: RunId) -> BoxFuture<'_, Result<Option<auv_tracing::RunSnapshot>, ReadError>> {
    let error = self.read_error.clone();
    Box::pin(async move { error.map_or(Ok(None), Err) })
  }

  fn commits_after(&self, _run_id: RunId, _after: RunRevision, _limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    let error = self.read_failure();
    Box::pin(async move { Err(error) })
  }

  fn subscribe(&self, _run_id: RunId, _after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    let error = self.read_failure();
    Box::pin(async move { Err(error) })
  }

  fn open_artifact(&self, _uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    let error = self.read_failure();
    Box::pin(async move { Err(error) })
  }
}

impl RunStore for BlockingArtifactStore {
  fn authority_id(&self) -> AuthorityId {
    self.inner.authority_id()
  }

  fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
    self.inner.commit(request)
  }

  fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
    let inner = self.inner.clone();
    let entered = self.entered.clone();
    let release = self.release.clone();
    let call = self.write_calls.fetch_add(1, Ordering::SeqCst);
    Box::pin(async move {
      if call == 0 {
        entered.wait().await;
        release.notified().await;
      }
      inner.write_artifact(request, body).await
    })
  }

  fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    self.lookup_calls.fetch_add(1, Ordering::SeqCst);
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

  fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    self.inner.open_artifact(uri)
  }
}

fn authority_id() -> AuthorityId {
  AUTHORITY.parse().expect("authority ID")
}

fn run_id() -> RunId {
  RUN.parse().expect("run ID")
}

fn error_code(value: &str) -> ErrorCode {
  ErrorCode::parse(value).expect("error code")
}

fn start_span() -> RunMutation {
  RunMutation::StartSpan(SpanStarted::new(
    SPAN.parse().expect("span ID"),
    None,
    None,
    SpanName::parse("auv.test.root").expect("span name"),
    Timestamp::new(1, 0).expect("timestamp"),
    Attributes::empty(),
  ))
}

struct EmittedSpan;

impl auv_tracing::SpanSpec for EmittedSpan {
  const NAME: &'static str = "auv.test.emitted";

  fn attributes(&self) -> Attributes {
    Attributes::empty()
  }
}

fn sample_commit_request() -> RunCommitRequest {
  RunCommitRequest::new(authority_id(), run_id(), KEY.parse().expect("key"), vec![start_span()]).expect("commit request")
}

fn artifact_request(authority: AuthorityId, artifact_id: ArtifactId, key: IdempotencyKey) -> StoreArtifactRequest {
  StoreArtifactRequest::new(
    authority,
    run_id(),
    key,
    artifact_id,
    None,
    ArtifactPurpose::parse("display.capture").expect("purpose"),
    ContentType::parse("text/plain").expect("content type"),
    ByteLength::new(3).expect("length"),
    Sha256Digest::from_str(ABC_SHA256).expect("digest"),
    Attributes::empty(),
  )
}

#[tokio::test]
async fn connect_fetches_authority_before_store_installation_and_all_core_reads_work() {
  let backing = MemoryRunStore::new(authority_id());
  let server = TestServer::start(router(Arc::new(backing))).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");

  assert_eq!(store.authority_id(), authority_id());
  let spawner = TokioTaskSpawner::current().expect("Tokio runtime");
  let dispatch = auv_tracing::configure().task_spawner(Arc::new(spawner)).run_store(Arc::new(store.clone())).build().expect("dispatch");

  let commit = store.commit(sample_commit_request()).await.expect("commit").into_commit();
  assert_eq!(commit.authority_id(), authority_id());
  assert_eq!(store.lookup_commit(run_id(), KEY.parse().expect("key")).await.expect("lookup"), Some(commit.clone()));
  assert_eq!(store.load_snapshot(run_id()).await.expect("snapshot").expect("existing run").through_revision(), commit.revision());
  let page =
    store.commits_after(run_id(), RunRevision::new(0).expect("revision"), PageLimit::new(32).expect("page limit")).await.expect("page");
  assert_eq!(page.commits(), std::slice::from_ref(&commit));

  let mut subscription = store.subscribe(run_id(), RunRevision::new(0).expect("revision")).await.expect("subscription");
  assert_eq!(subscription.next().await.expect("subscription item").expect("commit"), commit);
  let emitted_run = RunId::new();
  let root = auv_tracing::dispatcher::with_default(&dispatch, || auv_tracing::Context::root(emitted_run));
  let span = root.in_scope(|| auv_tracing::start_span(EmittedSpan));
  drop(span);
  dispatch.flush().await.expect("flush");
  assert!(store.load_snapshot(emitted_run).await.expect("emitted snapshot").is_some());
}

#[tokio::test]
async fn connect_rejects_an_authority_redirect_without_contacting_the_new_origin() {
  let attacker_hits = Arc::new(AtomicUsize::new(0));
  let observed_hits = attacker_hits.clone();
  let attacker = TestServer::start(Router::new().route(
    "/v1/authority",
    get(move || {
      let observed_hits = observed_hits.clone();
      async move {
        observed_hits.fetch_add(1, Ordering::SeqCst);
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }
    }),
  ))
  .await;
  let redirect_target = attacker.base_url.join("v1/authority").unwrap();
  let trusted = TestServer::start(Router::new().route(
    "/v1/authority",
    get(move || {
      let redirect_target = redirect_target.clone();
      async move { redirect_response(redirect_target) }
    }),
  ))
  .await;

  assert!(InspectRunStore::connect(trusted.base_url.clone()).await.is_err());
  assert_eq!(attacker_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn write_redirects_never_send_commit_metadata_or_artifact_bytes_to_another_origin() {
  let captured = Arc::new(Mutex::new(Vec::<(String, usize)>::new()));
  let observed = captured.clone();
  let attacker = TestServer::start(Router::new().fallback(move |request: AxumRequest| {
    let observed = observed.clone();
    async move {
      let path = request.uri().path().to_string();
      let bytes = to_bytes(request.into_body(), 1024 * 1024).await.expect("captured redirect body");
      observed.lock().expect("redirect capture").push((path, bytes.len()));
      response(
        StatusCode::SERVICE_UNAVAILABLE,
        ARTIFACT_UPLOAD_MEDIA_TYPE,
        Body::from(
          serde_json::to_vec(&ArtifactApiError {
            error: error_code("auv.test.redirected"),
          })
          .unwrap(),
        ),
      )
    }
  }))
  .await;
  let content_artifact_id = ArtifactId::new();
  let content_request = artifact_request(authority_id(), content_artifact_id, IdempotencyKey::new());
  let state = WriteRedirectState {
    attacker_base_url: attacker.base_url.clone(),
    draft: ArtifactUploadDraft::new(
      ArtifactUploadId::new(),
      ArtifactUri::from_ids(run_id(), content_artifact_id),
      Timestamp::new(1_784_620_800, 0).unwrap(),
    ),
    draft_calls: Arc::new(AtomicUsize::new(0)),
  };
  let trusted = TestServer::start(
    Router::new()
      .route(
        "/v1/authority",
        get(|| async {
          run_json(
            StatusCode::OK,
            &AuthorityResponse {
              authority_id: authority_id(),
            },
          )
        }),
      )
      .route("/v1/runs/{run_id}/commits", post(redirecting_commit))
      .route("/v1/runs/{run_id}/artifact-uploads", post(redirecting_draft))
      .route("/v1/runs/{run_id}/artifact-uploads/{upload_id}/content", put(redirecting_content))
      .route("/v1/runs/{run_id}/commits/by-idempotency-key/{key}", get(|| async { run_json(StatusCode::NOT_FOUND, &RunApiError::NotFound) }))
      .with_state(state),
  )
  .await;
  let store = InspectRunStore::connect(trusted.base_url.clone()).await.expect("connect");

  assert!(store.commit(sample_commit_request()).await.is_err());
  assert!(
    store
      .write_artifact(artifact_request(authority_id(), ArtifactId::new(), IdempotencyKey::new()), Box::pin(Cursor::new(b"abc".to_vec())),)
      .await
      .is_err()
  );
  assert!(store.write_artifact(content_request, Box::pin(Cursor::new(b"abc".to_vec()))).await.is_err());
  assert!(captured.lock().expect("redirect capture").is_empty());
}

async fn redirecting_commit(State(state): State<WriteRedirectState>) -> Response {
  redirect_response(state.attacker_base_url.join("stolen-commit").unwrap())
}

async fn redirecting_draft(State(state): State<WriteRedirectState>) -> Response {
  if state.draft_calls.fetch_add(1, Ordering::SeqCst) == 0 {
    redirect_response(state.attacker_base_url.join("stolen-draft").unwrap())
  } else {
    response(StatusCode::CREATED, ARTIFACT_UPLOAD_MEDIA_TYPE, Body::from(serde_json::to_vec(&state.draft).unwrap()))
  }
}

async fn redirecting_content(State(state): State<WriteRedirectState>) -> Response {
  redirect_response(state.attacker_base_url.join("stolen-content").unwrap())
}

fn redirect_response(target: Url) -> Response {
  let mut response = Response::new(Body::empty());
  *response.status_mut() = StatusCode::TEMPORARY_REDIRECT;
  response.headers_mut().insert("Location", HeaderValue::from_str(target.as_str()).unwrap());
  response
}

#[tokio::test]
async fn client_reconstructs_every_typed_http_error_class() {
  let rejected_code = error_code("auv.test.rejected");
  let rejected = TestServer::start(router(Arc::new(FaultStore::commit(CommitError::Rejected(rejected_code.clone()))))).await;
  let rejected = InspectRunStore::connect(rejected.base_url.clone()).await.expect("connect rejected store");
  assert_eq!(rejected.commit(sample_commit_request()).await.unwrap_err(), CommitError::Rejected(rejected_code));

  let ahead = ReadError::CursorAhead {
    requested_after: RunRevision::new(9).expect("requested"),
    latest: RunRevision::new(4).expect("latest"),
  };
  assert_read_error(ahead.clone(), |store| async move {
    store.commits_after(run_id(), RunRevision::new(9).unwrap(), PageLimit::new(1).unwrap()).await.map(|_| ())
  })
  .await;

  let gap = ReadError::HistoryGap {
    requested_after: RunRevision::new(4).expect("requested"),
    earliest_available: RunRevision::new(9).expect("earliest"),
  };
  assert_read_error(gap.clone(), |store| async move {
    store.commits_after(run_id(), RunRevision::new(4).unwrap(), PageLimit::new(1).unwrap()).await.map(|_| ())
  })
  .await;

  let integrity = ReadError::Integrity(error_code("auv.test.integrity"));
  assert_read_error(integrity.clone(), |store| async move { store.load_snapshot(run_id()).await.map(|_| ()) }).await;

  let unavailable = ReadError::Unavailable(error_code("auv.test.unavailable"));
  assert_read_error(unavailable.clone(), |store| async move { store.load_snapshot(run_id()).await.map(|_| ()) }).await;

  let missing_server = TestServer::start(router(Arc::new(MemoryRunStore::new(authority_id())))).await;
  let missing = InspectRunStore::connect(missing_server.base_url.clone()).await.expect("connect missing store");
  let uri = ArtifactUri::from_ids(run_id(), ArtifactId::new());
  match missing.open_artifact(uri).await {
    Err(error) => assert_eq!(error, ReadError::NotFound),
    Ok(_) => panic!("missing artifact unexpectedly opened"),
  }
}

#[tokio::test]
async fn client_reconstructs_every_artifact_write_error_class() {
  let cases = [
    ArtifactWriteError::AuthorityMismatch {
      expected: authority_id(),
      received: authority_id(),
    },
    ArtifactWriteError::IdempotencyMismatch,
    ArtifactWriteError::Rejected(error_code("auv.test.rejected")),
    ArtifactWriteError::Integrity(error_code("auv.test.integrity")),
    ArtifactWriteError::Unavailable(error_code("auv.test.unavailable")),
    ArtifactWriteError::PublicationUnknown(error_code("auv.inspect.publication_unknown")),
  ];

  for expected in cases {
    let server = TestServer::start(router(Arc::new(FaultStore::artifact(expected.clone())))).await;
    let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect artifact error store");
    let request = artifact_request(authority_id(), ArtifactId::new(), IdempotencyKey::new());

    let error = store.write_artifact(request, Box::pin(Cursor::new(b"abc".to_vec()))).await.unwrap_err();

    assert_eq!(error, expected);
  }
}

async fn assert_read_error<F, Fut>(expected: ReadError, operation: F)
where
  F: FnOnce(InspectRunStore) -> Fut,
  Fut: std::future::Future<Output = Result<(), ReadError>>,
{
  let server = TestServer::start(router(Arc::new(FaultStore::read(expected.clone())))).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect fault store");
  assert_eq!(operation(store).await.unwrap_err(), expected);
}

struct PollProbe {
  polls: Arc<AtomicUsize>,
}

impl AsyncRead for PollProbe {
  fn poll_read(self: Pin<&mut Self>, _context: &mut Context<'_>, _buffer: &mut [u8]) -> Poll<io::Result<usize>> {
    self.polls.fetch_add(1, Ordering::SeqCst);
    Poll::Ready(Ok(0))
  }
}

struct FailingPollProbe {
  polls: Arc<AtomicUsize>,
}

impl AsyncRead for FailingPollProbe {
  fn poll_read(self: Pin<&mut Self>, _context: &mut Context<'_>, _buffer: &mut [u8]) -> Poll<io::Result<usize>> {
    self.polls.fetch_add(1, Ordering::SeqCst);
    Poll::Ready(Err(io::Error::other("one-shot body was polled")))
  }
}

struct CountingReader {
  inner: Cursor<Vec<u8>>,
  polls: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct DraftPostBarrier {
  entered: Arc<tokio::sync::Barrier>,
  decided: Arc<tokio::sync::Barrier>,
}

async fn synchronize_draft_posts(State(state): State<DraftPostBarrier>, request: AxumRequest, next: Next) -> Response {
  if request.method() != Method::POST || !request.uri().path().ends_with("/artifact-uploads") {
    return next.run(request).await;
  }
  state.entered.wait().await;
  let response = next.run(request).await;
  state.decided.wait().await;
  response
}

impl AsyncRead for CountingReader {
  fn poll_read(mut self: Pin<&mut Self>, context: &mut Context<'_>, buffer: &mut [u8]) -> Poll<io::Result<usize>> {
    self.polls.fetch_add(1, Ordering::SeqCst);
    Pin::new(&mut self.inner).poll_read(context, buffer)
  }
}

#[tokio::test]
async fn binary_write_read_resolution_and_published_replay_preserve_one_shot_bodies() {
  let server = TestServer::start_store(Arc::new(MemoryRunStore::new(authority_id()))).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");
  let artifact_id = ArtifactId::new();
  let key = IdempotencyKey::new();
  let request = artifact_request(authority_id(), artifact_id, key);

  let published = store.write_artifact(request.clone(), Box::pin(Cursor::new(b"abc".to_vec()))).await.expect("publish");
  assert!(published.is_appended());
  let replacement_polls = Arc::new(AtomicUsize::new(0));
  let replayed = store
    .write_artifact(
      request,
      Box::pin(FailingPollProbe {
        polls: replacement_polls.clone(),
      }),
    )
    .await
    .expect("published replay");
  assert!(!replayed.is_appended());
  assert_eq!(replayed.commit(), published.commit());
  assert_eq!(replacement_polls.load(Ordering::SeqCst), 0);

  let uri = ArtifactUri::from_ids(run_id(), artifact_id);
  let mut reader = store.open_artifact(uri.clone()).await.expect("open artifact");
  let mut bytes = Vec::new();
  while let Some(chunk) = reader.next().await {
    bytes.extend_from_slice(&chunk.expect("verified chunk"));
  }
  assert_eq!(bytes, b"abc");

  let missing = ArtifactUri::from_ids(run_id(), ArtifactId::new());
  let resolved = store.resolve_artifacts(vec![uri.clone(), missing.clone(), uri.clone()]).await.expect("resolve artifacts");
  assert_eq!(resolved.len(), 3);
  assert_eq!(resolved[0], resolved[2]);
  assert!(matches!(&resolved[0], auv_tracing_inspect::ResolvedArtifact::Available { uri: found, .. } if found == &uri));
  assert!(matches!(&resolved[1], auv_tracing_inspect::ResolvedArtifact::NotFound { uri: found } if found == &missing));
}

#[tokio::test]
async fn equal_concurrent_upload_posts_admit_one_body_before_either_client_can_put() {
  let backing = BlockingArtifactStore::new();
  let barrier = DraftPostBarrier {
    entered: Arc::new(tokio::sync::Barrier::new(2)),
    decided: Arc::new(tokio::sync::Barrier::new(2)),
  };
  let app = router(Arc::new(backing.clone())).layer(middleware::from_fn_with_state(barrier, synchronize_draft_posts));
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");
  let request = artifact_request(authority_id(), ArtifactId::new(), IdempotencyKey::new());
  let first_store = store.clone();
  let first_request = request.clone();
  let first_polls = Arc::new(AtomicUsize::new(0));
  let first_reader_polls = first_polls.clone();
  let first = tokio::spawn(async move {
    first_store
      .write_artifact(
        first_request,
        Box::pin(CountingReader {
          inner: Cursor::new(b"abc".to_vec()),
          polls: first_reader_polls,
        }),
      )
      .await
  });
  let second_polls = Arc::new(AtomicUsize::new(0));
  let second_reader_polls = second_polls.clone();
  let second = tokio::spawn(async move {
    store
      .write_artifact(
        request,
        Box::pin(CountingReader {
          inner: Cursor::new(b"abc".to_vec()),
          polls: second_reader_polls,
        }),
      )
      .await
  });
  backing.entered.wait().await;
  tokio::time::timeout(std::time::Duration::from_secs(1), async {
    while !first.is_finished() && !second.is_finished() {
      tokio::task::yield_now().await;
    }
  })
  .await
  .expect("busy upload must finish while admitted upload is blocked");
  backing.release.notify_one();
  let results = [
    first.await.expect("first upload task"),
    second.await.expect("second upload task"),
  ];
  assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
  assert_eq!(results.iter().filter(|result| matches!(result, Err(ArtifactWriteError::PublicationUnknown(_)))).count(), 1);
  assert!(!results.iter().any(|result| matches!(result, Err(ArtifactWriteError::IdempotencyMismatch))));
  assert_eq!(backing.write_calls.load(Ordering::SeqCst), 1);
  assert_eq!(usize::from(first_polls.load(Ordering::SeqCst) > 0) + usize::from(second_polls.load(Ordering::SeqCst) > 0), 1);
}

#[tokio::test]
async fn same_client_retry_reacquires_after_a_safe_body_failure() {
  let backing = MemoryRunStore::new(authority_id());
  let server = TestServer::start(router(Arc::new(backing.clone()))).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");
  let request = artifact_request(authority_id(), ArtifactId::new(), IdempotencyKey::new());

  let first = store.write_artifact(request.clone(), Box::pin(Cursor::new(b"ab".to_vec()))).await;
  assert!(matches!(first, Err(ArtifactWriteError::Integrity(_))));

  let retried = store.write_artifact(request, Box::pin(Cursor::new(b"abc".to_vec()))).await.expect("reacquired upload");
  assert!(retried.is_appended());
}

#[derive(Clone, Default)]
struct LostDraftResponseProbe {
  calls: Arc<AtomicUsize>,
  admissions: Arc<Mutex<Vec<String>>>,
}

#[tokio::test]
async fn lost_draft_response_replays_the_post_with_the_same_admission() {
  let backing = MemoryRunStore::new(authority_id());
  let probe = LostDraftResponseProbe::default();
  let app = router(Arc::new(backing)).layer(middleware::from_fn_with_state(probe.clone(), lose_first_draft_response));
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");
  let polls = Arc::new(AtomicUsize::new(0));
  let request = artifact_request(authority_id(), ArtifactId::new(), IdempotencyKey::new());

  let result = store
    .write_artifact(
      request,
      Box::pin(CountingReader {
        inner: Cursor::new(b"abc".to_vec()),
        polls: polls.clone(),
      }),
    )
    .await
    .expect("replayed draft response");

  assert!(result.is_appended());
  assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
  let admissions = probe.admissions.lock().expect("admission probe");
  assert_eq!(admissions.len(), 2);
  assert_eq!(admissions[0], admissions[1]);
  assert!(polls.load(Ordering::SeqCst) > 0);
}

async fn lose_first_draft_response(State(probe): State<LostDraftResponseProbe>, request: AxumRequest, next: Next) -> Response {
  let is_draft = request.method() == Method::POST && request.uri().path().ends_with("/artifact-uploads");
  if !is_draft {
    return next.run(request).await;
  }

  let admission =
    request.headers().get(ARTIFACT_UPLOAD_ADMISSION_HEADER).and_then(|value| value.to_str().ok()).expect("draft admission").to_owned();
  probe.admissions.lock().expect("admission probe").push(admission);
  let response = next.run(request).await;
  if probe.calls.fetch_add(1, Ordering::SeqCst) != 0 {
    return response;
  }

  let (parts, _) = response.into_parts();
  let body = Body::from_stream(futures_util::stream::once(async {
    Err::<bytes::Bytes, _>(io::Error::new(io::ErrorKind::ConnectionReset, "lost draft response"))
  }));
  Response::from_parts(parts, body)
}

#[tokio::test]
async fn overlong_one_shot_upload_reaches_the_store_and_publishes_no_fact() {
  let backing = MemoryRunStore::new(authority_id());
  let server = TestServer::start_store(Arc::new(backing.clone())).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");
  let key = IdempotencyKey::new();
  let request = artifact_request(authority_id(), ArtifactId::new(), key);

  let error = store.write_artifact(request, Box::pin(Cursor::new(b"abcd".to_vec()))).await.unwrap_err();

  assert!(matches!(error, ArtifactWriteError::Integrity(_)));
  assert!(backing.lookup_commit(run_id(), key).await.expect("lookup").is_none());
}

#[tokio::test]
async fn authority_mismatch_is_rejected_before_artifact_body_polling() {
  let server = TestServer::start(router(Arc::new(MemoryRunStore::new(authority_id())))).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");
  let polls = Arc::new(AtomicUsize::new(0));
  let request = artifact_request(OTHER_AUTHORITY.parse().expect("other authority"), ArtifactId::new(), IdempotencyKey::new());

  let error = store
    .write_artifact(
      request,
      Box::pin(PollProbe {
        polls: polls.clone(),
      }),
    )
    .await
    .unwrap_err();

  assert_eq!(
    error,
    ArtifactWriteError::AuthorityMismatch {
      expected: authority_id(),
      received: OTHER_AUTHORITY.parse().expect("other authority"),
    }
  );
  assert_eq!(polls.load(Ordering::SeqCst), 0);
}

#[derive(Clone)]
struct LostResponseState {
  store: MemoryRunStore,
  draft: ArtifactUploadDraft,
  request: StoreArtifactRequest,
  lookup_calls: Arc<AtomicUsize>,
  lookup_mode: usize,
}

#[tokio::test]
async fn unknown_publication_transport_outcome_uses_exactly_one_lookup() {
  let artifact_id = ArtifactId::new();
  let key = IdempotencyKey::new();
  let request = artifact_request(authority_id(), artifact_id, key);
  let draft: ArtifactUploadDraft = serde_json::from_value(serde_json::json!({
    "upload_id": "019f8b1e-4b2d-7a00-8f00-000000000005",
    "artifact_uri": ArtifactUri::from_ids(run_id(), artifact_id),
    "expires_at": {"unix_seconds": 1_784_620_800_i64, "nanoseconds": 0},
  }))
  .expect("draft");
  let state = LostResponseState {
    store: MemoryRunStore::new(authority_id()),
    draft,
    request: request.clone(),
    lookup_calls: Arc::new(AtomicUsize::new(0)),
    lookup_mode: 0,
  };
  let app = Router::new()
    .route(
      "/v1/authority",
      get(|| async {
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }),
    )
    .route("/v1/runs/{run_id}/artifact-uploads", post(lost_draft))
    .route("/v1/runs/{run_id}/artifact-uploads/{upload_id}/content", put(lost_content))
    .route("/v1/runs/{run_id}/commits/by-idempotency-key/{key}", get(lost_lookup))
    .with_state(state.clone());
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");

  let result = store.write_artifact(request, Box::pin(Cursor::new(b"abc".to_vec()))).await.expect("resolved publication");

  assert_eq!(result.commit().idempotency_key(), key);
  assert_eq!(state.lookup_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn artifact_recovery_distinguishes_missing_and_mismatching_publication_lookups() {
  for lookup_mode in [1, 2] {
    let artifact_id = ArtifactId::new();
    let key = IdempotencyKey::new();
    let request = artifact_request(authority_id(), artifact_id, key);
    let draft: ArtifactUploadDraft = serde_json::from_value(serde_json::json!({
      "upload_id": ArtifactId::new().to_string(),
      "artifact_uri": ArtifactUri::from_ids(run_id(), artifact_id),
      "expires_at": {"unix_seconds": 1_784_620_800_i64, "nanoseconds": 0},
    }))
    .expect("draft");
    let state = LostResponseState {
      store: MemoryRunStore::new(authority_id()),
      draft,
      request: request.clone(),
      lookup_calls: Arc::new(AtomicUsize::new(0)),
      lookup_mode,
    };
    let app = Router::new()
      .route(
        "/v1/authority",
        get(|| async {
          run_json(
            StatusCode::OK,
            &AuthorityResponse {
              authority_id: authority_id(),
            },
          )
        }),
      )
      .route("/v1/runs/{run_id}/artifact-uploads", post(lost_draft))
      .route("/v1/runs/{run_id}/artifact-uploads/{upload_id}/content", put(lost_content))
      .route("/v1/runs/{run_id}/commits/by-idempotency-key/{key}", get(lost_lookup))
      .with_state(state.clone());
    let server = TestServer::start(app).await;
    let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");

    let error = store.write_artifact(request, Box::pin(Cursor::new(b"abc".to_vec()))).await.unwrap_err();
    if lookup_mode == 1 {
      assert!(matches!(error, ArtifactWriteError::PublicationUnknown(_)));
    } else {
      assert_eq!(error, ArtifactWriteError::IdempotencyMismatch);
    }
    assert_eq!(state.lookup_calls.load(Ordering::SeqCst), 1);
  }
}

#[tokio::test]
async fn server_reported_publication_unknown_triggers_one_client_lookup() {
  let artifact_id = ArtifactId::new();
  let key = IdempotencyKey::new();
  let request = artifact_request(authority_id(), artifact_id, key);
  let draft: ArtifactUploadDraft = serde_json::from_value(serde_json::json!({
    "upload_id": ArtifactId::new().to_string(),
    "artifact_uri": ArtifactUri::from_ids(run_id(), artifact_id),
    "expires_at": {"unix_seconds": 1_784_620_800_i64, "nanoseconds": 0},
  }))
  .unwrap();
  let state = LostResponseState {
    store: MemoryRunStore::new(authority_id()),
    draft,
    request: request.clone(),
    lookup_calls: Arc::new(AtomicUsize::new(0)),
    lookup_mode: 3,
  };
  let app = Router::new()
    .route(
      "/v1/authority",
      get(|| async {
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }),
    )
    .route("/v1/runs/{run_id}/artifact-uploads", post(lost_draft))
    .route("/v1/runs/{run_id}/artifact-uploads/{upload_id}/content", put(lost_content))
    .route("/v1/runs/{run_id}/commits/by-idempotency-key/{key}", get(lost_lookup))
    .with_state(state.clone());
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();

  assert!(matches!(
    store.write_artifact(request, Box::pin(Cursor::new(b"abc".to_vec()))).await,
    Err(ArtifactWriteError::PublicationUnknown(_))
  ));
  assert_eq!(state.lookup_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn replayed_draft_preflight_mismatch_or_lookup_error_never_polls_the_body() {
  for (lookup_mode, expect_mismatch) in [(2, true), (4, false)] {
    let artifact_id = ArtifactId::new();
    let request = artifact_request(authority_id(), artifact_id, IdempotencyKey::new());
    let state = LostResponseState {
      store: MemoryRunStore::new(authority_id()),
      draft: ArtifactUploadDraft::new(
        ArtifactUploadId::new(),
        ArtifactUri::from_ids(run_id(), artifact_id),
        Timestamp::new(1_784_620_800, 0).unwrap(),
      ),
      request: request.clone(),
      lookup_calls: Arc::new(AtomicUsize::new(0)),
      lookup_mode,
    };
    let app = Router::new()
      .route(
        "/v1/authority",
        get(|| async {
          run_json(
            StatusCode::OK,
            &AuthorityResponse {
              authority_id: authority_id(),
            },
          )
        }),
      )
      .route("/v1/runs/{run_id}/artifact-uploads", post(replayed_draft))
      .route("/v1/runs/{run_id}/commits/by-idempotency-key/{key}", get(lost_lookup))
      .with_state(state.clone());
    let server = TestServer::start(app).await;
    let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();
    let polls = Arc::new(AtomicUsize::new(0));

    let error = store
      .write_artifact(
        request,
        Box::pin(FailingPollProbe {
          polls: polls.clone(),
        }),
      )
      .await
      .unwrap_err();

    assert_eq!(matches!(error, ArtifactWriteError::IdempotencyMismatch), expect_mismatch);
    assert_eq!(matches!(error, ArtifactWriteError::PublicationUnknown(_)), !expect_mismatch);
    assert_eq!(state.lookup_calls.load(Ordering::SeqCst), 1);
    assert_eq!(polls.load(Ordering::SeqCst), 0);
  }
}

#[tokio::test]
async fn replayed_draft_preflight_without_a_commit_continues_the_one_shot_upload() {
  let artifact_id = ArtifactId::new();
  let key = IdempotencyKey::new();
  let request = artifact_request(authority_id(), artifact_id, key);
  let state = LostResponseState {
    store: MemoryRunStore::new(authority_id()),
    draft: ArtifactUploadDraft::new(
      ArtifactUploadId::new(),
      ArtifactUri::from_ids(run_id(), artifact_id),
      Timestamp::new(1_784_620_800, 0).unwrap(),
    ),
    request: request.clone(),
    lookup_calls: Arc::new(AtomicUsize::new(0)),
    lookup_mode: 5,
  };
  let app = Router::new()
    .route(
      "/v1/authority",
      get(|| async {
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }),
    )
    .route("/v1/runs/{run_id}/artifact-uploads", post(replayed_draft))
    .route("/v1/runs/{run_id}/artifact-uploads/{upload_id}/content", put(lost_content))
    .route("/v1/runs/{run_id}/commits/by-idempotency-key/{key}", get(lost_lookup))
    .with_state(state.clone());
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();
  let polls = Arc::new(AtomicUsize::new(0));

  let result = store
    .write_artifact(
      request,
      Box::pin(CountingReader {
        inner: Cursor::new(b"abc".to_vec()),
        polls: polls.clone(),
      }),
    )
    .await
    .unwrap();

  assert_eq!(result.commit().idempotency_key(), key);
  assert_eq!(state.lookup_calls.load(Ordering::SeqCst), 2);
  assert!(polls.load(Ordering::SeqCst) > 0);
}

#[derive(Clone, Copy)]
enum DraftAdmissionFault {
  Duplicate,
  Mismatch,
}

#[derive(Clone)]
struct DraftAdmissionFaultState {
  draft: ArtifactUploadDraft,
  fault: DraftAdmissionFault,
}

#[tokio::test]
async fn duplicate_or_mismatched_draft_admission_responses_never_poll_the_body() {
  for fault in [
    DraftAdmissionFault::Duplicate,
    DraftAdmissionFault::Mismatch,
  ] {
    let artifact_id = ArtifactId::new();
    let request = artifact_request(authority_id(), artifact_id, IdempotencyKey::new());
    let state = DraftAdmissionFaultState {
      draft: ArtifactUploadDraft::new(
        ArtifactUploadId::new(),
        ArtifactUri::from_ids(run_id(), artifact_id),
        Timestamp::new(1_784_620_800, 0).unwrap(),
      ),
      fault,
    };
    let app = Router::new()
      .route(
        "/v1/authority",
        get(|| async {
          run_json(
            StatusCode::OK,
            &AuthorityResponse {
              authority_id: authority_id(),
            },
          )
        }),
      )
      .route("/v1/runs/{run_id}/artifact-uploads", post(faulty_draft_admission))
      .with_state(state);
    let server = TestServer::start(app).await;
    let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();
    let polls = Arc::new(AtomicUsize::new(0));

    let error = store
      .write_artifact(
        request,
        Box::pin(FailingPollProbe {
          polls: polls.clone(),
        }),
      )
      .await
      .unwrap_err();

    assert!(matches!(error, ArtifactWriteError::Unavailable(_)));
    assert_eq!(polls.load(Ordering::SeqCst), 0);
  }
}

async fn faulty_draft_admission(State(state): State<DraftAdmissionFaultState>, headers: HeaderMap) -> Response {
  let mut response = response(StatusCode::CREATED, ARTIFACT_UPLOAD_MEDIA_TYPE, Body::from(serde_json::to_vec(&state.draft).unwrap()));
  match state.fault {
    DraftAdmissionFault::Duplicate => {
      let admission = headers.get(ARTIFACT_UPLOAD_ADMISSION_HEADER).unwrap().clone();
      response.headers_mut().append(ARTIFACT_UPLOAD_ADMISSION_HEADER, admission.clone());
      response.headers_mut().append(ARTIFACT_UPLOAD_ADMISSION_HEADER, admission);
    }
    DraftAdmissionFault::Mismatch => {
      response.headers_mut().insert(ARTIFACT_UPLOAD_ADMISSION_HEADER, "019f8b1e-4b2d-7a00-8f00-000000000099".parse().unwrap());
    }
  }
  response
}

#[tokio::test]
async fn ambiguous_artifact_responses_use_one_lookup_and_remain_unknown() {
  for response_mode in 10..=14 {
    let artifact_id = ArtifactId::new();
    let request = artifact_request(authority_id(), artifact_id, IdempotencyKey::new());
    let state = LostResponseState {
      store: MemoryRunStore::new(authority_id()),
      draft: ArtifactUploadDraft::new(
        ArtifactUploadId::new(),
        ArtifactUri::from_ids(run_id(), artifact_id),
        Timestamp::new(1_784_620_800, 0).unwrap(),
      ),
      request: request.clone(),
      lookup_calls: Arc::new(AtomicUsize::new(0)),
      lookup_mode: response_mode,
    };
    let app = Router::new()
      .route(
        "/v1/authority",
        get(|| async {
          run_json(
            StatusCode::OK,
            &AuthorityResponse {
              authority_id: authority_id(),
            },
          )
        }),
      )
      .route("/v1/runs/{run_id}/artifact-uploads", post(lost_draft))
      .route("/v1/runs/{run_id}/artifact-uploads/{upload_id}/content", put(ambiguous_artifact_content))
      .route("/v1/runs/{run_id}/commits/by-idempotency-key/{key}", get(lost_lookup))
      .with_state(state.clone());
    let server = TestServer::start(app).await;
    let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();

    assert!(matches!(
      store.write_artifact(request, Box::pin(Cursor::new(b"abc".to_vec()))).await,
      Err(ArtifactWriteError::PublicationUnknown(_))
    ));
    assert_eq!(state.lookup_calls.load(Ordering::SeqCst), 1);
  }
}

async fn lost_draft(State(state): State<LostResponseState>, headers: HeaderMap) -> Response {
  let bytes = serde_json::to_vec(&state.draft).expect("draft JSON");
  granted_draft_response(StatusCode::CREATED, bytes, &headers)
}

async fn replayed_draft(State(state): State<LostResponseState>, headers: HeaderMap) -> Response {
  let bytes = serde_json::to_vec(&state.draft).expect("draft JSON");
  granted_draft_response(StatusCode::OK, bytes, &headers)
}

fn granted_draft_response(status: StatusCode, bytes: Vec<u8>, request_headers: &HeaderMap) -> Response {
  let mut response = response(status, ARTIFACT_UPLOAD_MEDIA_TYPE, Body::from(bytes));
  response.headers_mut().insert(ARTIFACT_UPLOAD_ADMISSION_HEADER, request_headers.get(ARTIFACT_UPLOAD_ADMISSION_HEADER).unwrap().clone());
  response
}

async fn lost_content(State(state): State<LostResponseState>, request: axum::extract::Request) -> Response {
  let bytes = to_bytes(request.into_body(), 16).await.expect("artifact bytes");
  if state.lookup_mode == 3 {
    return response(
      StatusCode::SERVICE_UNAVAILABLE,
      ARTIFACT_UPLOAD_MEDIA_TYPE,
      Body::from(
        serde_json::to_vec(&ArtifactApiError {
          error: error_code("auv.inspect.publication_unknown"),
        })
        .expect("artifact error JSON"),
      ),
    );
  }
  state.store.write_artifact(state.request.clone(), Box::pin(Cursor::new(bytes.to_vec()))).await.expect("published before loss");
  let stream =
    futures_util::stream::once(async { Err::<axum::body::Bytes, _>(io::Error::new(io::ErrorKind::ConnectionReset, "lost response")) });
  response(StatusCode::CREATED, RUN_MEDIA_TYPE, Body::from_stream(stream))
}

async fn ambiguous_artifact_content(State(state): State<LostResponseState>, request: axum::extract::Request) -> Response {
  let _ = to_bytes(request.into_body(), 16).await.expect("artifact bytes");
  let error = serde_json::to_vec(&ArtifactApiError {
    error: error_code("auv.test.rejected"),
  })
  .unwrap();
  match state.lookup_mode {
    10 => response(StatusCode::UNPROCESSABLE_ENTITY, ARTIFACT_UPLOAD_MEDIA_TYPE, Body::from("{")),
    11 => response(StatusCode::UNPROCESSABLE_ENTITY, "application/json", Body::from(error)),
    12 => response(StatusCode::UNPROCESSABLE_ENTITY, ARTIFACT_UPLOAD_MEDIA_TYPE, Body::from(vec![b' '; 32 * 1024 * 1024 + 1])),
    13 => response(StatusCode::BAD_GATEWAY, ARTIFACT_UPLOAD_MEDIA_TYPE, Body::from(error)),
    14 => response(StatusCode::CREATED, RUN_MEDIA_TYPE, Body::from("{")),
    _ => unreachable!(),
  }
}

async fn lost_lookup(State(state): State<LostResponseState>, Path((_run_id, _key)): Path<(String, String)>) -> Response {
  let call = state.lookup_calls.fetch_add(1, Ordering::SeqCst);
  if state.lookup_mode == 5 && call == 0 {
    return run_json(StatusCode::NOT_FOUND, &RunApiError::NotFound);
  }
  if matches!(state.lookup_mode, 1 | 3 | 10..=14) {
    return run_json(StatusCode::NOT_FOUND, &RunApiError::NotFound);
  }
  if state.lookup_mode == 2 {
    return run_json(StatusCode::OK, &sample_commit(1, state.request.idempotency_key()));
  }
  if state.lookup_mode == 4 {
    return run_json(
      StatusCode::SERVICE_UNAVAILABLE,
      &RunApiError::Unavailable {
        code: error_code("auv.test.lookup_unavailable"),
      },
    );
  }
  let commit = state.store.lookup_commit(run_id(), state.request.idempotency_key()).await.expect("lookup").expect("published commit");
  run_json(StatusCode::OK, &commit)
}

#[derive(Clone)]
struct SseState {
  commits: Arc<Vec<RunCommit>>,
  requests: SseRequestLog,
}

type SseRequestLog = Arc<Mutex<Vec<(u64, Option<String>)>>>;

#[derive(Deserialize)]
struct AfterQuery {
  after_revision: u64,
}

#[tokio::test]
async fn sse_parser_accepts_id_event_multiline_data_and_reconnects_from_last_commit() {
  let commits = vec![
    sample_commit(1, IdempotencyKey::new()),
    sample_commit(2, IdempotencyKey::new()),
  ];
  let state = SseState {
    commits: Arc::new(commits.clone()),
    requests: Arc::new(Mutex::new(Vec::new())),
  };
  let app = Router::new()
    .route(
      "/v1/authority",
      get(|| async {
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }),
    )
    .route("/v1/runs/{run_id}/commits/stream", get(scripted_sse))
    .with_state(state.clone());
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");
  let mut subscription = store.subscribe(run_id(), RunRevision::new(0).unwrap()).await.expect("subscribe");

  assert_eq!(subscription.next().await.unwrap().unwrap(), commits[0]);
  assert_eq!(subscription.next().await.unwrap().unwrap(), commits[1]);

  let requests = state.requests.lock().expect("requests").clone();
  assert_eq!(requests, vec![(0, None), (1, Some("1".to_string()))]);
}

async fn scripted_sse(State(state): State<SseState>, Query(query): Query<AfterQuery>, headers: HeaderMap) -> Response {
  let last_event_id = headers.get("Last-Event-ID").and_then(|value| value.to_str().ok()).map(str::to_owned);
  state.requests.lock().expect("requests").push((query.after_revision, last_event_id));
  let commit = &state.commits[query.after_revision as usize];
  let data = serde_json::to_string(commit).expect("commit JSON");
  let split = data.find(',').expect("JSON comma") + 1;
  let event = format!("id: {}\nevent: commit\ndata: {}\ndata: {}\n\n", commit.revision().get(), &data[..split], &data[split..]);
  response(StatusCode::OK, "text/event-stream", Body::from(event))
}

#[derive(Clone, Copy)]
enum ArtifactReadMode {
  MissingContentType,
  MissingLength,
  MissingDigest,
  BadDigestHeader,
  DuplicateContentType,
  DuplicateLength,
  DuplicateDigest,
  Short,
  Long,
  Corrupt,
  Interrupted,
}

#[tokio::test]
async fn artifact_open_rejects_missing_or_malformed_integrity_headers() {
  for mode in [
    ArtifactReadMode::MissingContentType,
    ArtifactReadMode::MissingLength,
    ArtifactReadMode::MissingDigest,
    ArtifactReadMode::BadDigestHeader,
    ArtifactReadMode::DuplicateContentType,
    ArtifactReadMode::DuplicateLength,
    ArtifactReadMode::DuplicateDigest,
  ] {
    let server = artifact_read_server(mode).await;
    let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");
    let result = store.open_artifact(ArtifactUri::from_ids(run_id(), ArtifactId::new())).await;
    assert!(matches!(result, Err(ReadError::Integrity(_))));
  }
}

#[tokio::test]
async fn artifact_reader_reports_short_long_corrupt_and_interrupted_streams() {
  for mode in [
    ArtifactReadMode::Short,
    ArtifactReadMode::Long,
    ArtifactReadMode::Corrupt,
    ArtifactReadMode::Interrupted,
  ] {
    let server = artifact_read_server(mode).await;
    let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");
    let mut reader = store.open_artifact(ArtifactUri::from_ids(run_id(), ArtifactId::new())).await.expect("open artifact");
    let error = loop {
      match reader.next().await {
        Some(Ok(_)) => {}
        Some(Err(error)) => break error,
        None => panic!("invalid artifact stream ended without a typed error"),
      }
    };
    assert!(matches!(error, auv_tracing::ArtifactReadError::Integrity(_)));
  }
}

async fn artifact_read_server(mode: ArtifactReadMode) -> TestServer {
  use tokio::io::{AsyncReadExt, AsyncWriteExt};

  let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind raw artifact server");
  let address = listener.local_addr().unwrap();
  let base_url = Url::parse(&format!("http://{address}/")).unwrap();
  let task = tokio::spawn(async move {
    for request_index in 0..2 {
      let (mut socket, _) = listener.accept().await.expect("accept request");
      let mut request = Vec::new();
      loop {
        let mut chunk = [0_u8; 1024];
        let count = socket.read(&mut chunk).await.expect("read request");
        if count == 0 {
          break;
        }
        request.extend_from_slice(&chunk[..count]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
          break;
        }
      }
      if request_index == 0 {
        let body = serde_json::to_vec(&AuthorityResponse {
          authority_id: authority_id(),
        })
        .unwrap();
        let headers =
          format!("HTTP/1.1 200 OK\r\nContent-Type: {RUN_MEDIA_TYPE}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.write_all(&body).await.unwrap();
        continue;
      }
      let bytes: &[u8] = match mode {
        ArtifactReadMode::Short => b"ab",
        ArtifactReadMode::Long => b"abcd",
        ArtifactReadMode::Corrupt => b"abd",
        ArtifactReadMode::Interrupted => b"a",
        _ => b"abc",
      };
      let mut headers = String::from("HTTP/1.1 200 OK\r\n");
      if !matches!(mode, ArtifactReadMode::MissingContentType) {
        headers.push_str("Content-Type: text/plain\r\n");
        if matches!(mode, ArtifactReadMode::DuplicateContentType) {
          headers.push_str("Content-Type: text/plain\r\n");
        }
      }
      if !matches!(mode, ArtifactReadMode::MissingLength) {
        let length = if matches!(mode, ArtifactReadMode::Long) {
          4
        } else {
          3
        };
        headers.push_str(&format!("Content-Length: {length}\r\n"));
        if matches!(mode, ArtifactReadMode::DuplicateLength) {
          headers.push_str(&format!("Content-Length: {length}\r\n"));
        }
      }
      if !matches!(mode, ArtifactReadMode::MissingDigest) {
        let digest = if matches!(mode, ArtifactReadMode::BadDigestHeader) {
          "sha-256=not-rfc9530"
        } else {
          "sha-256=:ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=:"
        };
        headers.push_str(&format!("Content-Digest: {digest}\r\n"));
        if matches!(mode, ArtifactReadMode::DuplicateDigest) {
          headers.push_str(&format!("Content-Digest: {digest}\r\n"));
        }
      }
      headers.push_str("Connection: close\r\n\r\n");
      socket.write_all(headers.as_bytes()).await.unwrap();
      socket.write_all(bytes).await.unwrap();
    }
  });
  TestServer { base_url, task }
}

#[derive(Clone)]
struct RawSse {
  content_type: &'static str,
  body: Arc<String>,
}

#[tokio::test]
async fn sse_requires_event_stream_media_type() {
  let server = raw_sse_server(RawSse {
    content_type: "application/json",
    body: Arc::new(String::new()),
  })
  .await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();

  assert!(matches!(store.subscribe(run_id(), RunRevision::new(0).unwrap()).await, Err(ReadError::Integrity(_))));
}

#[tokio::test]
async fn invalid_sse_commit_identity_revision_or_size_is_terminal_without_cursor_acceptance() {
  let other_run = RunId::new();
  let other_authority: AuthorityId = OTHER_AUTHORITY.parse().unwrap();
  let invalid = [
    sse_commit_frame("2", &sample_commit(1, IdempotencyKey::new())),
    sse_commit_frame("1", &sample_commit_for(authority_id(), other_run, 1)),
    sse_commit_frame("1", &sample_commit_for(other_authority, run_id(), 1)),
    sse_commit_frame("2", &sample_commit(2, IdempotencyKey::new())),
    "id: 1\nevent: commit\ndata: {not-json}\n\n".to_string(),
    format!("id: 1\nevent: commit\ndata: {}\n\n", "x".repeat(32 * 1024 * 1024 + 1)),
  ];
  for body in invalid {
    let server = raw_sse_server(RawSse {
      content_type: "text/event-stream",
      body: Arc::new(body),
    })
    .await;
    let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();
    let mut stream = store.subscribe(run_id(), RunRevision::new(0).unwrap()).await.unwrap();
    assert!(matches!(stream.next().await, Some(Err(auv_tracing::SubscriptionError::Store(ReadError::Integrity(_))))));
    assert!(stream.next().await.is_none());
  }
}

#[tokio::test]
async fn sse_gap_and_typed_error_events_are_terminal() {
  let gap = RunStreamGap {
    requested_after: RunRevision::new(4).unwrap(),
    earliest_available: RunRevision::new(9).unwrap(),
  };
  let cases = [
    (format!("event: gap\ndata: {}\n\n", serde_json::to_string(&gap).unwrap()), true),
    (
      format!(
        "event: error\ndata: {}\n\n",
        serde_json::to_string(&RunApiError::Unavailable {
          code: error_code("auv.test.unavailable")
        })
        .unwrap()
      ),
      false,
    ),
  ];
  for (body, is_gap) in cases {
    let server = raw_sse_server(RawSse {
      content_type: "text/event-stream",
      body: Arc::new(body),
    })
    .await;
    let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();
    let mut stream = store.subscribe(run_id(), RunRevision::new(4).unwrap()).await.unwrap();
    let item = stream.next().await.expect("terminal SSE item");
    assert_eq!(matches!(item, Err(auv_tracing::SubscriptionError::Gap { .. })), is_gap);
    assert_eq!(matches!(item, Err(auv_tracing::SubscriptionError::Store(ReadError::Unavailable(_)))), !is_gap);
    assert!(stream.next().await.is_none());
  }
}

#[derive(Clone)]
struct ReconnectGapState {
  calls: Arc<AtomicUsize>,
}

#[tokio::test]
async fn sse_reconnect_http_history_gap_is_terminal_gap() {
  let state = ReconnectGapState {
    calls: Arc::new(AtomicUsize::new(0)),
  };
  let app = Router::new()
    .route(
      "/v1/authority",
      get(|| async {
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }),
    )
    .route(
      "/v1/runs/{run_id}/commits/stream",
      get(|State(state): State<ReconnectGapState>| async move {
        if state.calls.fetch_add(1, Ordering::SeqCst) == 0 {
          return response(StatusCode::OK, "text/event-stream", Body::from(sse_commit_frame("1", &sample_commit(1, IdempotencyKey::new()))));
        }
        run_json(
          StatusCode::GONE,
          &RunApiError::HistoryGap {
            requested_after: RunRevision::new(1).unwrap(),
            earliest_available: RunRevision::new(3).unwrap(),
          },
        )
      }),
    )
    .with_state(state);
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();
  let mut stream = store.subscribe(run_id(), RunRevision::new(0).unwrap()).await.unwrap();

  assert!(stream.next().await.unwrap().is_ok());
  assert!(matches!(
    stream.next().await,
    Some(Err(auv_tracing::SubscriptionError::Gap {
      requested_after,
      earliest_available,
    })) if requested_after == RunRevision::new(1).unwrap() && earliest_available == RunRevision::new(3).unwrap()
  ));
  assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn complete_oversized_sse_comment_is_terminal() {
  let server = raw_sse_server(RawSse {
    content_type: "text/event-stream",
    body: Arc::new(format!(":{}\n\n", "x".repeat(33 * 1024 * 1024))),
  })
  .await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();
  let mut stream = store.subscribe(run_id(), RunRevision::new(0).unwrap()).await.unwrap();

  let item = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next()).await.expect("bounded frame must terminate");
  assert!(matches!(item, Some(Err(auv_tracing::SubscriptionError::Store(ReadError::Integrity(_))))));
}

#[tokio::test]
async fn immediate_sse_eof_uses_bounded_exponential_backoff() {
  let calls = Arc::new(AtomicUsize::new(0));
  let observed = calls.clone();
  let app = Router::new()
    .route(
      "/v1/authority",
      get(|| async {
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }),
    )
    .route(
      "/v1/runs/{run_id}/commits/stream",
      get(move || {
        let calls = calls.clone();
        async move {
          calls.fetch_add(1, Ordering::SeqCst);
          response(StatusCode::OK, "text/event-stream", Body::empty())
        }
      }),
    );
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();
  let mut stream = store.subscribe(run_id(), RunRevision::new(0).unwrap()).await.unwrap();
  let pending = tokio::spawn(async move { stream.next().await });

  tokio::time::sleep(std::time::Duration::from_millis(240)).await;
  let request_count = observed.load(Ordering::SeqCst);
  pending.abort();

  assert!((2..=6).contains(&request_count), "unexpected reconnect count: {request_count}");
}

async fn raw_sse_server(state: RawSse) -> TestServer {
  let app =
    Router::new()
      .route(
        "/v1/authority",
        get(|| async {
          run_json(
            StatusCode::OK,
            &AuthorityResponse {
              authority_id: authority_id(),
            },
          )
        }),
      )
      .route(
        "/v1/runs/{run_id}/commits/stream",
        get(|State(state): State<RawSse>| async move {
          response(StatusCode::OK, state.content_type, Body::from(state.body.as_str().to_owned()))
        }),
      )
      .with_state(state);
  TestServer::start(app).await
}

fn sse_commit_frame(id: &str, commit: &RunCommit) -> String {
  format!("id: {id}\nevent: commit\ndata: {}\n\n", serde_json::to_string(commit).unwrap())
}

fn sample_commit_for(authority_id: AuthorityId, run_id: RunId, revision: u64) -> RunCommit {
  RunCommit::new(
    authority_id,
    run_id,
    RunRevision::new(revision).unwrap(),
    IdempotencyKey::new(),
    Timestamp::new(revision as i64, 0).unwrap(),
    vec![match start_span() {
      RunMutation::StartSpan(span) => RunFact::SpanStarted(span),
      _ => unreachable!(),
    }],
  )
  .unwrap()
}

fn sample_commit(revision: u64, key: IdempotencyKey) -> RunCommit {
  let fact = match start_span() {
    RunMutation::StartSpan(span) => RunFact::SpanStarted(span),
    _ => unreachable!(),
  };
  RunCommit::new(
    authority_id(),
    run_id(),
    RunRevision::new(revision).expect("revision"),
    key,
    Timestamp::new(revision as i64, 0).expect("timestamp"),
    vec![fact],
  )
  .expect("commit")
}

#[tokio::test]
async fn load_snapshot_deliberately_accepts_valid_json_larger_than_thirty_two_mib() {
  let backing = MemoryRunStore::new(authority_id());
  let payload = JsonPayload::from_str(&format!(r#""{}""#, "x".repeat(65_000))).expect("large payload");
  for batch in 0..3 {
    let count = if batch < 2 { 256 } else { 16 };
    let mutations = (0..count)
      .map(|offset| {
        let index = batch * 256 + offset;
        RunMutation::EmitEvent(EventOccurred::new(
          EventId::new(),
          None,
          Timestamp::new(index as i64 + 1, 0).expect("timestamp"),
          EventSchema::new(EventName::parse("auv.test.large").expect("event name"), 1).expect("event schema"),
          payload.clone(),
        ))
      })
      .collect();
    backing
      .commit(RunCommitRequest::new(authority_id(), run_id(), IdempotencyKey::new(), mutations).expect("large commit"))
      .await
      .expect("commit");
  }
  let snapshot = backing.load_snapshot(run_id()).await.expect("snapshot read").expect("snapshot");
  assert!(serde_json::to_vec(&snapshot).expect("snapshot JSON").len() > 32 * 1024 * 1024);
  let server = TestServer::start(router(Arc::new(backing))).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");

  let loaded = store.load_snapshot(run_id()).await.expect("remote snapshot").expect("snapshot");

  assert_eq!(loaded, snapshot);
}

fn run_json(status: StatusCode, value: &impl serde::Serialize) -> Response {
  response(status, RUN_MEDIA_TYPE, Body::from(serde_json::to_vec(value).expect("run JSON")))
}

fn response(status: StatusCode, content_type: &'static str, body: Body) -> Response {
  let mut response = Response::new(body);
  *response.status_mut() = status;
  response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
  response
}

#[test]
fn tokio_task_spawner_requires_an_entered_runtime() {
  assert!(TokioTaskSpawner::current().is_err());
}

#[test]
fn captured_tokio_handle_spawns_from_another_thread() {
  let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
  let spawner = runtime.block_on(async { TokioTaskSpawner::current().expect("spawner") });
  let (sender, receiver) = tokio::sync::oneshot::channel();
  std::thread::spawn(move || {
    auv_tracing::TaskSpawner::spawn(
      &spawner,
      Box::pin(async move {
        let _ = sender.send(());
      }),
    )
    .expect("cross-thread spawn");
  })
  .join()
  .expect("spawning thread");
  runtime.block_on(receiver).expect("spawned task completion");
}

#[derive(Clone)]
struct LostCommitState {
  store: MemoryRunStore,
  post_calls: Arc<AtomicUsize>,
  lookup_calls: Arc<AtomicUsize>,
  mismatched_lookup: bool,
}

#[tokio::test]
async fn ordinary_commit_response_loss_uses_one_lookup_without_reposting() {
  let state = LostCommitState {
    store: MemoryRunStore::new(authority_id()),
    post_calls: Arc::new(AtomicUsize::new(0)),
    lookup_calls: Arc::new(AtomicUsize::new(0)),
    mismatched_lookup: false,
  };
  let app = Router::new()
    .route(
      "/v1/authority",
      get(|| async {
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }),
    )
    .route("/v1/runs/{run_id}/commits", post(lost_commit))
    .route("/v1/runs/{run_id}/commits/by-idempotency-key/{key}", get(lost_commit_lookup))
    .with_state(state.clone());
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");

  let result = store.commit(sample_commit_request()).await.expect("resolved commit");

  assert!(!result.is_appended());
  assert_eq!(state.post_calls.load(Ordering::SeqCst), 1);
  assert_eq!(state.lookup_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ordinary_commit_mismatched_recovery_is_idempotency_mismatch_without_reposting() {
  let state = LostCommitState {
    store: MemoryRunStore::new(authority_id()),
    post_calls: Arc::new(AtomicUsize::new(0)),
    lookup_calls: Arc::new(AtomicUsize::new(0)),
    mismatched_lookup: true,
  };
  let app = Router::new()
    .route(
      "/v1/authority",
      get(|| async {
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }),
    )
    .route("/v1/runs/{run_id}/commits", post(lost_commit))
    .route("/v1/runs/{run_id}/commits/by-idempotency-key/{key}", get(lost_commit_lookup))
    .with_state(state.clone());
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.expect("connect");

  assert_eq!(store.commit(sample_commit_request()).await.unwrap_err(), CommitError::IdempotencyMismatch);
  assert_eq!(state.post_calls.load(Ordering::SeqCst), 1);
  assert_eq!(state.lookup_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ordinary_commit_integrity_response_uses_one_lookup_without_reposting() {
  let state = LostCommitState {
    store: MemoryRunStore::new(authority_id()),
    post_calls: Arc::new(AtomicUsize::new(0)),
    lookup_calls: Arc::new(AtomicUsize::new(0)),
    mismatched_lookup: false,
  };
  let app = Router::new()
    .route(
      "/v1/authority",
      get(|| async {
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }),
    )
    .route("/v1/runs/{run_id}/commits", post(integrity_commit))
    .route("/v1/runs/{run_id}/commits/by-idempotency-key/{key}", get(lost_commit_lookup))
    .with_state(state.clone());
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();

  let result = store.commit(sample_commit_request()).await.expect("lookup resolves accepted commit");

  assert!(!result.is_appended());
  assert_eq!(state.post_calls.load(Ordering::SeqCst), 1);
  assert_eq!(state.lookup_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ordinary_commit_unknown_wire_error_is_preserved_without_reposting() {
  let post_calls = Arc::new(AtomicUsize::new(0));
  let observed_post_calls = post_calls.clone();
  let app = Router::new()
    .route(
      "/v1/authority",
      get(|| async {
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }),
    )
    .route(
      "/v1/runs/{run_id}/commits",
      post(move || {
        let observed_post_calls = observed_post_calls.clone();
        async move {
          observed_post_calls.fetch_add(1, Ordering::SeqCst);
          response(StatusCode::SERVICE_UNAVAILABLE, RUN_MEDIA_TYPE, Body::from(r#"{"unavailable":{"code":"auv.inspect.commit_unknown"}}"#))
        }
      }),
    )
    .route("/v1/runs/{run_id}/commits/by-idempotency-key/{key}", get(|| async { run_json(StatusCode::NOT_FOUND, &RunApiError::NotFound) }));
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();

  assert_eq!(store.commit(sample_commit_request()).await.unwrap_err(), CommitError::CommitUnknown(error_code("auv.inspect.commit_unknown")));
  assert_eq!(post_calls.load(Ordering::SeqCst), 1);
}

async fn lost_commit(State(state): State<LostCommitState>, headers: HeaderMap, request: axum::extract::Request) -> Response {
  state.post_calls.fetch_add(1, Ordering::SeqCst);
  let key = headers.get("Idempotency-Key").unwrap().to_str().unwrap().parse().unwrap();
  let body = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
  let body: auv_tracing_inspect::protocol::RunCommitBody = auv_tracing_inspect::protocol::decode_strict(&body).unwrap();
  state.store.commit(RunCommitRequest::new(body.authority_id, run_id(), key, body.mutations.into_vec()).unwrap()).await.unwrap();
  let stream =
    futures_util::stream::once(async { Err::<axum::body::Bytes, _>(io::Error::new(io::ErrorKind::ConnectionReset, "lost response")) });
  response(StatusCode::CREATED, RUN_MEDIA_TYPE, Body::from_stream(stream))
}

async fn integrity_commit(State(state): State<LostCommitState>, headers: HeaderMap, request: axum::extract::Request) -> Response {
  state.post_calls.fetch_add(1, Ordering::SeqCst);
  let key = headers.get("Idempotency-Key").unwrap().to_str().unwrap().parse().unwrap();
  let body = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
  let body: auv_tracing_inspect::protocol::RunCommitBody = auv_tracing_inspect::protocol::decode_strict(&body).unwrap();
  state.store.commit(RunCommitRequest::new(body.authority_id, run_id(), key, body.mutations.into_vec()).unwrap()).await.unwrap();
  run_json(
    StatusCode::INTERNAL_SERVER_ERROR,
    &RunApiError::Integrity {
      code: error_code("auv.test.integrity"),
    },
  )
}

async fn lost_commit_lookup(State(state): State<LostCommitState>) -> Response {
  state.lookup_calls.fetch_add(1, Ordering::SeqCst);
  if state.mismatched_lookup {
    let mismatched = RunCommit::new(
      authority_id(),
      run_id(),
      RunRevision::new(1).unwrap(),
      KEY.parse().unwrap(),
      Timestamp::new(1, 0).unwrap(),
      vec![RunFact::SpanStarted(SpanStarted::new(
        SPAN.parse().unwrap(),
        None,
        None,
        SpanName::parse("auv.test.mismatch").unwrap(),
        Timestamp::new(1, 0).unwrap(),
        Attributes::empty(),
      ))],
    )
    .unwrap();
    return run_json(StatusCode::OK, &mismatched);
  }
  let commit = state.store.lookup_commit(run_id(), KEY.parse().unwrap()).await.unwrap().unwrap();
  run_json(StatusCode::OK, &commit)
}

#[derive(Clone)]
struct InvalidSuccessState {
  snapshot: auv_tracing::RunSnapshot,
  page: RunCommitPage,
  draft: ArtifactUploadDraft,
  wrong_uri: ArtifactUri,
}

#[tokio::test]
async fn client_validates_success_status_media_and_canonical_identities() {
  let other_run = RunId::new();
  let backing = MemoryRunStore::new(authority_id());
  backing.commit(RunCommitRequest::new(authority_id(), other_run, IdempotencyKey::new(), vec![start_span()]).unwrap()).await.unwrap();
  let snapshot = backing.load_snapshot(other_run).await.unwrap().unwrap();
  let wrong_commit = RunCommit::new(
    OTHER_AUTHORITY.parse().unwrap(),
    run_id(),
    RunRevision::new(1).unwrap(),
    IdempotencyKey::new(),
    Timestamp::new(1, 0).unwrap(),
    vec![match start_span() {
      RunMutation::StartSpan(span) => RunFact::SpanStarted(span),
      _ => unreachable!(),
    }],
  )
  .unwrap();
  let page = RunCommitPage::new(vec![wrong_commit], RunRevision::new(1).unwrap(), false).unwrap();
  let draft = ArtifactUploadDraft::new(
    ArtifactUploadId::new(),
    ArtifactUri::from_ids(run_id(), ArtifactId::new()),
    Timestamp::new(1_784_620_800, 0).unwrap(),
  );
  let state = InvalidSuccessState {
    snapshot,
    page,
    draft,
    wrong_uri: ArtifactUri::from_ids(run_id(), ArtifactId::new()),
  };
  let app = Router::new()
    .route(
      "/v1/authority",
      get(|| async {
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }),
    )
    .route("/v1/runs/{run_id}/snapshot", get(invalid_snapshot))
    .route("/v1/runs/{run_id}/commits", get(invalid_page))
    .route("/v1/runs/{run_id}/artifact-uploads", post(invalid_draft))
    .route("/v1/resources/artifacts/resolve", post(invalid_resolver))
    .with_state(state.clone());
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();

  assert!(matches!(store.load_snapshot(run_id()).await, Err(ReadError::Integrity(_))));
  assert!(matches!(
    store.commits_after(run_id(), RunRevision::new(0).unwrap(), PageLimit::new(1).unwrap()).await,
    Err(ReadError::Integrity(_))
  ));
  let polls = Arc::new(AtomicUsize::new(0));
  let write = store
    .write_artifact(
      artifact_request(authority_id(), ArtifactId::new(), IdempotencyKey::new()),
      Box::pin(PollProbe {
        polls: polls.clone(),
      }),
    )
    .await;
  assert!(matches!(write, Err(ArtifactWriteError::Unavailable(_))));
  assert_eq!(polls.load(Ordering::SeqCst), 0);
  let requested = ArtifactUri::from_ids(run_id(), ArtifactId::new());
  assert!(matches!(store.resolve_artifacts(vec![requested]).await, Err(ReadError::Integrity(_))));
}

#[tokio::test]
async fn resolver_rejects_untrusted_origins_and_noncanonical_artifact_paths() {
  let requested = ArtifactUri::from_ids(run_id(), ArtifactId::new());
  for malicious_origin in [true, false] {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let base_url = Url::parse(&format!("http://{address}/")).unwrap();
    let content_url = if malicious_origin {
      Url::parse(&format!("https://attacker.example/v1/runs/{}/artifacts/{}", requested.run_id(), requested.artifact_id())).unwrap()
    } else {
      base_url.join("v1/runs/not-the-requested-run/artifacts/not-the-requested-artifact").unwrap()
    };
    let payload = ResolveArtifactsResponse::new(vec![ResolvedArtifact::Available {
      uri: requested.clone(),
      content_type: ContentType::parse("text/plain").unwrap(),
      byte_length: ByteLength::new(3).unwrap(),
      sha256: Sha256Digest::from_str(ABC_SHA256).unwrap(),
      content_url,
    }]);
    let app = Router::new()
      .route(
        "/v1/authority",
        get(|| async {
          run_json(
            StatusCode::OK,
            &AuthorityResponse {
              authority_id: authority_id(),
            },
          )
        }),
      )
      .route(
        "/v1/resources/artifacts/resolve",
        post(move || {
          let payload = payload.clone();
          async move { response(StatusCode::OK, "application/json", Body::from(serde_json::to_vec(&payload).unwrap())) }
        }),
      );
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let server = TestServer { base_url, task };
    let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();

    assert!(matches!(store.resolve_artifacts(vec![requested.clone()]).await, Err(ReadError::Integrity(_))));
  }
}

#[tokio::test]
async fn commit_page_cannot_exceed_the_requested_limit() {
  let page = RunCommitPage::new(
    vec![
      sample_commit(1, IdempotencyKey::new()),
      sample_commit(2, IdempotencyKey::new()),
    ],
    RunRevision::new(2).unwrap(),
    false,
  )
  .unwrap();
  let app = Router::new()
    .route(
      "/v1/authority",
      get(|| async {
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }),
    )
    .route(
      "/v1/runs/{run_id}/commits",
      get(move || {
        let page = page.clone();
        async move { run_json(StatusCode::OK, &page) }
      }),
    );
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();

  assert!(matches!(
    store.commits_after(run_id(), RunRevision::new(0).unwrap(), PageLimit::new(1).unwrap()).await,
    Err(ReadError::Integrity(_))
  ));
}

async fn invalid_snapshot(State(state): State<InvalidSuccessState>) -> Response {
  run_json(StatusCode::OK, &state.snapshot)
}

async fn invalid_page(State(state): State<InvalidSuccessState>) -> Response {
  run_json(StatusCode::OK, &state.page)
}

async fn invalid_draft(State(state): State<InvalidSuccessState>) -> Response {
  response(StatusCode::ACCEPTED, "application/json", Body::from(serde_json::to_vec(&state.draft).unwrap()))
}

async fn invalid_resolver(State(state): State<InvalidSuccessState>) -> Response {
  let body = ResolveArtifactsResponse::new(vec![ResolvedArtifact::NotFound {
    uri: state.wrong_uri.clone(),
  }]);
  response(StatusCode::OK, "application/json", Body::from(serde_json::to_vec(&body).unwrap()))
}

#[tokio::test]
async fn invalid_commit_success_does_not_repost_and_becomes_commit_unknown() {
  let post_calls = Arc::new(AtomicUsize::new(0));
  let calls = post_calls.clone();
  let app = Router::new()
    .route(
      "/v1/authority",
      get(|| async {
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }),
    )
    .route(
      "/v1/runs/{run_id}/commits",
      post(move || {
        let calls = calls.clone();
        async move {
          calls.fetch_add(1, Ordering::SeqCst);
          response(
            StatusCode::CREATED,
            "application/json",
            Body::from(serde_json::to_vec(&sample_commit(1, IdempotencyKey::new())).unwrap()),
          )
        }
      }),
    )
    .route("/v1/runs/{run_id}/commits/by-idempotency-key/{key}", get(|| async { run_json(StatusCode::NOT_FOUND, &RunApiError::NotFound) }));
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();

  assert!(matches!(store.commit(sample_commit_request()).await, Err(CommitError::CommitUnknown(_))));
  assert_eq!(post_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn resolver_request_error_uses_application_json_and_maps_typed_unavailable() {
  let app = Router::new()
    .route(
      "/v1/authority",
      get(|| async {
        run_json(
          StatusCode::OK,
          &AuthorityResponse {
            authority_id: authority_id(),
          },
        )
      }),
    )
    .route(
      "/v1/resources/artifacts/resolve",
      post(|| async {
        response(
          StatusCode::SERVICE_UNAVAILABLE,
          "application/json",
          Body::from(
            serde_json::to_vec(&ArtifactApiError {
              error: error_code("auv.test.resolver_unavailable"),
            })
            .expect("resolver error JSON"),
          ),
        )
      }),
    );
  let server = TestServer::start(app).await;
  let store = InspectRunStore::connect(server.base_url.clone()).await.unwrap();

  assert_eq!(
    store.resolve_artifacts(vec![ArtifactUri::from_ids(run_id(), ArtifactId::new())]).await.unwrap_err(),
    ReadError::Unavailable(error_code("auv.test.resolver_unavailable"))
  );
}

#[test]
fn protocol_error_shapes_remain_strict() {
  let duplicate = br#"{"unavailable":{"code":"auv.test.unavailable","code":"auv.test.other"}}"#;
  let unknown = br#"{"unavailable":{"code":"auv.test.unavailable","retry":true}}"#;

  assert!(auv_tracing_inspect::protocol::decode_strict::<RunApiError>(duplicate).is_err());
  assert!(auv_tracing_inspect::protocol::decode_strict::<RunApiError>(unknown).is_err());
}
