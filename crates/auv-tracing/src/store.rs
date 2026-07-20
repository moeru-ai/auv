use std::future::Future;
use std::pin::Pin;

use bytes::Bytes;
use futures_core::Stream;
use futures_io::AsyncRead;
use serde::de;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
  ArtifactId, ArtifactPurpose, ArtifactUri, Attributes, AuthorityId, ByteLength, ContentType, ErrorCode, IdempotencyKey, PageLimit,
  RunCommit, RunCommitRequest, RunId, RunRevision, RunSnapshot, Sha256Digest, SpanId, ValidationError,
};

const MAX_COMMIT_PAGE_JSON_BYTES: usize = 32 * 1024 * 1024;

/// A boxed asynchronous operation returned by the object-safe store port.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A one-shot artifact body supplied to a store authority.
pub type ArtifactBody = Pin<Box<dyn AsyncRead + Send>>;

/// A fallible stream of artifact byte chunks.
pub type ArtifactReader = Pin<Box<dyn Stream<Item = Result<Bytes, ArtifactReadError>> + Send>>;

/// A fallible ordered stream of commits after an explicit revision.
pub type RunSubscription = Pin<Box<dyn Stream<Item = Result<RunCommit, SubscriptionError>> + Send>>;

/// The single authority port for ordered run history and artifact bytes.
pub trait RunStore: Send + Sync {
  /// Returns the stable identity of this store authority.
  fn authority_id(&self) -> AuthorityId;

  /// Atomically validates and appends ordinary run mutations.
  fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<RunCommit, CommitError>>;

  /// Validates, stores, and atomically publishes one artifact.
  fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<RunCommit, ArtifactWriteError>>;

  /// Resolves an already accepted ordinary or artifact write.
  fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>>;

  /// Loads the deterministic snapshot for a run with committed facts.
  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>>;

  /// Reads a bounded page of commits after an explicit revision.
  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>>;

  /// Subscribes to commits strictly after an explicit revision.
  fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>>;

  /// Opens the verified bytes identified by a canonical artifact URI.
  fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>>;
}

// TODO(run-store-backends-v1): concrete authorities are deferred to the
// owner-approved backend tasks beginning with Task 5 of the V1 implementation plan.

/// Metadata required before an authority consumes a one-shot artifact body.
#[derive(Clone, Debug, PartialEq)]
pub struct StoreArtifactRequest {
  authority_id: AuthorityId,
  run_id: RunId,
  idempotency_key: IdempotencyKey,
  artifact_id: ArtifactId,
  span_id: Option<SpanId>,
  purpose: ArtifactPurpose,
  content_type: ContentType,
  expected_byte_length: ByteLength,
  expected_sha256: Sha256Digest,
  attributes: Attributes,
}

impl StoreArtifactRequest {
  /// Creates an artifact write request from validated contract values.
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    authority_id: AuthorityId,
    run_id: RunId,
    idempotency_key: IdempotencyKey,
    artifact_id: ArtifactId,
    span_id: Option<SpanId>,
    purpose: ArtifactPurpose,
    content_type: ContentType,
    expected_byte_length: ByteLength,
    expected_sha256: Sha256Digest,
    attributes: Attributes,
  ) -> Self {
    Self {
      authority_id,
      run_id,
      idempotency_key,
      artifact_id,
      span_id,
      purpose,
      content_type,
      expected_byte_length,
      expected_sha256,
      attributes,
    }
  }

  /// Returns the target authority identity.
  pub fn authority_id(&self) -> AuthorityId {
    self.authority_id
  }

  /// Returns the owning run identity.
  pub fn run_id(&self) -> RunId {
    self.run_id
  }

  /// Returns the write idempotency key.
  pub fn idempotency_key(&self) -> IdempotencyKey {
    self.idempotency_key
  }

  /// Returns the artifact identity used to derive its canonical URI.
  pub fn artifact_id(&self) -> ArtifactId {
    self.artifact_id
  }

  /// Returns the associated span identity, when present.
  pub fn span_id(&self) -> Option<SpanId> {
    self.span_id
  }

  /// Returns the stable artifact relationship name.
  pub fn purpose(&self) -> &ArtifactPurpose {
    &self.purpose
  }

  /// Returns the declared artifact media type.
  pub fn content_type(&self) -> &ContentType {
    &self.content_type
  }

  /// Returns the byte length the body must match before publication.
  pub fn expected_byte_length(&self) -> ByteLength {
    self.expected_byte_length
  }

  /// Returns the digest the body must match before publication.
  pub fn expected_sha256(&self) -> Sha256Digest {
    self.expected_sha256
  }

  /// Returns the bounded artifact attributes.
  pub fn attributes(&self) -> &Attributes {
    &self.attributes
  }
}

/// One bounded page of canonical commits and its continuation cursor.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunCommitPage {
  commits: Vec<RunCommit>,
  last_revision: RunRevision,
  has_more: bool,
}

impl RunCommitPage {
  /// Creates a page whose cursor and compact JSON size agree with its commits.
  pub fn new(commits: Vec<RunCommit>, last_revision: RunRevision, has_more: bool) -> Result<Self, ValidationError> {
    if has_more && commits.is_empty() {
      return Err(ValidationError::new("a page with more history must make progress"));
    }
    if let Some(last) = commits.last()
      && last.revision() != last_revision
    {
      return Err(ValidationError::new("page cursor must equal the last returned revision"));
    }
    if commits.windows(2).any(|pair| {
      pair[1].authority_id() != pair[0].authority_id()
        || pair[1].run_id() != pair[0].run_id()
        || pair[1].revision().get() != pair[0].revision().get() + 1
    }) {
      return Err(ValidationError::new("page commits must have contiguous revisions"));
    }
    #[derive(Serialize)]
    struct Wire<'a> {
      commits: &'a [RunCommit],
      last_revision: RunRevision,
      has_more: bool,
    }

    let compact_json_bytes = serde_json::to_vec(&Wire {
      commits: &commits,
      last_revision,
      has_more,
    })
    .map_err(|_| ValidationError::new("commit page could not be encoded"))?
    .len();
    if compact_json_bytes > MAX_COMMIT_PAGE_JSON_BYTES {
      return Err(ValidationError::new("page commits exceed 32 MiB of compact JSON"));
    }
    Ok(Self {
      commits,
      last_revision,
      has_more,
    })
  }

  /// Returns commits in ascending revision order.
  pub fn commits(&self) -> &[RunCommit] {
    &self.commits
  }

  /// Returns the last returned revision or the requested cursor for an empty page.
  pub fn last_revision(&self) -> RunRevision {
    self.last_revision
  }

  /// Reports whether retained history remains after this page.
  pub fn has_more(&self) -> bool {
    self.has_more
  }
}

impl<'de> Deserialize<'de> for RunCommitPage {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      commits: Vec<RunCommit>,
      last_revision: RunRevision,
      has_more: bool,
    }

    let wire = Wire::deserialize(deserializer)?;
    Self::new(wire.commits, wire.last_revision, wire.has_more).map_err(de::Error::custom)
  }
}

/// Reports the outcome class for an ordinary commit attempt.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CommitError {
  /// The request targeted a different authority.
  #[error("authority mismatch: expected {expected}, received {received}")]
  AuthorityMismatch {
    expected: AuthorityId,
    received: AuthorityId,
  },
  /// The idempotency key was already used for a different canonical request.
  #[error("idempotency key was already used for a different request")]
  IdempotencyMismatch,
  /// The authority rejected the mutation.
  #[error("commit rejected: {0}")]
  Rejected(ErrorCode),
  /// The authority was unavailable before accepting the commit.
  #[error("store unavailable: {0}")]
  Unavailable(ErrorCode),
  /// The caller cannot know whether the authority accepted the commit.
  #[error("commit outcome unknown: {0}")]
  CommitUnknown(ErrorCode),
}

/// Reports the outcome class for an artifact publication attempt.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ArtifactWriteError {
  /// The request targeted a different authority.
  #[error("authority mismatch: expected {expected}, received {received}")]
  AuthorityMismatch {
    expected: AuthorityId,
    received: AuthorityId,
  },
  /// The idempotency key was already used for a different canonical request.
  #[error("idempotency key was already used for a different request")]
  IdempotencyMismatch,
  /// The authority rejected the artifact mutation.
  #[error("artifact write rejected: {0}")]
  Rejected(ErrorCode),
  /// The body did not satisfy the committed integrity contract.
  #[error("artifact integrity failure: {0}")]
  Integrity(ErrorCode),
  /// The authority was unavailable before publication.
  #[error("store unavailable: {0}")]
  Unavailable(ErrorCode),
  /// The caller cannot know whether publication completed.
  #[error("artifact publication outcome unknown: {0}")]
  PublicationUnknown(ErrorCode),
}

/// Reports a typed failure while reading run history or artifact bytes.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
  /// The requested run, commit, or artifact does not exist.
  #[error("run data not found")]
  NotFound,
  /// The caller is not allowed to read the requested data.
  #[error("run data is forbidden")]
  Forbidden,
  /// The supplied typed reference is invalid for this authority.
  #[error("invalid run-data reference: {0}")]
  InvalidReference(ErrorCode),
  /// Retention has removed history needed to serve the cursor.
  #[error("history gap after {requested_after:?}; earliest available revision is {earliest_available:?}")]
  HistoryGap {
    requested_after: RunRevision,
    earliest_available: RunRevision,
  },
  /// The supplied cursor is later than the authority's latest revision.
  #[error("cursor {requested_after:?} is ahead of latest revision {latest:?}")]
  CursorAhead {
    requested_after: RunRevision,
    latest: RunRevision,
  },
  /// The authority is temporarily unavailable.
  #[error("store unavailable: {0}")]
  Unavailable(ErrorCode),
  /// Stored data failed integrity verification.
  #[error("stored data integrity failure: {0}")]
  Integrity(ErrorCode),
}

/// Reports a failure encountered after an artifact reader was opened.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ArtifactReadError {
  /// The artifact source became unavailable.
  #[error("artifact source unavailable: {0}")]
  Unavailable(ErrorCode),
  /// Streamed bytes failed integrity verification.
  #[error("artifact integrity failure: {0}")]
  Integrity(ErrorCode),
}

/// Reports a recoverable subscription boundary or underlying store failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SubscriptionError {
  /// Retention removed history required to continue from the cursor.
  #[error("subscription gap after {requested_after:?}; earliest available revision is {earliest_available:?}")]
  Gap {
    requested_after: RunRevision,
    earliest_available: RunRevision,
  },
  /// The underlying store read failed.
  #[error(transparent)]
  Store(ReadError),
}
