use std::io::Write;

use auv_tracing::{ArtifactMetadata, ArtifactPurpose, Attributes, ByteLength, ContentType, EventPayload, NewArtifact, Sha256Digest};
use futures_util::io::Cursor as AsyncCursor;
use image::{ExtendedColorType, ImageEncoder, RgbaImage, codecs::png::PngEncoder};
use sha2::{Digest, Sha256};

pub(crate) type OwnedArtifact = NewArtifact<AsyncCursor<Vec<u8>>>;

#[derive(serde::Serialize)]
struct ArtifactPreparationFailed {
  purpose: String,
  error: String,
}

impl EventPayload for ArtifactPreparationFailed {
  const NAME: &'static str = "auv.invoke.artifact_preparation_failed";
  const VERSION: u32 = 1;
}

pub(crate) fn emit_png(purpose: &str, image: &RgbaImage) {
  if emission_enabled() {
    emit_prepared(purpose, png_artifact(purpose, image, Attributes::empty()));
  }
}

pub(crate) async fn emit_bytes_with_receipt(purpose: &str, content_type: &str, body: Vec<u8>) -> Option<ArtifactMetadata> {
  if !emission_enabled() {
    return None;
  }
  let artifact = match bytes_artifact(purpose, content_type, body, Attributes::empty()) {
    Ok(artifact) => artifact,
    Err(error) => {
      auv_tracing::emit_event!(ArtifactPreparationFailed {
        purpose: purpose.to_string(),
        error,
      });
      return None;
    }
  };
  auv_tracing::emit_artifact!(artifact).await.ok().flatten()
}

pub(crate) fn emit_prepared<R>(purpose: &str, artifact: Result<NewArtifact<R>, String>)
where
  R: futures_util::io::AsyncRead + Unpin + Send + 'static,
{
  if !emission_enabled() {
    return;
  }
  match artifact {
    Ok(artifact) => drop(auv_tracing::emit_artifact!(artifact)),
    Err(error) => auv_tracing::emit_event!(ArtifactPreparationFailed {
      purpose: purpose.to_string(),
      error,
    }),
  }
}

pub(crate) fn emission_enabled() -> bool {
  auv_tracing::Context::current().can_publish_artifacts()
}

pub(crate) fn png_artifact(purpose: &str, image: &RgbaImage, attributes: Attributes) -> Result<OwnedArtifact, String> {
  let body = encode_png_exact(purpose, image)?;
  bytes_artifact(purpose, "image/png", body, attributes)
}

fn bytes_artifact(purpose: &str, content_type: &str, body: Vec<u8>, attributes: Attributes) -> Result<OwnedArtifact, String> {
  NewArtifact::from_bytes(parse_purpose(purpose)?, parse_content_type(purpose, content_type)?, attributes, body)
    .map_err(|error| format!("invalid {purpose} artifact bytes: {error}"))
}

fn parse_purpose(purpose: &str) -> Result<ArtifactPurpose, String> {
  ArtifactPurpose::parse(purpose).map_err(|error| format!("invalid {purpose} artifact purpose: {error}"))
}

fn parse_content_type(purpose: &str, content_type: &str) -> Result<ContentType, String> {
  ContentType::parse(content_type).map_err(|error| format!("invalid {purpose} artifact content type: {error}"))
}

fn bounded_length(purpose: &str, length: usize) -> Result<ByteLength, String> {
  let length = u64::try_from(length).map_err(|_| format!("{purpose} artifact length does not fit u64"))?;
  ByteLength::new(length).map_err(|error| format!("invalid {purpose} artifact length: {error}"))
}

fn encode_png_exact(purpose: &str, image: &RgbaImage) -> Result<Vec<u8>, String> {
  // RunStore admission needs the encoded length and digest up front. Measure
  // without retaining bytes, then encode once into that fixed allocation.
  bounded_length(purpose, image.as_raw().len())?;
  let mut measurement = ArtifactLengthMeasurement::new(purpose);
  PngEncoder::new(&mut measurement)
    .write_image(image.as_raw(), image.width(), image.height(), ExtendedColorType::Rgba8)
    .map_err(|error| format!("failed to measure encoded {purpose} artifact: {error}"))?;
  let (measured_length, measured_digest) = measurement.finish();
  let measured_length = usize::try_from(measured_length).map_err(|_| format!("{purpose} artifact length does not fit usize"))?;
  let mut body = ExactArtifactBuffer::try_new(purpose, measured_length)?;
  PngEncoder::new(&mut body)
    .write_image(image.as_raw(), image.width(), image.height(), ExtendedColorType::Rgba8)
    .map_err(|error| format!("failed to encode {purpose} artifact: {error}"))?;
  body.finish(measured_digest).ok_or_else(|| {
    format!("failed to encode {purpose} artifact deterministically: encoded bytes changed between measurement and construction")
  })
}

struct ArtifactLengthMeasurement<'a> {
  purpose: &'a str,
  byte_length: u64,
  hasher: Sha256,
}

impl<'a> ArtifactLengthMeasurement<'a> {
  fn new(purpose: &'a str) -> Self {
    Self {
      purpose,
      byte_length: 0,
      hasher: Sha256::new(),
    }
  }

  fn finish(self) -> (u64, Sha256Digest) {
    (self.byte_length, Sha256Digest::new(self.hasher.finalize().into()))
  }
}

impl Write for ArtifactLengthMeasurement<'_> {
  fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
    let buffer_length = u64::try_from(buffer.len()).map_err(std::io::Error::other)?;
    let actual = self
      .byte_length
      .checked_add(buffer_length)
      .ok_or_else(|| std::io::Error::other(format!("{} artifact length overflow", self.purpose)))?;
    ByteLength::new(actual).map_err(|error| std::io::Error::other(error.to_string()))?;
    self.hasher.update(buffer);
    self.byte_length = actual;
    Ok(buffer.len())
  }

  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}

struct ExactArtifactBuffer {
  bytes: Vec<u8>,
  measured_length: usize,
  actual_length: usize,
  hasher: Sha256,
}

impl ExactArtifactBuffer {
  fn try_new(purpose: &str, measured_length: usize) -> Result<Self, String> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(measured_length).map_err(|error| format!("failed to allocate {purpose} artifact bytes: {error}"))?;
    Ok(Self {
      bytes,
      measured_length,
      actual_length: 0,
      hasher: Sha256::new(),
    })
  }

  fn finish(self, measured_digest: Sha256Digest) -> Option<Vec<u8>> {
    if self.actual_length != self.measured_length || Sha256Digest::new(self.hasher.finalize().into()) != measured_digest {
      return None;
    }
    Some(self.bytes)
  }
}

impl Write for ExactArtifactBuffer {
  fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
    self.actual_length =
      self.actual_length.checked_add(buffer.len()).ok_or_else(|| std::io::Error::other("artifact buffer length overflow"))?;
    self.hasher.update(buffer);
    let remaining = self.measured_length - self.bytes.len();
    self.bytes.extend_from_slice(&buffer[..buffer.len().min(remaining)]);
    Ok(buffer.len())
  }

  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use super::*;
  use auv_tracing::{
    ArtifactBody, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, CommitError, CommitResult, Context, ErrorCode,
    IdempotencyKey, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision, RunStore,
    RunSubscription, StoreArtifactRequest, configure, dispatcher,
  };
  use futures_util::StreamExt;

  #[test]
  fn png_artifact_stream_decodes_to_the_exact_source_pixels() {
    let image = RgbaImage::from_fn(2, 3, |x, y| image::Rgba([x as u8, y as u8, (x + y) as u8, 255]));
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));

    let artifact = png_artifact("auv.test.png", &image, Attributes::empty()).expect("artifact");
    let metadata = futures_executor::block_on(root.in_scope(|| auv_tracing::emit_artifact!(artifact)))
      .expect("publication")
      .expect("enabled publication");
    futures_executor::block_on(dispatch.flush()).expect("flush");
    let mut reader = futures_executor::block_on(store.open_artifact(metadata.uri().clone())).expect("open PNG artifact");
    let mut encoded = Vec::new();
    futures_executor::block_on(async {
      while let Some(chunk) = reader.next().await {
        encoded.extend_from_slice(&chunk.expect("PNG chunk"));
      }
    });
    let decoded = image::load_from_memory_with_format(&encoded, image::ImageFormat::Png).expect("decode PNG").into_rgba8();

    assert_eq!(metadata.byte_length().get(), encoded.len() as u64);
    assert_eq!(metadata.sha256(), Sha256Digest::new(Sha256::digest(&encoded).into()));
    assert_eq!(decoded, image);
  }

  #[test]
  fn png_encoding_preserves_the_measured_payload() {
    let image = RgbaImage::from_fn(257, 257, |x, y| image::Rgba([x as u8, y as u8, (x ^ y) as u8, 255]));

    let body = encode_png_exact("auv.test.png", &image).expect("encode PNG");

    assert_eq!(image::load_from_memory_with_format(&body, image::ImageFormat::Png).expect("decode PNG").into_rgba8(), image);
  }

  #[test]
  fn exact_artifact_buffer_accepts_the_measured_payload() {
    let expected = b"measured payload";
    let measured_digest = Sha256Digest::new(Sha256::digest(expected).into());
    let mut body = ExactArtifactBuffer::try_new("auv.test.measured", expected.len()).expect("bounded buffer");

    body.write_all(expected).expect("write measured payload");

    assert_eq!(body.finish(measured_digest), Some(expected.to_vec()));
  }

  #[test]
  fn exact_artifact_buffer_rejects_writes_beyond_the_measured_length() {
    let measured_length = 3;
    let written = b"four";
    let written_digest = Sha256Digest::new(Sha256::digest(written).into());
    let mut body = ExactArtifactBuffer::try_new("auv.test.overlong", measured_length).expect("bounded buffer");

    body.write_all(written).expect("bounded write");

    assert_eq!(body.bytes, written[..measured_length]);
    assert!(body.finish(written_digest).is_none());
  }

  #[test]
  fn detached_artifact_failure_does_not_change_primary_value() {
    let store = Arc::new(RejectArtifactStore::new());
    let dispatch = configure().run_store(store).build().expect("dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let image = RgbaImage::new(1, 1);
    let value = root.in_scope(|| {
      emit_png("auv.test.rejected", &image);
      42
    });

    assert_eq!(value, 42);
    futures_executor::block_on(dispatch.flush()).expect_err("detached write rejection must reach the dispatch reporter");
  }

  #[test]
  fn detached_artifact_publication_is_read_from_the_run_store() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let image = RgbaImage::new(1, 1);
    root.in_scope(|| {
      emit_png("auv.test.direct_metadata", &image);
    });

    futures_executor::block_on(dispatch.flush()).expect("detached publication must flush");
    let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).expect("snapshot read").expect("run snapshot");
    let artifacts = snapshot.artifacts().values().collect::<Vec<_>>();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].metadata().purpose().as_str(), "auv.test.direct_metadata");
  }

  struct RejectArtifactStore {
    inner: MemoryRunStore,
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

    fn write_artifact(
      &self,
      _request: StoreArtifactRequest,
      _body: ArtifactBody,
    ) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
      Box::pin(async { Err(ArtifactWriteError::Rejected(ErrorCode::parse("auv.test.artifact_rejected").unwrap())) })
    }

    fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
      self.inner.lookup_commit(run_id, key)
    }

    fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<auv_tracing::RunSnapshot>, ReadError>> {
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
}
