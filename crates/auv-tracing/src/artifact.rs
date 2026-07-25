use std::fmt;
use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context as TaskContext, Poll};

use futures_channel::oneshot;
use futures_io::AsyncRead;
use futures_util::io::Cursor;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
  ArtifactBody, ArtifactId, ArtifactPurpose, Attributes, ByteLength, ContentType, RunId, Sha256Digest, StoreError, ValidationError,
};

/// One caller-owned artifact body and its validated metadata.
pub struct NewArtifact<R> {
  artifact_id: ArtifactId,
  purpose: ArtifactPurpose,
  content_type: ContentType,
  byte_length: ByteLength,
  sha256: Sha256Digest,
  attributes: Attributes,
  body: R,
}

impl<R> NewArtifact<R> {
  pub fn new(
    purpose: ArtifactPurpose,
    content_type: ContentType,
    byte_length: ByteLength,
    sha256: Sha256Digest,
    attributes: Attributes,
    body: R,
  ) -> Self {
    Self {
      artifact_id: ArtifactId::new(),
      purpose,
      content_type,
      byte_length,
      sha256,
      attributes,
      body,
    }
  }

  pub(crate) fn detach(self) -> DetachedArtifact
  where
    R: AsyncRead + Unpin + Send + 'static,
  {
    DetachedArtifact {
      artifact_id: self.artifact_id,
      purpose: self.purpose,
      content_type: self.content_type,
      byte_length: self.byte_length,
      sha256: self.sha256,
      attributes: self.attributes,
      body: Box::pin(self.body),
    }
  }
}

impl NewArtifact<Cursor<Vec<u8>>> {
  pub fn from_bytes(
    purpose: ArtifactPurpose,
    content_type: ContentType,
    attributes: Attributes,
    body: Vec<u8>,
  ) -> Result<Self, ValidationError> {
    let length = u64::try_from(body.len()).map_err(|_| ValidationError::new("artifact byte length is out of range"))?;
    Ok(Self::new(
      purpose,
      content_type,
      ByteLength::new(length)?,
      Sha256Digest::new(Sha256::digest(&body).into()),
      attributes,
      Cursor::new(body),
    ))
  }

  pub fn from_json<T: Serialize>(
    purpose: ArtifactPurpose,
    attributes: Attributes,
    byte_limit: ByteLength,
    value: &T,
  ) -> Result<Self, JsonArtifactError> {
    let mut buffer = BoundedBuffer::new(byte_limit);
    serde_json::to_writer(&mut buffer, value).map_err(|error| buffer.failure.take().unwrap_or(JsonArtifactError::Serialize(error)))?;
    Self::from_bytes(purpose, ContentType::parse("application/json").expect("static content type"), attributes, buffer.bytes)
      .map_err(JsonArtifactError::Validation)
  }
}

pub(crate) struct DetachedArtifact {
  pub artifact_id: ArtifactId,
  pub purpose: ArtifactPurpose,
  pub content_type: ContentType,
  pub byte_length: ByteLength,
  pub sha256: Sha256Digest,
  pub attributes: Attributes,
  pub body: ArtifactBody,
}

#[derive(Debug, thiserror::Error)]
pub enum JsonArtifactError {
  #[error("failed to serialize JSON artifact: {0}")]
  Serialize(serde_json::Error),
  #[error("JSON artifact exceeds the {limit}-byte limit")]
  PayloadTooLarge { limit: ByteLength },
  #[error("invalid JSON artifact metadata: {0}")]
  Validation(ValidationError),
}

struct BoundedBuffer {
  limit: ByteLength,
  bytes: Vec<u8>,
  failure: Option<JsonArtifactError>,
}
impl BoundedBuffer {
  fn new(limit: ByteLength) -> Self {
    Self {
      limit,
      bytes: Vec::new(),
      failure: None,
    }
  }
}
impl Write for BoundedBuffer {
  fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
    if self.bytes.len().saturating_add(bytes.len()) as u64 > self.limit.get() {
      self.failure = Some(JsonArtifactError::PayloadTooLarge { limit: self.limit });
      return Err(std::io::Error::other("JSON artifact is too large"));
    }
    self.bytes.extend_from_slice(bytes);
    Ok(bytes.len())
  }
  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}

/// Durable metadata for bytes written by a tracing store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactMetadata {
  uri: ArtifactUri,
  purpose: ArtifactPurpose,
  content_type: ContentType,
  byte_length: ByteLength,
  sha256: Sha256Digest,
  attributes: Attributes,
}

impl ArtifactMetadata {
  pub fn new(
    uri: ArtifactUri,
    purpose: ArtifactPurpose,
    content_type: ContentType,
    byte_length: ByteLength,
    sha256: Sha256Digest,
    attributes: Attributes,
  ) -> Self {
    Self {
      uri,
      purpose,
      content_type,
      byte_length,
      sha256,
      attributes,
    }
  }
  pub fn uri(&self) -> &ArtifactUri {
    &self.uri
  }
  pub fn purpose(&self) -> &ArtifactPurpose {
    &self.purpose
  }
  pub fn content_type(&self) -> &ContentType {
    &self.content_type
  }
  pub fn byte_length(&self) -> ByteLength {
    self.byte_length
  }
  pub fn sha256(&self) -> Sha256Digest {
    self.sha256
  }
  pub fn attributes(&self) -> &Attributes {
    &self.attributes
  }
}

/// Awaitable receipt for an admitted artifact write.
pub struct ArtifactEmission {
  receiver: Option<oneshot::Receiver<Result<ArtifactMetadata, StoreError>>>,
}
impl ArtifactEmission {
  pub(crate) fn disabled() -> Self {
    Self { receiver: None }
  }
  pub(crate) fn pending() -> (oneshot::Sender<Result<ArtifactMetadata, StoreError>>, Self) {
    let (sender, receiver) = oneshot::channel();
    (
      sender,
      Self {
        receiver: Some(receiver),
      },
    )
  }
}
impl Future for ArtifactEmission {
  type Output = Result<Option<ArtifactMetadata>, StoreError>;
  fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
    let Some(receiver) = &mut self.receiver else {
      return Poll::Ready(Ok(None));
    };
    match Pin::new(receiver).poll(cx) {
      Poll::Ready(Ok(result)) => {
        self.receiver = None;
        Poll::Ready(result.map(Some))
      }
      Poll::Ready(Err(_)) => {
        self.receiver = None;
        Poll::Ready(Err(crate::store::store_error("auv.tracing.artifact_receipt_closed")))
      }
      Poll::Pending => Poll::Pending,
    }
  }
}

pub fn emit_json_artifact<T: Serialize>(
  purpose: ArtifactPurpose,
  attributes: Attributes,
  byte_limit: ByteLength,
  value: &T,
) -> Result<ArtifactEmission, JsonArtifactError> {
  if !crate::Context::current().can_publish_artifacts() {
    return Ok(ArtifactEmission::disabled());
  }
  Ok(emit_artifact(NewArtifact::from_json(purpose, attributes, byte_limit, value)?))
}

pub fn emit_bytes_artifact(
  purpose: ArtifactPurpose,
  content_type: ContentType,
  attributes: Attributes,
  body: Vec<u8>,
) -> Result<ArtifactEmission, ValidationError> {
  if !crate::Context::current().can_publish_artifacts() {
    return Ok(ArtifactEmission::disabled());
  }
  Ok(emit_artifact(NewArtifact::from_bytes(purpose, content_type, attributes, body)?))
}

pub fn emit_artifact<R: AsyncRead + Unpin + Send + 'static>(artifact: NewArtifact<R>) -> ArtifactEmission {
  let context = crate::Context::current();
  if !context.can_publish_artifacts() {
    return ArtifactEmission::disabled();
  }
  let (Some(dispatch), Some(run_id)) = (context.dispatch().cloned(), context.run_id().copied()) else {
    return ArtifactEmission::disabled();
  };
  dispatch.submit_artifact(run_id, context.span_id().copied(), artifact.detach())
}

/// Transport-independent identity for one run artifact.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactUri(Url);
impl ArtifactUri {
  pub fn from_ids(run_id: RunId, artifact_id: ArtifactId) -> Self {
    format!("auv://runs/{run_id}/artifacts/{artifact_id}").parse().expect("IDs form a URI")
  }
  pub fn run_id(&self) -> RunId {
    self.ids().0
  }
  pub fn artifact_id(&self) -> ArtifactId {
    self.ids().1
  }
  fn ids(&self) -> (RunId, ArtifactId) {
    let values: Vec<_> = self.0.path_segments().unwrap().collect();
    (values[0].parse().unwrap(), values[2].parse().unwrap())
  }
}
impl fmt::Display for ArtifactUri {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(self.0.as_str())
  }
}
impl FromStr for ArtifactUri {
  type Err = ValidationError;
  fn from_str(value: &str) -> Result<Self, Self::Err> {
    let url = Url::parse(value).map_err(|_| ValidationError::new("artifact URI is invalid"))?;
    let segments: Vec<_> = url.path_segments().ok_or_else(|| ValidationError::new("artifact URI is invalid"))?.collect();
    if url.scheme() != "auv"
      || url.host_str() != Some("runs")
      || segments.len() != 3
      || segments[1] != "artifacts"
      || url.query().is_some()
      || url.fragment().is_some()
    {
      return Err(ValidationError::new("artifact URI is not canonical"));
    }
    let run: RunId = segments[0].parse().map_err(|_| ValidationError::new("artifact URI run ID is invalid"))?;
    let artifact: ArtifactId = segments[2].parse().map_err(|_| ValidationError::new("artifact URI artifact ID is invalid"))?;
    if value != format!("auv://runs/{run}/artifacts/{artifact}") {
      return Err(ValidationError::new("artifact URI is not canonical"));
    }
    Ok(Self(url))
  }
}
impl Serialize for ArtifactUri {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(self)
  }
}
impl<'de> Deserialize<'de> for ArtifactUri {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    String::deserialize(deserializer)?.parse().map_err(de::Error::custom)
  }
}
