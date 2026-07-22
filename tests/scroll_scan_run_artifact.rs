use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use auv_runtime::contract::{
  OBSERVATION_SNAPSHOT_API_VERSION, ObservationSnapshot, ObservationSource, RecognitionScope, RecognitionSurface,
};
use auv_runtime::inspect::CorePrefixSection;
use auv_runtime::run_read::read_scroll_scan;
use auv_runtime::runtime::Runtime;
use auv_runtime::scroll_scan::{
  CompletenessClaim, SCROLL_SCAN_PURPOSE, ScanRegion, ScanTarget, ScanWindowRegionOptions, ScrollScanArtifact, StopEvidence, StopPolicy,
  StopReason, scan_window_region,
};
use auv_tracing::{
  ArtifactBody, ArtifactId, ArtifactMetadata, ArtifactPurpose, ArtifactReader, ArtifactUri, ArtifactWriteError, Attributes, AuthorityId,
  BoxFuture, ByteLength, CommitError, CommitResult, ContentType, Context, Dispatch, ErrorCode, IdempotencyKey, MemoryRunStore, NewArtifact,
  PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision, RunStore, RunSubscription, Sha256Digest,
  StoreArtifactRequest, configure, dispatcher,
};
use auv_tracing_driver::store::LocalStore;
use auv_tracing_driver::trace::{RunId as LegacyRunId, SpanId as LegacySpanId};
use futures_util::io::Cursor;
use sha2::{Digest, Sha256};

struct RootRunFixture {
  store: Arc<MemoryRunStore>,
  dispatch: Dispatch,
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

  fn store(&self) -> &dyn RunStore {
    self.store.as_ref()
  }

  async fn publish_scroll_scan(&self, artifact: &ScrollScanArtifact) -> ArtifactMetadata {
    let bytes = serde_json::to_vec(artifact).expect("serialize scroll scan");
    self.publish_bytes(SCROLL_SCAN_PURPOSE, "application/json", bytes).await
  }

  async fn publish_bytes(&self, purpose: &str, content_type: &str, bytes: Vec<u8>) -> ArtifactMetadata {
    let byte_length = ByteLength::new(bytes.len() as u64).expect("bounded scroll scan");
    let digest = Sha256Digest::new(Sha256::digest(&bytes).into());
    let artifact = NewArtifact::new(
      ArtifactPurpose::parse(purpose).expect("artifact purpose"),
      ContentType::parse(content_type).expect("artifact content type"),
      byte_length,
      digest,
      Attributes::empty(),
      Cursor::new(bytes),
    );
    let emission = self.root.in_scope(|| auv_tracing::emit_artifact!(artifact));
    let published = self.root.instrument(emission).await.expect("publish scroll scan").expect("enabled publication");
    self.dispatch.flush().await.expect("flush scroll scan");
    published
  }

  async fn snapshot(&self) -> Arc<auv_tracing::RunSnapshot> {
    Arc::new(self.store.load_snapshot(self.run_id).await.expect("load snapshot").expect("scroll-scan snapshot"))
  }
}

struct ArtifactBytesStore {
  inner: Arc<MemoryRunStore>,
  bytes: Vec<u8>,
}

impl ArtifactBytesStore {
  fn new(inner: Arc<MemoryRunStore>, bytes: Vec<u8>) -> Self {
    Self { inner, bytes }
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

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<auv_tracing::RunSnapshot>, ReadError>> {
    self.inner.load_snapshot(run_id)
  }

  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    self.inner.commits_after(run_id, after, limit)
  }

  fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    self.inner.subscribe(run_id, after)
  }

  fn open_artifact(&self, _uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
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
      Err(ArtifactWriteError::Rejected(ErrorCode::parse("auv.test.scroll_scan_artifact_rejected").expect("test error code")))
    })
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

  fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    self.inner.open_artifact(uri)
  }
}

#[tokio::test]
async fn scroll_scan_round_trips_through_the_run_store() {
  let fixture = RootRunFixture::memory();
  let expected = sample_scroll_scan_artifact();

  let published = fixture.publish_scroll_scan(&expected).await;

  assert_eq!(published.purpose().as_str(), "auv.runtime.scroll_scan");
  let snapshot = fixture.snapshot().await;
  let decoded = read_scroll_scan(fixture.store(), snapshot.as_ref(), published.uri()).await.expect("read scroll scan");
  assert_eq!(decoded, expected);
  let section_snapshots =
    CorePrefixSection.read_scroll_scan_observations(fixture.store(), snapshot.as_ref()).await.expect("read root scroll-scan section");
  assert_eq!(section_snapshots, expected.snapshots);
}

#[tokio::test]
async fn root_scroll_scan_publishes_partial_result_to_the_current_run() {
  let fixture = RootRunFixture::memory();
  let directory = std::env::temp_dir().join(format!("auv-scroll-scan-v1-{}", fixture.run_id));
  let _ = std::fs::remove_dir_all(&directory);
  std::fs::create_dir_all(&directory).expect("scroll-scan directory");
  let runtime = Runtime::new(directory.join("project"), LocalStore::new(directory.join("legacy-store")).expect("legacy store"));
  let future = fixture.root.in_scope(|| scan_window_region(&runtime, bounded_scan_options()));

  let error = fixture.root.instrument(future).await.expect_err("typed observe-region gap remains direct scan truth");

  assert!(error.contains("typed window region observation API"));
  fixture.dispatch.flush().await.expect("flush partial scroll scan");
  let snapshot = fixture.snapshot().await;
  let published = snapshot
    .artifacts()
    .values()
    .find(|published| published.metadata().purpose().as_str() == SCROLL_SCAN_PURPOSE)
    .expect("partial scroll scan publication")
    .metadata();
  let decoded = read_scroll_scan(fixture.store(), snapshot.as_ref(), published.uri()).await.expect("read partial scroll scan");
  assert_eq!(decoded.completeness_claim, CompletenessClaim::Unknown);
  assert!(decoded.warnings.iter().any(|warning| warning == "scan ended with an error; artifact is partial"));
  let _ = std::fs::remove_dir_all(directory);
}

#[tokio::test]
async fn scroll_scan_reader_rejects_snapshot_and_uri_ownership_mismatches() {
  let fixture = RootRunFixture::memory();
  let published = fixture.publish_scroll_scan(&sample_scroll_scan_artifact()).await;
  let snapshot = fixture.snapshot().await;
  let wrong_owner = ArtifactUri::from_ids(RunId::new(), ArtifactId::new());
  let dangling = ArtifactUri::from_ids(snapshot.run_id(), ArtifactId::new());

  let error = read_scroll_scan(fixture.store(), snapshot.as_ref(), &wrong_owner).await.expect_err("wrong artifact owner");
  assert_eq!(error.code().as_str(), "auv.runtime.scroll_scan.wrong_owner");
  let error = read_scroll_scan(fixture.store(), snapshot.as_ref(), &dangling).await.expect_err("dangling artifact URI");
  assert_eq!(error.code().as_str(), "auv.runtime.scroll_scan.dangling_uri");

  let other_store = MemoryRunStore::new(AuthorityId::new());
  let error = read_scroll_scan(&other_store, snapshot.as_ref(), published.uri()).await.expect_err("wrong snapshot authority");
  assert_eq!(error.code().as_str(), "auv.runtime.scroll_scan.snapshot_authority_mismatch");
}

#[tokio::test]
async fn scroll_scan_reader_requires_exact_purpose_and_content_type() {
  let fixture = RootRunFixture::memory();
  let bytes = serde_json::to_vec(&sample_scroll_scan_artifact()).expect("serialize scroll scan");
  let wrong_purpose = fixture.publish_bytes("auv.runtime.other", "application/json", bytes.clone()).await;
  let wrong_content_type = fixture.publish_bytes(SCROLL_SCAN_PURPOSE, "application/problem+json", bytes).await;
  let snapshot = fixture.snapshot().await;

  let error = read_scroll_scan(fixture.store(), snapshot.as_ref(), wrong_purpose.uri()).await.expect_err("wrong purpose");
  assert_eq!(error.code().as_str(), "auv.runtime.scroll_scan.wrong_purpose");
  let error = read_scroll_scan(fixture.store(), snapshot.as_ref(), wrong_content_type.uri()).await.expect_err("wrong content type");
  assert_eq!(error.code().as_str(), "auv.runtime.scroll_scan.wrong_content_type");
}

#[tokio::test]
async fn scroll_scan_reader_requires_committed_length_and_digest() {
  let fixture = RootRunFixture::memory();
  let bytes = serde_json::to_vec(&sample_scroll_scan_artifact()).expect("serialize scroll scan");
  let published = fixture.publish_bytes(SCROLL_SCAN_PURPOSE, "application/json", bytes.clone()).await;
  let snapshot = fixture.snapshot().await;

  let short_store = ArtifactBytesStore::new(fixture.store.clone(), bytes[..bytes.len() - 1].to_vec());
  let error = read_scroll_scan(&short_store, snapshot.as_ref(), published.uri()).await.expect_err("wrong byte length");
  assert_eq!(error.code().as_str(), "auv.runtime.scroll_scan.length_mismatch");

  let mut changed = bytes;
  let last = changed.last_mut().expect("non-empty payload");
  *last ^= 1;
  let changed_store = ArtifactBytesStore::new(fixture.store.clone(), changed);
  let error = read_scroll_scan(&changed_store, snapshot.as_ref(), published.uri()).await.expect_err("wrong digest");
  assert_eq!(error.code().as_str(), "auv.runtime.scroll_scan.digest_mismatch");
}

#[tokio::test]
async fn scroll_scan_reader_rejects_malformed_json() {
  let fixture = RootRunFixture::memory();
  let published = fixture.publish_bytes(SCROLL_SCAN_PURPOSE, "application/json", b"{\"scan_id\":".to_vec()).await;
  let snapshot = fixture.snapshot().await;

  let error = read_scroll_scan(fixture.store(), snapshot.as_ref(), published.uri()).await.expect_err("malformed scroll scan");

  assert_eq!(error.code().as_str(), "auv.runtime.scroll_scan.malformed_json");
}

#[tokio::test]
async fn artifact_failure_does_not_replace_or_reexecute_the_scroll_scan() {
  let store = Arc::new(RejectArtifactStore::new());
  let dispatch = configure().run_store(store.clone()).build().expect("rejecting dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let directory = std::env::temp_dir().join(format!("auv-scroll-scan-reject-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&directory);
  std::fs::create_dir_all(&directory).expect("scroll-scan directory");
  let runtime = Runtime::new(directory.join("project"), LocalStore::new(directory.join("legacy-store")).expect("legacy store"));
  let future = root.in_scope(|| scan_window_region(&runtime, bounded_scan_options()));

  let error = root.instrument(future).await.expect_err("direct scan error");

  assert!(error.contains("typed window region observation API"));
  assert!(!error.contains("artifact"), "artifact failure replaced direct result: {error}");
  assert_eq!(store.writes.load(Ordering::Relaxed), 1);
  let flush = dispatch.flush().await.expect_err("artifact failure remains frontend instrumentation state");
  assert_eq!(flush.failure_count().get(), 1);
  let _ = std::fs::remove_dir_all(directory);
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
      run_id: LegacyRunId::new("run_fixture"),
      span_id: LegacySpanId::new("span_fixture"),
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

fn bounded_scan_options() -> ScanWindowRegionOptions {
  ScanWindowRegionOptions {
    target: ScanTarget {
      application_id: Some("com.example.fixture".to_string()),
      window_title: None,
      region: ScanRegion {
        left_ratio: 0.2,
        top_ratio: 0.3,
        right_ratio: 0.9,
        bottom_ratio: 0.8,
      },
    },
    stop_policy: StopPolicy::Bounded {
      max_pages: 1,
      max_scrolls: 0,
    },
    direction: "down".to_string(),
    scroll_amount: 40.0,
    settle_ms: 250,
    min_confidence: 0.0,
    max_observations: 128,
  }
}
