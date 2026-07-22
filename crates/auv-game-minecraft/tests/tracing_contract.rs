use std::error::Error as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use auv_game_minecraft::artifact::{MINECRAFT_PROJECTION_PURPOSE, publish_minecraft_projection, read_minecraft_projection};
use auv_game_minecraft::scene_packet::{MINECRAFT_SCENE_PACKET_PURPOSE, publish_minecraft_scene_packet, read_minecraft_scene_packet};
use auv_game_minecraft::training_job::{MINECRAFT_TRAINING_JOB_PURPOSE, publish_minecraft_training_job, read_minecraft_training_job};
use auv_game_minecraft::training_package::{
  MINECRAFT_TRAINING_PACKAGE_PURPOSE, publish_minecraft_training_package, read_minecraft_training_package,
};
use auv_game_minecraft::training_result::{
  MINECRAFT_TRAINING_RESULT_PURPOSE, publish_minecraft_training_result, read_minecraft_training_result,
};
use auv_game_minecraft::training_result_holdout_preview::{
  MINECRAFT_TRAINING_HOLDOUT_PREVIEW_PURPOSE, publish_minecraft_training_holdout_preview, read_minecraft_training_holdout_preview,
};
use auv_game_minecraft::training_result_holdout_render_quality::{
  MINECRAFT_TRAINING_HOLDOUT_RENDER_QUALITY_PURPOSE, publish_minecraft_training_holdout_render_quality,
  read_minecraft_training_holdout_render_quality,
};
use auv_game_minecraft::training_result_semantic::{
  MINECRAFT_TRAINING_SEMANTIC_PURPOSE, publish_minecraft_training_semantic, read_minecraft_training_semantic,
};
use auv_game_minecraft::training_result_spatial_query::{
  MINECRAFT_TRAINING_SPATIAL_QUERY_PURPOSE, publish_minecraft_training_spatial_query, read_minecraft_training_spatial_query,
};
use auv_game_minecraft::{
  MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, MinecraftArtifactPublishError, MinecraftArtifactReadError, MinecraftProjectionArtifact,
  ProjectionViewportBounds, ProjectionVisibility,
};
use auv_tracing::{
  ArtifactBody, ArtifactId, ArtifactPurpose, ArtifactReadError, ArtifactReader, ArtifactUri, ArtifactWriteError, Attributes, AuthorityId,
  BoxFuture, ByteLength, CommitError, CommitResult, ContentType, Context, ErrorCode, IdempotencyKey, MemoryRunStore, PageLimit, ReadError,
  RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision, RunSnapshot, RunStore, RunSubscription, Sha256Digest,
  StoreArtifactRequest, TelemetryError, TelemetryItem, TelemetryProjector, TelemetryRoutePolicy, configure, dispatcher,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Default)]
struct CountingProjector {
  item_count: AtomicUsize,
}

impl TelemetryProjector for CountingProjector {
  fn project(&self, _item: TelemetryItem) -> BoxFuture<'_, Result<(), TelemetryError>> {
    self.item_count.fetch_add(1, Ordering::Relaxed);
    Box::pin(async { Ok(()) })
  }

  fn flush(&self) -> BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async { Ok(()) })
  }
}

#[test]
fn minecraft_artifact_purposes_are_exact() {
  assert_eq!(
    [
      MINECRAFT_PROJECTION_PURPOSE,
      MINECRAFT_SCENE_PACKET_PURPOSE,
      MINECRAFT_TRAINING_JOB_PURPOSE,
      MINECRAFT_TRAINING_PACKAGE_PURPOSE,
      MINECRAFT_TRAINING_RESULT_PURPOSE,
      MINECRAFT_TRAINING_HOLDOUT_PREVIEW_PURPOSE,
      MINECRAFT_TRAINING_HOLDOUT_RENDER_QUALITY_PURPOSE,
      MINECRAFT_TRAINING_SEMANTIC_PURPOSE,
      MINECRAFT_TRAINING_SPATIAL_QUERY_PURPOSE,
    ],
    [
      "auv.minecraft.projection",
      "auv.minecraft.scene_packet",
      "auv.minecraft.training.job",
      "auv.minecraft.training.package",
      "auv.minecraft.training.result",
      "auv.minecraft.training.holdout_preview",
      "auv.minecraft.training.holdout_render_quality",
      "auv.minecraft.training.semantic",
      "auv.minecraft.training.spatial_query",
    ]
  );
}

#[test]
fn projection_round_trips_without_a_file_locator() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let expected = expected_projection();

    let published =
      publish_minecraft_projection(Some(&root), &expected).await.expect("publish projection").expect("enabled projection publication");
    dispatch.flush().await.expect("flush projection publication");
    let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("projection snapshot");

    let decoded = read_minecraft_projection(store.as_ref(), &snapshot, published.uri()).await.expect("read projection");

    assert_eq!(decoded, expected);
    assert_eq!(published.purpose().as_str(), MINECRAFT_PROJECTION_PURPOSE);
    assert!(published.uri().to_string().starts_with("auv://runs/"));
  });
}

#[test]
fn all_typed_minecraft_payloads_round_trip_under_exact_json_purposes() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let scene_packet = sample_scene_packet();
    let training_job = sample_training_job();
    let training_package = sample_training_package();
    let training_result = sample_training_result();
    let preview = sample_holdout_preview();
    let render_quality = sample_render_quality();
    let semantic = sample_semantic();
    let spatial_query = sample_spatial_query();

    let scene_packet_metadata =
      publish_minecraft_scene_packet(Some(&root), &scene_packet).await.expect("publish scene packet").expect("enabled publication");
    let training_job_metadata =
      publish_minecraft_training_job(Some(&root), &training_job).await.expect("publish training job").expect("enabled publication");
    let training_package_metadata = publish_minecraft_training_package(Some(&root), &training_package)
      .await
      .expect("publish training package")
      .expect("enabled publication");
    let training_result_metadata =
      publish_minecraft_training_result(Some(&root), &training_result).await.expect("publish training result").expect("enabled publication");
    let preview_metadata = publish_minecraft_training_holdout_preview(Some(&root), &preview)
      .await
      .expect("publish holdout preview")
      .expect("enabled publication");
    let render_quality_metadata = publish_minecraft_training_holdout_render_quality(Some(&root), &render_quality)
      .await
      .expect("publish render quality")
      .expect("enabled publication");
    let semantic_metadata =
      publish_minecraft_training_semantic(Some(&root), &semantic).await.expect("publish semantic").expect("enabled publication");
    let spatial_query_metadata = publish_minecraft_training_spatial_query(Some(&root), &spatial_query)
      .await
      .expect("publish spatial query")
      .expect("enabled publication");
    dispatch.flush().await.expect("flush typed artifacts");
    let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("typed artifact snapshot");

    let json_content_type = ContentType::parse("application/json").expect("JSON content type");
    for metadata in [
      &scene_packet_metadata,
      &training_job_metadata,
      &training_package_metadata,
      &training_result_metadata,
      &preview_metadata,
      &render_quality_metadata,
      &semantic_metadata,
      &spatial_query_metadata,
    ] {
      assert_eq!(metadata.content_type(), &json_content_type);
    }
    assert_eq!(read_minecraft_scene_packet(store.as_ref(), &snapshot, scene_packet_metadata.uri()).await.unwrap(), scene_packet);
    assert_eq!(read_minecraft_training_job(store.as_ref(), &snapshot, training_job_metadata.uri()).await.unwrap(), training_job);
    assert_eq!(read_minecraft_training_package(store.as_ref(), &snapshot, training_package_metadata.uri()).await.unwrap(), training_package);
    assert_eq!(read_minecraft_training_result(store.as_ref(), &snapshot, training_result_metadata.uri()).await.unwrap(), training_result);
    assert_eq!(read_minecraft_training_holdout_preview(store.as_ref(), &snapshot, preview_metadata.uri()).await.unwrap(), preview);
    assert_eq!(
      read_minecraft_training_holdout_render_quality(store.as_ref(), &snapshot, render_quality_metadata.uri()).await.unwrap(),
      render_quality
    );
    assert_eq!(read_minecraft_training_semantic(store.as_ref(), &snapshot, semantic_metadata.uri()).await.unwrap(), semantic);
    assert_eq!(read_minecraft_training_spatial_query(store.as_ref(), &snapshot, spatial_query_metadata.uri()).await.unwrap(), spatial_query);
  });
}

#[test]
fn telemetry_only_publication_skips_validation_serialization_and_projection() {
  futures_executor::block_on(async {
    let projector = Arc::new(CountingProjector::default());
    let dispatch =
      configure().project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().expect("telemetry dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let mut projection = expected_projection();
    projection.viewport_bounds.width = f64::NAN;

    let published = publish_minecraft_projection(Some(&root), &projection).await.expect("telemetry-only publication is disabled");
    dispatch.flush().await.expect("flush telemetry-only dispatch");

    assert!(published.is_none());
    assert!(projection.viewport_bounds.width.is_nan());
    assert_eq!(projector.item_count.load(Ordering::Relaxed), 0);
  });
}

#[test]
fn enabled_publication_preserves_typed_domain_validation() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).build().expect("memory dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let mut projection = expected_projection();
    projection.viewport_bounds.width = f64::NAN;

    let error = publish_minecraft_projection(Some(&root), &projection).await.expect_err("invalid projection must fail");

    assert!(matches!(error, MinecraftArtifactPublishError::InvalidPayload { .. }));
  });
}

#[test]
fn enabled_publication_rejects_json_over_the_minecraft_limit() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).build().expect("memory dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let mut projection = expected_projection();
    projection.resource_pack_ids =
      vec!["x".repeat(usize::try_from(MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1).expect("test limit fits usize"))];

    let error = publish_minecraft_projection(Some(&root), &projection).await.expect_err("oversized JSON must fail");

    assert!(matches!(
      error,
      MinecraftArtifactPublishError::PayloadTooLarge { limit: MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, actual, .. }
        if actual > MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT
    ));
  });
}

#[test]
fn reader_rejects_snapshot_from_another_authority() {
  futures_executor::block_on(async {
    let published = published_projection().await;
    let other_store = MemoryRunStore::new(AuthorityId::new());

    let error = read_minecraft_projection(&other_store, &published.snapshot, &published.uri)
      .await
      .expect_err("snapshot authority must match store authority");

    assert!(matches!(error, MinecraftArtifactReadError::SnapshotAuthorityMismatch { .. }));
  });
}

#[test]
fn reader_rejects_uri_owned_by_another_run() {
  futures_executor::block_on(async {
    let published = published_projection().await;
    let wrong_owner = ArtifactUri::from_ids(RunId::new(), ArtifactId::new());

    let error = read_minecraft_projection(published.store.as_ref(), &published.snapshot, &wrong_owner)
      .await
      .expect_err("URI owner must match snapshot run");

    assert!(matches!(error, MinecraftArtifactReadError::WrongOwner { .. }));
  });
}

#[test]
fn reader_rejects_same_run_uri_absent_from_snapshot() {
  futures_executor::block_on(async {
    let published = published_projection().await;
    let dangling = ArtifactUri::from_ids(published.snapshot.run_id(), ArtifactId::new());

    let error = read_minecraft_projection(published.store.as_ref(), &published.snapshot, &dangling)
      .await
      .expect_err("URI must be committed in snapshot");

    assert!(matches!(error, MinecraftArtifactReadError::DanglingUri { .. }));
  });
}

#[test]
fn reader_rejects_wrong_committed_purpose_and_content_type() {
  futures_executor::block_on(async {
    let published = published_projection().await;
    let wrong_purpose = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["purpose"] = json!(MINECRAFT_SCENE_PACKET_PURPOSE);
    });
    let wrong_content_type = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["content_type"] = json!("text/plain");
    });

    let purpose_error = read_minecraft_projection(published.store.as_ref(), &wrong_purpose, &published.uri)
      .await
      .expect_err("purpose must match typed reader");
    let content_type_error = read_minecraft_projection(published.store.as_ref(), &wrong_content_type, &published.uri)
      .await
      .expect_err("content type must be application/json");

    assert!(matches!(purpose_error, MinecraftArtifactReadError::WrongPurpose { .. }));
    assert!(matches!(content_type_error, MinecraftArtifactReadError::WrongContentType { .. }));
  });
}

#[test]
fn reader_rejects_committed_length_mismatches_in_both_directions() {
  futures_executor::block_on(async {
    let published = published_projection().await;
    let committed = published.snapshot.artifacts().get(&published.uri).expect("published projection").metadata().byte_length().get();
    let shorter = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["byte_length"] = json!(committed - 1);
    });
    let longer = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["byte_length"] = json!(committed + 1);
    });

    let shorter_error =
      read_minecraft_projection(published.store.as_ref(), &shorter, &published.uri).await.expect_err("extra bytes must fail");
    let longer_error =
      read_minecraft_projection(published.store.as_ref(), &longer, &published.uri).await.expect_err("short bytes must fail");

    assert!(matches!(shorter_error, MinecraftArtifactReadError::LengthMismatch { .. }));
    assert!(matches!(longer_error, MinecraftArtifactReadError::LengthMismatch { .. }));
  });
}

#[test]
fn reader_preserves_open_failure_source() {
  futures_executor::block_on(async {
    let published = published_projection().await;
    let source = ReadError::Unavailable(ErrorCode::parse("auv.test.open_unavailable").expect("open code"));
    let store = ControlledOpenRunStore::new(published.store.clone(), OpenArtifactBehavior::Fail(source.clone()));

    let error = read_minecraft_projection(&store, &published.snapshot, &published.uri).await.expect_err("open failure must propagate");

    assert_eq!(error.source().and_then(|value| value.downcast_ref::<ReadError>()), Some(&source));
    assert!(matches!(error, MinecraftArtifactReadError::Open { .. }));
    assert_eq!(store.open_count(), 1);
  });
}

#[test]
fn reader_preserves_mid_stream_failure_source() {
  futures_executor::block_on(async {
    let published = published_projection().await;
    let body = serde_json::to_vec(&expected_projection()).expect("projection JSON");
    let source = ArtifactReadError::Unavailable(ErrorCode::parse("auv.test.stream_unavailable").expect("stream code"));
    let store =
      ControlledOpenRunStore::new(published.store.clone(), OpenArtifactBehavior::Stream(vec![Ok(body[..7].to_vec()), Err(source.clone())]));

    let error = read_minecraft_projection(&store, &published.snapshot, &published.uri).await.expect_err("stream failure must propagate");

    assert_eq!(error.source().and_then(|value| value.downcast_ref::<ArtifactReadError>()), Some(&source));
    assert!(matches!(error, MinecraftArtifactReadError::Stream { .. }));
    assert_eq!(store.open_count(), 1);
  });
}

#[test]
fn reader_accepts_chunked_stream_and_rejects_short_and_extra_streams() {
  futures_executor::block_on(async {
    let published = published_projection().await;
    let body = serde_json::to_vec(&expected_projection()).expect("projection JSON");
    let chunks = body.chunks(11).map(|chunk| Ok(chunk.to_vec())).collect();
    let chunked = ControlledOpenRunStore::new(published.store.clone(), OpenArtifactBehavior::Stream(chunks));
    let mut short_body = body.clone();
    short_body.pop().expect("projection JSON is non-empty");
    let short = ControlledOpenRunStore::new(published.store.clone(), OpenArtifactBehavior::Stream(vec![Ok(short_body)]));
    let mut extra_body = body;
    extra_body.extend_from_slice(b"extra");
    let extra = ControlledOpenRunStore::new(published.store.clone(), OpenArtifactBehavior::Stream(vec![Ok(extra_body)]));

    assert_eq!(
      read_minecraft_projection(&chunked, &published.snapshot, &published.uri).await.expect("chunked projection"),
      expected_projection()
    );
    assert!(matches!(
      read_minecraft_projection(&short, &published.snapshot, &published.uri).await.expect_err("short stream must fail"),
      MinecraftArtifactReadError::LengthMismatch { .. }
    ));
    assert!(matches!(
      read_minecraft_projection(&extra, &published.snapshot, &published.uri).await.expect_err("extra stream must fail"),
      MinecraftArtifactReadError::LengthMismatch { .. }
    ));
  });
}

#[test]
fn reader_rejects_wrong_digest_and_oversize_metadata_before_opening() {
  futures_executor::block_on(async {
    let published = published_projection().await;
    let wrong_digest = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["sha256"] = json!("00".repeat(32));
    });
    let oversized = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["byte_length"] = json!(MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1);
    });
    let store = ControlledOpenRunStore::new(published.store.clone(), OpenArtifactBehavior::Delegate);

    let digest_error =
      read_minecraft_projection(published.store.as_ref(), &wrong_digest, &published.uri).await.expect_err("digest mismatch must fail");
    let oversize_error =
      read_minecraft_projection(&store, &oversized, &published.uri).await.expect_err("oversized metadata must fail before open");

    assert!(matches!(digest_error, MinecraftArtifactReadError::DigestMismatch { .. }));
    assert!(matches!(oversize_error, MinecraftArtifactReadError::PayloadTooLarge { .. }));
    assert_eq!(store.open_count(), 0);
  });
}

#[test]
fn empty_committed_artifact_is_a_member_but_not_valid_projection_json() {
  futures_executor::block_on(async {
    let store = MemoryRunStore::new(AuthorityId::new());
    let run_id = RunId::new();
    let uri = write_artifact(&store, run_id, MINECRAFT_PROJECTION_PURPOSE, "application/json", Vec::new()).await;
    let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("empty artifact snapshot");

    let error = read_minecraft_projection(&store, &snapshot, &uri).await.expect_err("empty artifact is malformed JSON");

    assert!(matches!(error, MinecraftArtifactReadError::MalformedJson { .. }));
  });
}

struct PublishedProjection {
  store: Arc<MemoryRunStore>,
  snapshot: RunSnapshot,
  uri: ArtifactUri,
}

#[derive(Clone)]
enum OpenArtifactBehavior {
  Delegate,
  Fail(ReadError),
  Stream(Vec<Result<Vec<u8>, ArtifactReadError>>),
}

struct ControlledOpenRunStore {
  inner: Arc<MemoryRunStore>,
  behavior: OpenArtifactBehavior,
  open_count: AtomicUsize,
}

impl ControlledOpenRunStore {
  fn new(inner: Arc<MemoryRunStore>, behavior: OpenArtifactBehavior) -> Self {
    Self {
      inner,
      behavior,
      open_count: AtomicUsize::new(0),
    }
  }

  fn open_count(&self) -> usize {
    self.open_count.load(Ordering::Relaxed)
  }
}

impl RunStore for ControlledOpenRunStore {
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

  fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    self.open_count.fetch_add(1, Ordering::Relaxed);
    match &self.behavior {
      OpenArtifactBehavior::Delegate => self.inner.open_artifact(uri),
      OpenArtifactBehavior::Fail(source) => {
        let source = source.clone();
        Box::pin(async move { Err(source) })
      }
      OpenArtifactBehavior::Stream(chunks) => {
        let chunks = chunks.clone();
        Box::pin(async move {
          let reader: ArtifactReader = Box::pin(futures_util::stream::iter(chunks.into_iter().map(|chunk| chunk.map(Into::into))));
          Ok(reader)
        })
      }
    }
  }
}

async fn published_projection() -> PublishedProjection {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
  let metadata =
    publish_minecraft_projection(Some(&root), &expected_projection()).await.expect("publish projection").expect("enabled publication");
  dispatch.flush().await.expect("flush projection");
  let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("projection snapshot");
  PublishedProjection {
    store,
    snapshot,
    uri: metadata.uri().clone(),
  }
}

fn snapshot_with_metadata(snapshot: &RunSnapshot, uri: &ArtifactUri, mutate: impl FnOnce(&mut Value)) -> RunSnapshot {
  let mut value = serde_json::to_value(snapshot).expect("serialize snapshot");
  let metadata = value["artifacts"][uri.to_string()]["metadata"].as_object_mut().expect("artifact metadata object");
  let mut metadata_value = Value::Object(std::mem::take(metadata));
  mutate(&mut metadata_value);
  value["artifacts"][uri.to_string()]["metadata"] = metadata_value;
  serde_json::from_value(value).expect("deserialize adversarial snapshot")
}

async fn write_artifact(store: &MemoryRunStore, run_id: RunId, purpose: &str, content_type: &str, body: Vec<u8>) -> ArtifactUri {
  let artifact_id = ArtifactId::new();
  let uri = ArtifactUri::from_ids(run_id, artifact_id);
  let request = StoreArtifactRequest::new(
    store.authority_id(),
    run_id,
    IdempotencyKey::new(),
    artifact_id,
    None,
    ArtifactPurpose::parse(purpose).expect("artifact purpose"),
    ContentType::parse(content_type).expect("content type"),
    ByteLength::new(body.len() as u64).expect("byte length"),
    Sha256Digest::new(Sha256::digest(&body).into()),
    Attributes::empty(),
  );
  store.write_artifact(request, Box::pin(futures_util::io::Cursor::new(body))).await.expect("write artifact");
  uri
}

fn sample_scene_packet() -> auv_game_minecraft::ScenePacketManifest {
  decode_fixture(json!({
    "schema_version": 1,
    "generated_at_millis": 20,
    "source_bundle_manifest_paths": ["bundle.json"],
    "source_run_ids": ["run-source"],
    "counts": {"frames": 0, "screenshots": 0, "missing_screenshots": 0},
    "frames": [],
    "known_limits": ["fixture"]
  }))
}

fn sample_training_job() -> auv_game_minecraft::TrainingLaunchJobManifest {
  decode_fixture(json!({
    "schema_version": 1,
    "generated_at_millis": 20,
    "source_training_launch_plan_path": "launch.json",
    "source_training_package_manifest_path": "package.json",
    "source_training_package_inspect_report_path": "package-inspect.json",
    "source_scene_packet_manifest_path": "scene.json",
    "source_bundle_manifest_paths": ["bundle.json"],
    "source_run_ids": ["run-source"],
    "counts": {"frames": 0, "images": 0, "compatibility_exported_frames": 0, "compatibility_skipped_frames": 0},
    "compatibility_view_name": "nerfstudio",
    "provider_backend": "fixture",
    "trainer_backend": "nerfstudio",
    "job_backend": "fixture",
    "job_submission_endpoint": "fixture://submit",
    "job_submission_command": "submit",
    "training_data_dir": "training-data",
    "export_report_path": "export.json",
    "suggested_output_dir": "result",
    "launch_command": "train",
    "status": "submitted",
    "known_limits": ["fixture"]
  }))
}

fn sample_training_package() -> auv_game_minecraft::TrainingPackageManifest {
  decode_fixture(json!({
    "schema_version": 1,
    "generated_at_millis": 20,
    "source_scene_packet_manifest_path": "scene.json",
    "source_bundle_manifest_paths": ["bundle.json"],
    "source_run_ids": ["run-source"],
    "counts": {"frames": 0, "images": 0, "compatibility_exported_frames": 0, "compatibility_skipped_frames": 0},
    "frames": [],
    "compatibility_views": [],
    "known_limits": ["fixture"]
  }))
}

fn sample_training_result() -> auv_game_minecraft::TrainingResultManifest {
  decode_fixture(json!({
    "schema_version": 1,
    "generated_at_millis": 20,
    "source_training_job_manifest_path": "job.json",
    "source_training_launch_plan_path": "launch.json",
    "source_training_package_manifest_path": "package.json",
    "source_scene_packet_manifest_path": "scene.json",
    "source_bundle_manifest_paths": ["bundle.json"],
    "source_run_ids": ["run-source"],
    "trainer_backend": "nerfstudio",
    "job_backend": "fixture",
    "job_submission_endpoint": "fixture://submit",
    "source_job_status": "submitted",
    "status": "succeeded",
    "job_id": "job-20",
    "result_dir": "result",
    "exported_frame_count": 0,
    "skipped_frame_count": 0,
    "result_artifacts": [],
    "known_limits": ["fixture"]
  }))
}

fn sample_holdout_preview() -> auv_game_minecraft::TrainingResultHoldoutPreviewManifest {
  decode_fixture(json!({
    "schema_version": 1,
    "generated_at_millis": 20,
    "training_result_semantic_manifest_path": "semantic.json",
    "source_training_result_artifact_manifest_path": "result-artifacts.json",
    "source_training_result_manifest_path": "result.json",
    "source_training_job_manifest_path": "job.json",
    "source_training_launch_plan_path": "launch.json",
    "source_training_package_manifest_path": "package.json",
    "source_scene_packet_manifest_path": "scene.json",
    "source_bundle_manifest_paths": ["bundle.json"],
    "source_run_ids": ["run-source"],
    "trainer_backend": "nerfstudio",
    "job_backend": "fixture",
    "normalized_result_dir": "normalized",
    "holdout_frame_index": 0,
    "status": "ready",
    "known_limits": ["fixture"]
  }))
}

fn sample_render_quality() -> auv_game_minecraft::TrainingResultHoldoutRenderQualityManifest {
  decode_fixture(json!({
    "schema_version": 1,
    "generated_at_millis": 20,
    "training_result_semantic_manifest_path": "semantic.json",
    "holdout_preview_manifest_path": "preview.json",
    "source_training_result_artifact_manifest_path": "result-artifacts.json",
    "source_training_result_manifest_path": "result.json",
    "source_training_job_manifest_path": "job.json",
    "source_training_launch_plan_path": "launch.json",
    "source_training_package_manifest_path": "package.json",
    "source_scene_packet_manifest_path": "scene.json",
    "source_bundle_manifest_paths": ["bundle.json"],
    "source_run_ids": ["run-source"],
    "trainer_backend": "nerfstudio",
    "job_backend": "fixture",
    "normalized_result_dir": "normalized",
    "holdout_frame_index": 0,
    "render_backend": "external_command",
    "image_size_match": true,
    "status": "ready",
    "verdict": "measured_only",
    "known_limits": ["fixture"]
  }))
}

fn sample_semantic() -> auv_game_minecraft::TrainingResultSemanticManifest {
  decode_fixture(json!({
    "schema_version": 1,
    "generated_at_millis": 20,
    "source_training_result_artifact_manifest_path": "result-artifacts.json",
    "source_training_result_manifest_path": "result.json",
    "source_training_job_manifest_path": "job.json",
    "source_training_launch_plan_path": "launch.json",
    "source_training_package_manifest_path": "package.json",
    "source_scene_packet_manifest_path": "scene.json",
    "source_bundle_manifest_paths": ["bundle.json"],
    "source_run_ids": ["run-source"],
    "trainer_backend": "nerfstudio",
    "job_backend": "fixture",
    "source_result_status": "succeeded",
    "normalized_result_dir": "normalized",
    "semantic_status": "ready",
    "config_path": "config.yml",
    "models_dir_path": "models",
    "checkpoint_files": [],
    "checkpoint_count": 0,
    "known_limits": ["fixture"]
  }))
}

fn sample_spatial_query() -> auv_game_minecraft::TrainingResultSpatialQueryManifest {
  decode_fixture(json!({
    "schema_version": 1,
    "generated_at_millis": 20,
    "training_result_semantic_manifest_path": "semantic.json",
    "source_training_result_artifact_manifest_path": "result-artifacts.json",
    "source_training_result_manifest_path": "result.json",
    "source_training_job_manifest_path": "job.json",
    "source_training_launch_plan_path": "launch.json",
    "source_training_package_manifest_path": "package.json",
    "source_scene_packet_manifest_path": "scene.json",
    "source_bundle_manifest_paths": ["bundle.json"],
    "source_run_ids": ["run-source"],
    "trainer_backend": "nerfstudio",
    "job_backend": "fixture",
    "normalized_result_dir": "normalized",
    "query_kind": "block_projection",
    "target_block": {"x": 1, "y": 2, "z": 3},
    "target_semantics": "block_center",
    "selected_backend": "projection_reference",
    "status": "answered",
    "visibility": "visible",
    "screen_point": {"x": 12.0, "y": 34.0},
    "match_radius_px": 8.0,
    "confidence": 1.0,
    "basis_frame_id": "frame-20",
    "comparison_verdict": "match",
    "known_limits": ["fixture"]
  }))
}

fn decode_fixture<T: serde::de::DeserializeOwned>(value: Value) -> T {
  serde_json::from_value(value).expect("typed Minecraft fixture")
}

fn expected_projection() -> MinecraftProjectionArtifact {
  MinecraftProjectionArtifact {
    spatial_frame_id: "frame-20".to_string(),
    world_tick: 20,
    monotonic_timestamp_ms: 2_000,
    screenshot_artifact_ref: Some("screenshot-20".to_string()),
    mc_capture_skew_ms: Some(8),
    viewport_bounds: ProjectionViewportBounds {
      x: 0.0,
      y: 0.0,
      width: 1_280.0,
      height: 720.0,
    },
    projected_point: None,
    visibility: ProjectionVisibility::OutsideWindow,
    raycast_block_id: Some("minecraft:stone".to_string()),
    screen_state: Some("in_game".to_string()),
    resource_pack_ids: vec!["vanilla".to_string()],
    mismatch_refusal_reason: None,
    verification_reference: Some("verification-20".to_string()),
  }
}
