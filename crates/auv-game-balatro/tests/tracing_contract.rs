use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use auv_game_balatro::card_detection_eval_witness::{
  CARD_DETECTION_EVAL_WITNESS_PURPOSE, publish_card_detection_witness, read_card_detection_witness,
};
use auv_game_balatro::card_detection_quality::{
  CARD_DETECTION_QUALITY_PURPOSE, publish_card_detection_quality, read_card_detection_quality,
};
use auv_game_balatro::card_detection_semantic::{
  CARD_DETECTION_SEMANTIC_PURPOSE, publish_card_detection_semantic, read_card_detection_semantic,
};
use auv_game_balatro::card_detection_spatial_query::{
  CARD_DETECTION_SPATIAL_QUERY_PURPOSE, publish_card_detection_spatial_query, read_card_detection_spatial_query,
};
use auv_game_balatro::{
  BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, BalatroArtifactPublishError, BalatroArtifactReadError, CardDetectionEvalWitnessManifest,
  CardDetectionQualityBackend, CardDetectionQualityManifest, CardDetectionQualityVerdict, CardDetectionSemanticManifest,
  CardDetectionSlotScore, CardDetectionSpatialQueryBackend, CardDetectionSpatialQueryManifest, CardDetectionSpatialQueryStatus,
};
use auv_stage_status::StageStatus;
use auv_tracing::{
  ArtifactId, ArtifactPurpose, ArtifactUri, Attributes, AuthorityId, BoxFuture, ByteLength, ContentType, Context, IdempotencyKey,
  MemoryRunStore, RunId, RunSnapshot, RunStore, Sha256Digest, StoreArtifactRequest, TelemetryError, TelemetryItem, TelemetryProjector,
  TelemetryRoutePolicy, configure, dispatcher,
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
fn card_detection_artifact_purposes_are_exact() {
  assert_eq!(
    [
      CARD_DETECTION_QUALITY_PURPOSE,
      CARD_DETECTION_SEMANTIC_PURPOSE,
      CARD_DETECTION_SPATIAL_QUERY_PURPOSE,
      CARD_DETECTION_EVAL_WITNESS_PURPOSE,
    ],
    [
      "auv.balatro.card_detection.quality",
      "auv.balatro.card_detection.semantic",
      "auv.balatro.card_detection.spatial_query",
      "auv.balatro.card_detection.eval_witness",
    ]
  );
}

#[test]
fn typed_witness_round_trips_through_the_run_store() {
  futures_executor::block_on(async {
    let expected = sample_witness();
    let published = published_witness(&expected).await;
    let selected = published.snapshot.artifacts().get(&published.uri).expect("published witness URI").metadata();
    assert!(published.uri.to_string().starts_with("auv://runs/"));

    let decoded = read_card_detection_witness(published.store.as_ref(), &published.snapshot, selected.uri()).await.expect("decode witness");

    assert_eq!(decoded, expected);
  });
}

#[test]
fn all_typed_card_detection_values_round_trip_under_exact_purposes() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let quality = sample_quality();
    let semantic = sample_semantic();
    let spatial_query = sample_spatial_query();
    let witness = sample_witness();

    let quality_metadata =
      publish_card_detection_quality(Some(&root), &quality).await.expect("publish quality").expect("enabled quality publication");
    let semantic_metadata =
      publish_card_detection_semantic(Some(&root), &semantic).await.expect("publish semantic").expect("enabled semantic publication");
    let spatial_query_metadata = publish_card_detection_spatial_query(Some(&root), &spatial_query)
      .await
      .expect("publish spatial query")
      .expect("enabled spatial-query publication");
    let witness_metadata =
      publish_card_detection_witness(Some(&root), &witness).await.expect("publish witness").expect("enabled witness publication");
    dispatch.flush().await.expect("flush card-detection artifacts");
    let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("card-detection snapshot");

    assert_eq!(quality_metadata.purpose(), &ArtifactPurpose::parse(CARD_DETECTION_QUALITY_PURPOSE).expect("quality purpose"));
    assert_eq!(semantic_metadata.purpose(), &ArtifactPurpose::parse(CARD_DETECTION_SEMANTIC_PURPOSE).expect("semantic purpose"));
    assert_eq!(
      spatial_query_metadata.purpose(),
      &ArtifactPurpose::parse(CARD_DETECTION_SPATIAL_QUERY_PURPOSE).expect("spatial-query purpose")
    );
    assert_eq!(witness_metadata.purpose(), &ArtifactPurpose::parse(CARD_DETECTION_EVAL_WITNESS_PURPOSE).expect("witness purpose"));
    assert_eq!(read_card_detection_quality(store.as_ref(), &snapshot, quality_metadata.uri()).await.expect("read quality"), quality);
    assert_eq!(read_card_detection_semantic(store.as_ref(), &snapshot, semantic_metadata.uri()).await.expect("read semantic"), semantic);
    assert_eq!(
      read_card_detection_spatial_query(store.as_ref(), &snapshot, spatial_query_metadata.uri()).await.expect("read spatial query"),
      spatial_query
    );
    assert_eq!(read_card_detection_witness(store.as_ref(), &snapshot, witness_metadata.uri()).await.expect("read witness"), witness);
  });
}

#[test]
fn telemetry_only_publication_does_not_serialize_or_project_witness_bytes() {
  futures_executor::block_on(async {
    let projector = Arc::new(CountingProjector::default());
    let dispatch =
      configure().project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().expect("telemetry-only dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let mut witness = sample_witness();
    witness.known_limits =
      vec!["x".repeat(usize::try_from(BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1).expect("test limit fits usize"))];
    let original_length = witness.known_limits[0].len();

    let published = publish_card_detection_witness(Some(&root), &witness).await.expect("telemetry-only publication is disabled");
    dispatch.flush().await.expect("flush telemetry-only dispatch");

    assert!(published.is_none());
    assert_eq!(witness.known_limits[0].len(), original_length);
    assert_eq!(projector.item_count.load(Ordering::Relaxed), 0);
  });
}

#[test]
fn absent_context_disables_every_producer_without_owning_a_runner_or_store() {
  futures_executor::block_on(async {
    assert!(publish_card_detection_quality(None, &sample_quality()).await.expect("disabled quality publication").is_none());
    assert!(publish_card_detection_semantic(None, &sample_semantic()).await.expect("disabled semantic publication").is_none());
    assert!(
      publish_card_detection_spatial_query(None, &sample_spatial_query()).await.expect("disabled spatial-query publication").is_none()
    );
    assert!(publish_card_detection_witness(None, &sample_witness()).await.expect("disabled witness publication").is_none());
  });
}

#[test]
fn enabled_publication_rejects_json_over_the_balatro_limit() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).build().expect("memory dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let mut witness = sample_witness();
    witness.known_limits =
      vec!["x".repeat(usize::try_from(BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1).expect("test limit fits usize"))];

    let error = publish_card_detection_witness(Some(&root), &witness).await.expect_err("oversized JSON must not be published");

    assert!(matches!(error, BalatroArtifactPublishError::PayloadTooLarge { .. }));
  });
}

#[test]
fn reader_rejects_snapshot_from_another_authority() {
  futures_executor::block_on(async {
    let published = published_witness(&sample_witness()).await;
    let other_store = MemoryRunStore::new(AuthorityId::new());

    let error = read_card_detection_witness(&other_store, &published.snapshot, &published.uri)
      .await
      .expect_err("snapshot authority must match store authority");

    assert!(matches!(error, BalatroArtifactReadError::SnapshotAuthorityMismatch { .. }));
  });
}

#[test]
fn reader_rejects_uri_owned_by_another_run() {
  futures_executor::block_on(async {
    let published = published_witness(&sample_witness()).await;
    let wrong_owner = ArtifactUri::from_ids(RunId::new(), ArtifactId::new());

    let error = read_card_detection_witness(published.store.as_ref(), &published.snapshot, &wrong_owner)
      .await
      .expect_err("artifact URI owner must match snapshot run");

    assert!(matches!(error, BalatroArtifactReadError::WrongOwner { .. }));
  });
}

#[test]
fn reader_rejects_same_run_uri_absent_from_the_snapshot() {
  futures_executor::block_on(async {
    let published = published_witness(&sample_witness()).await;
    let dangling = ArtifactUri::from_ids(published.snapshot.run_id(), ArtifactId::new());

    let error = read_card_detection_witness(published.store.as_ref(), &published.snapshot, &dangling)
      .await
      .expect_err("artifact URI must be committed in the supplied snapshot");

    assert!(matches!(error, BalatroArtifactReadError::DanglingUri { .. }));
  });
}

#[test]
fn reader_rejects_wrong_committed_purpose() {
  futures_executor::block_on(async {
    let published = published_witness(&sample_witness()).await;
    let snapshot = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["purpose"] = json!(CARD_DETECTION_QUALITY_PURPOSE);
    });

    let error = read_card_detection_witness(published.store.as_ref(), &snapshot, &published.uri)
      .await
      .expect_err("committed purpose must match the typed reader");

    assert!(matches!(error, BalatroArtifactReadError::WrongPurpose { .. }));
  });
}

#[test]
fn reader_rejects_wrong_committed_content_type() {
  futures_executor::block_on(async {
    let published = published_witness(&sample_witness()).await;
    let snapshot = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["content_type"] = json!("text/plain");
    });

    let error = read_card_detection_witness(published.store.as_ref(), &snapshot, &published.uri)
      .await
      .expect_err("committed content type must be application/json");

    assert!(matches!(error, BalatroArtifactReadError::WrongContentType { .. }));
  });
}

#[test]
fn reader_rejects_committed_length_mismatches_in_both_directions() {
  futures_executor::block_on(async {
    let published = published_witness(&sample_witness()).await;
    let committed_length = published.snapshot.artifacts().get(&published.uri).expect("published witness").metadata().byte_length().get();
    let shorter = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["byte_length"] = json!(committed_length - 1);
    });
    let longer = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["byte_length"] = json!(committed_length + 1);
    });

    let shorter_error = read_card_detection_witness(published.store.as_ref(), &shorter, &published.uri)
      .await
      .expect_err("bytes longer than committed metadata must fail");
    let longer_error = read_card_detection_witness(published.store.as_ref(), &longer, &published.uri)
      .await
      .expect_err("bytes shorter than committed metadata must fail");

    assert!(matches!(shorter_error, BalatroArtifactReadError::LengthMismatch { .. }));
    assert!(matches!(longer_error, BalatroArtifactReadError::LengthMismatch { .. }));
  });
}

#[test]
fn reader_rejects_wrong_committed_digest() {
  futures_executor::block_on(async {
    let published = published_witness(&sample_witness()).await;
    let snapshot = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["sha256"] = json!("00".repeat(32));
    });

    let error = read_card_detection_witness(published.store.as_ref(), &snapshot, &published.uri)
      .await
      .expect_err("committed digest must match streamed bytes");

    assert!(matches!(error, BalatroArtifactReadError::DigestMismatch { .. }));
  });
}

#[test]
fn reader_rejects_committed_json_over_the_balatro_limit_before_opening() {
  futures_executor::block_on(async {
    let published = published_witness(&sample_witness()).await;
    let snapshot = snapshot_with_metadata(&published.snapshot, &published.uri, |metadata| {
      metadata["byte_length"] = json!(BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1);
    });

    let error = read_card_detection_witness(published.store.as_ref(), &snapshot, &published.uri)
      .await
      .expect_err("oversized committed JSON must be rejected");

    assert!(matches!(error, BalatroArtifactReadError::PayloadTooLarge { .. }));
  });
}

#[test]
fn reader_applies_typed_json_validation_to_an_empty_committed_artifact() {
  futures_executor::block_on(async {
    let store = MemoryRunStore::new(AuthorityId::new());
    let run_id = RunId::new();
    let uri = write_artifact(&store, run_id, CARD_DETECTION_EVAL_WITNESS_PURPOSE, "application/json", Vec::new()).await;
    let snapshot = store.load_snapshot(run_id).await.expect("load empty artifact snapshot").expect("empty artifact snapshot");

    let error = read_card_detection_witness(&store, &snapshot, &uri).await.expect_err("empty bytes are not a typed JSON witness");

    assert!(matches!(error, BalatroArtifactReadError::MalformedJson { .. }));
  });
}

struct PublishedWitness {
  store: Arc<MemoryRunStore>,
  snapshot: RunSnapshot,
  uri: ArtifactUri,
}

async fn published_witness(expected: &CardDetectionEvalWitnessManifest) -> PublishedWitness {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
  let metadata = publish_card_detection_witness(Some(&root), expected).await.expect("publish witness").expect("enabled publication");
  dispatch.flush().await.expect("flush witness");
  let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("witness snapshot");
  assert_eq!(
    snapshot.artifacts().get(metadata.uri()).expect("published metadata in snapshot").metadata().purpose(),
    &ArtifactPurpose::parse(CARD_DETECTION_EVAL_WITNESS_PURPOSE).expect("witness purpose")
  );
  PublishedWitness {
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
    ContentType::parse(content_type).expect("artifact content type"),
    ByteLength::new(u64::try_from(body.len()).expect("body length fits u64")).expect("artifact byte length"),
    Sha256Digest::new(Sha256::digest(&body).into()),
    Attributes::empty(),
  );
  store.write_artifact(request, Box::pin(futures_util::io::Cursor::new(body))).await.expect("write artifact directly");
  uri
}

fn sample_witness() -> CardDetectionEvalWitnessManifest {
  CardDetectionEvalWitnessManifest {
    schema_version: 1,
    generated_at_millis: 42,
    card_detection_semantic_manifest_path: "semantic.json".to_string(),
    card_detection_spatial_query_manifest_path: "spatial-query.json".to_string(),
    expected_slots_path: "expected-slots.json".to_string(),
    source_detection_bundle_dir: "bundle".to_string(),
    expected_slot_count: 1,
    scored_slot_count: 1,
    unscored_slot_count: 0,
    below_confidence_slot_count: 0,
    quality_backend: CardDetectionQualityBackend::UltralyticsOnnxEntities,
    detector_model_id: Some("fixture-detector".to_string()),
    slot_scores: vec![CardDetectionSlotScore {
      zone: "hand".to_string(),
      index: 0,
      scored: true,
      confidence: Some(0.9),
    }],
    status: StageStatus::Ready,
    reason: None,
    known_limits: vec!["fixture".to_string()],
  }
}

fn sample_quality() -> CardDetectionQualityManifest {
  CardDetectionQualityManifest {
    schema_version: 2,
    generated_at_millis: 42,
    card_detection_eval_witness_manifest_path: "witness.json".to_string(),
    witness_status: StageStatus::Ready,
    status: StageStatus::Ready,
    reason: None,
    verdict: CardDetectionQualityVerdict::MeasuredOnly,
    quality_backend: Some(CardDetectionQualityBackend::UltralyticsOnnxEntities),
    detector_model_id: Some("fixture-detector".to_string()),
    metrics: None,
    known_limits: vec!["fixture".to_string()],
  }
}

fn sample_semantic() -> CardDetectionSemanticManifest {
  CardDetectionSemanticManifest {
    schema_version: 1,
    generated_at_millis: 42,
    source_detection_bundle_path: "bundle/manifest.json".to_string(),
    source_detection_bundle_dir: "bundle".to_string(),
    frame_source: "fixture.png".to_string(),
    image_width: 1920,
    image_height: 1080,
    ui_detection_count: 1,
    entities_detection_count: 1,
    semantic_status: StageStatus::Ready,
    semantic_reason: None,
    known_limits: vec!["fixture".to_string()],
  }
}

fn sample_spatial_query() -> CardDetectionSpatialQueryManifest {
  CardDetectionSpatialQueryManifest {
    schema_version: 1,
    generated_at_millis: 42,
    card_detection_semantic_manifest_path: "semantic.json".to_string(),
    source_detection_bundle_dir: "bundle".to_string(),
    target_zone: "hand".to_string(),
    target_index: 0,
    query_backend: CardDetectionSpatialQueryBackend::DetectionBundleReference,
    status: CardDetectionSpatialQueryStatus::Answered,
    reason: None,
    pixel_x: Some(100.0),
    pixel_y: Some(200.0),
    image_width: Some(1920),
    image_height: Some(1080),
    known_limits: vec!["fixture".to_string()],
  }
}
