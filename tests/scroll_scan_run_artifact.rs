use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use auv_runtime::run_read::{RootArtifactReadError, SCAN_COVERAGE_PURPOSE, publish_scan_coverage};
use auv_runtime::scene_state_read::read_scan_coverage;
use auv_scan::{CompletenessWire, CoverageEntryWire, NegativeEvidenceWire, SCAN_COVERAGE_SCHEMA_VERSION, ScanCoverageWire};
use auv_tracing::{
  ArtifactBody, ArtifactMetadata, ArtifactPurpose, ArtifactReader, ArtifactUri, ArtifactWriteError, Attributes, AuthorityId, BoxFuture,
  ByteLength, CommitError, CommitResult, ContentType, Context, ErrorCode, IdempotencyKey, MemoryRunStore, NewArtifact, PageLimit, ReadError,
  RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision, RunSnapshot, RunStore, RunSubscription, Sha256Digest,
  StoreArtifactRequest, configure, dispatcher,
};
use futures_util::io::Cursor;
use sha2::{Digest, Sha256};

struct RootRunFixture {
  store: Arc<MemoryRunStore>,
  dispatch: auv_tracing::Dispatch,
  root: Context,
  run_id: RunId,
}

impl RootRunFixture {
  fn memory() -> Self {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    Self {
      store,
      dispatch,
      root,
      run_id,
    }
  }

  async fn publish_coverage(&self, coverage: &ScanCoverageWire) -> ArtifactMetadata {
    publish_scan_coverage(Some(&self.root), coverage).await.expect("publish scan coverage").expect("coverage publication enabled")
  }

  async fn publish_bytes(&self, purpose: &str, content_type: &str, bytes: Vec<u8>) -> ArtifactMetadata {
    let artifact = NewArtifact::new(
      ArtifactPurpose::parse(purpose).expect("artifact purpose"),
      ContentType::parse(content_type).expect("artifact content type"),
      ByteLength::new(bytes.len() as u64).expect("artifact byte length"),
      Sha256Digest::new(Sha256::digest(&bytes).into()),
      Attributes::empty(),
      Cursor::new(bytes),
    );
    self
      .root
      .instrument(self.root.in_scope(|| auv_tracing::emit_artifact!(artifact)))
      .await
      .expect("publish fixture artifact")
      .expect("fixture publication enabled")
  }

  async fn snapshot(&self) -> RunSnapshot {
    self.dispatch.flush().await.expect("flush run");
    self.store.load_snapshot(self.run_id).await.expect("load snapshot").expect("run snapshot")
  }
}

struct ArtifactBytesStore {
  inner: Arc<MemoryRunStore>,
  bytes: Vec<u8>,
  opens: AtomicUsize,
}

impl ArtifactBytesStore {
  fn new(inner: Arc<MemoryRunStore>, bytes: Vec<u8>) -> Self {
    Self {
      inner,
      bytes,
      opens: AtomicUsize::new(0),
    }
  }

  fn open_count(&self) -> usize {
    self.opens.load(Ordering::Relaxed)
  }
}

impl RunStore for ArtifactBytesStore {
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

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>> {
    self.inner.load_snapshot(run_id)
  }

  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    self.inner.commits_after(run_id, after, limit)
  }

  fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    self.inner.subscribe(run_id, after)
  }

  fn open_artifact(&self, _uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    self.opens.fetch_add(1, Ordering::Relaxed);
    let bytes = self.bytes.clone();
    Box::pin(async move {
      let reader: ArtifactReader = Box::pin(futures_util::stream::once(async move { Ok(bytes.into()) }));
      Ok(reader)
    })
  }
}

struct RejectArtifactStore {
  inner: MemoryRunStore,
  writes: AtomicUsize,
}

impl RejectArtifactStore {
  fn new() -> Self {
    Self {
      inner: MemoryRunStore::new(AuthorityId::new()),
      writes: AtomicUsize::new(0),
    }
  }
}

impl RunStore for RejectArtifactStore {
  fn authority_id(&self) -> AuthorityId {
    self.inner.authority_id()
  }

  fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
    self.inner.commit(request)
  }

  fn write_artifact(&self, _request: StoreArtifactRequest, _body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
    self.writes.fetch_add(1, Ordering::Relaxed);
    Box::pin(async {
      Err(ArtifactWriteError::Rejected(ErrorCode::parse("auv.test.scan_coverage_artifact_rejected").expect("test error code")))
    })
  }

  fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    self.inner.lookup_commit(run_id, key)
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

fn sample_coverage() -> ScanCoverageWire {
  ScanCoverageWire {
    schema_version: SCAN_COVERAGE_SCHEMA_VERSION.to_string(),
    entries: vec![CoverageEntryWire {
      track_id: "track-1".to_string(),
      last_seen_frame_id: "frame-2".to_string(),
      observation_count: 2,
    }],
    open_uncertainty_codes: vec!["partial-occlusion".to_string()],
    negative_evidence: vec![NegativeEvidenceWire {
      code: "no-new-observation".to_string(),
      after_frame_id: "frame-2".to_string(),
    }],
    completeness: CompletenessWire::Incomplete {
      reason: "fixture retains one uncertainty".to_string(),
    },
  }
}

async fn direct_coverage_result(calls: &AtomicUsize, coverage: ScanCoverageWire) -> ScanCoverageWire {
  calls.fetch_add(1, Ordering::Relaxed);
  let context = Context::current();
  let _ = publish_scan_coverage(Some(&context), &coverage).await;
  coverage
}

#[tokio::test]
async fn scan_coverage_publisher_is_a_noop_without_a_context() {
  let expected = sample_coverage();

  let published = publish_scan_coverage(None, &expected).await.expect("disabled publication");

  assert!(published.is_none());
  assert_eq!(expected, sample_coverage());
}

#[tokio::test]
async fn scan_coverage_round_trips_through_one_snapshot_authority() {
  let fixture = RootRunFixture::memory();
  let expected = sample_coverage();
  let metadata = fixture.publish_coverage(&expected).await;
  let snapshot = fixture.snapshot().await;

  assert_eq!(metadata.purpose().as_str(), SCAN_COVERAGE_PURPOSE);
  assert_eq!(metadata.content_type().to_string(), "application/json");
  assert_eq!(metadata.uri().run_id(), fixture.run_id);
  assert!(snapshot.artifacts().contains_key(metadata.uri()));
  assert_eq!(read_scan_coverage(fixture.store.as_ref(), &snapshot).await.expect("read coverage"), Some(expected));
}

#[tokio::test]
async fn scan_coverage_reader_checks_authority_purpose_content_type_and_uniqueness() {
  let fixture = RootRunFixture::memory();
  let bytes = serde_json::to_vec(&sample_coverage()).expect("serialize coverage");
  fixture.publish_bytes("auv.runtime.other", "application/json", bytes.clone()).await;
  let snapshot = fixture.snapshot().await;

  assert_eq!(read_scan_coverage(fixture.store.as_ref(), &snapshot).await.expect("unrelated artifact"), None);
  let wrong_store = MemoryRunStore::new(AuthorityId::new());
  assert!(matches!(
    read_scan_coverage(&wrong_store, &snapshot).await.expect_err("wrong authority"),
    RootArtifactReadError::SnapshotAuthorityMismatch { .. }
  ));

  let wrong_content = RootRunFixture::memory();
  wrong_content.publish_bytes(SCAN_COVERAGE_PURPOSE, "application/problem+json", bytes.clone()).await;
  let snapshot = wrong_content.snapshot().await;
  assert!(matches!(
    read_scan_coverage(wrong_content.store.as_ref(), &snapshot).await.expect_err("wrong content type"),
    RootArtifactReadError::WrongContentType { .. }
  ));

  let duplicate = RootRunFixture::memory();
  duplicate.publish_bytes(SCAN_COVERAGE_PURPOSE, "application/json", bytes.clone()).await;
  duplicate.publish_bytes(SCAN_COVERAGE_PURPOSE, "application/json", bytes).await;
  let snapshot = duplicate.snapshot().await;
  assert!(matches!(
    read_scan_coverage(duplicate.store.as_ref(), &snapshot).await.expect_err("duplicate coverage"),
    RootArtifactReadError::AmbiguousPurpose { actual: 2, .. }
  ));
}

#[tokio::test]
async fn scan_coverage_reader_checks_committed_length_and_digest() {
  let fixture = RootRunFixture::memory();
  let expected = sample_coverage();
  let bytes = serde_json::to_vec(&expected).expect("serialize coverage");
  fixture.publish_coverage(&expected).await;
  let snapshot = fixture.snapshot().await;

  let short_store = ArtifactBytesStore::new(fixture.store.clone(), bytes[..bytes.len() - 1].to_vec());
  assert!(matches!(
    read_scan_coverage(&short_store, &snapshot).await.expect_err("short artifact"),
    RootArtifactReadError::LengthMismatch { .. }
  ));
  assert_eq!(short_store.open_count(), 1);

  let mut changed = bytes;
  *changed.last_mut().expect("coverage bytes") ^= 1;
  let corrupt_store = ArtifactBytesStore::new(fixture.store.clone(), changed);
  assert!(matches!(
    read_scan_coverage(&corrupt_store, &snapshot).await.expect_err("corrupt artifact"),
    RootArtifactReadError::DigestMismatch { .. }
  ));
}

#[tokio::test]
async fn scan_coverage_reader_rejects_malformed_json_and_wrong_schema() {
  let malformed = RootRunFixture::memory();
  malformed.publish_bytes(SCAN_COVERAGE_PURPOSE, "application/json", br#"{"schema_version":"#.to_vec()).await;
  let snapshot = malformed.snapshot().await;
  assert!(matches!(
    read_scan_coverage(malformed.store.as_ref(), &snapshot).await.expect_err("malformed coverage"),
    RootArtifactReadError::MalformedJson { .. }
  ));

  let wrong_schema = RootRunFixture::memory();
  let mut coverage = sample_coverage();
  coverage.schema_version = "scan-coverage-v999".to_string();
  let bytes = serde_json::to_vec(&coverage).expect("serialize wrong schema");
  wrong_schema.publish_bytes(SCAN_COVERAGE_PURPOSE, "application/json", bytes).await;
  let snapshot = wrong_schema.snapshot().await;
  assert!(matches!(
    read_scan_coverage(wrong_schema.store.as_ref(), &snapshot).await.expect_err("wrong schema"),
    RootArtifactReadError::InvalidPayload { .. }
  ));
}

#[tokio::test]
async fn recording_failure_does_not_replace_or_reexecute_the_direct_coverage_value() {
  let store = Arc::new(RejectArtifactStore::new());
  let dispatch = configure().run_store(store.clone()).build().expect("rejecting dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let calls = AtomicUsize::new(0);
  let expected = sample_coverage();
  let future = root.in_scope(|| direct_coverage_result(&calls, expected.clone()));

  let direct = root.instrument(future).await;

  assert_eq!(direct, expected);
  assert_eq!(calls.load(Ordering::Relaxed), 1);
  assert_eq!(store.writes.load(Ordering::Relaxed), 1);
  let flush = dispatch.flush().await.expect_err("recording failure remains frontend-visible");
  assert_eq!(flush.failure_count().get(), 1);
}
