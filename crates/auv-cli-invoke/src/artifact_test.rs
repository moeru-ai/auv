use std::sync::Arc;

use image::RgbaImage;

use super::*;
use auv_tracing::{
  ArtifactBody, ArtifactRequest, BoxFuture, Context, ErrorCode, MemoryTracingStore, RunId, StoreError, TraceRecord, TracingStore, configure,
  dispatcher,
};

/// A capture of `image` at `scale_factor` backing pixels per point.
fn capture(image: RgbaImage, scale_factor: f64) -> auv_driver::Capture {
  auv_driver::Capture {
    origin: None,
    bounds: auv_driver::Rect::new(0.0, 0.0, f64::from(image.width()) / scale_factor, f64::from(image.height()) / scale_factor),
    image,
    scale_factor,
    backend: "fixture".to_string(),
    fallback_reason: None,
  }
}

#[test]
fn emitted_image_decodes_to_the_exact_source_pixels() {
  let image = RgbaImage::from_fn(2, 3, |x, y| image::Rgba([x as u8, y as u8, (x + y) as u8, 255]));
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().expect("dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));

  root.in_scope(|| emit_capture("auv.test.image", &capture(image.clone(), 1.0)));
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
  let capture = capture(image, 1.0);
  let future = root.in_scope(|| emit_capture_with_receipt("auv.test.primary_image", &capture));

  let metadata = futures_executor::block_on(root.instrument(future)).expect("image receipt");

  assert_eq!(metadata.purpose().as_str(), "auv.test.primary_image");
  assert_eq!(metadata.content_type().as_str(), "image/webp");
  assert_eq!(metadata.file_extension(), Some("webp"));
}

#[test]
fn retina_captures_are_recorded_at_logical_resolution() {
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().expect("dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));

  root.in_scope(|| emit_capture("auv.test.retina", &capture(RgbaImage::new(8, 6), 2.0)));
  futures_executor::block_on(dispatch.flush()).expect("flush");
  let records = store.records();
  let metadata = records
    .iter()
    .find_map(|record| match record {
      TraceRecord::Artifact { metadata, .. } => Some(metadata),
      _ => None,
    })
    .expect("image artifact");
  let decoded = image::load_from_memory(&store.artifact(metadata.uri()).expect("image body")).expect("decode").into_rgba8();

  assert_eq!(decoded.dimensions(), (4, 3), "evidence matches logical bounds");
  assert_eq!(metadata.attributes().get("image.source_width"), Some(&auv_tracing::AttributeValue::integer(8)));
}

#[test]
fn detached_artifact_failure_does_not_change_primary_value() {
  let store = Arc::new(RejectArtifactStore::new());
  let dispatch = configure().tracing_store(store).build().expect("dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let image = RgbaImage::new(1, 1);
  let value = root.in_scope(|| {
    emit_capture("auv.test.rejected", &capture(image, 1.0));
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
    emit_capture("auv.test.direct_metadata", &capture(image, 1.0));
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
