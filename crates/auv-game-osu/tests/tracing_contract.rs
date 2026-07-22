use std::error::Error as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use auv_game_osu::detection_eval_quality::{
  DETECTION_EVAL_QUALITY_MANIFEST_SCHEMA_VERSION, DetectionEvalQualityManifest, OSU_DETECTION_EVAL_QUALITY_PURPOSE,
  publish_osu_detection_eval_quality, read_osu_detection_eval_quality,
};
use auv_game_osu::detection_eval_witness::{
  DETECTION_EVAL_WITNESS_MANIFEST_SCHEMA_VERSION, DetectionEvalWitnessManifest, OSU_DETECTION_EVAL_WITNESS_PURPOSE,
  publish_osu_detection_eval_witness, read_osu_detection_eval_witness,
};
use auv_game_osu::projection::{OSU_PROJECTION_PURPOSE, ProjectionArtifact, publish_osu_projection, read_osu_projection};
use auv_game_osu::run_read::{OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, OsuArtifactPublishError, OsuArtifactReadError};
use auv_game_osu::visual_truth_semantic::{
  OSU_VISUAL_TRUTH_SEMANTIC_PURPOSE, VISUAL_TRUTH_SEMANTIC_MANIFEST_SCHEMA_VERSION, VisualTruthSemanticManifest,
  publish_osu_visual_truth_semantic, read_osu_visual_truth_semantic,
};
use auv_game_osu::visual_truth_spatial_query::{
  OSU_VISUAL_TRUTH_SPATIAL_QUERY_PURPOSE, VISUAL_TRUTH_SPATIAL_QUERY_MANIFEST_SCHEMA_VERSION, VisualTruthSpatialQueryManifest,
  publish_osu_visual_truth_spatial_query, read_osu_visual_truth_spatial_query,
};
use auv_tracing::{
  ArtifactBody, ArtifactId, ArtifactPurpose, ArtifactReadError, ArtifactReader, ArtifactUri, ArtifactWriteError, Attributes, AuthorityId,
  BoxFuture, ByteLength, CommitError, CommitResult, ContentType, Context, ErrorCode, IdempotencyKey, MemoryRunStore, PageLimit, ReadError,
  RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision, RunSnapshot, RunStore, RunSubscription, Sha256Digest,
  StoreArtifactRequest, configure, dispatcher,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[test]
fn osu_artifact_purposes_are_exact() {
  assert_eq!(
    [
      OSU_PROJECTION_PURPOSE,
      OSU_DETECTION_EVAL_QUALITY_PURPOSE,
      OSU_DETECTION_EVAL_WITNESS_PURPOSE,
      OSU_VISUAL_TRUTH_SEMANTIC_PURPOSE,
      OSU_VISUAL_TRUTH_SPATIAL_QUERY_PURPOSE,
    ],
    [
      "auv.osu.projection",
      "auv.osu.detection_eval.quality",
      "auv.osu.detection_eval.witness",
      "auv.osu.visual_truth.semantic",
      "auv.osu.visual_truth.spatial_query",
    ]
  );
}

#[test]
fn all_osu_payloads_round_trip_by_canonical_uri() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let projection = sample_projection();
    let quality = sample_quality();
    let witness = sample_witness();
    let semantic = sample_semantic();
    let spatial_query = sample_spatial_query();

    let projection_metadata =
      publish_osu_projection(Some(&root), &projection).await.expect("publish projection").expect("enabled projection publication");
    let quality_metadata =
      publish_osu_detection_eval_quality(Some(&root), &quality).await.expect("publish quality").expect("enabled quality publication");
    let witness_metadata =
      publish_osu_detection_eval_witness(Some(&root), &witness).await.expect("publish witness").expect("enabled witness publication");
    let semantic_metadata =
      publish_osu_visual_truth_semantic(Some(&root), &semantic).await.expect("publish semantic").expect("enabled semantic publication");
    let spatial_metadata = publish_osu_visual_truth_spatial_query(Some(&root), &spatial_query)
      .await
      .expect("publish spatial query")
      .expect("enabled spatial-query publication");
    dispatch.flush().await.expect("flush osu! artifacts");
    let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("osu! snapshot");

    assert_eq!(read_osu_projection(store.as_ref(), &snapshot, projection_metadata.uri()).await.expect("read projection"), projection);
    assert_eq!(read_osu_detection_eval_quality(store.as_ref(), &snapshot, quality_metadata.uri()).await.expect("read quality"), quality);
    assert_eq!(read_osu_detection_eval_witness(store.as_ref(), &snapshot, witness_metadata.uri()).await.expect("read witness"), witness);
    assert_eq!(read_osu_visual_truth_semantic(store.as_ref(), &snapshot, semantic_metadata.uri()).await.expect("read semantic"), semantic);
    assert_eq!(
      read_osu_visual_truth_spatial_query(store.as_ref(), &snapshot, spatial_metadata.uri()).await.expect("read spatial query"),
      spatial_query
    );
    assert_eq!(projection_metadata.purpose().as_str(), OSU_PROJECTION_PURPOSE);
    assert_eq!(quality_metadata.purpose().as_str(), OSU_DETECTION_EVAL_QUALITY_PURPOSE);
    assert_eq!(witness_metadata.purpose().as_str(), OSU_DETECTION_EVAL_WITNESS_PURPOSE);
    assert_eq!(semantic_metadata.purpose().as_str(), OSU_VISUAL_TRUTH_SEMANTIC_PURPOSE);
    assert_eq!(spatial_metadata.purpose().as_str(), OSU_VISUAL_TRUTH_SPATIAL_QUERY_PURPOSE);
  });
}

#[test]
fn publishers_reject_unsupported_versions_and_domain_invalid_payloads() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let mut quality = sample_quality();
    quality.schema_version += 1;
    let mut witness = sample_witness();
    witness.schema_version += 1;
    let mut semantic = sample_semantic();
    semantic.schema_version += 1;
    let mut spatial = sample_spatial_query();
    spatial.schema_version += 1;

    for result in [
      publish_osu_projection(Some(&root), &invalid_projection()).await,
      publish_osu_detection_eval_quality(Some(&root), &quality).await,
      publish_osu_detection_eval_witness(Some(&root), &witness).await,
      publish_osu_visual_truth_semantic(Some(&root), &semantic).await,
      publish_osu_visual_truth_spatial_query(Some(&root), &spatial).await,
    ] {
      assert!(matches!(result, Err(OsuArtifactPublishError::InvalidPayload { .. })));
    }

    let mut invalid_quality = sample_quality();
    invalid_quality.metrics.as_mut().expect("quality metrics").total_frames += 1;
    let mut invalid_witness = sample_witness();
    invalid_witness.frame_witnesses.clear();
    let mut invalid_semantic = sample_semantic();
    invalid_semantic.frame_count = 0;
    let mut invalid_spatial = sample_spatial_query();
    invalid_spatial.pixel_x = None;
    for result in [
      publish_osu_detection_eval_quality(Some(&root), &invalid_quality).await,
      publish_osu_detection_eval_witness(Some(&root), &invalid_witness).await,
      publish_osu_visual_truth_semantic(Some(&root), &invalid_semantic).await,
      publish_osu_visual_truth_spatial_query(Some(&root), &invalid_spatial).await,
    ] {
      assert!(matches!(result, Err(OsuArtifactPublishError::InvalidPayload { .. })));
    }
    dispatch.flush().await.expect("flush rejected publications");
    assert!(store.load_snapshot(run_id).await.expect("load rejected run").is_none());
  });
}

#[test]
fn quality_and_witness_publishers_reject_overflowing_count_invariants() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).build().expect("memory dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));

    let mut quality = sample_quality();
    let metrics = quality.metrics.as_mut().expect("quality metrics");
    metrics.total_frames = usize::MAX;
    metrics.label_matched_frames = usize::MAX;
    metrics.label_missing_frames = 1;
    metrics.spatial_matched_frames = usize::MAX;

    let mut witness_counts = sample_witness();
    witness_counts.total_frames = 1;
    witness_counts.label_matched_frames = usize::MAX;
    witness_counts.label_missing_frames = 1;

    let mut witness_spatial = sample_witness();
    witness_spatial.total_frames = 1;
    witness_spatial.spatial_matched_frames = usize::MAX;
    witness_spatial.spatial_missing_frames = 1;

    let mut witness_spurious = sample_witness();
    witness_spurious.total_frames = 2;
    witness_spurious.label_matched_frames = 2;
    witness_spurious.spatial_matched_frames = 2;
    let mut second_frame = witness_spurious.frame_witnesses[0].clone();
    witness_spurious.frame_witnesses[0].spurious_detection_count = usize::MAX;
    second_frame.spurious_detection_count = 1;
    witness_spurious.frame_witnesses.push(second_frame);

    for (result, expected_purpose, expected_message) in [
      (
        publish_osu_detection_eval_quality(Some(&root), &quality).await,
        OSU_DETECTION_EVAL_QUALITY_PURPOSE,
        "quality label counts overflow usize",
      ),
      (
        publish_osu_detection_eval_witness(Some(&root), &witness_counts).await,
        OSU_DETECTION_EVAL_WITNESS_PURPOSE,
        "witness label counts overflow usize",
      ),
      (
        publish_osu_detection_eval_witness(Some(&root), &witness_spatial).await,
        OSU_DETECTION_EVAL_WITNESS_PURPOSE,
        "witness spatial counts overflow usize",
      ),
      (
        publish_osu_detection_eval_witness(Some(&root), &witness_spurious).await,
        OSU_DETECTION_EVAL_WITNESS_PURPOSE,
        "witness frame spurious counts overflow usize",
      ),
    ] {
      match result.expect_err("overflowing counts must not publish") {
        OsuArtifactPublishError::InvalidPayload { purpose, message } => {
          assert_eq!(purpose.as_str(), expected_purpose);
          assert_eq!(message, expected_message);
        }
        other => panic!("expected InvalidPayload, got {other:?}"),
      }
    }
  });
}

#[test]
fn quality_and_witness_readers_reject_overflowing_count_invariants() {
  futures_executor::block_on(async {
    let store = MemoryRunStore::new(AuthorityId::new());

    let mut quality = sample_quality();
    let metrics = quality.metrics.as_mut().expect("quality metrics");
    metrics.total_frames = usize::MAX;
    metrics.label_matched_frames = usize::MAX;
    metrics.spatial_matched_frames = usize::MAX;
    metrics.spatial_missing_frames = 1;
    let (quality_snapshot, quality_uri) = write_json(&store, OSU_DETECTION_EVAL_QUALITY_PURPOSE, &quality).await;

    let mut witness = sample_witness();
    witness.total_frames = 2;
    witness.label_matched_frames = 2;
    witness.spatial_matched_frames = 2;
    let mut second_frame = witness.frame_witnesses[0].clone();
    witness.frame_witnesses[0].spurious_detection_count = usize::MAX;
    second_frame.spurious_detection_count = 1;
    witness.frame_witnesses.push(second_frame);
    let (witness_snapshot, witness_uri) = write_json(&store, OSU_DETECTION_EVAL_WITNESS_PURPOSE, &witness).await;

    match read_osu_detection_eval_quality(&store, &quality_snapshot, &quality_uri).await.expect_err("overflowing quality counts must fail") {
      OsuArtifactReadError::InvalidPayload { uri, message } => {
        assert_eq!(uri, quality_uri);
        assert_eq!(message, "quality spatial counts overflow usize");
      }
      other => panic!("expected InvalidPayload, got {other:?}"),
    }
    match read_osu_detection_eval_witness(&store, &witness_snapshot, &witness_uri).await.expect_err("overflowing witness counts must fail") {
      OsuArtifactReadError::InvalidPayload { uri, message } => {
        assert_eq!(uri, witness_uri);
        assert_eq!(message, "witness frame spurious counts overflow usize");
      }
      other => panic!("expected InvalidPayload, got {other:?}"),
    }
  });
}

#[test]
fn projection_publish_and_read_reject_f64_values_not_representable_as_f32() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let cases = [
      ("scale_x", {
        let mut value = sample_projection();
        value.scale_x = f64::MAX;
        value
      }),
      ("scale_y", {
        let mut value = sample_projection();
        value.scale_y = f64::MAX;
        value
      }),
      ("offset_x", {
        let mut value = sample_projection();
        value.offset_x = f64::MAX;
        value
      }),
      ("offset_y", {
        let mut value = sample_projection();
        value.offset_y = f64::MAX;
        value
      }),
    ];

    for (field, projection) in cases {
      assert!(
        matches!(publish_osu_projection(Some(&root), &projection).await, Err(OsuArtifactPublishError::InvalidPayload { .. })),
        "publisher accepted {field}=f64::MAX"
      );
      let (snapshot, uri) = write_json(store.as_ref(), OSU_PROJECTION_PURPOSE, &projection).await;
      assert!(
        matches!(read_osu_projection(store.as_ref(), &snapshot, &uri).await, Err(OsuArtifactReadError::InvalidPayload { .. })),
        "reader accepted {field}=f64::MAX"
      );
    }
  });
}

#[test]
fn reader_validates_authority_owner_and_membership_before_opening() {
  futures_executor::block_on(async {
    let published = published_projection().await;
    let other_store = MemoryRunStore::new(AuthorityId::new());
    let wrong_owner = ArtifactUri::from_ids(RunId::new(), ArtifactId::new());
    let dangling = ArtifactUri::from_ids(published.snapshot.run_id(), ArtifactId::new());

    let authority = read_osu_projection(&other_store, &published.snapshot, &published.uri).await.expect_err("authority must match first");
    let owner =
      read_osu_projection(published.store.as_ref(), &published.snapshot, &wrong_owner).await.expect_err("URI owner must match snapshot");
    let membership =
      read_osu_projection(published.store.as_ref(), &published.snapshot, &dangling).await.expect_err("URI must be committed in snapshot");

    assert!(matches!(authority, OsuArtifactReadError::SnapshotAuthorityMismatch { .. }));
    assert!(matches!(owner, OsuArtifactReadError::WrongOwner { .. }));
    assert!(matches!(membership, OsuArtifactReadError::DanglingUri { .. }));
  });
}

#[test]
fn reader_rejects_wrong_purpose_and_content_type_before_opening() {
  futures_executor::block_on(async {
    let published = published_projection().await;
    let wrong_purpose = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["purpose"] = json!(OSU_DETECTION_EVAL_QUALITY_PURPOSE);
    });
    let wrong_content_type = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["content_type"] = json!("text/plain");
    });
    let purpose_store = ControlledOpenRunStore::new(published.store.clone(), OpenArtifactBehavior::Delegate);
    let content_store = ControlledOpenRunStore::new(published.store.clone(), OpenArtifactBehavior::Delegate);

    let purpose = read_osu_projection(&purpose_store, &wrong_purpose, &published.uri).await.expect_err("purpose must match");
    let content = read_osu_projection(&content_store, &wrong_content_type, &published.uri).await.expect_err("content type must match");

    assert!(matches!(purpose, OsuArtifactReadError::WrongPurpose { .. }));
    assert!(matches!(content, OsuArtifactReadError::WrongContentType { .. }));
    assert_eq!(purpose_store.open_count(), 0);
    assert_eq!(content_store.open_count(), 0);
  });
}

#[test]
fn reader_enforces_metadata_and_exact_stream_lengths() {
  futures_executor::block_on(async {
    let published = published_projection().await;
    let body = serde_json::to_vec(&sample_projection()).expect("projection JSON");
    let committed = published.snapshot.artifacts().get(&published.uri).expect("projection metadata").metadata().byte_length().get();
    let short_metadata = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["byte_length"] = json!(committed - 1);
    });
    let long_metadata = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["byte_length"] = json!(committed + 1);
    });
    let oversized_metadata = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["byte_length"] = json!(OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1);
    });
    let bounded = ControlledOpenRunStore::new(published.store.clone(), OpenArtifactBehavior::Delegate);
    let stream_bound_snapshot = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["byte_length"] = json!(OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT);
    });
    let stream_bound = ControlledOpenRunStore::new(
      published.store.clone(),
      OpenArtifactBehavior::Stream(vec![Ok(vec![
        0;
        (OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1)
          as usize
      ])]),
    );
    let short_stream =
      ControlledOpenRunStore::new(published.store.clone(), OpenArtifactBehavior::Stream(vec![Ok(body[..body.len() - 1].to_vec())]));
    let mut extra_body = body;
    extra_body.push(b' ');
    let extra_stream = ControlledOpenRunStore::new(published.store.clone(), OpenArtifactBehavior::Stream(vec![Ok(extra_body)]));

    assert!(matches!(
      read_osu_projection(published.store.as_ref(), &short_metadata, &published.uri).await.expect_err("metadata too short"),
      OsuArtifactReadError::LengthMismatch { .. }
    ));
    assert!(matches!(
      read_osu_projection(published.store.as_ref(), &long_metadata, &published.uri).await.expect_err("metadata too long"),
      OsuArtifactReadError::LengthMismatch { .. }
    ));
    assert!(matches!(
      read_osu_projection(&bounded, &oversized_metadata, &published.uri).await.expect_err("metadata over bound"),
      OsuArtifactReadError::PayloadTooLarge { .. }
    ));
    assert_eq!(bounded.open_count(), 0);
    assert!(matches!(
      read_osu_projection(&stream_bound, &stream_bound_snapshot, &published.uri).await.expect_err("stream over bound"),
      OsuArtifactReadError::PayloadTooLarge { .. }
    ));
    assert!(matches!(
      read_osu_projection(&short_stream, &published.snapshot, &published.uri).await.expect_err("short stream"),
      OsuArtifactReadError::LengthMismatch { .. }
    ));
    assert!(matches!(
      read_osu_projection(&extra_stream, &published.snapshot, &published.uri).await.expect_err("extra stream"),
      OsuArtifactReadError::LengthMismatch { .. }
    ));
  });
}

#[test]
fn reader_accepts_chunked_body_and_verifies_digest() {
  futures_executor::block_on(async {
    let published = published_projection().await;
    let body = serde_json::to_vec(&sample_projection()).expect("projection JSON");
    let chunks = body.chunks(7).map(|chunk| Ok(chunk.to_vec())).collect();
    let chunked = ControlledOpenRunStore::new(published.store.clone(), OpenArtifactBehavior::Stream(chunks));
    let wrong_digest = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["sha256"] = json!("00".repeat(32));
    });

    assert_eq!(read_osu_projection(&chunked, &published.snapshot, &published.uri).await.expect("chunked projection"), sample_projection());
    assert!(matches!(
      read_osu_projection(published.store.as_ref(), &wrong_digest, &published.uri).await.expect_err("digest mismatch"),
      OsuArtifactReadError::DigestMismatch { .. }
    ));
  });
}

#[test]
fn reader_preserves_open_and_mid_stream_source_errors() {
  futures_executor::block_on(async {
    let published = published_projection().await;
    let open_source = ReadError::Unavailable(ErrorCode::parse("auv.test.osu.open_unavailable").expect("open code"));
    let open_store = ControlledOpenRunStore::new(published.store.clone(), OpenArtifactBehavior::Fail(open_source.clone()));
    let stream_source = ArtifactReadError::Unavailable(ErrorCode::parse("auv.test.osu.stream_unavailable").expect("stream code"));
    let body = serde_json::to_vec(&sample_projection()).expect("projection JSON");
    let stream_store = ControlledOpenRunStore::new(
      published.store.clone(),
      OpenArtifactBehavior::Stream(vec![Ok(body[..7].to_vec()), Err(stream_source.clone())]),
    );

    let open = read_osu_projection(&open_store, &published.snapshot, &published.uri).await.expect_err("open failure");
    let stream = read_osu_projection(&stream_store, &published.snapshot, &published.uri).await.expect_err("stream failure");

    assert_eq!(open.source().and_then(|source| source.downcast_ref::<ReadError>()), Some(&open_source));
    assert_eq!(stream.source().and_then(|source| source.downcast_ref::<ArtifactReadError>()), Some(&stream_source));
    assert!(matches!(open, OsuArtifactReadError::Open { .. }));
    assert!(matches!(stream, OsuArtifactReadError::Stream { .. }));
  });
}

#[test]
fn readers_reject_malformed_json_unsupported_versions_and_domain_invalid_payloads() {
  futures_executor::block_on(async {
    let store = MemoryRunStore::new(AuthorityId::new());
    let malformed_uri = write_artifact(&store, RunId::new(), OSU_PROJECTION_PURPOSE, "application/json", Vec::new()).await;
    let malformed_snapshot =
      store.load_snapshot(malformed_uri.run_id()).await.expect("load malformed snapshot").expect("malformed snapshot");
    assert!(matches!(
      read_osu_projection(&store, &malformed_snapshot, &malformed_uri).await.expect_err("malformed JSON"),
      OsuArtifactReadError::MalformedJson { .. }
    ));

    let mut invalid_projection = sample_projection();
    invalid_projection.match_radius_px = -1.0;
    let (projection_snapshot, projection_uri) = write_json(&store, OSU_PROJECTION_PURPOSE, &invalid_projection).await;
    assert!(matches!(
      read_osu_projection(&store, &projection_snapshot, &projection_uri).await.expect_err("invalid projection"),
      OsuArtifactReadError::InvalidPayload { .. }
    ));

    let mut quality = sample_quality();
    quality.schema_version += 1;
    let (quality_snapshot, quality_uri) = write_json(&store, OSU_DETECTION_EVAL_QUALITY_PURPOSE, &quality).await;
    let mut witness = sample_witness();
    witness.schema_version += 1;
    let (witness_snapshot, witness_uri) = write_json(&store, OSU_DETECTION_EVAL_WITNESS_PURPOSE, &witness).await;
    let mut semantic = sample_semantic();
    semantic.schema_version += 1;
    let (semantic_snapshot, semantic_uri) = write_json(&store, OSU_VISUAL_TRUTH_SEMANTIC_PURPOSE, &semantic).await;
    let mut spatial = sample_spatial_query();
    spatial.schema_version += 1;
    let (spatial_snapshot, spatial_uri) = write_json(&store, OSU_VISUAL_TRUTH_SPATIAL_QUERY_PURPOSE, &spatial).await;

    for result in [
      read_osu_detection_eval_quality(&store, &quality_snapshot, &quality_uri).await.map(|_| ()),
      read_osu_detection_eval_witness(&store, &witness_snapshot, &witness_uri).await.map(|_| ()),
      read_osu_visual_truth_semantic(&store, &semantic_snapshot, &semantic_uri).await.map(|_| ()),
      read_osu_visual_truth_spatial_query(&store, &spatial_snapshot, &spatial_uri).await.map(|_| ()),
    ] {
      assert!(matches!(result, Err(OsuArtifactReadError::InvalidPayload { .. })));
    }
  });
}

#[test]
fn publisher_rejects_json_over_bound_without_committing() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let mut semantic = sample_semantic();
    semantic.known_limits = vec!["x".repeat((OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1) as usize)];

    let error = publish_osu_visual_truth_semantic(Some(&root), &semantic).await.expect_err("oversized JSON");

    assert!(matches!(error, OsuArtifactPublishError::PayloadTooLarge { .. }));
    dispatch.flush().await.expect("flush oversized publication");
    assert!(store.load_snapshot(run_id).await.expect("load oversized run").is_none());
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
  let metadata = publish_osu_projection(Some(&root), &sample_projection()).await.expect("publish projection").expect("enabled publication");
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

async fn write_json<T: serde::Serialize>(store: &MemoryRunStore, purpose: &str, value: &T) -> (RunSnapshot, ArtifactUri) {
  let body = serde_json::to_vec(value).expect("serialize direct artifact");
  let run_id = RunId::new();
  let uri = write_artifact(store, run_id, purpose, "application/json", body).await;
  let snapshot = store.load_snapshot(run_id).await.expect("load direct snapshot").expect("direct snapshot");
  (snapshot, uri)
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

fn sample_projection() -> ProjectionArtifact {
  decode_fixture(serde_json::from_str(include_str!("fixtures/osu_eval_run_artifacts/projection.json")).expect("projection fixture JSON"))
}

fn invalid_projection() -> ProjectionArtifact {
  let mut projection = sample_projection();
  projection.scale_x = f64::NAN;
  projection
}

fn sample_quality() -> DetectionEvalQualityManifest {
  decode_fixture(json!({
    "schema_version": DETECTION_EVAL_QUALITY_MANIFEST_SCHEMA_VERSION,
    "generated_at_millis": 20,
    "detection_eval_witness_manifest_path": "witness.json",
    "source_visual_eval_report_path": "visual-eval.json",
    "source_run_artifact_dir": "run-artifacts",
    "detector_model_id": "model-1",
    "witness_status": "ready",
    "status": "ready",
    "verdict": "measured_only",
    "metrics": {
      "total_frames": 1,
      "label_matched_frames": 1,
      "label_missing_frames": 0,
      "label_unmapped_frames": 0,
      "spatial_matched_frames": 1,
      "spatial_missing_frames": 0,
      "spatial_unscored_frames": 0,
      "spurious_detection_count": 0,
      "label_recall": 1.0,
      "spatial_recall": 1.0,
      "projection_kind": "playfield_to_pixels"
    },
    "known_limits": ["fixture"]
  }))
}

fn sample_witness() -> DetectionEvalWitnessManifest {
  decode_fixture(json!({
    "schema_version": DETECTION_EVAL_WITNESS_MANIFEST_SCHEMA_VERSION,
    "generated_at_millis": 20,
    "source_visual_eval_report_path": "visual-eval.json",
    "source_detection_eval_manifest_path": "detection-eval.json",
    "source_run_artifact_dir": "run-artifacts",
    "source_visual_truth_manifest_path": "visual-truth.json",
    "source_projection_path": "projection.json",
    "detector_model_id": "model-1",
    "total_frames": 1,
    "label_matched_frames": 1,
    "label_missing_frames": 0,
    "label_unmapped_frames": 0,
    "spatial_matched_frames": 1,
    "spatial_missing_frames": 0,
    "spatial_unscored_frames": 0,
    "spurious_detection_count": 0,
    "projection_kind": "playfield_to_pixels",
    "frame_witnesses": [{
      "object_index": 0,
      "capture_phase": "before_dispatch",
      "capture_file_name": "capture.png",
      "object_kind": "circle",
      "expected_label": "hit_circle",
      "label_outcome": "matched",
      "spatial_outcome": "matched",
      "spurious_detection_count": 0
    }],
    "status": "ready",
    "known_limits": ["fixture"]
  }))
}

fn sample_semantic() -> VisualTruthSemanticManifest {
  decode_fixture(json!({
    "schema_version": VISUAL_TRUTH_SEMANTIC_MANIFEST_SCHEMA_VERSION,
    "generated_at_millis": 20,
    "source_run_artifact_dir": "run-artifacts",
    "source_visual_truth_manifest_path": "visual-truth.json",
    "source_projection_path": "projection.json",
    "beatmap_path": "map.osu",
    "frame_count": 1,
    "semantic_status": "ready",
    "known_limits": ["fixture"]
  }))
}

fn sample_spatial_query() -> VisualTruthSpatialQueryManifest {
  decode_fixture(json!({
    "schema_version": VISUAL_TRUTH_SPATIAL_QUERY_MANIFEST_SCHEMA_VERSION,
    "generated_at_millis": 20,
    "visual_truth_semantic_manifest_path": "semantic.json",
    "source_run_artifact_dir": "run-artifacts",
    "source_visual_truth_manifest_path": "visual-truth.json",
    "source_projection_path": "projection.json",
    "object_index": 0,
    "capture_phase": "before_dispatch",
    "object_kind": "circle",
    "query_backend": "playfield_projection_reference",
    "status": "answered",
    "pixel_visibility": "inside_capture",
    "pixel_x": 320.0,
    "pixel_y": 240.0,
    "match_radius_px": 20.0,
    "capture_width": 640,
    "capture_height": 480,
    "known_limits": ["fixture"]
  }))
}

fn decode_fixture<T: DeserializeOwned>(value: Value) -> T {
  serde_json::from_value(value).expect("decode typed fixture")
}
