use std::future::Future;
use std::pin::Pin;

use futures_io::AsyncRead;
use futures_util::AsyncReadExt;
use sha2::{Digest, Sha256};

use crate::{
  ArtifactId, ArtifactMetadata, ArtifactPurpose, ArtifactUri, Attributes, ByteLength, ContentType, RunId, Sha256Digest, SpanId, TraceRecord,
};

#[cfg(feature = "file-store")]
mod file;
#[cfg(feature = "memory-store")]
mod memory;

#[cfg(feature = "file-store")]
pub use file::FileTracingStore;
#[cfg(feature = "memory-store")]
pub use memory::MemoryTracingStore;

/// A boxed asynchronous operation returned by an object-safe tracing port.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A one-shot artifact body supplied to a tracing store.
pub type ArtifactBody = Pin<Box<dyn AsyncRead + Send>>;

/// Full metadata required to write an artifact body.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactRequest {
  run_id: RunId,
  span_id: Option<SpanId>,
  artifact_id: ArtifactId,
  purpose: ArtifactPurpose,
  content_type: ContentType,
  expected_byte_length: ByteLength,
  expected_sha256: Sha256Digest,
  attributes: Attributes,
}

impl ArtifactRequest {
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    run_id: RunId,
    span_id: Option<SpanId>,
    artifact_id: ArtifactId,
    purpose: ArtifactPurpose,
    content_type: ContentType,
    expected_byte_length: ByteLength,
    expected_sha256: Sha256Digest,
    attributes: Attributes,
  ) -> Self {
    Self {
      run_id,
      span_id,
      artifact_id,
      purpose,
      content_type,
      expected_byte_length,
      expected_sha256,
      attributes,
    }
  }

  pub fn run_id(&self) -> RunId {
    self.run_id
  }
  pub fn span_id(&self) -> Option<SpanId> {
    self.span_id
  }
  pub fn artifact_id(&self) -> ArtifactId {
    self.artifact_id
  }
  pub fn purpose(&self) -> &ArtifactPurpose {
    &self.purpose
  }
  pub fn content_type(&self) -> &ContentType {
    &self.content_type
  }
  pub fn expected_byte_length(&self) -> ByteLength {
    self.expected_byte_length
  }
  pub fn expected_sha256(&self) -> Sha256Digest {
    self.expected_sha256
  }
  pub fn attributes(&self) -> &Attributes {
    &self.attributes
  }

  pub fn metadata(&self) -> ArtifactMetadata {
    ArtifactMetadata::new(
      ArtifactUri::from_ids(self.run_id, self.artifact_id),
      self.purpose.clone(),
      self.content_type.clone(),
      self.expected_byte_length,
      self.expected_sha256,
      self.attributes.clone(),
    )
  }
}

/// A write-only destination for full-fidelity tracing records and artifacts.
///
/// This port deliberately has no lookup, snapshot, cursor, subscription, or
/// recovery API. Inspection is a separate read-side responsibility.
// TODO(auv-inspector): read/index APIs stay omitted until the owner approves a
// separate inspector contract over persisted trace data.
pub trait TracingStore: Send + Sync {
  /// Appends one record.
  fn write(&self, record: TraceRecord) -> BoxFuture<'_, Result<(), StoreError>>;

  /// Writes one artifact body and returns its stable metadata.
  fn write_artifact(&self, request: ArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<ArtifactMetadata, StoreError>>;

  /// Flushes store-owned buffering after preceding writes.
  fn flush(&self) -> BoxFuture<'_, Result<(), StoreError>>;
}

/// A stable observational failure from a tracing store.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("tracing store failed: {code}")]
pub struct StoreError {
  code: crate::ErrorCode,
}

impl StoreError {
  pub fn new(code: crate::ErrorCode) -> Self {
    Self { code }
  }
  pub fn code(&self) -> &crate::ErrorCode {
    &self.code
  }
}

pub(crate) fn store_error(code: &'static str) -> StoreError {
  StoreError::new(crate::ErrorCode::parse(code).expect("static store error code is valid"))
}

pub(crate) async fn read_artifact_body(request: &ArtifactRequest, body: ArtifactBody) -> Result<Vec<u8>, StoreError> {
  let mut bytes = Vec::new();
  body
    .take(request.expected_byte_length().get().saturating_add(1))
    .read_to_end(&mut bytes)
    .await
    .map_err(|_| store_error("auv.tracing.store.artifact_read_failed"))?;
  if bytes.len() as u64 != request.expected_byte_length().get()
    || crate::Sha256Digest::new(Sha256::digest(&bytes).into()) != request.expected_sha256()
  {
    return Err(store_error("auv.tracing.store.artifact_integrity_mismatch"));
  }
  Ok(bytes)
}
