use std::sync::Arc;

use auv_game_balatro::card_detection_quality::{
  CARD_DETECTION_QUALITY_MANIFEST_SCHEMA_VERSION, CARD_DETECTION_QUALITY_PURPOSE, CardDetectionQualityManifest, CardDetectionQualityVerdict,
  publish_card_detection_quality,
};
use auv_stage_status::StageStatus;
use auv_tracing::{Context, MemoryTracingStore, RunId, TraceRecord, configure, dispatcher};

#[test]
fn public_quality_publisher_records_the_artifact() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryTracingStore::new());
    let dispatch = configure().tracing_store(store.clone()).build().expect("memory tracing dispatch");
    let context = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let manifest = CardDetectionQualityManifest {
      schema_version: CARD_DETECTION_QUALITY_MANIFEST_SCHEMA_VERSION,
      generated_at_millis: 1,
      card_detection_eval_witness_manifest_path: "fixture.json".to_string(),
      witness_status: StageStatus::Ready,
      status: StageStatus::Ready,
      reason: None,
      verdict: CardDetectionQualityVerdict::MeasuredOnly,
      quality_backend: None,
      detector_model_id: None,
      metrics: None,
      known_limits: vec![],
    };

    let metadata =
      publish_card_detection_quality(Some(&context), &manifest).await.expect("quality publication").expect("enabled publication");
    dispatch.flush().await.expect("flush tracing");

    assert_eq!(metadata.purpose().as_str(), CARD_DETECTION_QUALITY_PURPOSE);
    assert!(store.artifact(metadata.uri()).is_some());
    assert!(store.records().iter().any(|record| matches!(record, TraceRecord::Artifact { metadata: stored, .. } if stored == &metadata)));
  });
}
