use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use auv_tracing::{ArtifactMetadata, ArtifactPurpose, Attributes, ByteLength, ContentType, ErrorCode, NewArtifact, Sha256Digest};
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArtifactInstrumentationReceipt {
  artifacts: Vec<ArtifactMetadata>,
  failures: Vec<ArtifactInstrumentationFailure>,
}

impl ArtifactInstrumentationReceipt {
  pub fn artifacts(&self) -> &[ArtifactMetadata] {
    &self.artifacts
  }

  pub fn failures(&self) -> &[ArtifactInstrumentationFailure] {
    &self.failures
  }

  pub fn into_parts(self) -> (Vec<ArtifactMetadata>, Vec<ArtifactInstrumentationFailure>) {
    (self.artifacts, self.failures)
  }

  pub async fn publish_json<T: serde::Serialize>(&mut self, purpose: &str, value: &T) {
    if emission_enabled() {
      self.publish(purpose, json_artifact(purpose, value, Attributes::empty())).await;
    }
  }

  /// Publishes pretty JSON only when its exact serialized length fits the
  /// caller's domain budget and the canonical whole-artifact limit.
  ///
  /// This evaluates `Serialize` twice to allocate the exact encoded length.
  /// Values must be deterministic, side-effect-free artifact structs so both
  /// evaluations produce identical bytes.
  pub async fn publish_json_bounded<T: serde::Serialize>(&mut self, purpose: &str, value: &T, max_bytes: u64, exceeded_code: &str) {
    if emission_enabled() {
      self.publish(purpose, json_artifact_bounded(purpose, value, Attributes::empty(), max_bytes, exceeded_code)).await;
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
      Ok(artifact) => auv_tracing::emit_artifact!(artifact).await,
      Err(error) => {
        self.failures.push(ArtifactInstrumentationFailure {
          purpose: purpose.to_string(),
          message: error,
        });
        return;
      }
    };
    match result {
      Ok(Some(metadata)) => self.artifacts.push(metadata),
      Ok(None) => {}
      Err(error) => {
        self.failures.push(ArtifactInstrumentationFailure {
          purpose: purpose.to_string(),
          message: format!("failed to publish {purpose} artifact: {error}"),
        });
      }
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
  auv_tracing::Context::current().can_publish_artifacts()
}

pub(crate) fn json_artifact<T: serde::Serialize>(purpose: &str, value: &T, attributes: Attributes) -> Result<OwnedArtifact, String> {
  let mut body = BoundedArtifactBuffer::default();
  serde_json::to_writer_pretty(&mut body, value).map_err(|error| format!("failed to serialize {purpose} artifact: {error}"))?;
  bytes_artifact(purpose, "application/json", body.into_inner(), attributes)
}

fn json_artifact_bounded<T: serde::Serialize>(
  purpose: &str,
  value: &T,
  attributes: Attributes,
  max_bytes: u64,
  exceeded_code: &str,
) -> Result<OwnedArtifact, String> {
  let max_bytes = ByteLength::new(max_bytes).map_err(|error| format!("invalid {purpose} JSON byte limit: {error}"))?.get();
  let exceeded_code = ErrorCode::parse(exceeded_code).map_err(|error| format!("invalid {purpose} JSON limit error code: {error}"))?;
  let body = serialize_json_exact(
    purpose,
    value,
    JsonArtifactLimit {
      max_bytes,
      exceeded_code: exceeded_code.as_str(),
    },
  )?;
  bytes_artifact(purpose, "application/json", body, attributes)
}

pub(crate) fn png_artifact(purpose: &str, image: &RgbaImage, attributes: Attributes) -> Result<OwnedArtifact, String> {
  let body = encode_png_exact(purpose, image)?;
  bytes_artifact(purpose, "image/png", body, attributes)
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

#[derive(Clone, Copy)]
struct JsonArtifactLimit<'a> {
  max_bytes: u64,
  exceeded_code: &'a str,
}

fn serialize_json_exact<T: serde::Serialize>(purpose: &str, value: &T, limit: JsonArtifactLimit<'_>) -> Result<Vec<u8>, String> {
  let mut measurement = ArtifactLengthMeasurement::new(purpose, Some(limit));
  serde_json::to_writer_pretty(&mut measurement, value).map_err(|error| format!("failed to serialize {purpose} artifact: {error}"))?;
  let (measured_length, measured_digest) = measurement.finish();
  let measured_length = usize::try_from(measured_length).map_err(|_| format!("{purpose} artifact length does not fit usize"))?;
  let mut body = ExactArtifactBuffer::try_new(purpose, measured_length)?;
  serde_json::to_writer_pretty(&mut body, value)
    .map_err(|error| format!("failed to serialize {purpose} artifact on second pass: {error}"))?;
  body.finish(measured_digest).ok_or_else(|| {
    format!("auv.invoke.artifact.nondeterministic_serialization: {purpose} JSON serialization changed between measurement and construction")
  })
}

fn encode_png_exact(purpose: &str, image: &RgbaImage) -> Result<Vec<u8>, String> {
  // RunStore admission needs the encoded length and digest up front. Measure
  // without retaining bytes, then encode once into that fixed allocation.
  bounded_length(purpose, image.as_raw().len())?;
  let mut measurement = ArtifactLengthMeasurement::new(purpose, None);
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
  limit: Option<JsonArtifactLimit<'a>>,
}

impl<'a> ArtifactLengthMeasurement<'a> {
  fn new(purpose: &'a str, limit: Option<JsonArtifactLimit<'a>>) -> Self {
    Self {
      purpose,
      byte_length: 0,
      hasher: Sha256::new(),
      limit,
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
    if let Some(limit) = self.limit
      && actual > limit.max_bytes
    {
      return Err(std::io::Error::other(format!(
        "{}: {} JSON is {actual} bytes, exceeding the {}-byte limit",
        limit.exceeded_code, self.purpose, limit.max_bytes
      )));
    }
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
  use std::cell::Cell;
  use std::sync::Arc;

  use super::*;
  use auv_tracing::{
    ArtifactBody, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, CommitError, CommitResult, Context, ErrorCode,
    IdempotencyKey, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision, RunStore,
    RunSubscription, StoreArtifactRequest, configure, dispatcher,
  };
  use futures_util::StreamExt;

  enum StatefulSerialization {
    SameLengthMutation,
    DifferentLengthMutation,
    SecondPassError,
  }

  struct StatefulSerializer {
    calls: Cell<usize>,
    behavior: StatefulSerialization,
  }

  impl StatefulSerializer {
    fn new(behavior: StatefulSerialization) -> Self {
      Self {
        calls: Cell::new(0),
        behavior,
      }
    }
  }

  impl serde::Serialize for StatefulSerializer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
      S: serde::Serializer,
    {
      let call = self.calls.get();
      self.calls.set(call + 1);
      match (&self.behavior, call) {
        (StatefulSerialization::SameLengthMutation, 0)
        | (StatefulSerialization::DifferentLengthMutation, 0)
        | (StatefulSerialization::SecondPassError, 0) => serializer.serialize_str("a"),
        (StatefulSerialization::SameLengthMutation, _) => serializer.serialize_str("b"),
        (StatefulSerialization::DifferentLengthMutation, _) => serializer.serialize_str("longer"),
        (StatefulSerialization::SecondPassError, _) => Err(serde::ser::Error::custom("stateful serializer failed on second pass")),
      }
    }
  }

  #[test]
  fn ordinary_json_artifact_serializes_stateful_values_once() {
    let value = StatefulSerializer::new(StatefulSerialization::SameLengthMutation);

    json_artifact("auv.test.one_pass", &value, Attributes::empty()).expect("ordinary JSON artifact");

    assert_eq!(value.calls.get(), 1);
  }

  #[test]
  fn bounded_json_rejects_same_length_serialization_mutation() {
    let value = StatefulSerializer::new(StatefulSerialization::SameLengthMutation);

    let error = match json_artifact_bounded("auv.test.deterministic", &value, Attributes::empty(), 1024, "auv.test.payload_too_large") {
      Ok(_) => panic!("same-length mutation must be rejected"),
      Err(error) => error,
    };

    assert_eq!(
      error,
      "auv.invoke.artifact.nondeterministic_serialization: auv.test.deterministic JSON serialization changed between measurement and construction"
    );
    assert_eq!(value.calls.get(), 2);
  }

  #[test]
  fn bounded_json_rejects_different_length_serialization_mutation() {
    let value = StatefulSerializer::new(StatefulSerialization::DifferentLengthMutation);

    let error = match json_artifact_bounded("auv.test.deterministic", &value, Attributes::empty(), 1024, "auv.test.payload_too_large") {
      Ok(_) => panic!("different-length mutation must be rejected"),
      Err(error) => error,
    };

    assert_eq!(
      error,
      "auv.invoke.artifact.nondeterministic_serialization: auv.test.deterministic JSON serialization changed between measurement and construction"
    );
    assert_eq!(value.calls.get(), 2);
  }

  #[test]
  fn bounded_json_preserves_second_pass_serialization_error() {
    let value = StatefulSerializer::new(StatefulSerialization::SecondPassError);

    let error = match json_artifact_bounded("auv.test.second_pass", &value, Attributes::empty(), 1024, "auv.test.payload_too_large") {
      Ok(_) => panic!("second-pass failure must be returned"),
      Err(error) => error,
    };

    assert!(error.contains("stateful serializer failed on second pass"), "{error}");
    assert_eq!(value.calls.get(), 2);
  }

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

  #[test]
  fn artifact_receipt_keeps_successful_publication_metadata() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).build().expect("dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let future = root.in_scope(|| async {
      let mut instrumentation = ArtifactInstrumentationReceipt::default();
      instrumentation.publish_json("auv.test.direct_metadata", &serde_json::json!({ "value": 42 })).await;
      instrumentation
    });

    let instrumentation = futures_executor::block_on(root.instrument(future));
    let (artifacts, failures) = instrumentation.into_parts();

    assert!(failures.is_empty());
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].uri().run_id(), run_id);
    assert_eq!(artifacts[0].purpose().as_str(), "auv.test.direct_metadata");
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
