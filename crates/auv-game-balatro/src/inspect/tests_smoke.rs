use std::sync::Arc;

use auv_tracing::{
  ArtifactPurpose, Attributes, AuthorityId, ByteLength, ContentType, Context, MemoryRunStore, NewArtifact, RunId, RunSnapshot, RunStore,
  Sha256Digest, configure, dispatcher,
};
use sha2::{Digest, Sha256};

#[test]
fn balatro_section_emits_empty_headers_from_a_canonical_snapshot() {
  futures_executor::block_on(async {
    let (store, snapshot) = snapshot_without_balatro_artifacts().await;

    let text = super::render_balatro_card_detection_text(store.as_ref(), &snapshot).await.expect("render canonical Balatro section");

    assert!(text.contains("\nBalatro Card Detection Semantic:\n- none\n"));
    assert!(text.contains("\nBalatro Card Detection Spatial Query:\n- none\n"));
    assert!(text.contains("\nBalatro Card Detection Eval Witness:\n- none\n"));
    assert!(text.contains("\nBalatro Card Detection Quality:\n- none\n"));
  });
}

#[test]
fn balatro_section_checks_authority_even_without_artifacts() {
  futures_executor::block_on(async {
    let (_, snapshot) = snapshot_without_balatro_artifacts().await;
    let other_store = MemoryRunStore::new(AuthorityId::new());

    let error =
      super::render_balatro_card_detection_text(&other_store, &snapshot).await.expect_err("empty snapshots retain authority checks");

    assert!(matches!(error, crate::BalatroArtifactReadError::SnapshotAuthorityMismatch { .. }));
  });
}

async fn snapshot_without_balatro_artifacts() -> (Arc<MemoryRunStore>, RunSnapshot) {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
  let body = b"{}".to_vec();
  let artifact = NewArtifact::new(
    ArtifactPurpose::parse("auv.test.balatro.unrelated").expect("unrelated artifact purpose"),
    ContentType::parse("application/json").expect("JSON content type"),
    ByteLength::new(u64::try_from(body.len()).expect("body length fits u64")).expect("artifact byte length"),
    Sha256Digest::new(Sha256::digest(&body).into()),
    Attributes::empty(),
    futures_util::io::Cursor::new(body),
  );
  let published = root.in_scope(|| auv_tracing::emit_artifact!(artifact)).await.expect("publish unrelated artifact");
  assert!(published.is_some());
  dispatch.flush().await.expect("flush unrelated artifact");
  let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("Balatro snapshot");
  (store, snapshot)
}
