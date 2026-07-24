use std::collections::TryReserveError;
use std::fmt;
use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context as TaskContext, Poll};

use futures_channel::oneshot;
use futures_io::AsyncRead;
use futures_util::StreamExt;
use futures_util::io::Cursor;
use serde::de;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
  ArtifactBody, ArtifactId, ArtifactMetadata, ArtifactPurpose, ArtifactReadError, ArtifactWriteError, Attributes, AuthorityId, ByteLength,
  ContentType, Dispatch, DispatchFailure, IdempotencyKey, ReadError, RunId, RunSnapshot, RunStore, Sha256Digest, ValidationError,
};

/// One validated, caller-owned artifact write.
pub struct NewArtifact<R> {
  artifact_id: ArtifactId,
  idempotency_key: IdempotencyKey,
  purpose: ArtifactPurpose,
  content_type: ContentType,
  expected_byte_length: ByteLength,
  expected_sha256: Sha256Digest,
  attributes: Attributes,
  body: R,
}

impl<R> NewArtifact<R> {
  /// Creates a one-shot artifact request with fresh publication identities.
  pub fn new(
    purpose: ArtifactPurpose,
    content_type: ContentType,
    expected_byte_length: ByteLength,
    expected_sha256: Sha256Digest,
    attributes: Attributes,
    body: R,
  ) -> Self {
    Self {
      artifact_id: ArtifactId::new(),
      idempotency_key: IdempotencyKey::new(),
      purpose,
      content_type,
      expected_byte_length,
      expected_sha256,
      attributes,
      body,
    }
  }

  pub(crate) fn into_detached(self) -> DetachedArtifact
  where
    R: AsyncRead + Unpin + Send + 'static,
  {
    DetachedArtifact {
      artifact_id: self.artifact_id,
      idempotency_key: self.idempotency_key,
      purpose: self.purpose,
      content_type: self.content_type,
      expected_byte_length: self.expected_byte_length,
      expected_sha256: self.expected_sha256,
      attributes: self.attributes,
      body: Box::pin(self.body),
    }
  }
}

impl NewArtifact<Cursor<Vec<u8>>> {
  /// Creates an artifact whose complete body is already owned in memory.
  ///
  /// Length and digest are derived here so producers cannot publish metadata
  /// that disagrees with the supplied bytes.
  pub fn from_bytes(
    purpose: ArtifactPurpose,
    content_type: ContentType,
    attributes: Attributes,
    body: Vec<u8>,
  ) -> Result<Self, ValidationError> {
    let byte_length =
      u64::try_from(body.len()).map_err(|_| ValidationError::new("artifact byte length does not fit the canonical integer range"))?;
    Ok(Self::new(
      purpose,
      content_type,
      ByteLength::new(byte_length)?,
      Sha256Digest::new(Sha256::digest(&body).into()),
      attributes,
      Cursor::new(body),
    ))
  }

  /// Serializes one typed value into a bounded JSON artifact.
  ///
  /// Domain validation remains the producer's responsibility. This constructor
  /// owns the shared encoding, allocation bound, content type, length, and
  /// digest rules.
  pub fn from_json<T>(purpose: ArtifactPurpose, attributes: Attributes, byte_limit: ByteLength, value: &T) -> Result<Self, JsonArtifactError>
  where
    T: Serialize,
  {
    let body = serialize_json_bounded(value, byte_limit)?;
    let byte_length = ByteLength::new(u64::try_from(body.len()).map_err(|_| JsonArtifactError::LengthOutOfRange {
      actual: body.len() as u128,
    })?)
    .expect("bounded JSON cannot exceed the canonical whole-artifact limit");
    Ok(Self::new(
      purpose,
      ContentType::parse("application/json").expect("static JSON content type is valid"),
      byte_length,
      Sha256Digest::new(Sha256::digest(&body).into()),
      attributes,
      Cursor::new(body),
    ))
  }
}

/// Failure to construct a bounded JSON artifact.
#[derive(Debug, thiserror::Error)]
pub enum JsonArtifactError {
  /// The value's serializer failed.
  #[error("failed to serialize JSON artifact: {0}")]
  Serialize(#[source] serde_json::Error),
  /// The serialized body crossed the caller-provided bound.
  #[error("JSON artifact is at least {actual} bytes, exceeding the {limit}-byte limit")]
  PayloadTooLarge { limit: ByteLength, actual: u64 },
  /// The encoded length cannot be represented by the canonical integer model.
  #[error("JSON artifact length {actual} is outside the canonical integer range")]
  LengthOutOfRange { actual: u128 },
  /// Memory allocation for the bounded body failed.
  #[error("failed to allocate JSON artifact body: {0}")]
  Allocation(#[source] TryReserveError),
}

fn serialize_json_bounded<T>(value: &T, limit: ByteLength) -> Result<Vec<u8>, JsonArtifactError>
where
  T: Serialize,
{
  let mut output = BoundedJsonBuffer::new(limit);
  let result = serde_json::to_writer(&mut output, value);
  if let Some(failure) = output.failure.take() {
    return Err(failure);
  }
  result.map_err(JsonArtifactError::Serialize)?;
  Ok(output.bytes)
}

struct BoundedJsonBuffer {
  limit: ByteLength,
  bytes: Vec<u8>,
  failure: Option<JsonArtifactError>,
}

impl BoundedJsonBuffer {
  fn new(limit: ByteLength) -> Self {
    Self {
      limit,
      bytes: Vec::new(),
      failure: None,
    }
  }

  fn fail(&mut self, failure: JsonArtifactError) -> std::io::Error {
    self.failure = Some(failure);
    std::io::Error::other("JSON artifact exceeded its bounded buffer")
  }
}

impl Write for BoundedJsonBuffer {
  fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
    let Some(next_length) = self.bytes.len().checked_add(buffer.len()) else {
      return Err(self.fail(JsonArtifactError::LengthOutOfRange { actual: u128::MAX }));
    };
    let next_length = match u64::try_from(next_length) {
      Ok(length) => length,
      Err(_) => {
        return Err(self.fail(JsonArtifactError::LengthOutOfRange {
          actual: next_length as u128,
        }));
      }
    };
    if next_length > self.limit.get() {
      return Err(self.fail(JsonArtifactError::PayloadTooLarge {
        limit: self.limit,
        actual: next_length,
      }));
    }
    if let Err(source) = self.bytes.try_reserve(buffer.len()) {
      return Err(self.fail(JsonArtifactError::Allocation(source)));
    }
    self.bytes.extend_from_slice(buffer);
    Ok(buffer.len())
  }

  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}

/// Reads one committed artifact body after validating its authority, owner,
/// metadata contract, consumer bound, length, and digest.
pub async fn read_artifact_bytes(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  expected_purpose: &ArtifactPurpose,
  expected_content_type: &ContentType,
  byte_limit: ByteLength,
) -> Result<Vec<u8>, ReadArtifactError> {
  let store_authority = store.authority_id();
  if snapshot.authority_id() != store_authority {
    return Err(ReadArtifactError::SnapshotAuthorityMismatch {
      snapshot_authority: snapshot.authority_id(),
      store_authority,
    });
  }
  if uri.run_id() != snapshot.run_id() {
    return Err(ReadArtifactError::WrongRun {
      snapshot_run_id: snapshot.run_id(),
      artifact_run_id: uri.run_id(),
    });
  }
  let metadata = snapshot.artifacts().get(uri).ok_or_else(|| ReadArtifactError::NotCommitted { uri: uri.clone() })?.metadata();
  if metadata.purpose() != expected_purpose {
    return Err(ReadArtifactError::WrongPurpose {
      uri: uri.clone(),
      expected: expected_purpose.clone(),
      actual: metadata.purpose().clone(),
    });
  }
  if metadata.content_type() != expected_content_type {
    return Err(ReadArtifactError::WrongContentType {
      uri: uri.clone(),
      expected: expected_content_type.clone(),
      actual: metadata.content_type().clone(),
    });
  }

  let expected_length = metadata.byte_length().get();
  if expected_length > byte_limit.get() {
    return Err(ReadArtifactError::PayloadTooLarge {
      uri: uri.clone(),
      limit: byte_limit,
      actual: expected_length,
    });
  }
  let expected_capacity = usize::try_from(expected_length).map_err(|_| ReadArtifactError::LengthOutOfRange {
    uri: uri.clone(),
    actual: expected_length,
  })?;
  let mut bytes = Vec::new();
  bytes.try_reserve_exact(expected_capacity).map_err(|source| ReadArtifactError::Allocation {
    uri: uri.clone(),
    expected: metadata.byte_length(),
    source,
  })?;
  let mut reader = store.open_artifact(uri.clone()).await.map_err(|source| ReadArtifactError::Open {
    uri: uri.clone(),
    source,
  })?;
  let mut actual_length = 0_u64;
  while let Some(chunk) = reader.next().await {
    let chunk = chunk.map_err(|source| ReadArtifactError::Stream {
      uri: uri.clone(),
      source,
    })?;
    actual_length = actual_length.checked_add(chunk.len() as u64).ok_or_else(|| ReadArtifactError::PayloadTooLarge {
      uri: uri.clone(),
      limit: byte_limit,
      actual: u64::MAX,
    })?;
    if actual_length > byte_limit.get() {
      return Err(ReadArtifactError::PayloadTooLarge {
        uri: uri.clone(),
        limit: byte_limit,
        actual: actual_length,
      });
    }
    if actual_length > expected_length {
      return Err(ReadArtifactError::LengthMismatch {
        uri: uri.clone(),
        expected: metadata.byte_length(),
        actual: actual_length,
      });
    }
    bytes.extend_from_slice(&chunk);
  }
  if actual_length != expected_length {
    return Err(ReadArtifactError::LengthMismatch {
      uri: uri.clone(),
      expected: metadata.byte_length(),
      actual: actual_length,
    });
  }
  let actual_digest = Sha256Digest::new(Sha256::digest(&bytes).into());
  if actual_digest != metadata.sha256() {
    return Err(ReadArtifactError::DigestMismatch {
      uri: uri.clone(),
      expected: metadata.sha256(),
      actual: actual_digest,
    });
  }
  Ok(bytes)
}

/// Reads and decodes one bounded JSON artifact after applying the canonical
/// artifact ownership and integrity checks.
///
/// This validates the transport contract only. Producers and consumers remain
/// responsible for domain-specific payload validation after decoding.
pub async fn read_json_artifact<T>(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  expected_purpose: &ArtifactPurpose,
  byte_limit: ByteLength,
) -> Result<T, JsonArtifactReadError>
where
  T: de::DeserializeOwned,
{
  let content_type = ContentType::parse("application/json").expect("static JSON content type is valid");
  let bytes = read_artifact_bytes(store, snapshot, uri, expected_purpose, &content_type, byte_limit).await?;
  serde_json::from_slice(&bytes).map_err(|source| JsonArtifactReadError::Decode {
    uri: uri.clone(),
    source,
  })
}

/// Failure to read or decode a typed JSON artifact.
#[derive(Debug, thiserror::Error)]
pub enum JsonArtifactReadError {
  #[error(transparent)]
  Artifact(#[from] ReadArtifactError),
  #[error("artifact {uri} contains invalid JSON: {source}")]
  Decode {
    uri: ArtifactUri,
    #[source]
    source: serde_json::Error,
  },
}

/// Failure to read bytes under a caller-specified artifact contract.
#[derive(Debug, thiserror::Error)]
pub enum ReadArtifactError {
  #[error("snapshot authority {snapshot_authority} does not match store authority {store_authority}")]
  SnapshotAuthorityMismatch {
    snapshot_authority: AuthorityId,
    store_authority: AuthorityId,
  },
  #[error("artifact belongs to run {artifact_run_id}, not snapshot run {snapshot_run_id}")]
  WrongRun {
    snapshot_run_id: RunId,
    artifact_run_id: RunId,
  },
  #[error("artifact is not committed in the supplied snapshot: {uri}")]
  NotCommitted { uri: ArtifactUri },
  #[error("artifact {uri} has purpose {actual}, expected {expected}")]
  WrongPurpose {
    uri: ArtifactUri,
    expected: ArtifactPurpose,
    actual: ArtifactPurpose,
  },
  #[error("artifact {uri} has content type {actual}, expected {expected}")]
  WrongContentType {
    uri: ArtifactUri,
    expected: ContentType,
    actual: ContentType,
  },
  #[error("artifact {uri} is {actual} bytes, exceeding the {limit}-byte consumer limit")]
  PayloadTooLarge {
    uri: ArtifactUri,
    limit: ByteLength,
    actual: u64,
  },
  #[error("artifact {uri} byte length {actual} cannot be represented by this process")]
  LengthOutOfRange { uri: ArtifactUri, actual: u64 },
  #[error("failed to reserve {expected} bytes for artifact {uri}: {source}")]
  Allocation {
    uri: ArtifactUri,
    expected: ByteLength,
    #[source]
    source: TryReserveError,
  },
  #[error("failed to open artifact {uri}: {source}")]
  Open {
    uri: ArtifactUri,
    #[source]
    source: ReadError,
  },
  #[error("failed to stream artifact {uri}: {source}")]
  Stream {
    uri: ArtifactUri,
    #[source]
    source: ArtifactReadError,
  },
  #[error("artifact {uri} length mismatch: expected {expected}, read {actual}")]
  LengthMismatch {
    uri: ArtifactUri,
    expected: ByteLength,
    actual: u64,
  },
  #[error("artifact {uri} digest mismatch: expected {expected}, read {actual}")]
  DigestMismatch {
    uri: ArtifactUri,
    expected: Sha256Digest,
    actual: Sha256Digest,
  },
}

pub(crate) struct DetachedArtifact {
  pub(crate) artifact_id: ArtifactId,
  pub(crate) idempotency_key: IdempotencyKey,
  pub(crate) purpose: ArtifactPurpose,
  pub(crate) content_type: ContentType,
  pub(crate) expected_byte_length: ByteLength,
  pub(crate) expected_sha256: Sha256Digest,
  pub(crate) attributes: Attributes,
  pub(crate) body: ArtifactBody,
}

pub(crate) struct ArtifactReceiptMessage {
  pub(crate) result: Result<ArtifactMetadata, ArtifactWriteError>,
  pub(crate) unclaimed_failure: Option<DispatchFailure>,
}

pub(crate) type ArtifactReceiptSender = oneshot::Sender<ArtifactReceiptMessage>;

/// A receipt for one synchronously admitted artifact job.
pub struct ArtifactEmission {
  receipt: ArtifactReceipt,
}

enum ArtifactReceipt {
  Disabled,
  Pending {
    receiver: oneshot::Receiver<ArtifactReceiptMessage>,
    dispatch: Dispatch,
  },
  Complete,
}

impl ArtifactEmission {
  pub(crate) fn disabled() -> Self {
    Self {
      receipt: ArtifactReceipt::Disabled,
    }
  }

  pub(crate) fn pending(dispatch: Dispatch) -> (ArtifactReceiptSender, Self) {
    let (sender, receiver) = oneshot::channel();
    (
      sender,
      Self {
        receipt: ArtifactReceipt::Pending { receiver, dispatch },
      },
    )
  }
}

impl Future for ArtifactEmission {
  type Output = Result<Option<ArtifactMetadata>, ArtifactWriteError>;

  fn poll(self: Pin<&mut Self>, context: &mut TaskContext<'_>) -> Poll<Self::Output> {
    let this = self.get_mut();
    match &mut this.receipt {
      ArtifactReceipt::Disabled => {
        this.receipt = ArtifactReceipt::Complete;
        Poll::Ready(Ok(None))
      }
      ArtifactReceipt::Pending { receiver, .. } => match Pin::new(receiver).poll(context) {
        Poll::Ready(Ok(message)) => {
          this.receipt = ArtifactReceipt::Complete;
          Poll::Ready(message.result.map(Some))
        }
        Poll::Ready(Err(_)) => {
          this.receipt = ArtifactReceipt::Complete;
          Poll::Ready(Err(ArtifactWriteError::Unavailable(receipt_closed_code())))
        }
        Poll::Pending => Poll::Pending,
      },
      ArtifactReceipt::Complete => panic!("completed ArtifactEmission futures must not be polled again"),
    }
  }
}

impl Drop for ArtifactEmission {
  fn drop(&mut self) {
    let ArtifactReceipt::Pending { receiver, dispatch } = &mut self.receipt else {
      return;
    };
    receiver.close();
    if let Ok(Some(message)) = receiver.try_recv()
      && let Some(failure) = message.unclaimed_failure
    {
      dispatch.report_unclaimed_artifact_failure(&failure);
    }
  }
}

/// Serializes and admits one typed JSON artifact under the current context.
///
/// When the current context has no artifact authority, this returns a disabled
/// emission without serializing `value`. Recording stays observational and
/// cannot make an otherwise unused domain value fail serialization.
pub fn emit_json_artifact<T>(
  purpose: ArtifactPurpose,
  attributes: Attributes,
  byte_limit: ByteLength,
  value: &T,
) -> Result<ArtifactEmission, JsonArtifactError>
where
  T: Serialize,
{
  if !crate::Context::current().can_publish_artifacts() {
    return Ok(ArtifactEmission::disabled());
  }
  Ok(emit_artifact(NewArtifact::from_json(purpose, attributes, byte_limit, value)?))
}

/// Admits an artifact under the current captured run context.
pub fn emit_artifact<R>(artifact: NewArtifact<R>) -> ArtifactEmission
where
  R: AsyncRead + Unpin + Send + 'static,
{
  let context = crate::Context::current();
  let Some(dispatch) = context.dispatch().filter(|dispatch| dispatch.authority_id().is_some()).cloned() else {
    return ArtifactEmission::disabled();
  };
  let Some(run_id) = context.run_id().copied() else {
    return ArtifactEmission::disabled();
  };
  dispatch.submit_artifact(run_id, context.span_id().copied(), artifact.into_detached())
}

fn receipt_closed_code() -> crate::ErrorCode {
  crate::ErrorCode::parse("auv.dispatch.artifact_receipt_closed").expect("static dispatch error code is valid")
}

#[cfg(test)]
mod tests {
  use serde::ser::Error as _;

  use super::*;

  #[test]
  fn byte_artifact_derives_committed_length_and_digest_from_the_body() {
    let body = b"artifact body".to_vec();
    let artifact = NewArtifact::from_bytes(
      ArtifactPurpose::parse("auv.test.bytes").unwrap(),
      ContentType::parse("application/octet-stream").unwrap(),
      Attributes::empty(),
      body.clone(),
    )
    .unwrap();

    assert_eq!(artifact.expected_byte_length.get(), body.len() as u64);
    assert_eq!(artifact.expected_sha256, Sha256Digest::new(Sha256::digest(&body).into()));
  }

  #[test]
  fn json_artifact_serializes_once_with_canonical_content_type_and_integrity() {
    let artifact = NewArtifact::from_json(
      ArtifactPurpose::parse("auv.test.json").unwrap(),
      Attributes::empty(),
      ByteLength::new(1024).unwrap(),
      &serde_json::json!({ "value": 42 }),
    )
    .unwrap();

    assert_eq!(artifact.content_type.to_string(), "application/json");
    assert_eq!(artifact.expected_byte_length.get(), artifact.body.get_ref().len() as u64);
    assert_eq!(artifact.expected_sha256, Sha256Digest::new(Sha256::digest(artifact.body.get_ref()).into()));
  }

  #[test]
  fn json_artifact_rejects_payloads_larger_than_the_caller_limit() {
    let error = match NewArtifact::from_json(
      ArtifactPurpose::parse("auv.test.json").unwrap(),
      Attributes::empty(),
      ByteLength::new(8).unwrap(),
      &serde_json::json!({ "value": "too large" }),
    ) {
      Ok(_) => panic!("bounded JSON must fail"),
      Err(error) => error,
    };

    assert!(matches!(error, JsonArtifactError::PayloadTooLarge { limit, actual } if limit.get() == 8 && actual > 8));
  }

  struct FailingJson;

  impl Serialize for FailingJson {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
      S: Serializer,
    {
      Err(S::Error::custom("intentional serialization failure"))
    }
  }

  #[test]
  fn json_artifact_preserves_serializer_failures() {
    let error = match NewArtifact::from_json(
      ArtifactPurpose::parse("auv.test.json").unwrap(),
      Attributes::empty(),
      ByteLength::new(1024).unwrap(),
      &FailingJson,
    ) {
      Ok(_) => panic!("serializer failure must propagate"),
      Err(error) => error,
    };

    assert!(matches!(error, JsonArtifactError::Serialize(_)));
  }

  #[test]
  fn disabled_json_emission_does_not_serialize_the_domain_value() {
    let emission = emit_json_artifact(
      ArtifactPurpose::parse("auv.test.json").unwrap(),
      Attributes::empty(),
      ByteLength::new(1024).unwrap(),
      &FailingJson,
    )
    .expect("disabled instrumentation must not inspect the value");

    assert!(futures_executor::block_on(emission).unwrap().is_none());
  }
}

/// The canonical transport-independent identity of one run artifact.
// TODO(inspect-artifact-resolution-v1): Enforce the 256-URI request bound on
// the resolver DTO when that later Inspect slice introduces the batch surface.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactUri(Url);

impl ArtifactUri {
  /// Constructs the sole V1 URI form from validated identifiers.
  pub fn from_ids(run_id: RunId, artifact_id: ArtifactId) -> Self {
    format!("auv://runs/{run_id}/artifacts/{artifact_id}").parse().expect("validated IDs always produce a canonical artifact URI")
  }

  /// Returns the owning run identifier.
  pub fn run_id(&self) -> RunId {
    self.path_ids().0
  }

  /// Returns the artifact identifier.
  pub fn artifact_id(&self) -> ArtifactId {
    self.path_ids().1
  }

  fn path_ids(&self) -> (RunId, ArtifactId) {
    let segments = self.0.path_segments().expect("canonical artifact URI has path segments").collect::<Vec<_>>();
    (
      segments[0].parse().expect("canonical artifact URI has a run ID"),
      segments[2].parse().expect("canonical artifact URI has an artifact ID"),
    )
  }
}

impl fmt::Display for ArtifactUri {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.0.as_str())
  }
}

impl FromStr for ArtifactUri {
  type Err = ValidationError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    let parsed = Url::parse(value).map_err(|_| ValidationError::new("artifact URI is not a valid URL"))?;
    if parsed.scheme() != "auv"
      || parsed.host_str() != Some("runs")
      || !parsed.username().is_empty()
      || parsed.password().is_some()
      || parsed.port().is_some()
      || parsed.query().is_some()
      || parsed.fragment().is_some()
    {
      return Err(ValidationError::new("artifact URI must use the canonical AUV authority"));
    }

    let segments = parsed.path_segments().ok_or_else(|| ValidationError::new("artifact URI path is invalid"))?.collect::<Vec<_>>();
    if segments.len() != 3 || segments[1] != "artifacts" {
      return Err(ValidationError::new("artifact URI must identify exactly one run artifact"));
    }
    let run_id = segments[0].parse::<RunId>().map_err(|_| ValidationError::new("artifact URI run ID is invalid"))?;
    let artifact_id = segments[2].parse::<ArtifactId>().map_err(|_| ValidationError::new("artifact URI artifact ID is invalid"))?;
    let canonical = format!("auv://runs/{run_id}/artifacts/{artifact_id}");
    if value != canonical || parsed.as_str() != canonical {
      return Err(ValidationError::new("artifact URI is not canonical"));
    }
    Ok(Self(parsed))
  }
}

impl Serialize for ArtifactUri {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.collect_str(self)
  }
}

impl<'de> Deserialize<'de> for ArtifactUri {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    String::deserialize(deserializer)?.parse().map_err(de::Error::custom)
  }
}
