use std::sync::Arc;

use auv_tracing::{
  ArtifactPurpose, Attributes, AuthorityId, ByteLength, ContentType, Context, MemoryRunStore, NewArtifact, RunId, RunSnapshot, RunStore,
  Sha256Digest, configure, dispatcher,
};
use sha2::{Digest, Sha256};

#[test]
fn osu_sections_preserve_empty_presentation_from_canonical_snapshot() {
  futures_executor::block_on(async {
    let (store, snapshot) = snapshot_without_osu_artifacts().await;
    let mut sections = crate::inspect_sections_primary(store.as_ref(), &snapshot).await.expect("primary sections");
    sections.extend(crate::inspect_sections_detection_eval(store.as_ref(), &snapshot).await.expect("detection eval sections"));

    assert_eq!(
      sections.iter().map(crate::inspect::OsuInspectSection::id).collect::<Vec<_>>(),
      ["osu_visual_truth_primary", "osu_detection_eval"]
    );
    let text = sections.into_iter().map(crate::inspect::OsuInspectSection::into_text).collect::<String>();
    assert!(text.contains("\nOsu Visual Truth Semantic:\n- none\n"));
    assert!(text.contains("\nOsu Visual Truth Spatial Query:\n- none\n"));
    assert!(text.contains("\nOsu Visual Truth Spatial Query Action Readiness:\n- none\n"));
    assert!(text.contains("\nOsu Detection Eval Witness:\n- none\n"));
    assert!(text.contains("\nOsu Detection Eval Quality:\n- none\n"));
    assert!(!text.contains("Query Wired Live Action"));
  });
}

#[test]
fn osu_sections_validate_authority_without_osu_artifacts() {
  futures_executor::block_on(async {
    let (_, snapshot) = snapshot_without_osu_artifacts().await;
    let other_store = MemoryRunStore::new(AuthorityId::new());

    let error = crate::inspect_sections_primary(&other_store, &snapshot).await.expect_err("authority mismatch");

    assert!(matches!(error, crate::run_read::OsuArtifactReadError::SnapshotAuthorityMismatch { .. }));
  });
}

async fn snapshot_without_osu_artifacts() -> (Arc<MemoryRunStore>, RunSnapshot) {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
  let body = b"{}".to_vec();
  let artifact = NewArtifact::new(
    ArtifactPurpose::parse("auv.test.osu.unrelated").expect("unrelated artifact purpose"),
    ContentType::parse("application/json").expect("JSON content type"),
    ByteLength::new(body.len() as u64).expect("body length"),
    Sha256Digest::new(Sha256::digest(&body).into()),
    Attributes::empty(),
    futures_util::io::Cursor::new(body),
  );
  root.in_scope(|| auv_tracing::emit_artifact!(artifact)).await.expect("publish unrelated artifact").expect("enabled publication");
  dispatch.flush().await.expect("flush unrelated artifact");
  let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("osu! snapshot");
  (store, snapshot)
}
