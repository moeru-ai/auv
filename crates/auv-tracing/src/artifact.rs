use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context as TaskContext, Poll};

use futures_channel::oneshot;
use futures_io::AsyncRead;
use serde::de;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use url::Url;

use crate::{
  ArtifactBody, ArtifactId, ArtifactMetadata, ArtifactPurpose, ArtifactWriteError, Attributes, ByteLength, ContentType, Dispatch,
  DispatchFailure, IdempotencyKey, RunId, Sha256Digest, ValidationError,
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
  pub(crate) unobserved_failure: Option<DispatchFailure>,
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
    if let Ok(Some(message)) = receiver.try_recv()
      && let Some(failure) = message.unobserved_failure
    {
      dispatch.report_unobserved_artifact_failure(&failure);
    }
  }
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
