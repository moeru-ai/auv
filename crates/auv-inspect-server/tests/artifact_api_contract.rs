use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use auv_inspect_server::{router, router_with_artifact_origin};
use auv_tracing::{
  ArtifactBody, ArtifactId, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, CommitError, CommitResult, ErrorCode,
  IdempotencyKey, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision, RunStore,
  RunSubscription, StoreArtifactRequest,
};
use auv_tracing_inspect::protocol::{
  ARTIFACT_UPLOAD_MEDIA_TYPE, ArtifactUploadDraft, RUN_MEDIA_TYPE, ResolveArtifactsResponse, ResolvedArtifact,
};
use axum::body::{Body, Bytes, to_bytes};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

const AUTHORITY: &str = "019f8b1e-4b2d-7a00-8f00-0000000000aa";
const OTHER_AUTHORITY: &str = "019f8b1e-4b2d-7a00-8f00-0000000000ab";
const RUN: &str = "019f8b1e-4b2d-7a00-8f00-000000000001";
const ARTIFACT: &str = "019f8b1e-4b2d-7a00-8f00-000000000002";
const OTHER_ARTIFACT: &str = "019f8b1e-4b2d-7a00-8f00-000000000003";
const SPAN: &str = "019f8b1e-4b2d-7a00-8f00-000000000004";
const KEY: &str = "019f8b1e-4b2d-7a00-8f00-000000000006";
const OTHER_KEY: &str = "019f8b1e-4b2d-7a00-8f00-000000000007";
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const ABC_CONTENT_DIGEST: &str = "sha-256=:ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=:";

type WriteGate = Arc<Mutex<Option<(Arc<tokio::sync::Barrier>, Arc<tokio::sync::Barrier>)>>>;

#[derive(Clone)]
struct ProbeStore {
  inner: MemoryRunStore,
  authority_override: Arc<Mutex<Option<AuthorityId>>>,
  unknown_after_write: Arc<AtomicBool>,
  unknown_without_write: Arc<AtomicBool>,
  write_calls: Arc<AtomicUsize>,
  lookup_calls: Arc<AtomicUsize>,
  snapshot_calls: Arc<AtomicUsize>,
  delay_lookups: Arc<AtomicBool>,
  fail_lookups: Arc<AtomicBool>,
  write_gate: WriteGate,
  after_write_gate: WriteGate,
}

impl ProbeStore {
  fn new() -> Self {
    Self {
      inner: MemoryRunStore::new(authority_id()),
      authority_override: Arc::new(Mutex::new(None)),
      unknown_after_write: Arc::new(AtomicBool::new(false)),
      unknown_without_write: Arc::new(AtomicBool::new(false)),
      write_calls: Arc::new(AtomicUsize::new(0)),
      lookup_calls: Arc::new(AtomicUsize::new(0)),
      snapshot_calls: Arc::new(AtomicUsize::new(0)),
      delay_lookups: Arc::new(AtomicBool::new(false)),
      fail_lookups: Arc::new(AtomicBool::new(false)),
      write_gate: Arc::new(Mutex::new(None)),
      after_write_gate: Arc::new(Mutex::new(None)),
    }
  }

  fn report_publication_unknown(&self, enabled: bool) {
    self.unknown_after_write.store(enabled, Ordering::SeqCst);
  }

  fn report_unresolved_publication(&self, enabled: bool) {
    self.unknown_without_write.store(enabled, Ordering::SeqCst);
  }

  fn override_authority(&self, authority_id: AuthorityId) {
    *self.authority_override.lock().expect("authority lock") = Some(authority_id);
  }

  fn delay_lookups(&self, enabled: bool) {
    self.delay_lookups.store(enabled, Ordering::SeqCst);
  }

  fn fail_lookups(&self, enabled: bool) {
    self.fail_lookups.store(enabled, Ordering::SeqCst);
  }

  fn gate_next_write(&self, entered: Arc<tokio::sync::Barrier>, release: Arc<tokio::sync::Barrier>) {
    *self.write_gate.lock().expect("write gate") = Some((entered, release));
  }

  fn gate_after_next_write(&self, entered: Arc<tokio::sync::Barrier>, release: Arc<tokio::sync::Barrier>) {
    *self.after_write_gate.lock().expect("after-write gate") = Some((entered, release));
  }
}

impl RunStore for ProbeStore {
  fn authority_id(&self) -> AuthorityId {
    self.authority_override.lock().expect("authority lock").unwrap_or_else(|| self.inner.authority_id())
  }

  fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
    self.inner.commit(request)
  }

  fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
    self.write_calls.fetch_add(1, Ordering::SeqCst);
    let inner = self.inner.clone();
    let unknown_after_write = self.unknown_after_write.clone();
    let unknown_without_write = self.unknown_without_write.clone();
    let write_gate = self.write_gate.lock().expect("write gate").take();
    let after_write_gate = self.after_write_gate.lock().expect("after-write gate").take();
    Box::pin(async move {
      if let Some((entered, release)) = write_gate {
        entered.wait().await;
        release.wait().await;
      }
      if unknown_without_write.load(Ordering::SeqCst) {
        return Err(ArtifactWriteError::PublicationUnknown(error_code("auv.test.publication_unknown")));
      }
      let result = inner.write_artifact(request, body).await;
      if let Some((entered, release)) = after_write_gate {
        entered.wait().await;
        release.wait().await;
      }
      if unknown_after_write.load(Ordering::SeqCst) && result.is_ok() {
        Err(ArtifactWriteError::PublicationUnknown(error_code("auv.test.publication_unknown")))
      } else {
        result
      }
    })
  }

  fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    self.lookup_calls.fetch_add(1, Ordering::SeqCst);
    let inner = self.inner.clone();
    let delay = self.delay_lookups.load(Ordering::SeqCst);
    let fail = self.fail_lookups.load(Ordering::SeqCst);
    Box::pin(async move {
      if delay {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
      }
      if fail {
        return Err(ReadError::Unavailable(error_code("auv.test.lookup_unavailable")));
      }
      inner.lookup_commit(run_id, key).await
    })
  }

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<auv_tracing::RunSnapshot>, ReadError>> {
    self.snapshot_calls.fetch_add(1, Ordering::SeqCst);
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
  AUTHORITY.parse().expect("authority id")
}

fn error_code(value: &str) -> ErrorCode {
  ErrorCode::parse(value).expect("error code")
}

fn draft_json(authority: &str, artifact: &str, span: Option<&str>, purpose: &str, byte_length: u64, sha256: &str) -> Vec<u8> {
  let mut body = json!({
    "authority_id": authority,
    "artifact_id": artifact,
    "purpose": purpose,
    "content_type": "text/plain",
    "byte_length": byte_length,
    "sha256": sha256,
    "attributes": {},
  });
  if let Some(span) = span {
    body["span_id"] = json!(span);
  }
  serde_json::to_vec(&body).expect("draft JSON")
}

fn post_draft(run: &str, key: &str, body: Vec<u8>) -> Request<Body> {
  Request::builder()
    .method("POST")
    .uri(format!("/v1/runs/{run}/artifact-uploads"))
    .header(CONTENT_TYPE, ARTIFACT_UPLOAD_MEDIA_TYPE)
    .header("Idempotency-Key", key)
    .body(Body::from(body))
    .expect("draft request")
}

fn put_content(run: &str, upload_id: impl std::fmt::Display, body: Body) -> Request<Body> {
  Request::builder()
    .method("PUT")
    .uri(format!("/v1/runs/{run}/artifact-uploads/{upload_id}/content"))
    .header(CONTENT_TYPE, "text/plain")
    .header("Content-Digest", ABC_CONTENT_DIGEST)
    .body(body)
    .expect("content request")
}

fn post_ordinary_commit(key: &str) -> Request<Body> {
  Request::builder()
    .method("POST")
    .uri(format!("/v1/runs/{RUN}/commits"))
    .header(CONTENT_TYPE, RUN_MEDIA_TYPE)
    .header("Idempotency-Key", key)
    .body(Body::from(
      serde_json::to_vec(&json!({
        "authority_id": AUTHORITY,
        "mutations": [{
          "start_span": {
            "span_id": SPAN,
            "name": "auv.test.ordinary",
            "started_at": {"unix_seconds": 1, "nanoseconds": 0},
            "attributes": {}
          }
        }]
      }))
      .unwrap(),
    ))
    .unwrap()
}

fn polled_body(bytes: &'static [u8], polls: Arc<AtomicUsize>) -> Body {
  let mut bytes = Some(Bytes::from_static(bytes));
  Body::from_stream(futures_util::stream::poll_fn(move |_| {
    polls.fetch_add(1, Ordering::SeqCst);
    std::task::Poll::Ready(bytes.take().map(Ok::<_, std::io::Error>))
  }))
}

async fn response_bytes(response: axum::response::Response) -> Bytes {
  to_bytes(response.into_body(), 34 * 1024 * 1024).await.expect("response bytes")
}

async fn response_json(response: axum::response::Response) -> Value {
  serde_json::from_slice(&response_bytes(response).await).expect("JSON response")
}

async fn create_draft(app: &axum::Router, artifact: &str, key: &str) -> ArtifactUploadDraft {
  let response = app
    .clone()
    .oneshot(post_draft(RUN, key, draft_json(AUTHORITY, artifact, None, "display.capture", 3, ABC_SHA256)))
    .await
    .expect("draft response");
  assert_eq!(response.status(), StatusCode::CREATED);
  serde_json::from_slice(&response_bytes(response).await).expect("draft response")
}

#[tokio::test]
async fn draft_creation_replays_equal_metadata_and_rejects_both_identity_conflicts() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let body = draft_json(AUTHORITY, ARTIFACT, None, "display.capture", 3, ABC_SHA256);

  let created = app.clone().oneshot(post_draft(RUN, KEY, body.clone())).await.expect("created draft");
  assert_eq!(created.status(), StatusCode::CREATED);
  let created: ArtifactUploadDraft = serde_json::from_slice(&response_bytes(created).await).expect("created body");

  let replayed = app.clone().oneshot(post_draft(RUN, KEY, body)).await.expect("replayed draft");
  assert_eq!(replayed.status(), StatusCode::OK);
  let replayed: ArtifactUploadDraft = serde_json::from_slice(&response_bytes(replayed).await).expect("replayed body");
  assert_eq!(replayed, created);

  let key_conflict = app
    .clone()
    .oneshot(post_draft(RUN, KEY, draft_json(AUTHORITY, ARTIFACT, None, "display.thumbnail", 3, ABC_SHA256)))
    .await
    .expect("key conflict");
  assert_eq!(key_conflict.status(), StatusCode::CONFLICT);

  let uri_conflict = app
    .oneshot(post_draft(RUN, OTHER_KEY, draft_json(AUTHORITY, ARTIFACT, None, "display.capture", 3, ABC_SHA256)))
    .await
    .expect("URI conflict");
  assert_eq!(uri_conflict.status(), StatusCode::CONFLICT);
}

#[tokio::test(start_paused = true)]
async fn unpublished_draft_expires_after_exactly_twenty_four_hours() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let draft = create_draft(&app, ARTIFACT, KEY).await;

  tokio::time::advance(std::time::Duration::from_secs(24 * 60 * 60)).await;
  let polls = Arc::new(AtomicUsize::new(0));
  let response = app.oneshot(put_content(RUN, draft.upload_id(), polled_body(b"abc", polls.clone()))).await.expect("expired response");

  assert_eq!(response.status(), StatusCode::GONE);
  assert_eq!(polls.load(Ordering::SeqCst), 0, "expired upload must reject before polling content");
}

#[tokio::test]
async fn draft_rejects_unknown_span_and_authority_before_store_lookup() {
  let store = ProbeStore::new();
  let app = router(Arc::new(store.clone()));

  let unknown_span = app
    .clone()
    .oneshot(post_draft(RUN, KEY, draft_json(AUTHORITY, ARTIFACT, Some(SPAN), "display.capture", 3, ABC_SHA256)))
    .await
    .expect("unknown span response");
  assert_eq!(unknown_span.status(), StatusCode::NOT_FOUND);

  let lookups_before = store.lookup_calls.load(Ordering::SeqCst);
  let snapshots_before = store.snapshot_calls.load(Ordering::SeqCst);
  let mismatch = app
    .oneshot(post_draft(RUN, OTHER_KEY, draft_json(OTHER_AUTHORITY, OTHER_ARTIFACT, None, "display.capture", 3, ABC_SHA256)))
    .await
    .expect("authority mismatch");
  assert_eq!(mismatch.status(), StatusCode::CONFLICT);
  assert_eq!(store.lookup_calls.load(Ordering::SeqCst), lookups_before);
  assert_eq!(store.snapshot_calls.load(Ordering::SeqCst), snapshots_before);
}

#[tokio::test]
async fn content_upload_checks_overflow_length_and_digest_before_publication() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));

  let overflow = create_draft(&app, ARTIFACT, KEY).await;
  let response = app.clone().oneshot(put_content(RUN, overflow.upload_id(), Body::from("abcd"))).await.expect("overflow response");
  assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

  let short = create_draft(&app, OTHER_ARTIFACT, OTHER_KEY).await;
  let response = app.clone().oneshot(put_content(RUN, short.upload_id(), Body::from("ab"))).await.expect("length response");
  assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

  let third_artifact = ArtifactId::new();
  let third_key = IdempotencyKey::new();
  let digest = create_draft(&app, &third_artifact.to_string(), &third_key.to_string()).await;
  let response = app.oneshot(put_content(RUN, digest.upload_id(), Body::from("abd"))).await.expect("digest response");
  assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn successful_publication_replays_without_polling_and_read_preserves_content_headers() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let draft = create_draft(&app, ARTIFACT, KEY).await;

  let published = app.clone().oneshot(put_content(RUN, draft.upload_id(), Body::from("abc"))).await.expect("publication response");
  assert_eq!(published.status(), StatusCode::CREATED);
  assert_eq!(published.headers().get(CONTENT_TYPE).unwrap(), "application/vnd.auv.run+json; version=1");
  assert_eq!(response_json(published).await["facts"][0]["artifact_published"]["metadata"]["uri"], draft.artifact_uri().to_string());

  let polls = Arc::new(AtomicUsize::new(0));
  let replayed = app
    .clone()
    .oneshot(put_content(RUN, draft.upload_id(), polled_body(b"replacement", polls.clone())))
    .await
    .expect("replayed publication");
  assert_eq!(replayed.status(), StatusCode::OK);
  assert_eq!(polls.load(Ordering::SeqCst), 0, "published replay must not poll a replacement body");

  let read = app
    .oneshot(Request::builder().uri(format!("/v1/runs/{RUN}/artifacts/{ARTIFACT}")).body(Body::empty()).expect("read request"))
    .await
    .expect("read response");
  assert_eq!(read.status(), StatusCode::OK);
  assert_eq!(read.headers().get(CONTENT_TYPE).unwrap(), "text/plain");
  assert_eq!(read.headers().get(CONTENT_LENGTH).unwrap(), "3");
  assert_eq!(read.headers().get("Content-Digest").unwrap(), ABC_CONTENT_DIGEST);
  assert_eq!(response_bytes(read).await, Bytes::from_static(b"abc"));
}

#[tokio::test]
async fn publication_unknown_is_resolved_by_one_lookup_without_reuploading() {
  let store = ProbeStore::new();
  store.report_publication_unknown(true);
  let app = router(Arc::new(store.clone()));
  let draft = create_draft(&app, ARTIFACT, KEY).await;
  let lookups_before = store.lookup_calls.load(Ordering::SeqCst);

  let published = app.oneshot(put_content(RUN, draft.upload_id(), Body::from("abc"))).await.expect("publication response");

  assert_eq!(published.status(), StatusCode::OK);
  assert_eq!(store.write_calls.load(Ordering::SeqCst), 1);
  assert_eq!(store.lookup_calls.load(Ordering::SeqCst) - lookups_before, 1);
}

#[tokio::test]
async fn unresolved_publication_uses_stable_uncertainty_code_instead_of_confirmed_unavailable() {
  let store = ProbeStore::new();
  store.report_unresolved_publication(true);
  let app = router(Arc::new(store.clone()));
  let draft = create_draft(&app, ARTIFACT, KEY).await;
  let lookups_before = store.lookup_calls.load(Ordering::SeqCst);

  let response = app.clone().oneshot(put_content(RUN, draft.upload_id(), Body::from("abc"))).await.unwrap();

  assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
  assert_eq!(response_json(response).await, json!({"error": "auv.inspect.publication_unknown"}));
  assert_eq!(store.lookup_calls.load(Ordering::SeqCst) - lookups_before, 1);

  let repeated =
    app.clone().oneshot(post_draft(RUN, KEY, draft_json(AUTHORITY, ARTIFACT, None, "display.capture", 3, ABC_SHA256))).await.unwrap();
  assert_eq!(repeated.status(), StatusCode::OK);
  let repeated: ArtifactUploadDraft = serde_json::from_slice(&response_bytes(repeated).await).unwrap();
  assert_eq!(repeated, draft);

  let polls = Arc::new(AtomicUsize::new(0));
  let replay = app.oneshot(put_content(RUN, draft.upload_id(), polled_body(b"abc", polls.clone()))).await.unwrap();
  assert_eq!(replay.status(), StatusCode::SERVICE_UNAVAILABLE);
  assert_eq!(response_json(replay).await, json!({"error": "auv.inspect.publication_unknown"}));
  assert_eq!(store.write_calls.load(Ordering::SeqCst), 1);
  assert_eq!(polls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn indeterminate_replay_with_a_mismatching_commit_never_polls_or_republishes() {
  let store = ProbeStore::new();
  store.report_unresolved_publication(true);
  let app = router(Arc::new(store.clone()));
  let draft = create_draft(&app, ARTIFACT, KEY).await;
  let first = app.clone().oneshot(put_content(RUN, draft.upload_id(), Body::from("abc"))).await.unwrap();
  assert_eq!(first.status(), StatusCode::SERVICE_UNAVAILABLE);

  store
    .inner
    .commit(
      RunCommitRequest::new(
        authority_id(),
        RUN.parse().unwrap(),
        KEY.parse().unwrap(),
        vec![auv_tracing::RunMutation::StartSpan(
          auv_tracing::SpanStarted::new(
            SPAN.parse().unwrap(),
            None,
            None,
            auv_tracing::SpanName::parse("auv.test.mismatch").unwrap(),
            auv_tracing::Timestamp::new(1, 0).unwrap(),
            auv_tracing::Attributes::empty(),
          ),
        )],
      )
      .unwrap(),
    )
    .await
    .unwrap();

  let polls = Arc::new(AtomicUsize::new(0));
  let replay = app.oneshot(put_content(RUN, draft.upload_id(), polled_body(b"replacement", polls.clone()))).await.unwrap();
  assert_eq!(replay.status(), StatusCode::CONFLICT);
  assert_eq!(store.write_calls.load(Ordering::SeqCst), 1);
  assert_eq!(polls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn indeterminate_replay_with_lookup_failure_never_polls_or_republishes() {
  let store = ProbeStore::new();
  store.report_unresolved_publication(true);
  let app = router(Arc::new(store.clone()));
  let draft = create_draft(&app, ARTIFACT, KEY).await;
  let first = app.clone().oneshot(put_content(RUN, draft.upload_id(), Body::from("abc"))).await.unwrap();
  assert_eq!(first.status(), StatusCode::SERVICE_UNAVAILABLE);
  store.fail_lookups(true);

  let polls = Arc::new(AtomicUsize::new(0));
  let replay = app.oneshot(put_content(RUN, draft.upload_id(), polled_body(b"replacement", polls.clone()))).await.unwrap();
  assert_eq!(replay.status(), StatusCode::SERVICE_UNAVAILABLE);
  assert_eq!(response_json(replay).await, json!({"error": "auv.inspect.publication_unknown"}));
  assert_eq!(store.write_calls.load(Ordering::SeqCst), 1);
  assert_eq!(polls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn changed_authority_rejects_content_before_body_polling() {
  let store = ProbeStore::new();
  let app = router(Arc::new(store.clone()));
  let draft = create_draft(&app, ARTIFACT, KEY).await;
  store.override_authority(OTHER_AUTHORITY.parse().expect("other authority"));
  let polls = Arc::new(AtomicUsize::new(0));

  let response = app.oneshot(put_content(RUN, draft.upload_id(), polled_body(b"abc", polls.clone()))).await.expect("authority response");

  assert_eq!(response.status(), StatusCode::CONFLICT);
  assert_eq!(polls.load(Ordering::SeqCst), 0);
  assert_eq!(store.write_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn resolver_preserves_partial_results_order_and_duplicate_positions() {
  let app = router_with_artifact_origin(Arc::new(MemoryRunStore::new(authority_id())), url::Url::parse("https://inspect.example/").unwrap())
    .unwrap();
  let draft = create_draft(&app, ARTIFACT, KEY).await;
  let published = app.clone().oneshot(put_content(RUN, draft.upload_id(), Body::from("abc"))).await.expect("publication");
  assert_eq!(published.status(), StatusCode::CREATED);

  let available = ArtifactUri::from_str(&format!("auv://runs/{RUN}/artifacts/{ARTIFACT}")).expect("available URI");
  let missing = ArtifactUri::from_str(&format!("auv://runs/{RUN}/artifacts/{OTHER_ARTIFACT}")).expect("missing URI");
  let response = app
    .clone()
    .oneshot(
      Request::builder()
        .method("POST")
        .uri("/v1/resources/artifacts/resolve")
        .header(CONTENT_TYPE, "application/json")
        .header(HOST, "inspect.example")
        .body(Body::from(
          serde_json::to_vec(&json!({"authority_id": AUTHORITY, "uris": [&available, &missing, &available]})).expect("resolver JSON"),
        ))
        .expect("resolver request"),
    )
    .await
    .expect("resolver response");

  assert_eq!(response.status(), StatusCode::OK);
  let response: ResolveArtifactsResponse = serde_json::from_slice(&response_bytes(response).await).expect("resolver response");
  assert_eq!(response.results().len(), 3);
  assert!(matches!(&response.results()[0], ResolvedArtifact::Available { uri, content_url, .. }
    if uri == &available && content_url.as_str() == format!("https://inspect.example/v1/runs/{RUN}/artifacts/{ARTIFACT}")));
  assert!(matches!(&response.results()[1], ResolvedArtifact::NotFound { uri } if uri == &missing));
  assert_eq!(response.results()[0], response.results()[2]);
}

#[tokio::test]
async fn resolver_validates_authority_and_the_complete_bounded_batch_before_lookup() {
  let store = ProbeStore::new();
  let app = router_with_artifact_origin(Arc::new(store.clone()), url::Url::parse("https://inspect.example/").unwrap()).unwrap();
  let valid_uri = ArtifactUri::from_ids(RUN.parse().expect("run ID"), ARTIFACT.parse().expect("artifact ID"));

  let mismatch =
    app.clone().oneshot(resolve_request(json!({"authority_id": OTHER_AUTHORITY, "uris": [&valid_uri]}))).await.expect("mismatch response");
  assert_eq!(mismatch.status(), StatusCode::CONFLICT);
  assert_eq!(mismatch.headers().get(CONTENT_TYPE).unwrap(), "application/json");

  let malformed = app
    .clone()
    .oneshot(resolve_request(json!({"authority_id": AUTHORITY, "uris": [&valid_uri, "auv://runs/not-a-run/artifacts/not-an-artifact"]})))
    .await
    .expect("malformed response");
  assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);

  let oversized = (0..257).map(|_| valid_uri.clone()).collect::<Vec<_>>();
  let oversized = app.oneshot(resolve_request(json!({"authority_id": AUTHORITY, "uris": oversized}))).await.expect("oversized response");
  assert_eq!(oversized.status(), StatusCode::BAD_REQUEST);

  assert_eq!(store.lookup_calls.load(Ordering::SeqCst), 0);
  assert_eq!(store.snapshot_calls.load(Ordering::SeqCst), 0);
}

fn resolve_request(body: Value) -> Request<Body> {
  Request::builder()
    .method("POST")
    .uri("/v1/resources/artifacts/resolve")
    .header(CONTENT_TYPE, "application/json")
    .header(HOST, "inspect.example")
    .body(Body::from(serde_json::to_vec(&body).expect("resolver JSON")))
    .expect("resolver request")
}

#[tokio::test]
async fn simultaneous_draft_posts_classify_equal_and_conflicting_requests_atomically() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let body = draft_json(AUTHORITY, ARTIFACT, None, "display.capture", 3, ABC_SHA256);
  let (first, second) =
    tokio::join!(app.clone().oneshot(post_draft(RUN, KEY, body.clone())), app.clone().oneshot(post_draft(RUN, KEY, body)),);
  let mut responses = [
    first.expect("first response"),
    second.expect("second response"),
  ];
  responses.sort_by_key(|response| response.status());
  assert_eq!(responses[0].status(), StatusCode::OK);
  assert_eq!(responses[1].status(), StatusCode::CREATED);
  let [first_response, second_response] = responses;
  let first: ArtifactUploadDraft = serde_json::from_slice(&response_bytes(first_response).await).expect("first draft");
  let second: ArtifactUploadDraft = serde_json::from_slice(&response_bytes(second_response).await).expect("second draft");
  assert_eq!(first, second);

  let other_artifact = ArtifactId::new();
  let other_key = IdempotencyKey::new();
  let (first, second) = tokio::join!(
    app.clone().oneshot(post_draft(
      RUN,
      &other_key.to_string(),
      draft_json(AUTHORITY, &other_artifact.to_string(), None, "display.capture", 3, ABC_SHA256),
    )),
    app.oneshot(post_draft(
      RUN,
      &other_key.to_string(),
      draft_json(AUTHORITY, &other_artifact.to_string(), None, "display.thumbnail", 3, ABC_SHA256),
    )),
  );
  let statuses = [
    first.expect("first conflict response").status(),
    second.expect("second conflict response").status(),
  ];
  assert!(statuses.contains(&StatusCode::CREATED));
  assert!(statuses.contains(&StatusCode::CONFLICT));
}

#[tokio::test]
async fn draft_indexes_are_rechecked_after_async_store_lookup() {
  let store = ProbeStore::new();
  store.delay_lookups(true);
  let app = router(Arc::new(store));
  let first_key = IdempotencyKey::new();
  let second_key = IdempotencyKey::new();
  let body = draft_json(AUTHORITY, ARTIFACT, None, "display.capture", 3, ABC_SHA256);

  let (first, second) = tokio::join!(
    app.clone().oneshot(post_draft(RUN, &first_key.to_string(), body.clone())),
    app.oneshot(post_draft(RUN, &second_key.to_string(), body)),
  );
  let statuses = [
    first.expect("first response").status(),
    second.expect("second response").status(),
  ];
  assert!(statuses.contains(&StatusCode::CREATED));
  assert!(statuses.contains(&StatusCode::CONFLICT));
}

#[tokio::test]
async fn simultaneous_content_puts_poll_exactly_one_body() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let draft = create_draft(&app, ARTIFACT, KEY).await;
  let first_polls = Arc::new(AtomicUsize::new(0));
  let second_polls = Arc::new(AtomicUsize::new(0));

  let (first, second) = tokio::join!(
    app.clone().oneshot(put_content(RUN, draft.upload_id(), polled_body(b"abc", first_polls.clone()))),
    app.oneshot(put_content(RUN, draft.upload_id(), polled_body(b"abc", second_polls.clone()))),
  );
  let statuses = [
    first.expect("first response").status(),
    second.expect("second response").status(),
  ];
  assert!(statuses.iter().all(|status| matches!(*status, StatusCode::CREATED | StatusCode::OK | StatusCode::CONFLICT)));
  assert_eq!(usize::from(first_polls.load(Ordering::SeqCst) > 0) + usize::from(second_polls.load(Ordering::SeqCst) > 0), 1);
}

#[tokio::test]
async fn equal_draft_post_during_content_upload_replays_the_same_draft() {
  let store = ProbeStore::new();
  let entered = Arc::new(tokio::sync::Barrier::new(2));
  let release = Arc::new(tokio::sync::Barrier::new(2));
  store.gate_next_write(entered.clone(), release.clone());
  let app = router(Arc::new(store));
  let body = draft_json(AUTHORITY, ARTIFACT, None, "display.capture", 3, ABC_SHA256);
  let draft = create_draft(&app, ARTIFACT, KEY).await;
  let upload = tokio::spawn(app.clone().oneshot(put_content(RUN, draft.upload_id(), Body::from("abc"))));
  entered.wait().await;

  let replay = app.clone().oneshot(post_draft(RUN, KEY, body)).await.expect("draft replay");
  assert_eq!(replay.status(), StatusCode::OK);
  let replay: ArtifactUploadDraft = serde_json::from_slice(&response_bytes(replay).await).expect("replayed draft");
  assert_eq!(replay, draft);

  release.wait().await;
  assert_eq!(upload.await.expect("upload task").expect("upload response").status(), StatusCode::CREATED);
}

#[tokio::test]
async fn cancellation_before_body_completion_restores_pending() {
  let store = ProbeStore::new();
  let entered = Arc::new(tokio::sync::Barrier::new(2));
  let release = Arc::new(tokio::sync::Barrier::new(2));
  store.gate_next_write(entered.clone(), release);
  let app = router(Arc::new(store.clone()));
  let draft = create_draft(&app, ARTIFACT, KEY).await;
  let upload = tokio::spawn(app.clone().oneshot(put_content(RUN, draft.upload_id(), Body::from("abc"))));
  entered.wait().await;
  upload.abort();
  assert!(upload.await.expect_err("upload must be cancelled").is_cancelled());

  let polls = Arc::new(AtomicUsize::new(0));
  let retried = app.oneshot(put_content(RUN, draft.upload_id(), polled_body(b"abc", polls.clone()))).await.expect("retried upload");
  assert_eq!(retried.status(), StatusCode::CREATED);
  assert_eq!(store.write_calls.load(Ordering::SeqCst), 2);
  assert!(polls.load(Ordering::SeqCst) > 0);
}

#[tokio::test]
async fn cancellation_after_body_completion_becomes_lookup_only_indeterminate() {
  let store = ProbeStore::new();
  let entered = Arc::new(tokio::sync::Barrier::new(2));
  let release = Arc::new(tokio::sync::Barrier::new(2));
  store.gate_after_next_write(entered.clone(), release);
  let app = router(Arc::new(store.clone()));
  let draft = create_draft(&app, ARTIFACT, KEY).await;
  let upload = tokio::spawn(app.clone().oneshot(put_content(RUN, draft.upload_id(), Body::from("abc"))));
  entered.wait().await;
  upload.abort();
  assert!(upload.await.expect_err("upload must be cancelled").is_cancelled());

  let polls = Arc::new(AtomicUsize::new(0));
  let retried = app.oneshot(put_content(RUN, draft.upload_id(), polled_body(b"replacement", polls.clone()))).await.unwrap();
  assert_eq!(retried.status(), StatusCode::OK);
  assert_eq!(store.write_calls.load(Ordering::SeqCst), 1);
  assert_eq!(polls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn ordinary_commit_key_conflicts_with_draft_without_polling_content() {
  let store = MemoryRunStore::new(authority_id());
  store
    .commit(
      auv_tracing::RunCommitRequest::new(
        authority_id(),
        RUN.parse().expect("run"),
        KEY.parse().expect("key"),
        vec![auv_tracing::RunMutation::StartSpan(
          auv_tracing::SpanStarted::new(
            SPAN.parse().expect("span"),
            None,
            None,
            auv_tracing::SpanName::parse("auv.test.root").expect("name"),
            auv_tracing::Timestamp::new(1, 0).expect("timestamp"),
            auv_tracing::Attributes::empty(),
          ),
        )],
      )
      .expect("commit request"),
    )
    .await
    .expect("ordinary commit");
  let app = router(Arc::new(store));

  let response = app.oneshot(post_draft(RUN, KEY, draft_json(AUTHORITY, ARTIFACT, None, "display.capture", 3, ABC_SHA256))).await.unwrap();

  assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn draft_reservation_rejects_a_later_ordinary_commit_with_the_same_key() {
  let store = MemoryRunStore::new(authority_id());
  let app = router(Arc::new(store.clone()));
  create_draft(&app, ARTIFACT, KEY).await;

  let response = app.oneshot(post_ordinary_commit(KEY)).await.unwrap();

  assert_eq!(response.status(), StatusCode::CONFLICT);
  assert!(store.lookup_commit(RUN.parse().unwrap(), KEY.parse().unwrap()).await.unwrap().is_none());
}

#[tokio::test]
async fn simultaneous_draft_and_ordinary_commit_admit_exactly_one_reservation() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let (draft, commit) = tokio::join!(
    app.clone().oneshot(post_draft(RUN, KEY, draft_json(AUTHORITY, ARTIFACT, None, "display.capture", 3, ABC_SHA256),)),
    app.oneshot(post_ordinary_commit(KEY)),
  );
  let statuses = [draft.unwrap().status(), commit.unwrap().status()];

  assert!(statuses.contains(&StatusCode::CREATED));
  assert!(statuses.contains(&StatusCode::CONFLICT));
}

#[tokio::test]
async fn concurrent_equal_drafts_after_publication_both_replay() {
  let store = ProbeStore::new();
  let first = router(Arc::new(store.clone()));
  let draft = create_draft(&first, ARTIFACT, KEY).await;
  assert_eq!(first.oneshot(put_content(RUN, draft.upload_id(), Body::from("abc"))).await.unwrap().status(), StatusCode::CREATED);

  store.delay_lookups(true);
  let second = router(Arc::new(store));
  let body = draft_json(AUTHORITY, ARTIFACT, None, "display.capture", 3, ABC_SHA256);
  let (left, right) = tokio::join!(second.clone().oneshot(post_draft(RUN, KEY, body.clone())), second.oneshot(post_draft(RUN, KEY, body)),);

  let left = left.unwrap();
  let right = right.unwrap();
  assert_eq!(left.status(), StatusCode::OK);
  assert_eq!(right.status(), StatusCode::OK);
  let left: ArtifactUploadDraft = serde_json::from_slice(&response_bytes(left).await).unwrap();
  let right: ArtifactUploadDraft = serde_json::from_slice(&response_bytes(right).await).unwrap();
  assert_eq!(left, right);
}

#[tokio::test]
async fn failed_integrity_leaves_the_same_draft_reusable() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let draft = create_draft(&app, ARTIFACT, KEY).await;
  let failed = app.clone().oneshot(put_content(RUN, draft.upload_id(), Body::from("ab"))).await.expect("failed upload");
  assert_eq!(failed.status(), StatusCode::UNPROCESSABLE_ENTITY);

  let retried = app.oneshot(put_content(RUN, draft.upload_id(), Body::from("abc"))).await.expect("retried upload");

  assert_eq!(retried.status(), StatusCode::CREATED);
}

#[tokio::test(start_paused = true)]
async fn draft_replay_does_not_extend_expiry_and_published_identity_never_expires() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let body = draft_json(AUTHORITY, ARTIFACT, None, "display.capture", 3, ABC_SHA256);
  let created = app.clone().oneshot(post_draft(RUN, KEY, body.clone())).await.unwrap();
  let created: ArtifactUploadDraft = serde_json::from_slice(&response_bytes(created).await).unwrap();
  tokio::time::advance(std::time::Duration::from_secs(23 * 60 * 60)).await;
  let replayed = app.clone().oneshot(post_draft(RUN, KEY, body)).await.unwrap();
  let replayed: ArtifactUploadDraft = serde_json::from_slice(&response_bytes(replayed).await).unwrap();
  assert_eq!(replayed.expires_at(), created.expires_at());
  tokio::time::advance(std::time::Duration::from_secs(60 * 60)).await;
  let polls = Arc::new(AtomicUsize::new(0));
  let expired = app.clone().oneshot(put_content(RUN, created.upload_id(), polled_body(b"abc", polls.clone()))).await.unwrap();
  assert_eq!(expired.status(), StatusCode::GONE);
  assert_eq!(polls.load(Ordering::SeqCst), 0);

  let published_draft = create_draft(&app, OTHER_ARTIFACT, OTHER_KEY).await;
  let published = app.clone().oneshot(put_content(RUN, published_draft.upload_id(), Body::from("abc"))).await.unwrap();
  assert_eq!(published.status(), StatusCode::CREATED);
  tokio::time::advance(std::time::Duration::from_secs(48 * 60 * 60)).await;
  let polls = Arc::new(AtomicUsize::new(0));
  let replayed = app.oneshot(put_content(RUN, published_draft.upload_id(), polled_body(b"replacement", polls.clone()))).await.unwrap();
  assert_eq!(replayed.status(), StatusCode::OK);
  assert_eq!(polls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn published_replay_after_router_recreation_uses_store_truth_without_polling() {
  let store = MemoryRunStore::new(authority_id());
  let first = router(Arc::new(store.clone()));
  let draft = create_draft(&first, ARTIFACT, KEY).await;
  assert_eq!(first.oneshot(put_content(RUN, draft.upload_id(), Body::from("abc"))).await.unwrap().status(), StatusCode::CREATED);

  let second = router(Arc::new(store));
  let replayed =
    second.clone().oneshot(post_draft(RUN, KEY, draft_json(AUTHORITY, ARTIFACT, None, "display.capture", 3, ABC_SHA256))).await.unwrap();
  assert_eq!(replayed.status(), StatusCode::OK);
  let replayed: ArtifactUploadDraft = serde_json::from_slice(&response_bytes(replayed).await).unwrap();
  let polls = Arc::new(AtomicUsize::new(0));
  let response = second.oneshot(put_content(RUN, replayed.upload_id(), polled_body(b"replacement", polls.clone()))).await.unwrap();
  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(polls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn content_headers_reject_strictly_before_body_polling() {
  let app = router(Arc::new(MemoryRunStore::new(authority_id())));
  let draft = create_draft(&app, ARTIFACT, KEY).await;
  for (content_type, digest, length, expected) in [
    ("application/octet-stream", ABC_CONTENT_DIGEST, None, StatusCode::UNSUPPORTED_MEDIA_TYPE),
    ("text/plain", "sha-256=ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=", None, StatusCode::BAD_REQUEST),
    ("text/plain", "sha-256=:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=:", None, StatusCode::UNPROCESSABLE_ENTITY),
    ("text/plain", ABC_CONTENT_DIGEST, Some("4"), StatusCode::UNPROCESSABLE_ENTITY),
  ] {
    let polls = Arc::new(AtomicUsize::new(0));
    let mut request = Request::builder()
      .method("PUT")
      .uri(format!("/v1/runs/{RUN}/artifact-uploads/{}/content", draft.upload_id()))
      .header(CONTENT_TYPE, content_type)
      .header("Content-Digest", digest);
    if let Some(length) = length {
      request = request.header(CONTENT_LENGTH, length);
    }
    let response = app.clone().oneshot(request.body(polled_body(b"abc", polls.clone())).unwrap()).await.unwrap();
    assert_eq!(response.status(), expected);
    assert_eq!(polls.load(Ordering::SeqCst), 0);
  }
}

#[tokio::test]
async fn duplicate_artifact_control_headers_are_rejected_before_lookup_or_body_polling() {
  let store = ProbeStore::new();
  let app = router(Arc::new(store.clone()));
  let mut duplicate_media = post_draft(RUN, KEY, draft_json(AUTHORITY, ARTIFACT, None, "display.capture", 3, ABC_SHA256));
  duplicate_media.headers_mut().append(CONTENT_TYPE, "application/json".parse().unwrap());
  let response = app.clone().oneshot(duplicate_media).await.unwrap();
  assert_eq!(response.status(), StatusCode::BAD_REQUEST);

  let mut duplicate_key = post_draft(RUN, KEY, draft_json(AUTHORITY, ARTIFACT, None, "display.capture", 3, ABC_SHA256));
  duplicate_key.headers_mut().append("Idempotency-Key", OTHER_KEY.parse().unwrap());
  let response = app.clone().oneshot(duplicate_key).await.unwrap();
  assert_eq!(response.status(), StatusCode::BAD_REQUEST);
  assert_eq!(store.lookup_calls.load(Ordering::SeqCst), 0);

  let draft = create_draft(&app, ARTIFACT, KEY).await;
  for header in [CONTENT_TYPE.as_str(), "Content-Digest"] {
    let polls = Arc::new(AtomicUsize::new(0));
    let mut request = put_content(RUN, draft.upload_id(), polled_body(b"abc", polls.clone()));
    let duplicate = if header == CONTENT_TYPE.as_str() {
      "application/octet-stream"
    } else {
      "sha-256=:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=:"
    };
    request.headers_mut().append(header, duplicate.parse().unwrap());
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(polls.load(Ordering::SeqCst), 0);
  }
}

#[tokio::test]
async fn request_stream_failure_publishes_nothing_and_leaves_draft_reusable() {
  let store = MemoryRunStore::new(authority_id());
  let app = router(Arc::new(store.clone()));
  let draft = create_draft(&app, ARTIFACT, KEY).await;
  let body = Body::from_stream(futures_util::stream::once(async {
    Err::<Bytes, _>(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "interrupted upload"))
  }));
  let failed = app.clone().oneshot(put_content(RUN, draft.upload_id(), body)).await.unwrap();
  assert_eq!(failed.status(), StatusCode::SERVICE_UNAVAILABLE);
  assert!(store.lookup_commit(RUN.parse().unwrap(), KEY.parse().unwrap()).await.unwrap().is_none());

  assert_eq!(app.oneshot(put_content(RUN, draft.upload_id(), Body::from("abc"))).await.unwrap().status(), StatusCode::CREATED);
}

#[tokio::test]
async fn resolver_uses_only_the_configured_trusted_origin() {
  let app = router_with_artifact_origin(Arc::new(MemoryRunStore::new(authority_id())), url::Url::parse("https://inspect.example/").unwrap())
    .unwrap();
  let draft = create_draft(&app, ARTIFACT, KEY).await;
  assert_eq!(app.clone().oneshot(put_content(RUN, draft.upload_id(), Body::from("abc"))).await.unwrap().status(), StatusCode::CREATED);
  let uri = ArtifactUri::from_ids(RUN.parse().unwrap(), ARTIFACT.parse().unwrap());
  let response = app
    .clone()
    .oneshot(
      Request::builder()
        .method("POST")
        .uri("/v1/resources/artifacts/resolve")
        .header(CONTENT_TYPE, "application/json")
        .header(HOST, "poisoned.example")
        .body(Body::from(serde_json::to_vec(&json!({"authority_id": AUTHORITY, "uris": [&uri]})).unwrap()))
        .unwrap(),
    )
    .await
    .unwrap();
  let response: ResolveArtifactsResponse = serde_json::from_slice(&response_bytes(response).await).unwrap();
  assert!(matches!(&response.results()[0], ResolvedArtifact::Available { content_url, .. }
    if content_url.as_str() == format!("https://inspect.example/v1/runs/{RUN}/artifacts/{ARTIFACT}")));

  assert!(
    router_with_artifact_origin(Arc::new(MemoryRunStore::new(authority_id())), url::Url::parse("https://user@inspect.example/").unwrap(),)
      .is_err()
  );

  let unconfigured = router(Arc::new(MemoryRunStore::new(authority_id())));
  let unavailable = unconfigured
    .oneshot(
      Request::builder()
        .method("POST")
        .uri("/v1/resources/artifacts/resolve")
        .header(CONTENT_TYPE, "application/json")
        .header(HOST, "poisoned.example")
        .body(Body::from(serde_json::to_vec(&json!({"authority_id": AUTHORITY, "uris": [&uri]})).unwrap()))
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
  assert_eq!(unavailable.headers().get(CONTENT_TYPE).unwrap(), "application/json");
}
