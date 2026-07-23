use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use auv_netease_music::recording::{
  NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, NETEASE_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE, NeteaseArtifactPublishError,
  NeteaseArtifactReadError, PLAYLIST_SELECT_RESULT_PURPOSE, PLAYLIST_SIDEBAR_SCAN_PURPOSE, PersistedLineage, VIEW_MEMORY_PURPOSE,
  lineage_manifest_path, persist_playlist_ls_artifacts, persist_playlist_select_proof, read_canonical_playlist_artifacts,
  read_lineage_manifest, read_playlist_select_result, read_playlist_sidebar_scan, read_view_memory, write_lineage_manifest,
};
use auv_netease_music::{
  Inputs, PlaylistSelectResult, PlaylistSidebarScan, decode_playlist_sidebar_scan_json, resolve_playlist_play_candidate,
};
use auv_tracing::{
  ArtifactBody, ArtifactId, ArtifactMetadata, ArtifactPurpose, ArtifactReader, ArtifactUri, ArtifactWriteError, Attributes, AuthorityId,
  BoxFuture, ByteLength, CommitError, CommitResult, ContentType, Context, Dispatch, ErrorCode, IdempotencyKey, MemoryRunStore, NewArtifact,
  PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision, RunSnapshot, RunStore, RunSubscription,
  Sha256Digest, StoreArtifactRequest, TelemetryError, TelemetryItem, TelemetryProjector, TelemetryRoutePolicy, configure, dispatcher,
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
    let persisted =
      self.root.instrument(future).await.expect("publish playlist scan and view memory").expect("publication should be enabled");
    self.dispatch.flush().await.expect("flush playlist artifacts");
    persisted
  }

  async fn persist_select_result(&self, result: &PlaylistSelectResult) -> ArtifactMetadata {
    let future = self.root.in_scope(|| persist_playlist_select_proof(result));
    let metadata = self.root.instrument(future).await.expect("select publication").expect("select publication should be enabled");
    self.dispatch.flush().await.expect("flush playlist-select result");
    metadata
  }

  async fn publish_bytes(&self, purpose: &str, content_type: &str, bytes: Vec<u8>) -> ArtifactMetadata {
    self.publish_bytes_in_run(self.root.clone(), purpose, content_type, bytes).await
  }

  async fn publish_bytes_in_run(&self, root: Context, purpose: &str, content_type: &str, bytes: Vec<u8>) -> ArtifactMetadata {
    let artifact = NewArtifact::new(
      ArtifactPurpose::parse(purpose).expect("artifact purpose"),
      ContentType::parse(content_type).expect("content type"),
      ByteLength::new(bytes.len() as u64).expect("artifact byte length"),
      Sha256Digest::new(Sha256::digest(&bytes).into()),
      Attributes::empty(),
      Cursor::new(bytes),
    );
    let emission = root.in_scope(|| auv_tracing::emit_artifact!(artifact));
    let metadata = root.instrument(emission).await.expect("artifact publication").expect("enabled artifact publication");
    self.dispatch.flush().await.expect("flush artifact");
    metadata
  }

  async fn publish_memory(&self, memory: &ViewMemory) -> ArtifactMetadata {
    self.publish_bytes(VIEW_MEMORY_PURPOSE, "application/json", serde_json::to_vec(memory).expect("view-memory JSON")).await
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

struct RejectArtifactStore {
  inner: MemoryRunStore,
}

struct NoopTelemetryProjector;

impl TelemetryProjector for NoopTelemetryProjector {
  fn project(&self, _item: TelemetryItem) -> BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async { Ok(()) })
  }

  fn flush(&self) -> BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async { Ok(()) })
  }
}

impl RejectArtifactStore {
  fn new() -> Self {
    Self {
      inner: MemoryRunStore::new(AuthorityId::new()),
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
    Box::pin(async { Err(ArtifactWriteError::Rejected(ErrorCode::parse("auv.test.netease_artifact_rejected").unwrap())) })
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

    let published = fixture.persist_select_result(&expected).await;
    let snapshot = fixture.snapshot().await;

    let decoded = read_playlist_select_result(fixture.store(), &snapshot, published.uri()).await.expect("read playlist-select result");
    assert_eq!(decoded, expected);
  });
}

#[test]
fn canonical_artifacts_use_exact_purposes_and_json_content_type() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let persisted = fixture.persist_playlist_scan(&sample_scan()).await;
    fixture.persist_select_result(&sample_select_result()).await;
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
    match error {
      NeteaseArtifactReadError::WrongPurpose {
        expected, actual, ..
      } => {
        assert_eq!(expected, ArtifactPurpose::parse(PLAYLIST_SIDEBAR_SCAN_PURPOSE).unwrap());
        assert_eq!(actual, ArtifactPurpose::parse("auv.netease.other").unwrap());
      }
      other => panic!("expected typed wrong-purpose error, got {other:?}"),
    }
    let error = read_playlist_sidebar_scan(fixture.store(), &snapshot, wrong_content_type.uri()).await.expect_err("wrong content type");
    match error {
      NeteaseArtifactReadError::WrongContentType {
        expected, actual, ..
      } => {
        assert_eq!(expected, ContentType::parse("application/json").unwrap());
        assert_eq!(actual, ContentType::parse("application/problem+json").unwrap());
      }
      other => panic!("expected typed wrong-content-type error, got {other:?}"),
    }
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
    let canonical = serde_json::to_value(&persisted.lineage).expect("canonical lineage JSON");
    assert_eq!(canonical["scan_uri"], persisted.lineage.scan_uri.to_string());
    assert_eq!(canonical["memory_uri"], persisted.lineage.memory_uri.as_ref().expect("memory URI").to_string());
    for forbidden in ["run_id", "scan_artifact_id", "memory_artifact_id"] {
      assert!(canonical.get(forbidden).is_none(), "canonical lineage exposed legacy field {forbidden:?}");
    }

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

#[cfg(unix)]
#[test]
fn lineage_manifest_does_not_follow_predictable_temporary_symlink() {
  use std::os::unix::fs::symlink;

  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let persisted = fixture.persist_playlist_scan(&sample_scan()).await;
    let directory = std::env::temp_dir().join(format!("auv-netease-lineage-symlink-{}", RunId::new()));
    std::fs::create_dir_all(&directory).expect("create lineage test directory");
    let victim = directory.join("unrelated.json");
    std::fs::write(&victim, b"unrelated-content").expect("write symlink victim");
    let predictable_temporary = directory.join("view-memory-run-lineage.json.tmp");
    symlink(&victim, &predictable_temporary).expect("install predictable temporary symlink");

    write_lineage_manifest(&directory, &persisted.lineage).expect("publish lineage without following attacker-controlled temp path");

    assert_eq!(std::fs::read(&victim).expect("read symlink victim"), b"unrelated-content");
    assert!(
      std::fs::symlink_metadata(&predictable_temporary).expect("predictable temporary symlink metadata").file_type().is_symlink(),
      "writer must clean only the unique temporary file it created"
    );
    assert!(std::fs::symlink_metadata(lineage_manifest_path(&directory)).expect("manifest metadata").file_type().is_file());
    assert_eq!(read_lineage_manifest(&directory).expect("read published lineage"), persisted.lineage);
    let _ = std::fs::remove_dir_all(directory);
  });
}

#[test]
fn concurrent_lineage_writers_use_independent_temporary_files() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let persisted = fixture.persist_playlist_scan(&sample_scan()).await;
    let directory = std::env::temp_dir().join(format!("auv-netease-lineage-concurrent-{}", RunId::new()));
    let writers = 12;
    let barrier = Arc::new(Barrier::new(writers));
    let mut handles = Vec::new();
    for index in 0..writers {
      let directory = directory.clone();
      let barrier = barrier.clone();
      let mut lineage = persisted.lineage.clone();
      lineage.written_at_millis = index as u64;
      handles.push(std::thread::spawn(move || {
        barrier.wait();
        write_lineage_manifest(&directory, &lineage)
      }));
    }

    for handle in handles {
      handle.join().expect("lineage writer thread").expect("independent atomic lineage update");
    }
    let final_lineage = read_lineage_manifest(&directory).expect("read final complete lineage");
    assert!(final_lineage.written_at_millis < writers as u64);
    let temporary_files = std::fs::read_dir(&directory)
      .expect("read lineage directory")
      .filter_map(Result::ok)
      .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
      .count();
    assert_eq!(temporary_files, 0, "successful writers must clean their own temporary files");
    let _ = std::fs::remove_dir_all(directory);
  });
}

#[cfg(unix)]
#[test]
fn failed_lineage_update_preserves_last_valid_manifest() {
  use std::os::unix::fs::PermissionsExt;

  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let persisted = fixture.persist_playlist_scan(&sample_scan()).await;
    let directory = std::env::temp_dir().join(format!("auv-netease-lineage-preserve-{}", RunId::new()));
    write_lineage_manifest(&directory, &persisted.lineage).expect("write initial lineage");
    let mut replacement = persisted.lineage.clone();
    replacement.written_at_millis = replacement.written_at_millis.saturating_add(1);
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500)).expect("make lineage directory read-only");

    let update = write_lineage_manifest(&directory, &replacement);

    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).expect("restore lineage directory permissions");
    assert!(update.is_err(), "read-only lineage directory must reject replacement");
    assert_eq!(read_lineage_manifest(&directory).expect("read retained lineage"), persisted.lineage);
    let _ = std::fs::remove_dir_all(directory);
  });
}

#[test]
fn disabled_context_preserves_select_result_and_is_not_a_publication_error() {
  futures_executor::block_on(async {
    let scan_publication = persist_playlist_ls_artifacts(&sample_scan(), &Inputs::with_defaults(), true)
      .await
      .expect("disabled scan publication is not an error");
    assert!(scan_publication.is_none());

    let expected = sample_select_result();
    let publication = persist_playlist_select_proof(&expected).await.expect("disabled select publication is not an error");
    assert!(publication.is_none());
  });
}

#[test]
fn disabled_context_skips_artifact_payload_validation() {
  futures_executor::block_on(async {
    let mut select = sample_select_result();
    select.known_limits.push("x".repeat((NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1) as usize));

    let publication = persist_playlist_select_proof(&select).await;

    assert!(matches!(publication, Ok(None)), "disabled instrumentation must not validate or reject artifact bytes");
  });
}

#[test]
fn telemetry_only_context_skips_artifact_payload_validation() {
  futures_executor::block_on(async {
    let dispatch = configure()
      .project_telemetry(Arc::new(NoopTelemetryProjector), TelemetryRoutePolicy::fixed_fields_only())
      .build()
      .expect("telemetry-only dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let mut select = sample_select_result();
    select.known_limits.push("x".repeat((NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1) as usize));
    let expected = select.clone();

    let future = root.in_scope(|| persist_playlist_select_proof(&select));
    let publication = root.instrument(future).await;

    assert!(matches!(publication, Ok(None)));
    assert_eq!(select, expected);
  });
}

#[test]
fn authority_context_still_rejects_oversized_artifact_payload() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let mut select = sample_select_result();
    select.known_limits.push("x".repeat((NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1) as usize));
    let expected = select.clone();

    let future = fixture.root.in_scope(|| persist_playlist_select_proof(&select));
    let publication = fixture.root.instrument(future).await;

    assert!(matches!(publication, Err(NeteaseArtifactPublishError::PayloadTooLarge { .. })));
    assert_eq!(select, expected);
  });
}

#[test]
fn rejected_publication_is_distinct_from_disabled_publication() {
  futures_executor::block_on(async {
    let store = Arc::new(RejectArtifactStore::new());
    let dispatch = configure().run_store(store).build().expect("rejecting dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));

    let scan = sample_scan();
    let inputs = Inputs::with_defaults();
    let scan_future = root.in_scope(|| persist_playlist_ls_artifacts(&scan, &inputs, true));
    let scan_error = root.instrument(scan_future).await.expect_err("rejected scan publication must be an error");
    assert!(scan_error.to_string().contains("auv.test.netease_artifact_rejected"));

    let select = sample_select_result();
    let select_future = root.in_scope(|| persist_playlist_select_proof(&select));
    let select_publication = root.instrument(select_future).await;
    assert!(matches!(select_publication, Err(NeteaseArtifactPublishError::Publication { .. })));
  });
}

#[test]
fn standalone_cli_store_root_installs_current_run_context() {
  let store_root = std::env::temp_dir().join(format!("auv-netease-cli-context-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&store_root);
  let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sidebar-scan-proof/hermetic_v0");

  let output = std::process::Command::new(env!("CARGO_BIN_EXE_auv-netease-music"))
    .arg("--store-root")
    .arg(&store_root)
    .arg("invoke")
    .arg("netease.playlist.sidebarScanProof")
    .arg("--fixture-dir")
    .arg(&fixture_dir)
    .output()
    .expect("standalone NetEase CLI should run");

  assert!(
    output.status.success(),
    "standalone CLI failed:\nstdout:\n{}\nstderr:\n{}",
    String::from_utf8_lossy(&output.stdout),
    String::from_utf8_lossy(&output.stderr)
  );
  let stdout = String::from_utf8(output.stdout).expect("CLI stdout should be UTF-8");
  assert!(stdout.lines().any(|line| line.starts_with("scan_uri=auv://runs/")), "missing canonical scan URI in {stdout:?}");

  let _ = std::fs::remove_dir_all(store_root);
}

#[test]
fn public_typed_candidate_operation_uses_caller_read_scan() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let scan = sample_scan();
    let persisted = fixture.persist_playlist_scan(&scan).await;
    let memory = persisted.memory.clone().expect("typed view memory");
    let memory_uri = persisted.lineage.memory_uri.as_ref().expect("view-memory URI");
    assert_eq!(memory_uri.run_id(), persisted.lineage.scan_uri.run_id());
    assert_eq!(memory.source_run_id, persisted.lineage.scan_uri.run_id().to_string());
    assert_eq!(memory.source_reconstruction_ref, persisted.lineage.scan_uri.to_string());
    let snapshot = fixture.snapshot().await;
    let artifacts = read_canonical_playlist_artifacts(fixture.store(), &snapshot, &persisted.lineage, true)
      .await
      .expect("caller-read canonical playlist artifacts");

    let candidate = resolve_playlist_play_candidate(&artifacts, "obs1.candidate.hermetic.test").expect("typed candidate should resolve");

    assert_eq!(candidate.scan(), &scan);
    assert_eq!(candidate.memory(), Some(&memory));
    assert_eq!(candidate.target().label, "Hermetic Fixture Playlist");
    assert_eq!(candidate.target().candidate_id.as_deref(), Some("obs1.candidate.hermetic.test"));
  });
}

#[test]
fn canonical_reader_rejects_cross_run_memory_before_candidate_reacquisition() {
  // ROOT CAUSE:
  //
  // A memory artifact from another run could carry source fields copied from
  // the scan. Payload equality cannot establish same-run artifact provenance.
  // The reader must reject the memory URI before loading or reacquiring from it.
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let scan = sample_scan();
    let persisted = fixture.persist_playlist_scan(&scan).await;
    let matching_memory = persisted.memory.clone().expect("typed view memory");
    assert_eq!(matching_memory.source_run_id, persisted.lineage.scan_uri.run_id().to_string());
    assert_eq!(matching_memory.source_reconstruction_ref, persisted.lineage.scan_uri.to_string());

    let other_run_id = RunId::new();
    let other_root = dispatcher::with_default(&fixture.dispatch, || Context::root(other_run_id));
    let other_metadata = fixture
      .publish_bytes_in_run(
        other_root,
        VIEW_MEMORY_PURPOSE,
        "application/json",
        serde_json::to_vec(&matching_memory).expect("view-memory JSON"),
      )
      .await;
    let other_snapshot = fixture.store.load_snapshot(other_run_id).await.expect("load other snapshot").expect("other run snapshot");
    assert_eq!(
      read_view_memory(fixture.store(), &other_snapshot, other_metadata.uri()).await.expect("read cross-run memory directly"),
      matching_memory,
      "the rejected cross-run artifact must otherwise be a valid matching payload"
    );

    let mut lineage = persisted.lineage;
    lineage.memory_uri = Some(other_metadata.uri().clone());
    let snapshot = fixture.snapshot().await;

    let artifacts = read_canonical_playlist_artifacts(fixture.store(), &snapshot, &lineage, true)
      .await
      .expect("scan remains usable when memory provenance is rejected");
    let candidate = resolve_playlist_play_candidate(&artifacts, "obs1.candidate.hermetic.test").expect("candidate from canonical scan");

    assert!(artifacts.memory().is_none());
    assert!(candidate.memory().is_none());
    assert!(artifacts.read_limits().iter().any(|limit| limit.contains("cross-run")));
  });
}

#[test]
fn canonical_reader_rejects_unrelated_memory_source_before_candidate_reacquisition() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let scan = sample_scan();
    let persisted = fixture.persist_playlist_scan(&scan).await;
    let mut memory = persisted.memory.expect("typed view memory");
    memory.source_reconstruction_ref = ArtifactUri::from_ids(fixture.run_id, ArtifactId::new()).to_string();
    let metadata = fixture.publish_memory(&memory).await;
    let mut lineage = persisted.lineage;
    lineage.memory_uri = Some(metadata.uri().clone());
    let snapshot = fixture.snapshot().await;

    let artifacts = read_canonical_playlist_artifacts(fixture.store(), &snapshot, &lineage, true)
      .await
      .expect("scan remains usable when memory lineage is rejected");
    let candidate = resolve_playlist_play_candidate(&artifacts, "obs1.candidate.hermetic.test").expect("candidate from canonical scan");

    assert!(artifacts.memory().is_none());
    assert!(candidate.memory().is_none());
    assert!(artifacts.read_limits().iter().any(|limit| limit.contains("source reconstruction artifact")));
  });
}

#[test]
fn canonical_reader_rejects_stale_memory_before_candidate_reacquisition() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let scan = sample_scan();
    let persisted = fixture.persist_playlist_scan(&scan).await;
    let mut memory = persisted.memory.expect("typed view memory");
    memory.last_reconstructed_at_millis = 1;
    let metadata = fixture.publish_memory(&memory).await;
    let mut lineage = persisted.lineage;
    lineage.memory_uri = Some(metadata.uri().clone());
    let snapshot = fixture.snapshot().await;

    let artifacts = read_canonical_playlist_artifacts(fixture.store(), &snapshot, &lineage, true)
      .await
      .expect("scan remains usable when stale memory is rejected");
    let candidate = resolve_playlist_play_candidate(&artifacts, "obs1.candidate.hermetic.test").expect("candidate from canonical scan");

    assert!(artifacts.memory().is_none());
    assert!(candidate.memory().is_none());
    assert!(artifacts.read_limits().iter().any(|limit| limit.contains("stale")));
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
