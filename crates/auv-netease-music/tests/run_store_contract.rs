#![cfg(feature = "tracing")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use auv_netease_music::run_artifacts::{
  NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, NETEASE_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE, NeteaseArtifactPublishError,
  NeteaseArtifactReadError, PLAYLIST_SELECT_RESULT_PURPOSE, PLAYLIST_SIDEBAR_SCAN_PURPOSE, PlaylistArtifactPublication, VIEW_MEMORY_PURPOSE,
  persist_playlist_ls_artifacts, persist_playlist_select_proof, read_canonical_playlist_artifacts, read_playlist_select_result,
  read_playlist_sidebar_scan, read_view_memory,
};
use auv_netease_music::{
  Inputs, PlaylistSelectResult, PlaylistSidebarScan, decode_playlist_sidebar_scan_json, resolve_playlist_play_candidate,
};
use auv_tracing::{
  ArtifactBody, ArtifactId, ArtifactMetadata, ArtifactPurpose, ArtifactReader, ArtifactUri, ArtifactWriteError, Attributes, AuthorityId,
  BoxFuture, ByteLength, CommitError, CommitResult, ContentType, Context, Dispatch, ErrorCode, FileRunStore, IdempotencyKey, MemoryRunStore,
  NewArtifact, PageLimit, ReadArtifactError, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision, RunSnapshot,
  RunStore, RunSubscription, Sha256Digest, StoreArtifactRequest, TelemetryError, TelemetryItem, TelemetryProjector, TelemetryRoutePolicy,
  configure, dispatcher,
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

  async fn persist_playlist_scan(&self, scan: &PlaylistSidebarScan) -> PlaylistArtifactPublication {
    self.persist_playlist_scan_with_memory(scan, true).await
  }

  async fn persist_playlist_scan_with_memory(&self, scan: &PlaylistSidebarScan, memory_enabled: bool) -> PlaylistArtifactPublication {
    let mut inputs = Inputs::with_defaults();
    inputs.app_id = scan.app().app_id.clone().expect("fixture app id");
    let future = self.root.in_scope(|| persist_playlist_ls_artifacts(scan, &inputs, memory_enabled));
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
    self.publish_bytes_with_attributes(self.root.clone(), purpose, content_type, Attributes::empty(), bytes).await
  }

  async fn publish_bytes_with_attributes(
    &self,
    root: Context,
    purpose: &str,
    content_type: &str,
    attributes: Attributes,
    bytes: Vec<u8>,
  ) -> ArtifactMetadata {
    let artifact = NewArtifact::new(
      ArtifactPurpose::parse(purpose).expect("artifact purpose"),
      ContentType::parse(content_type).expect("content type"),
      ByteLength::new(bytes.len() as u64).expect("artifact byte length"),
      Sha256Digest::new(Sha256::digest(&bytes).into()),
      attributes,
      Cursor::new(bytes),
    );
    let emission = root.in_scope(|| auv_tracing::emit_artifact!(artifact));
    let metadata = root.instrument(emission).await.expect("artifact publication").expect("enabled artifact publication");
    self.dispatch.flush().await.expect("flush artifact");
    metadata
  }

  async fn publish_memory(&self, memory: &ViewMemory) -> ArtifactMetadata {
    self
      .publish_bytes_with_attributes(
        self.root.clone(),
        VIEW_MEMORY_PURPOSE,
        "application/json",
        Attributes::empty(),
        serde_json::to_vec(memory).expect("view-memory JSON"),
      )
      .await
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

    assert!(persisted.scan_uri.to_string().starts_with("auv://runs/"));
    let snapshot = fixture.snapshot().await;
    assert_eq!(fixture.read_scan(&snapshot, &persisted.scan_uri).await, scan);
    let memory_uri = snapshot
      .artifacts()
      .iter()
      .find_map(|(uri, published)| (published.metadata().purpose().as_str() == VIEW_MEMORY_PURPOSE).then_some(uri))
      .expect("view-memory URI");
    let memory = fixture.read_memory(&snapshot, memory_uri).await;
    assert_eq!(memory, persisted.memory.expect("persisted view memory"));
    assert_eq!(memory.source_scan_uri, persisted.scan_uri);
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

    let scan = snapshot.artifacts().get(&persisted.scan_uri).expect("scan metadata").metadata();
    assert_eq!(scan.purpose().as_str(), PLAYLIST_SIDEBAR_SCAN_PURPOSE);
    assert_eq!(scan.content_type().to_string(), "application/json");
    let memory = snapshot
      .artifacts()
      .values()
      .find(|published| published.metadata().purpose().as_str() == VIEW_MEMORY_PURPOSE)
      .expect("memory metadata")
      .metadata();
    assert_eq!(memory.purpose().as_str(), VIEW_MEMORY_PURPOSE);
    assert_eq!(memory.content_type().to_string(), "application/json");
    assert!(memory.attributes().is_empty(), "view-memory lineage belongs to the typed payload");
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
      NeteaseArtifactReadError::Read {
        source: ReadArtifactError::WrongPurpose {
          expected, actual, ..
        },
      } => {
        assert_eq!(expected, ArtifactPurpose::parse(PLAYLIST_SIDEBAR_SCAN_PURPOSE).unwrap());
        assert_eq!(actual, ArtifactPurpose::parse("auv.netease.other").unwrap());
      }
      other => panic!("expected typed wrong-purpose error, got {other:?}"),
    }
    let error = read_playlist_sidebar_scan(fixture.store(), &snapshot, wrong_content_type.uri()).await.expect_err("wrong content type");
    match error {
      NeteaseArtifactReadError::Read {
        source: ReadArtifactError::WrongContentType {
          expected, actual, ..
        },
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

    assert!(matches!(
      publication,
      Err(NeteaseArtifactPublishError::Json {
        source: auv_tracing::JsonArtifactError::PayloadTooLarge { .. },
        ..
      })
    ));
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
  let run_id = stdout
    .lines()
    .find_map(|line| line.strip_prefix("OK. Run: "))
    .expect("standalone invoke should return its run id")
    .parse::<RunId>()
    .expect("standalone invoke run id");
  let store = FileRunStore::open(&store_root).expect("open standalone CLI store");
  let snapshot =
    futures_executor::block_on(store.load_snapshot(run_id)).expect("load standalone CLI run").expect("standalone CLI run must exist");
  assert!(
    snapshot.artifacts().values().any(|artifact| artifact.metadata().purpose().as_str() == PLAYLIST_SIDEBAR_SCAN_PURPOSE),
    "standalone CLI run should contain the sidebar scan artifact"
  );

  let _ = std::fs::remove_dir_all(store_root);
}

#[test]
fn standalone_invoke_without_store_renders_the_direct_report() {
  let fixture_dir = auv_netease_music::invoke::hermetic_select_proof_fixture_dir();
  let output = std::process::Command::new(env!("CARGO_BIN_EXE_auv-netease-music"))
    .args([
      "invoke",
      auv_netease_music::invoke::SELECT_PROOF_COMMAND_ID,
      "--fixture-dir",
      fixture_dir.to_str().expect("fixture path should be UTF-8"),
    ])
    .output()
    .expect("standalone CLI should launch");

  assert!(
    output.status.success(),
    "standalone CLI failed:\nstdout:\n{}\nstderr:\n{}",
    String::from_utf8_lossy(&output.stdout),
    String::from_utf8_lossy(&output.stderr)
  );
  let stdout = String::from_utf8(output.stdout).expect("CLI stdout should be UTF-8");
  assert!(stdout.contains("Query: hermetic-fixture"), "direct invoke report missing from stdout:\n{stdout}");
  assert!(stdout.contains("Artifact purpose: auv.netease.playlist_select_result"), "typed artifact purpose missing from stdout:\n{stdout}");
}

#[test]
fn public_typed_candidate_operation_uses_caller_read_scan() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let scan = sample_scan();
    let persisted = fixture.persist_playlist_scan(&scan).await;
    let memory = persisted.memory.clone().expect("typed view memory");
    let snapshot = fixture.snapshot().await;
    let artifacts = read_canonical_playlist_artifacts(fixture.store(), &snapshot, &persisted.scan_uri, "com.netease.163music", true)
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
fn canonical_reader_uses_the_typed_payload_uri_as_the_memory_link_authority() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let scan = sample_scan();
    let first = fixture.persist_playlist_scan(&scan).await;
    let second = fixture.persist_playlist_scan_with_memory(&scan, false).await;
    let mut second_memory = first.memory.clone().expect("first typed view memory");
    second_memory.source_scan_uri = second.scan_uri.clone();
    fixture.publish_memory(&second_memory).await;
    let snapshot = fixture.snapshot().await;

    let first_artifacts = read_canonical_playlist_artifacts(fixture.store(), &snapshot, &first.scan_uri, "com.netease.163music", true)
      .await
      .expect("first scan artifacts");
    let second_artifacts = read_canonical_playlist_artifacts(fixture.store(), &snapshot, &second.scan_uri, "com.netease.163music", true)
      .await
      .expect("second scan artifacts");

    assert!(first_artifacts.memory().is_some(), "first scan should find its linked memory");
    assert_eq!(second_artifacts.memory(), Some(&second_memory), "typed payload URI must link memory to the second scan");
  });
}

#[test]
fn canonical_reader_rejects_scan_for_another_requested_app() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let persisted = fixture.persist_playlist_scan(&sample_scan()).await;
    let snapshot = fixture.snapshot().await;

    let error = read_canonical_playlist_artifacts(fixture.store(), &snapshot, &persisted.scan_uri, "com.example.OtherPlayer", true)
      .await
      .expect_err("scan app must match the caller-selected app");

    assert_eq!(error.code().as_str(), "auv.netease.artifact.invalid_reference");
    assert!(error.to_string().contains("com.example.OtherPlayer"));
  });
}

#[test]
fn canonical_reader_rejects_stale_memory_before_candidate_reacquisition() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let scan = sample_scan();
    let source = fixture.persist_playlist_scan(&scan).await;
    let requested = fixture.persist_playlist_scan_with_memory(&scan, false).await;
    let mut memory = source.memory.expect("typed view memory");
    memory.source_scan_uri = requested.scan_uri.clone();
    memory.last_reconstructed_at_millis = 1;
    fixture.publish_memory(&memory).await;
    let snapshot = fixture.snapshot().await;

    let artifacts = read_canonical_playlist_artifacts(fixture.store(), &snapshot, &requested.scan_uri, "com.netease.163music", true)
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
