use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use auv_tracing::{ArtifactPurpose, Attributes, ByteLength, ContentType, NewArtifact, Sha256Digest};
use futures_util::io::{AllowStdIo, AsyncRead, Cursor as AsyncCursor};
use image::{ExtendedColorType, ImageEncoder, RgbaImage, codecs::png::PngEncoder};
use sha2::{Digest, Sha256};

pub(crate) type OwnedArtifact = NewArtifact<OwnedArtifactReader>;

pub(crate) enum OwnedArtifactReader {
  Memory(AsyncCursor<Vec<u8>>),
  File(AllowStdIo<File>),
}

impl AsyncRead for OwnedArtifactReader {
  fn poll_read(mut self: Pin<&mut Self>, context: &mut Context<'_>, buffer: &mut [u8]) -> Poll<std::io::Result<usize>> {
    match &mut *self {
      Self::Memory(reader) => Pin::new(reader).poll_read(context, buffer),
      Self::File(reader) => Pin::new(reader).poll_read(context, buffer),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ArtifactInstrumentationFailure {
  pub purpose: String,
  pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArtifactInstrumentationReceipt {
  failures: Vec<ArtifactInstrumentationFailure>,
}

impl ArtifactInstrumentationReceipt {
  pub fn failures(&self) -> &[ArtifactInstrumentationFailure] {
    &self.failures
  }

  pub fn into_failures(self) -> Vec<ArtifactInstrumentationFailure> {
    self.failures
  }

  pub async fn publish_json<T: serde::Serialize>(&mut self, purpose: &str, value: &T) {
    if emission_enabled() {
      self.publish(purpose, json_artifact(purpose, value, Attributes::empty())).await;
    }
  }

  pub(crate) async fn publish_png(&mut self, purpose: &str, image: &RgbaImage) {
    if emission_enabled() {
      self.publish(purpose, png_artifact(purpose, image, Attributes::empty())).await;
    }
  }

  pub(crate) async fn publish_file(&mut self, purpose: &str, content_type: &str, path: &Path) {
    if emission_enabled() {
      self.publish(purpose, file_artifact(purpose, content_type, path, Attributes::empty())).await;
    }
  }

  async fn publish(&mut self, purpose: &str, artifact: Result<OwnedArtifact, String>) {
    let result = match artifact {
      Ok(artifact) => auv_tracing::emit_artifact!(artifact).await.map(|_| ()),
      Err(error) => {
        self.failures.push(ArtifactInstrumentationFailure {
          purpose: purpose.to_string(),
          message: error,
        });
        return;
      }
    };
    if let Err(error) = result {
      self.failures.push(ArtifactInstrumentationFailure {
        purpose: purpose.to_string(),
        message: format!("failed to publish {purpose} artifact: {error}"),
      });
    }
  }
}

#[derive(Clone, Debug)]
pub struct ArtifactPublication<T> {
  value: T,
  instrumentation: ArtifactInstrumentationReceipt,
}

impl<T> ArtifactPublication<T> {
  pub fn new(value: T, instrumentation: ArtifactInstrumentationReceipt) -> Self {
    Self {
      value,
      instrumentation,
    }
  }

  pub fn into_parts(self) -> (T, ArtifactInstrumentationReceipt) {
    (self.value, self.instrumentation)
  }

  pub fn value(&self) -> &T {
    &self.value
  }
}

pub(crate) fn emission_enabled() -> bool {
  auv_tracing::Context::current().authority_id().is_some()
}

pub(crate) fn json_artifact<T: serde::Serialize>(purpose: &str, value: &T, attributes: Attributes) -> Result<OwnedArtifact, String> {
  let mut body = BoundedArtifactBuffer::default();
  serde_json::to_writer_pretty(&mut body, value).map_err(|error| format!("failed to serialize {purpose} artifact: {error}"))?;
  bytes_artifact(purpose, "application/json", body.into_inner(), attributes)
}

pub(crate) fn png_artifact(purpose: &str, image: &RgbaImage, attributes: Attributes) -> Result<OwnedArtifact, String> {
  // PNG encoding is buffered because RunStore admission requires the exact
  // digest and byte length up front. Validate the raw source and every encoded
  // write against the canonical ByteLength policy before growing the buffer.
  bounded_length(purpose, image.as_raw().len())?;
  let mut body = BoundedArtifactBuffer::default();
  PngEncoder::new(&mut body)
    .write_image(image.as_raw(), image.width(), image.height(), ExtendedColorType::Rgba8)
    .map_err(|error| format!("failed to encode {purpose} artifact: {error}"))?;
  bytes_artifact(purpose, "image/png", body.into_inner(), attributes)
}

pub(crate) fn file_artifact(purpose: &str, content_type: &str, path: &Path, attributes: Attributes) -> Result<OwnedArtifact, String> {
  let mut file = File::open(path).map_err(|error| format!("failed to open {purpose} artifact bytes: {error}"))?;
  let mut hasher = Sha256::new();
  let mut byte_length = 0_u64;
  let mut buffer = [0_u8; 64 * 1024];
  loop {
    let read = file.read(&mut buffer).map_err(|error| format!("failed to read {purpose} artifact bytes: {error}"))?;
    if read == 0 {
      break;
    }
    let read = u64::try_from(read).map_err(|_| format!("{purpose} artifact read length does not fit u64"))?;
    byte_length = byte_length.checked_add(read).ok_or_else(|| format!("{purpose} artifact length overflow"))?;
    ByteLength::new(byte_length).map_err(|error| format!("invalid {purpose} artifact length: {error}"))?;
    hasher.update(&buffer[..usize::try_from(read).expect("read length originated as usize")]);
  }
  file.rewind().map_err(|error| format!("failed to rewind {purpose} artifact bytes: {error}"))?;

  Ok(NewArtifact::new(
    parse_purpose(purpose)?,
    parse_content_type(purpose, content_type)?,
    ByteLength::new(byte_length).map_err(|error| format!("invalid {purpose} artifact length: {error}"))?,
    Sha256Digest::new(hasher.finalize().into()),
    attributes,
    OwnedArtifactReader::File(AllowStdIo::new(file)),
  ))
}

fn bytes_artifact(purpose: &str, content_type: &str, body: Vec<u8>, attributes: Attributes) -> Result<OwnedArtifact, String> {
  let byte_length = bounded_length(purpose, body.len())?;
  Ok(NewArtifact::new(
    parse_purpose(purpose)?,
    parse_content_type(purpose, content_type)?,
    byte_length,
    Sha256Digest::new(Sha256::digest(&body).into()),
    attributes,
    OwnedArtifactReader::Memory(AsyncCursor::new(body)),
  ))
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

#[derive(Default)]
struct BoundedArtifactBuffer {
  bytes: Vec<u8>,
}

impl BoundedArtifactBuffer {
  fn into_inner(self) -> Vec<u8> {
    self.bytes
  }
}

impl Write for BoundedArtifactBuffer {
  fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
    let length = self.bytes.len().checked_add(buffer.len()).ok_or_else(|| std::io::Error::other("artifact buffer length overflow"))?;
    ByteLength::new(u64::try_from(length).map_err(std::io::Error::other)?).map_err(|error| std::io::Error::other(error.to_string()))?;
    self.bytes.try_reserve(buffer.len()).map_err(std::io::Error::other)?;
    self.bytes.extend_from_slice(buffer);
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
  fn file_artifact_streams_owned_reader_with_exact_digest_and_length() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("artifact.json");
    std::fs::write(&path, b"{\"value\":42}").expect("fixture");

    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let artifact = file_artifact("auv.test.streamed", "application/json", &path, Attributes::empty()).expect("artifact");
    let metadata = futures_executor::block_on(root.in_scope(|| auv_tracing::emit_artifact!(artifact)))
      .expect("publication")
      .expect("enabled publication");
    futures_executor::block_on(dispatch.flush()).expect("flush");

    assert_eq!(metadata.byte_length().get(), 12);
    assert_eq!(metadata.sha256(), Sha256Digest::new(Sha256::digest(b"{\"value\":42}").into()));
    let mut reader = futures_executor::block_on(store.open_artifact(metadata.uri().clone())).expect("open artifact");
    let mut body = Vec::new();
    futures_executor::block_on(async {
      while let Some(chunk) = reader.next().await {
        body.extend_from_slice(&chunk.expect("artifact chunk"));
      }
    });
    assert_eq!(body, b"{\"value\":42}");
  }

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
  fn artifact_write_failure_is_returned_without_changing_primary_value() {
    let store = Arc::new(RejectArtifactStore::new());
    let dispatch = configure().run_store(store).build().expect("dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let future = root.in_scope(|| async {
      let mut instrumentation = ArtifactInstrumentationReceipt::default();
      instrumentation.publish_json("auv.test.rejected", &serde_json::json!({ "value": 42 })).await;
      ArtifactPublication::new(42, instrumentation)
    });

    let publication = futures_executor::block_on(root.instrument(future));
    let (value, instrumentation) = publication.into_parts();

    assert_eq!(value, 42);
    assert_eq!(instrumentation.failures().len(), 1);
    assert!(instrumentation.failures()[0].message.contains("artifact write rejected"));
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
