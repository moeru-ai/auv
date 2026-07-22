use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use auv_netease_music::recording::{
  NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, NETEASE_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE, PLAYLIST_SELECT_RESULT_PURPOSE,
  PLAYLIST_SIDEBAR_SCAN_PURPOSE, PersistedLineage, VIEW_MEMORY_PURPOSE, persist_playlist_ls_artifacts, persist_playlist_select_proof,
  read_lineage_manifest, read_playlist_select_result, read_playlist_sidebar_scan, read_view_memory, write_lineage_manifest,
};
use auv_netease_music::{Inputs, PlaylistSelectResult, PlaylistSidebarScan, decode_playlist_sidebar_scan_json};
use auv_tracing::{
  ArtifactBody, ArtifactId, ArtifactMetadata, ArtifactPurpose, ArtifactReader, ArtifactUri, ArtifactWriteError, Attributes, AuthorityId,
  BoxFuture, ByteLength, CommitError, CommitResult, ContentType, Context, Dispatch, IdempotencyKey, MemoryRunStore, NewArtifact, PageLimit,
  ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision, RunSnapshot, RunStore, RunSubscription, Sha256Digest,
  StoreArtifactRequest, configure, dispatcher,
};
use auv_view::memory::ViewMemory;
use futures_util::io::Cursor;
use sha2::{Digest, Sha256};

struct NeteaseRunFixture {
  store: Arc<MemoryRunStore>,
  dispatch: Dispatch,
  root: Context,
  run_id: RunId,
}

impl NeteaseRunFixture {
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

  async fn persist_playlist_scan(&self, scan: &PlaylistSidebarScan) -> PersistedLineage {
    let mut inputs = Inputs::with_defaults();
    inputs.app_id = scan.app().app_id.clone().expect("fixture app id");
    let future = self.root.in_scope(|| persist_playlist_ls_artifacts(scan, &inputs, true));
    let persisted = self.root.instrument(future).await.expect("publish playlist scan and view memory");
    self.dispatch.flush().await.expect("flush playlist artifacts");
    persisted
  }

  async fn persist_select_result(&self, result: PlaylistSelectResult) -> PlaylistSelectResult {
    let future = self.root.in_scope(|| persist_playlist_select_proof(result));
    let publication = self.root.instrument(future).await;
    let (result, instrumentation) = publication.into_parts();
    assert!(!instrumentation.is_disabled(), "select publication was disabled");
    assert!(instrumentation.failure().is_none(), "select publication failed: {}", instrumentation.failure().unwrap());
    self.dispatch.flush().await.expect("flush playlist-select result");
    result
  }

  async fn publish_bytes(&self, purpose: &str, content_type: &str, bytes: Vec<u8>) -> ArtifactMetadata {
    let artifact = NewArtifact::new(
      ArtifactPurpose::parse(purpose).expect("artifact purpose"),
      ContentType::parse(content_type).expect("content type"),
      ByteLength::new(bytes.len() as u64).expect("artifact byte length"),
      Sha256Digest::new(Sha256::digest(&bytes).into()),
      Attributes::empty(),
      Cursor::new(bytes),
    );
    let emission = self.root.in_scope(|| auv_tracing::emit_artifact!(artifact));
    let metadata = self.root.instrument(emission).await.expect("artifact publication").expect("enabled artifact publication");
    self.dispatch.flush().await.expect("flush artifact");
    metadata
  }

  async fn snapshot(&self) -> RunSnapshot {
    self.store.load_snapshot(self.run_id).await.expect("load snapshot").expect("run snapshot")
  }

  async fn read_scan(&self, snapshot: &RunSnapshot, uri: &ArtifactUri) -> PlaylistSidebarScan {
    read_playlist_sidebar_scan(self.store(), snapshot, uri).await.expect("read playlist scan")
  }

  async fn read_memory(&self, snapshot: &RunSnapshot, uri: &ArtifactUri) -> ViewMemory {
    read_view_memory(self.store(), snapshot, uri).await.expect("read view memory")
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

#[test]
fn playlist_scan_and_view_memory_round_trip_by_uri() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let scan = sample_scan();

    let persisted = fixture.persist_playlist_scan(&scan).await;

    assert!(persisted.lineage.scan_uri.to_string().starts_with("auv://runs/"));
    let snapshot = fixture.snapshot().await;
    assert_eq!(fixture.read_scan(&snapshot, &persisted.lineage.scan_uri).await, scan);
    let memory_uri = persisted.lineage.memory_uri.as_ref().expect("view-memory URI");
    assert_eq!(fixture.read_memory(&snapshot, memory_uri).await, persisted.memory.expect("persisted view memory"));
  });
}

#[test]
fn playlist_select_result_round_trips_by_uri() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let expected = sample_select_result();

    let expected = fixture.persist_select_result(expected).await;
    let snapshot = fixture.snapshot().await;
    let published = snapshot
      .artifacts()
      .values()
      .find(|published| published.metadata().purpose().as_str() == PLAYLIST_SELECT_RESULT_PURPOSE)
      .expect("playlist-select publication")
      .metadata();

    let decoded = read_playlist_select_result(fixture.store(), &snapshot, published.uri()).await.expect("read playlist-select result");
    assert_eq!(decoded, expected);
  });
}

#[test]
fn canonical_artifacts_use_exact_purposes_and_json_content_type() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let persisted = fixture.persist_playlist_scan(&sample_scan()).await;
    fixture.persist_select_result(sample_select_result()).await;
    let snapshot = fixture.snapshot().await;

    let scan = snapshot.artifacts().get(&persisted.lineage.scan_uri).expect("scan metadata").metadata();
    assert_eq!(scan.purpose().as_str(), PLAYLIST_SIDEBAR_SCAN_PURPOSE);
    assert_eq!(scan.content_type().to_string(), "application/json");
    let memory = snapshot.artifacts().get(persisted.lineage.memory_uri.as_ref().expect("memory URI")).expect("memory metadata").metadata();
    assert_eq!(memory.purpose().as_str(), VIEW_MEMORY_PURPOSE);
    assert_eq!(memory.content_type().to_string(), "application/json");
    let select = snapshot
      .artifacts()
      .values()
      .find(|published| published.metadata().purpose().as_str() == PLAYLIST_SELECT_RESULT_PURPOSE)
      .expect("select metadata")
      .metadata();
    assert_eq!(select.content_type().to_string(), "application/json");
  });
}

#[test]
fn readers_reject_wrong_authority_owner_membership_purpose_and_content_type() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let scan = sample_scan();
    let bytes = serde_json::to_vec(&scan).expect("scan JSON");
    let scan_metadata = fixture.publish_bytes(PLAYLIST_SIDEBAR_SCAN_PURPOSE, "application/json", bytes.clone()).await;
    let wrong_purpose = fixture.publish_bytes("auv.netease.other", "application/json", bytes.clone()).await;
    let wrong_content_type = fixture.publish_bytes(PLAYLIST_SIDEBAR_SCAN_PURPOSE, "application/problem+json", bytes).await;
    let snapshot = fixture.snapshot().await;

    let other_store = MemoryRunStore::new(AuthorityId::new());
    let error = read_playlist_sidebar_scan(&other_store, &snapshot, scan_metadata.uri()).await.expect_err("wrong authority");
    assert_eq!(error.code().as_str(), "auv.netease.artifact.snapshot_authority_mismatch");

    let wrong_owner = ArtifactUri::from_ids(RunId::new(), ArtifactId::new());
    let error = read_playlist_sidebar_scan(fixture.store(), &snapshot, &wrong_owner).await.expect_err("wrong owner");
    assert_eq!(error.code().as_str(), "auv.netease.artifact.wrong_owner");

    let dangling = ArtifactUri::from_ids(snapshot.run_id(), ArtifactId::new());
    let error = read_playlist_sidebar_scan(fixture.store(), &snapshot, &dangling).await.expect_err("dangling URI");
    assert_eq!(error.code().as_str(), "auv.netease.artifact.dangling_uri");

    let error = read_playlist_sidebar_scan(fixture.store(), &snapshot, wrong_purpose.uri()).await.expect_err("wrong purpose");
    assert_eq!(error.code().as_str(), "auv.netease.artifact.wrong_purpose");
    let error = read_playlist_sidebar_scan(fixture.store(), &snapshot, wrong_content_type.uri()).await.expect_err("wrong content type");
    assert_eq!(error.code().as_str(), "auv.netease.artifact.wrong_content_type");
  });
}

#[test]
fn reader_requires_committed_length_digest_and_structured_artifact_bound() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let bytes = serde_json::to_vec(&sample_scan()).expect("scan JSON");
    let metadata = fixture.publish_bytes(PLAYLIST_SIDEBAR_SCAN_PURPOSE, "application/json", bytes.clone()).await;
    let snapshot = fixture.snapshot().await;

    let short = ArtifactBytesStore::new(fixture.store.clone(), bytes[..bytes.len() - 1].to_vec());
    let error = read_playlist_sidebar_scan(&short, &snapshot, metadata.uri()).await.expect_err("short body");
    assert_eq!(error.code().as_str(), "auv.netease.artifact.length_mismatch");

    let mut changed = bytes;
    *changed.last_mut().expect("non-empty scan JSON") ^= 1;
    let corrupt = ArtifactBytesStore::new(fixture.store.clone(), changed);
    let error = read_playlist_sidebar_scan(&corrupt, &snapshot, metadata.uri()).await.expect_err("digest mismatch");
    assert_eq!(error.code().as_str(), "auv.netease.artifact.digest_mismatch");

    let oversized = vec![b' '; (NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1) as usize];
    let oversized = fixture.publish_bytes(PLAYLIST_SIDEBAR_SCAN_PURPOSE, "application/json", oversized).await;
    let snapshot = fixture.snapshot().await;
    let unopened = ArtifactBytesStore::new(fixture.store.clone(), Vec::new());
    let error = read_playlist_sidebar_scan(&unopened, &snapshot, oversized.uri()).await.expect_err("oversized metadata");
    assert_eq!(error.code().as_str(), NETEASE_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE);
    assert_eq!(unopened.open_count(), 0, "oversized metadata must fail before opening bytes");
  });
}

#[test]
fn lineage_manifest_round_trips_only_canonical_uri_references() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let persisted = fixture.persist_playlist_scan(&sample_scan()).await;
    let directory = std::env::temp_dir().join(format!("auv-netease-lineage-{}", fixture.run_id));
    let _ = std::fs::remove_dir_all(&directory);

    write_lineage_manifest(&directory, &persisted.lineage).expect("write URI lineage");
    assert_eq!(read_lineage_manifest(&directory).expect("read URI lineage"), persisted.lineage);

    let legacy = serde_json::json!({
      "schema_version": "view-memory-lineage-v0",
      "run_id": fixture.run_id.to_string(),
      "scan_artifact_id": "legacy",
      "memory_artifact_id": null,
      "memory_id": "legacy",
      "scope_id": "playlist_sidebar",
      "app_bundle_id": "com.netease.163music",
      "written_at_millis": 0
    });
    std::fs::write(directory.join("view-memory-run-lineage.json"), serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert!(read_lineage_manifest(&directory).is_err(), "legacy bare IDs must not deserialize");
    let _ = std::fs::remove_dir_all(directory);
  });
}

#[test]
fn disabled_context_preserves_select_result_and_exposes_scan_publication_failure() {
  futures_executor::block_on(async {
    let scan_error = persist_playlist_ls_artifacts(&sample_scan(), &Inputs::with_defaults(), true)
      .await
      .expect_err("scan publication needs caller authority");
    assert!(scan_error.to_string().contains("no caller-owned run authority"));

    let expected = sample_select_result();
    let publication = persist_playlist_select_proof(expected.clone()).await;
    let (actual, instrumentation) = publication.into_parts();
    assert_eq!(actual, expected);
    assert!(instrumentation.is_disabled());
    assert!(instrumentation.failure().is_none());
  });
}

fn sample_scan() -> PlaylistSidebarScan {
  let path =
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sidebar-scan-proof/hermetic_v0/playlist-sidebar-scan.json");
  let json = std::fs::read_to_string(path).expect("read sidebar scan fixture");
  decode_playlist_sidebar_scan_json(&json).expect("decode sidebar scan fixture")
}

fn sample_select_result() -> PlaylistSelectResult {
  let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/select-proof/hermetic_v0/select-result.json");
  let bytes = std::fs::read(path).expect("read playlist-select fixture");
  serde_json::from_slice(&bytes).expect("decode playlist-select fixture")
}
