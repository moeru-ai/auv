use std::collections::BTreeSet;

use crate::{
  ArtifactPurpose, ArtifactUri, AttributeKey, Attributes, AuthorityId, ByteLength, ContentType, DispatchFailure, ErrorCode, EventId,
  EventSchema, RunId, RunRevision, Sha256Digest, SpanId, SpanName, Timestamp,
};

/// One closed, bounded fact approved for external telemetry projection.
///
/// Canonical event payload JSON and artifact bodies are deliberately absent.
#[derive(Clone, Debug, PartialEq)]
pub enum TelemetryItem {
  /// A span start with optional allowlisted producer attributes.
  SpanStart {
    /// The durable authority identity, when one exists.
    authority_id: Option<AuthorityId>,
    /// The explicit run identity.
    run_id: RunId,
    /// The local span identity.
    span_id: SpanId,
    /// The local parent span identity, when present.
    parent_span_id: Option<SpanId>,
    /// The propagated remote span identity, when present.
    remote_span_id: Option<SpanId>,
    /// The stable typed span name.
    name: SpanName,
    /// The wall-clock start time.
    started_at: Timestamp,
    /// The authority revision that committed the start, when durable.
    start_revision: Option<RunRevision>,
    /// Producer attributes retained by this projector route's allowlist.
    attributes: Attributes,
  },
  /// A timestamp-only span finish.
  SpanEnd {
    /// The durable authority identity, when one exists.
    authority_id: Option<AuthorityId>,
    /// The explicit run identity.
    run_id: RunId,
    /// The finished local span identity.
    span_id: SpanId,
    /// The wall-clock finish time.
    ended_at: Timestamp,
    /// The authority revision that committed the finish, when durable.
    end_revision: Option<RunRevision>,
  },
  /// A typed point event without its canonical JSON payload.
  Event {
    /// The durable authority identity, when one exists.
    authority_id: Option<AuthorityId>,
    /// The explicit run identity.
    run_id: RunId,
    /// The associated local span identity, when present.
    span_id: Option<SpanId>,
    /// The immutable event identity.
    event_id: EventId,
    /// The bounded event schema identity.
    schema: EventSchema,
    /// The event wall-clock time.
    occurred_at: Timestamp,
    /// The authority revision that committed the event, when durable.
    revision: Option<RunRevision>,
  },
  /// Artifact metadata without artifact bytes or authority-local locations.
  Artifact {
    /// The durable authority identity.
    authority_id: AuthorityId,
    /// The explicit run identity.
    run_id: RunId,
    /// The associated local span identity, when present.
    span_id: Option<SpanId>,
    /// The canonical artifact URI.
    uri: ArtifactUri,
    /// The stable artifact relationship name.
    purpose: ArtifactPurpose,
    /// The concrete canonical MIME type.
    content_type: ContentType,
    /// The committed artifact byte length.
    byte_length: ByteLength,
    /// The committed artifact SHA-256 digest.
    sha256: Sha256Digest,
    /// Producer attributes retained by this projector route's allowlist.
    attributes: Attributes,
    /// The authority revision that published the artifact.
    revision: RunRevision,
  },
}

/// Reports a downstream telemetry projection failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("telemetry projection failed: {code}")]
pub struct TelemetryError {
  code: ErrorCode,
}

impl TelemetryError {
  /// Creates a projection failure with a stable machine-readable code.
  pub fn new(code: ErrorCode) -> Self {
    Self { code }
  }

  /// Returns the stable machine-readable failure code.
  pub fn code(&self) -> &ErrorCode {
    &self.code
  }
}

/// Selects which bounded producer attributes one telemetry route may receive.
///
/// Canonical AUV correlation fields are fixed by [`TelemetryItem`] and do not
/// need to be allowlisted. The default forwards no producer attributes.
#[derive(Clone, Debug, Default)]
pub struct TelemetryRoutePolicy {
  span_attribute_keys: BTreeSet<AttributeKey>,
  artifact_attribute_keys: BTreeSet<AttributeKey>,
}

impl TelemetryRoutePolicy {
  /// Creates a policy that forwards only the fixed AUV telemetry vocabulary.
  pub fn fixed_fields_only() -> Self {
    Self::default()
  }

  /// Allows one producer span attribute on this route.
  pub fn allow_span_attribute(mut self, key: AttributeKey) -> Self {
    self.span_attribute_keys.insert(key);
    self
  }

  /// Allows one producer artifact attribute on this route.
  pub fn allow_artifact_attribute(mut self, key: AttributeKey) -> Self {
    self.artifact_attribute_keys.insert(key);
    self
  }

  pub(crate) fn span_attributes(&self, attributes: &Attributes) -> Attributes {
    filtered_attributes(attributes, &self.span_attribute_keys)
  }

  // TODO(auv-run-contract-v1-task-9): consume this filter when the detached
  // artifact lane begins emitting dispatch-owned publication telemetry.
  #[allow(dead_code)]
  pub(crate) fn artifact_attributes(&self, attributes: &Attributes) -> Attributes {
    filtered_attributes(attributes, &self.artifact_attribute_keys)
  }
}

fn filtered_attributes(attributes: &Attributes, allowed: &BTreeSet<AttributeKey>) -> Attributes {
  Attributes::try_from_iter(attributes.iter().filter(|(key, _)| allowed.contains(*key)).map(|(key, value)| (key.clone(), value.clone())))
    .expect("a subset of validated attributes remains valid")
}

/// Receives one dispatch's approved telemetry items in serialized order.
pub trait TelemetryProjector: Send + Sync {
  /// Projects one bounded item.
  fn project(&self, item: TelemetryItem) -> crate::BoxFuture<'_, Result<(), TelemetryError>>;

  /// Flushes projector-owned buffering after preceding projection calls finish.
  fn flush(&self) -> crate::BoxFuture<'_, Result<(), TelemetryError>>;
}

/// Receives asynchronous dispatch failures for non-blocking diagnostics.
///
/// Reporters must not block or panic. Reporting is diagnostic only and cannot
/// retry a mutation, roll back a commit, or run application operations.
pub trait DispatchErrorReporter: Send + Sync {
  /// Reports one retained dispatch failure exactly once.
  fn report(&self, failure: &DispatchFailure);
}
