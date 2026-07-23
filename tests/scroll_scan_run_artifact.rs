use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use auv_cli_invoke::ArtifactInstrumentationReceipt;
use auv_runtime::contract::{
  OBSERVATION_SNAPSHOT_API_VERSION, ObservationSnapshot, ObservationSource, RecognitionScope, RecognitionSurface,
};
use auv_runtime::run_read::{
  ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, RootArtifactReadError, SCAN_COVERAGE_PURPOSE, publish_scan_coverage, read_scroll_scan,
};
use auv_runtime::scene_state_read::read_scan_coverage;
use auv_runtime::scroll_scan::{
  CompletenessClaim, SCROLL_SCAN_JSON_BYTE_LIMIT, SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE, SCROLL_SCAN_PURPOSE, ScanRegion, ScanTarget,
  ScrollScanArtifact, StopEvidence, StopPolicy, StopReason,
};
use auv_scan::{CompletenessWire, CoverageEntryWire, NegativeEvidenceWire, SCAN_COVERAGE_SCHEMA_VERSION, ScanCoverageWire};
use auv_tracing::{
  ArtifactBody, ArtifactId, ArtifactMetadata, ArtifactPurpose, ArtifactReadError, ArtifactReader, ArtifactUri, ArtifactWriteError,
  Attributes, AuthorityId, BoxFuture, ByteLength, CommitError, CommitResult, ContentType, Context, ErrorCode, IdempotencyKey,
  MemoryRunStore, NewArtifact, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision, RunSnapshot, RunStore,
  RunSubscription, Sha256Digest, SpanId, StoreArtifactRequest, configure, dispatcher,
};
use futures_util::io::Cursor;
use sha2::{Digest, Sha256};

static PRODUCED_COVERAGE_COUNTER: AtomicUsize = AtomicUsize::new(0);

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

  fn store(&self) -> &dyn RunStore {
    self.store.as_ref()
  }

  async fn publish_scroll_scan(&self, artifact: &ScrollScanArtifact) -> ArtifactMetadata {
    let future = self.root.in_scope(|| async {
      let mut instrumentation = ArtifactInstrumentationReceipt::default();
      instrumentation
        .publish_json_bounded(SCROLL_SCAN_PURPOSE, artifact, SCROLL_SCAN_JSON_BYTE_LIMIT, SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE)
        .await;
      instrumentation
    });
    let instrumentation = self.root.instrument(future).await;
    assert!(instrumentation.failures().is_empty(), "{:?}", instrumentation.failures());
    self.dispatch.flush().await.expect("flush scroll scan");
    self
      .store
      .load_snapshot(self.run_id)
      .await
      .expect("load scroll-scan snapshot")
      .expect("scroll-scan snapshot")
      .artifacts()
      .values()
      .find(|published| published.metadata().purpose().as_str() == SCROLL_SCAN_PURPOSE)
      .expect("canonical scroll-scan artifact")
      .metadata()
      .clone()
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
  read: ArtifactReadFixture,
  opens: AtomicUsize,
  chunk_reads: Arc<AtomicUsize>,
}

enum ArtifactReadFixture {
  Chunks(Vec<Vec<u8>>),
  OpenError(ReadError),
  StreamError(ArtifactReadError),
}

impl ArtifactBytesStore {
  fn new(inner: Arc<MemoryRunStore>, bytes: Vec<u8>) -> Self {
    Self::from_chunks(inner, vec![bytes])
  }

  fn from_chunks(inner: Arc<MemoryRunStore>, chunks: Vec<Vec<u8>>) -> Self {
    Self {
      inner,
      read: ArtifactReadFixture::Chunks(chunks),
      opens: AtomicUsize::new(0),
      chunk_reads: Arc::new(AtomicUsize::new(0)),
    }
  }

  fn open_error(inner: Arc<MemoryRunStore>, error: ReadError) -> Self {
    Self {
      inner,
      read: ArtifactReadFixture::OpenError(error),
      opens: AtomicUsize::new(0),
      chunk_reads: Arc::new(AtomicUsize::new(0)),
    }
  }

  fn stream_error(inner: Arc<MemoryRunStore>, error: ArtifactReadError) -> Self {
    Self {
      inner,
      read: ArtifactReadFixture::StreamError(error),
      opens: AtomicUsize::new(0),
      chunk_reads: Arc::new(AtomicUsize::new(0)),
    }
  }

  fn open_count(&self) -> usize {
    self.opens.load(Ordering::Relaxed)
  }

  fn chunk_read_count(&self) -> usize {
    self.chunk_reads.load(Ordering::Relaxed)
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
    let chunk_reads = self.chunk_reads.clone();
    match &self.read {
      ArtifactReadFixture::Chunks(chunks) => {
        let chunks = chunks.clone();
        Box::pin(async move {
          let reader: ArtifactReader = Box::pin(futures_util::stream::iter(chunks.into_iter().map(move |chunk| {
            chunk_reads.fetch_add(1, Ordering::Relaxed);
            Ok(chunk.into())
          })));
          Ok(reader)
        })
      }
      ArtifactReadFixture::OpenError(error) => {
        let error = error.clone();
        Box::pin(async move { Err(error) })
      }
      ArtifactReadFixture::StreamError(error) => {
        let error = error.clone();
        Box::pin(async move {
          let reader: ArtifactReader = Box::pin(futures_util::stream::once(async move { Err(error) }));
          Ok(reader)
        })
      }
    }
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

fn produced_coverage() -> ScanCoverageWire {
  let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/auv-scan/tests/fixtures/scan/coverage/coverage_stable_v0");
  let invocation = PRODUCED_COVERAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
  let output_dir = std::env::temp_dir().join(format!("auv-scroll-scan-run-artifact-{}-{invocation}", std::process::id()));
  let _ = std::fs::remove_dir_all(&output_dir);
  let produced = auv_scan::produce_coverage_from_fixture_dir(&fixture_dir, &output_dir).expect("produce canonical scan coverage");
  let _ = std::fs::remove_dir_all(output_dir);
  produced.wire
}

async fn direct_coverage_result(calls: &AtomicUsize, coverage: ScanCoverageWire) -> ScanCoverageWire {
  calls.fetch_add(1, Ordering::Relaxed);
  let context = Context::current();
  let _ = publish_scan_coverage(Some(&context), &coverage).await;
  coverage
}

fn sample_scroll_scan_artifact() -> ScrollScanArtifact {
  ScrollScanArtifact {
    scan_id: "scan_fixture".to_string(),
    target: ScanTarget {
      application_id: Some("com.example.fixture".to_string()),
      window_title: Some("Fixture".to_string()),
      region: ScanRegion {
        left_ratio: 0.1,
        top_ratio: 0.2,
        right_ratio: 0.9,
        bottom_ratio: 0.8,
      },
    },
    stop_policy: StopPolicy::Bounded {
      max_pages: 1,
      max_scrolls: 0,
    },
    pages: Vec::new(),
    observations: Vec::new(),
    nodes: Vec::new(),
    snapshots: vec![ObservationSnapshot {
      api_version: OBSERVATION_SNAPSHOT_API_VERSION.to_string(),
      snapshot_id: "snapshot_fixture_0001".to_string(),
      run_id: RunId::new(),
      span_id: SpanId::new(),
      captured_at_millis: 1,
      source: ObservationSource::Visual,
      scope: RecognitionScope {
        surface: RecognitionSurface::Region,
        display_ref: None,
        native_display_id: None,
        app_bundle_id: Some("com.example.fixture".to_string()),
        window_title: Some("Fixture".to_string()),
        window_number: None,
        region_hint: None,
        capture_artifact: None,
        capture_contract_artifact: None,
      },
      capture_contract_ref: None,
      evidence: Vec::new(),
      nodes: Vec::new(),
      detail: serde_json::json!({ "page_index": 0 }),
      known_limits: vec!["fixture observation has no capture contract".to_string()],
    }],
    clusters: Vec::new(),
    section_candidates: Vec::new(),
    scroll_boundary_candidates: Vec::new(),
    hook_decisions: Vec::new(),
    stop_evidence: StopEvidence {
      reason: StopReason::MaxPages,
      message: "fixture reached max pages".to_string(),
      page_index: 0,
    },
    completeness_claim: CompletenessClaim::PartialMaxPages,
    warnings: vec!["fixture warning".to_string()],
  }
}

#[tokio::test]
async fn scroll_scan_producer_round_trips_through_the_shared_reader() {
  let fixture = RootRunFixture::memory();
  let expected = sample_scroll_scan_artifact();

  let metadata = fixture.publish_scroll_scan(&expected).await;
  let snapshot = fixture.snapshot().await;
  let decoded = read_scroll_scan(fixture.store(), &snapshot, metadata.uri()).await.expect("read canonical scroll scan");

  assert_eq!(metadata.purpose().as_str(), SCROLL_SCAN_PURPOSE);
  assert_eq!(metadata.content_type().to_string(), "application/json");
  assert_eq!(metadata.uri().run_id(), fixture.run_id);
  assert_eq!(decoded, expected);
}

#[tokio::test]
async fn scroll_scan_reader_checks_authority_owner_purpose_and_content_type() {
  let fixture = RootRunFixture::memory();
  let published = fixture.publish_scroll_scan(&sample_scroll_scan_artifact()).await;
  let bytes = serde_json::to_vec(&sample_scroll_scan_artifact()).expect("serialize scroll scan");
  let wrong_purpose = fixture.publish_bytes("auv.runtime.other", "application/json", bytes.clone()).await;
  let wrong_content_type = fixture.publish_bytes(SCROLL_SCAN_PURPOSE, "application/problem+json", bytes).await;
  let snapshot = fixture.snapshot().await;

  let wrong_store = MemoryRunStore::new(AuthorityId::new());
  assert_eq!(
    read_scroll_scan(&wrong_store, &snapshot, published.uri()).await.expect_err("wrong authority").code().as_str(),
    "auv.runtime.scroll_scan.snapshot_authority_mismatch"
  );
  let wrong_owner = ArtifactUri::from_ids(RunId::new(), ArtifactId::new());
  assert_eq!(
    read_scroll_scan(fixture.store(), &snapshot, &wrong_owner).await.expect_err("wrong owner").code().as_str(),
    "auv.runtime.scroll_scan.wrong_owner"
  );
  let dangling = ArtifactUri::from_ids(snapshot.run_id(), ArtifactId::new());
  assert_eq!(
    read_scroll_scan(fixture.store(), &snapshot, &dangling).await.expect_err("dangling URI").code().as_str(),
    "auv.runtime.scroll_scan.dangling_uri"
  );
  assert_eq!(
    read_scroll_scan(fixture.store(), &snapshot, wrong_purpose.uri()).await.expect_err("wrong purpose").code().as_str(),
    "auv.runtime.scroll_scan.wrong_purpose"
  );
  assert_eq!(
    read_scroll_scan(fixture.store(), &snapshot, wrong_content_type.uri()).await.expect_err("wrong content type").code().as_str(),
    "auv.runtime.scroll_scan.wrong_content_type"
  );
}

#[tokio::test]
async fn scroll_scan_reader_checks_committed_length_and_digest() {
  let fixture = RootRunFixture::memory();
  let artifact = sample_scroll_scan_artifact();
  let bytes = serde_json::to_vec_pretty(&artifact).expect("serialize canonical scroll scan");
  let published = fixture.publish_scroll_scan(&artifact).await;
  let snapshot = fixture.snapshot().await;

  let short_store = ArtifactBytesStore::new(fixture.store.clone(), bytes[..bytes.len() - 1].to_vec());
  assert_eq!(
    read_scroll_scan(&short_store, &snapshot, published.uri()).await.expect_err("short artifact").code().as_str(),
    "auv.runtime.scroll_scan.length_mismatch"
  );

  let mut changed = bytes;
  *changed.last_mut().expect("scroll-scan bytes") ^= 1;
  let corrupt_store = ArtifactBytesStore::new(fixture.store.clone(), changed);
  assert_eq!(
    read_scroll_scan(&corrupt_store, &snapshot, published.uri()).await.expect_err("corrupt artifact").code().as_str(),
    "auv.runtime.scroll_scan.digest_mismatch"
  );
}

#[tokio::test]
async fn scroll_scan_reader_enforces_metadata_and_midstream_bounds() {
  let oversized_metadata = RootRunFixture::memory();
  let bytes = vec![b' '; usize::try_from(SCROLL_SCAN_JSON_BYTE_LIMIT + 1).expect("test payload size")];
  let published = oversized_metadata.publish_bytes(SCROLL_SCAN_PURPOSE, "application/json", bytes).await;
  let snapshot = oversized_metadata.snapshot().await;
  let unopened = ArtifactBytesStore::new(oversized_metadata.store.clone(), Vec::new());

  assert_eq!(
    read_scroll_scan(&unopened, &snapshot, published.uri()).await.expect_err("oversized metadata").code().as_str(),
    SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE
  );
  assert_eq!(unopened.open_count(), 0, "metadata bounds must be enforced before opening the body");

  let fixture = RootRunFixture::memory();
  let published = fixture.publish_scroll_scan(&sample_scroll_scan_artifact()).await;
  let snapshot = fixture.snapshot().await;
  let first = vec![0; usize::try_from(SCROLL_SCAN_JSON_BYTE_LIMIT + 1).expect("test payload size")];
  let streamed = ArtifactBytesStore::from_chunks(fixture.store.clone(), vec![first, vec![1]]);

  assert_eq!(
    read_scroll_scan(&streamed, &snapshot, published.uri()).await.expect_err("oversized stream").code().as_str(),
    SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE
  );
  assert_eq!(streamed.chunk_read_count(), 1, "the reader must not poll chunks after the bound is crossed");
}

#[tokio::test]
async fn scroll_scan_reader_preserves_open_stream_and_json_failure_codes() {
  let fixture = RootRunFixture::memory();
  let published = fixture.publish_scroll_scan(&sample_scroll_scan_artifact()).await;
  let snapshot = fixture.snapshot().await;
  let open_store = ArtifactBytesStore::open_error(
    fixture.store.clone(),
    ReadError::Unavailable(ErrorCode::parse("auv.test.scroll_scan_open").expect("test error code")),
  );
  assert_eq!(
    read_scroll_scan(&open_store, &snapshot, published.uri()).await.expect_err("open failure").code().as_str(),
    "auv.runtime.scroll_scan.open_failed"
  );

  let stream_store = ArtifactBytesStore::stream_error(
    fixture.store.clone(),
    ArtifactReadError::Unavailable(ErrorCode::parse("auv.test.scroll_scan_stream").expect("test error code")),
  );
  assert_eq!(
    read_scroll_scan(&stream_store, &snapshot, published.uri()).await.expect_err("stream failure").code().as_str(),
    "auv.runtime.scroll_scan.stream_failed"
  );

  let malformed = RootRunFixture::memory();
  let published = malformed.publish_bytes(SCROLL_SCAN_PURPOSE, "application/json", br#"{"scan_id":"#.to_vec()).await;
  let snapshot = malformed.snapshot().await;
  assert_eq!(
    read_scroll_scan(malformed.store(), &snapshot, published.uri()).await.expect_err("malformed JSON").code().as_str(),
    "auv.runtime.scroll_scan.malformed_json"
  );
}

#[tokio::test]
async fn scroll_scan_producer_rejects_payload_above_its_domain_bound_without_a_commit() {
  let fixture = RootRunFixture::memory();
  let mut oversized = sample_scroll_scan_artifact();
  oversized.warnings = vec!["x".repeat(usize::try_from(SCROLL_SCAN_JSON_BYTE_LIMIT + 1).expect("test payload size"))];
  let future = fixture.root.in_scope(|| async {
    let mut instrumentation = ArtifactInstrumentationReceipt::default();
    instrumentation
      .publish_json_bounded(SCROLL_SCAN_PURPOSE, &oversized, SCROLL_SCAN_JSON_BYTE_LIMIT, SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE)
      .await;
    instrumentation
  });

  let instrumentation = fixture.root.instrument(future).await;

  assert_eq!(instrumentation.failures().len(), 1);
  assert!(instrumentation.failures()[0].message.contains(SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE));
  fixture.dispatch.flush().await.expect("construction failure emits no artifact job");
  assert!(fixture.store.load_snapshot(fixture.run_id).await.expect("load snapshot").is_none());
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
  let expected = produced_coverage();
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
async fn scan_coverage_reader_rejects_committed_payload_above_the_canonical_bound_before_opening() {
  let fixture = RootRunFixture::memory();
  let oversized = vec![b' '; usize::try_from(ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1).expect("test payload size")];
  fixture.publish_bytes(SCAN_COVERAGE_PURPOSE, "application/json", oversized).await;
  let snapshot = fixture.snapshot().await;
  let store = ArtifactBytesStore::new(fixture.store.clone(), Vec::new());

  assert!(matches!(
    read_scan_coverage(&store, &snapshot).await.expect_err("oversized coverage"),
    RootArtifactReadError::PayloadTooLarge { limit, actual, .. }
      if limit == ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT && actual == ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1
  ));
  assert_eq!(store.open_count(), 0, "metadata bounds must be enforced before reading the body");
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
  let expected = produced_coverage();
  let future = root.in_scope(|| direct_coverage_result(&calls, expected.clone()));

  let direct = root.instrument(future).await;

  assert_eq!(direct, expected);
  assert_eq!(calls.load(Ordering::Relaxed), 1);
  assert_eq!(store.writes.load(Ordering::Relaxed), 1);
  let flush = dispatch.flush().await.expect_err("recording failure remains frontend-visible");
  assert_eq!(flush.failure_count().get(), 1);
}

#[test]
fn scan_coverage_producer_reports_missing_source_without_manufacturing_output() {
  let fixture_dir = std::env::temp_dir().join(format!("auv-scroll-scan-missing-source-{}", std::process::id()));
  let output_dir = fixture_dir.join("out");
  let _ = std::fs::remove_dir_all(&fixture_dir);

  let error = auv_scan::produce_coverage_from_fixture_dir(&fixture_dir, &output_dir).expect_err("missing source must fail");

  assert!(matches!(error, auv_scan::CoverageProducerError::MissingManifest { .. }));
  assert!(!output_dir.exists(), "a source failure must not leave a canonical coverage artifact");
}
