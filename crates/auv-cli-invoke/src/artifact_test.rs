use std::sync::Arc;

use super::*;
use auv_tracing::{
  ArtifactBody, ArtifactRequest, BoxFuture, Context, ErrorCode, MemoryTracingStore, RunId, StoreError, TraceRecord, TracingStore, configure,
  dispatcher,
};

#[test]
fn emitted_image_decodes_to_the_exact_source_pixels() {
  let image = RgbaImage::from_fn(2, 3, |x, y| image::Rgba([x as u8, y as u8, (x + y) as u8, 255]));
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().expect("dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));

  root.in_scope(|| emit_image("auv.test.image", &image));
  futures_executor::block_on(dispatch.flush()).expect("flush");
  let records = store.records();
  let metadata = records
    .iter()
    .find_map(|record| match record {
      TraceRecord::Artifact { metadata, .. } => Some(metadata),
      _ => None,
    })
    .expect("image artifact");
  let encoded = store.artifact(metadata.uri()).expect("image body");
  let decoded = image::load_from_memory_with_format(&encoded, image::ImageFormat::WebP).expect("decode WebP").into_rgba8();

  assert_eq!(metadata.byte_length().get(), encoded.len() as u64);
  assert_eq!(metadata.file_extension(), Some("webp"));
  assert_eq!(decoded, image);
}

#[test]
fn emitted_image_receipt_can_be_attached_to_the_direct_command_result() {
  let image = RgbaImage::new(2, 3);
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store).build().expect("dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let future = root.in_scope(|| emit_image_with_receipt("auv.test.primary_image", &image));

  let metadata = futures_executor::block_on(root.instrument(future)).expect("image receipt");

  assert_eq!(metadata.purpose().as_str(), "auv.test.primary_image");
  assert_eq!(metadata.content_type().as_str(), "image/webp");
  assert_eq!(metadata.file_extension(), Some("webp"));
}

#[test]
fn detached_artifact_failure_does_not_change_primary_value() {
  let store = Arc::new(RejectArtifactStore::new());
  let dispatch = configure().tracing_store(store).build().expect("dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let image = RgbaImage::new(1, 1);
  let value = root.in_scope(|| {
    emit_image("auv.test.rejected", &image);
    42
  });

  assert_eq!(value, 42);
  futures_executor::block_on(dispatch.flush()).expect_err("detached write rejection must reach the dispatch reporter");
}

#[test]
fn detached_artifact_publication_is_read_from_the_tracing_store() {
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().expect("dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let image = RgbaImage::new(1, 1);
  root.in_scope(|| {
    emit_image("auv.test.direct_metadata", &image);
  });

  futures_executor::block_on(dispatch.flush()).expect("detached publication must flush");
  let records = store.records();
  assert!(matches!(
    records.as_slice(),
    [TraceRecord::Artifact { metadata, .. }] if metadata.purpose().as_str() == "auv.test.direct_metadata"
  ));
}

struct RejectArtifactStore;

impl RejectArtifactStore {
  fn new() -> Self {
    Self
  }
}

impl TracingStore for RejectArtifactStore {
  fn write(&self, _record: TraceRecord) -> BoxFuture<'_, Result<(), StoreError>> {
    Box::pin(async { Ok(()) })
  }

  fn write_artifact(&self, _request: ArtifactRequest, _body: ArtifactBody) -> BoxFuture<'_, Result<ArtifactMetadata, StoreError>> {
    Box::pin(async { Err(StoreError::new(ErrorCode::new("auv.test.artifact_rejected"))) })
  }

  fn flush(&self) -> BoxFuture<'_, Result<(), StoreError>> {
    Box::pin(async { Ok(()) })
  }
}
