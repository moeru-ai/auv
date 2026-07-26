use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use auv_cli_invoke::commands::scan::{produce_scan_coverage, produce_scan_frame};
use auv_tracing::{
  ArtifactBody, ArtifactMetadata, ArtifactRequest, BoxFuture, Context, ErrorCode, RunId, StoreError, TraceRecord, TracingStore, configure,
  dispatcher,
};

fn single_frame_fixture_dir() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("..")
    .join("auv-scan")
    .join("tests")
    .join("testdata")
    .join("scan")
    .join("temporal")
    .join("single_frame_v0")
}

fn coverage_stable_fixture_dir() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("..")
    .join("auv-scan")
    .join("tests")
    .join("testdata")
    .join("scan")
    .join("coverage")
    .join("coverage_stable_v0")
}

struct RejectArtifactStore {
  writes: AtomicUsize,
}

impl RejectArtifactStore {
  fn new() -> Self {
    Self {
      writes: AtomicUsize::new(0),
    }
  }
}

impl TracingStore for RejectArtifactStore {
  fn write(&self, _record: TraceRecord) -> BoxFuture<'_, Result<(), StoreError>> {
    Box::pin(async { Ok(()) })
  }

  fn write_artifact(&self, _request: ArtifactRequest, _body: ArtifactBody) -> BoxFuture<'_, Result<ArtifactMetadata, StoreError>> {
    self.writes.fetch_add(1, Ordering::SeqCst);
    Box::pin(async { Err(StoreError::new(ErrorCode::parse("auv.test.scan_artifact_rejected").expect("test error code"))) })
  }

  fn flush(&self) -> BoxFuture<'_, Result<(), StoreError>> {
    Box::pin(async { Ok(()) })
  }
}

#[test]
fn typed_scan_calls_return_domain_values_without_cli_context() {
  let frame = futures_executor::block_on(produce_scan_frame(single_frame_fixture_dir())).expect("typed frame");
  let coverage = futures_executor::block_on(produce_scan_coverage(coverage_stable_fixture_dir())).expect("typed coverage");

  assert_eq!(frame.schema_version, auv_scan::SCAN_FRAME_SCHEMA_VERSION);
  let frame_json = serde_json::to_value(&frame).expect("frame JSON");
  assert!(!frame_json.to_string().contains("file_name"));
  assert!(!frame_json.to_string().contains("media_type"));
  assert_eq!(coverage.status(), &auv_scan::CoverageStatus::Complete);
}

#[tokio::test]
async fn coverage_result_is_unchanged_by_publication_failure() {
  let store = Arc::new(RejectArtifactStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().expect("rejecting dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let future = root.in_scope(|| produce_scan_coverage(coverage_stable_fixture_dir()));

  let coverage = root.instrument(future).await.expect("domain coverage remains successful");

  assert_eq!(coverage.status(), &auv_scan::CoverageStatus::Complete);
  assert!(dispatch.flush().await.is_err(), "recording failure remains on the dispatch");
  assert_eq!(store.writes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn frame_does_not_publish_a_dangling_record_when_image_publication_fails() {
  let store = Arc::new(RejectArtifactStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().expect("rejecting dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let future = root.in_scope(|| produce_scan_frame(single_frame_fixture_dir()));

  let frame = root.instrument(future).await.expect("domain frame remains successful");

  assert_eq!(frame.schema_version, auv_scan::SCAN_FRAME_SCHEMA_VERSION);
  assert!(dispatch.flush().await.is_err(), "recording failure remains on the dispatch");
  assert_eq!(store.writes.load(Ordering::SeqCst), 1, "frame payload must not be attempted without a committed image URI");
}
