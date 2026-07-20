use std::collections::{BTreeMap, BTreeSet};

use serde::de;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
  ArtifactPurpose, ArtifactUri, Attributes, AuthorityId, ByteLength, ContentType, EventId, EventSchema, IdempotencyKey, JsonPayload,
  NonEmptyVec, RunId, RunRevision, Sha256Digest, SpanId, SpanName, Timestamp, ValidationError,
};

const MAX_COMMIT_ITEMS: usize = 256;

/// A propagated span identity that correlates a new local root with remote work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpanLink {
  span_id: SpanId,
}

impl SpanLink {
  /// Creates a link to a propagated span identity.
  pub fn new(span_id: SpanId) -> Self {
    Self { span_id }
  }

  /// Returns the propagated span identity.
  pub fn span_id(&self) -> SpanId {
    self.span_id
  }
}

impl<'de> Deserialize<'de> for SpanLink {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      span_id: SpanId,
    }

    let wire = Wire::deserialize(deserializer)?;
    Ok(Self::new(wire.span_id))
  }
}

/// The immutable start fact for one named span.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpanStarted {
  span_id: SpanId,
  parent_span_id: Option<SpanId>,
  remote_link: Option<SpanLink>,
  name: SpanName,
  started_at: Timestamp,
  attributes: Attributes,
}

impl SpanStarted {
  /// Creates a span start fact from validated values.
  pub fn new(
    span_id: SpanId,
    parent_span_id: Option<SpanId>,
    remote_link: Option<SpanLink>,
    name: SpanName,
    started_at: Timestamp,
    attributes: Attributes,
  ) -> Self {
    Self {
      span_id,
      parent_span_id,
      remote_link,
      name,
      started_at,
      attributes,
    }
  }

  /// Returns the span identity.
  pub fn span_id(&self) -> SpanId {
    self.span_id
  }

  /// Returns the local parent identity, when present.
  pub fn parent_span_id(&self) -> Option<SpanId> {
    self.parent_span_id
  }

  /// Returns the remote correlation link, when present.
  pub fn remote_link(&self) -> Option<&SpanLink> {
    self.remote_link.as_ref()
  }

  /// Returns the typed span name.
  pub fn name(&self) -> &SpanName {
    &self.name
  }

  /// Returns the wall-clock start time.
  pub fn started_at(&self) -> Timestamp {
    self.started_at
  }

  /// Returns the bounded start attributes.
  pub fn attributes(&self) -> &Attributes {
    &self.attributes
  }
}

impl<'de> Deserialize<'de> for SpanStarted {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      span_id: SpanId,
      parent_span_id: Option<SpanId>,
      remote_link: Option<SpanLink>,
      name: SpanName,
      started_at: Timestamp,
      attributes: Attributes,
    }

    let wire = Wire::deserialize(deserializer)?;
    Ok(Self::new(wire.span_id, wire.parent_span_id, wire.remote_link, wire.name, wire.started_at, wire.attributes))
  }
}

/// The timestamp-only finish fact for one span.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpanEnded {
  span_id: SpanId,
  ended_at: Timestamp,
}

impl SpanEnded {
  /// Creates a span finish fact without inferring an outcome.
  pub fn new(span_id: SpanId, ended_at: Timestamp) -> Self {
    Self { span_id, ended_at }
  }

  /// Returns the finished span identity.
  pub fn span_id(&self) -> SpanId {
    self.span_id
  }

  /// Returns the wall-clock finish time.
  pub fn ended_at(&self) -> Timestamp {
    self.ended_at
  }
}

impl<'de> Deserialize<'de> for SpanEnded {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      span_id: SpanId,
      ended_at: Timestamp,
    }

    let wire = Wire::deserialize(deserializer)?;
    Ok(Self::new(wire.span_id, wire.ended_at))
  }
}

/// One immutable typed point event in a run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventOccurred {
  event_id: EventId,
  span_id: Option<SpanId>,
  occurred_at: Timestamp,
  schema: EventSchema,
  payload: JsonPayload,
}

impl EventOccurred {
  /// Creates a typed event fact from validated schema and payload values.
  pub fn new(event_id: EventId, span_id: Option<SpanId>, occurred_at: Timestamp, schema: EventSchema, payload: JsonPayload) -> Self {
    Self {
      event_id,
      span_id,
      occurred_at,
      schema,
      payload,
    }
  }

  /// Returns the event identity.
  pub fn event_id(&self) -> EventId {
    self.event_id
  }

  /// Returns the associated span identity, when present.
  pub fn span_id(&self) -> Option<SpanId> {
    self.span_id
  }

  /// Returns the event wall-clock time.
  pub fn occurred_at(&self) -> Timestamp {
    self.occurred_at
  }

  /// Returns the typed event schema.
  pub fn schema(&self) -> &EventSchema {
    &self.schema
  }

  /// Returns the canonical event payload.
  pub fn payload(&self) -> &JsonPayload {
    &self.payload
  }
}

impl<'de> Deserialize<'de> for EventOccurred {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      event_id: EventId,
      span_id: Option<SpanId>,
      occurred_at: Timestamp,
      schema: EventSchema,
      payload: JsonPayload,
    }

    let wire = Wire::deserialize(deserializer)?;
    Ok(Self::new(wire.event_id, wire.span_id, wire.occurred_at, wire.schema, wire.payload))
  }
}

/// Durable metadata for bytes published at one canonical artifact URI.
#[derive(Clone, Debug, PartialEq, Serialize)]
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
  /// Creates artifact metadata from validated values.
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

  /// Returns the canonical artifact URI.
  pub fn uri(&self) -> &ArtifactUri {
    &self.uri
  }

  /// Returns the stable artifact purpose.
  pub fn purpose(&self) -> &ArtifactPurpose {
    &self.purpose
  }

  /// Returns the concrete MIME content type.
  pub fn content_type(&self) -> &ContentType {
    &self.content_type
  }

  /// Returns the committed byte length.
  pub fn byte_length(&self) -> ByteLength {
    self.byte_length
  }

  /// Returns the committed SHA-256 digest.
  pub fn sha256(&self) -> Sha256Digest {
    self.sha256
  }

  /// Returns the bounded artifact attributes.
  pub fn attributes(&self) -> &Attributes {
    &self.attributes
  }
}

impl<'de> Deserialize<'de> for ArtifactMetadata {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      uri: ArtifactUri,
      purpose: ArtifactPurpose,
      content_type: ContentType,
      byte_length: ByteLength,
      sha256: Sha256Digest,
      attributes: Attributes,
    }

    let wire = Wire::deserialize(deserializer)?;
    Ok(Self::new(wire.uri, wire.purpose, wire.content_type, wire.byte_length, wire.sha256, wire.attributes))
  }
}

/// The committed publication fact for one artifact.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactPublished {
  span_id: Option<SpanId>,
  metadata: ArtifactMetadata,
}

impl ArtifactPublished {
  /// Creates an artifact publication fact.
  pub fn new(span_id: Option<SpanId>, metadata: ArtifactMetadata) -> Self {
    Self { span_id, metadata }
  }

  /// Returns the associated span identity, when present.
  pub fn span_id(&self) -> Option<SpanId> {
    self.span_id
  }

  /// Returns the committed artifact metadata.
  pub fn metadata(&self) -> &ArtifactMetadata {
    &self.metadata
  }
}

impl<'de> Deserialize<'de> for ArtifactPublished {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      span_id: Option<SpanId>,
      metadata: ArtifactMetadata,
    }

    let wire = Wire::deserialize(deserializer)?;
    Ok(Self::new(wire.span_id, wire.metadata))
  }
}

/// The closed union accepted by ordinary run commits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RunMutation {
  /// Starts one span.
  StartSpan(SpanStarted),
  /// Ends one span without assigning an outcome.
  EndSpan(SpanEnded),
  /// Emits one immutable typed point event.
  EmitEvent(EventOccurred),
}

/// The closed union stored in canonical committed run history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RunFact {
  /// A committed span start.
  SpanStarted(SpanStarted),
  /// A committed span finish.
  SpanEnded(SpanEnded),
  /// A committed typed event.
  EventOccurred(EventOccurred),
  /// A store-produced artifact publication.
  ArtifactPublished(ArtifactPublished),
}

/// One bounded ordinary write request to a run authority.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunCommitRequest {
  authority_id: AuthorityId,
  run_id: RunId,
  idempotency_key: IdempotencyKey,
  mutations: NonEmptyVec<RunMutation>,
}

impl RunCommitRequest {
  /// Creates a request containing `1..=256` ordinary mutations.
  pub fn new(
    authority_id: AuthorityId,
    run_id: RunId,
    idempotency_key: IdempotencyKey,
    mutations: Vec<RunMutation>,
  ) -> Result<Self, ValidationError> {
    Ok(Self {
      authority_id,
      run_id,
      idempotency_key,
      mutations: bounded_commit_items(mutations)?,
    })
  }

  /// Returns the target authority identity.
  pub fn authority_id(&self) -> AuthorityId {
    self.authority_id
  }

  /// Returns the target run identity.
  pub fn run_id(&self) -> RunId {
    self.run_id
  }

  /// Returns the idempotency key.
  pub fn idempotency_key(&self) -> IdempotencyKey {
    self.idempotency_key
  }

  /// Returns the validated mutations.
  pub fn mutations(&self) -> &[RunMutation] {
    self.mutations.as_slice()
  }
}

impl<'de> Deserialize<'de> for RunCommitRequest {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      authority_id: AuthorityId,
      run_id: RunId,
      idempotency_key: IdempotencyKey,
      mutations: Vec<RunMutation>,
    }

    let wire = Wire::deserialize(deserializer)?;
    Self::new(wire.authority_id, wire.run_id, wire.idempotency_key, wire.mutations).map_err(de::Error::custom)
  }
}

/// One atomic append accepted by a run authority.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunCommit {
  authority_id: AuthorityId,
  run_id: RunId,
  revision: RunRevision,
  idempotency_key: IdempotencyKey,
  committed_at: Timestamp,
  facts: NonEmptyVec<RunFact>,
}

impl RunCommit {
  /// Creates a commit containing `1..=256` facts.
  pub fn new(
    authority_id: AuthorityId,
    run_id: RunId,
    revision: RunRevision,
    idempotency_key: IdempotencyKey,
    committed_at: Timestamp,
    facts: Vec<RunFact>,
  ) -> Result<Self, ValidationError> {
    Ok(Self {
      authority_id,
      run_id,
      revision,
      idempotency_key,
      committed_at,
      facts: bounded_commit_items(facts)?,
    })
  }

  /// Returns the committing authority identity.
  pub fn authority_id(&self) -> AuthorityId {
    self.authority_id
  }

  /// Returns the run identity.
  pub fn run_id(&self) -> RunId {
    self.run_id
  }

  /// Returns the authority-allocated revision.
  pub fn revision(&self) -> RunRevision {
    self.revision
  }

  /// Returns the idempotency key for the append.
  pub fn idempotency_key(&self) -> IdempotencyKey {
    self.idempotency_key
  }

  /// Returns when the authority committed the append.
  pub fn committed_at(&self) -> Timestamp {
    self.committed_at
  }

  /// Returns the committed facts in their within-commit order.
  pub fn facts(&self) -> &[RunFact] {
    self.facts.as_slice()
  }
}

impl<'de> Deserialize<'de> for RunCommit {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      authority_id: AuthorityId,
      run_id: RunId,
      revision: RunRevision,
      idempotency_key: IdempotencyKey,
      committed_at: Timestamp,
      facts: Vec<RunFact>,
    }

    let wire = Wire::deserialize(deserializer)?;
    Self::new(wire.authority_id, wire.run_id, wire.revision, wire.idempotency_key, wire.committed_at, wire.facts).map_err(de::Error::custom)
  }
}

fn bounded_commit_items<T>(items: Vec<T>) -> Result<NonEmptyVec<T>, ValidationError> {
  if items.len() > MAX_COMMIT_ITEMS {
    return Err(ValidationError::new("commit batch exceeds 256 items"));
  }
  NonEmptyVec::new(items)
}

/// Materialized lifecycle state for one span.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpanSnapshot {
  started: SpanStarted,
  ended: Option<SpanEnded>,
}

impl SpanSnapshot {
  /// Returns the immutable span start fact.
  pub fn started(&self) -> &SpanStarted {
    &self.started
  }

  /// Returns the optional span finish fact without inferring a reason when absent.
  pub fn ended(&self) -> Option<&SpanEnded> {
    self.ended.as_ref()
  }

  fn new(started: SpanStarted, ended: Option<SpanEnded>) -> Result<Self, ReduceError> {
    if let Some(ended) = &ended {
      if ended.span_id() != started.span_id() {
        return Err(ReduceError::MismatchedSpanEnd);
      }
      if ended.ended_at() < started.started_at() {
        return Err(ReduceError::EndBeforeStart);
      }
    }
    Ok(Self { started, ended })
  }
}

impl<'de> Deserialize<'de> for SpanSnapshot {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      started: SpanStarted,
      ended: Option<SpanEnded>,
    }

    let wire = Wire::deserialize(deserializer)?;
    Self::new(wire.started, wire.ended).map_err(de::Error::custom)
  }
}

/// A deterministic read model through one committed revision.
// TODO(run-seal-v2): Run finalization is intentionally absent in V1; add it
// only through an owner-approved, separately versioned seal contract.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunSnapshot {
  authority_id: AuthorityId,
  run_id: RunId,
  through_revision: RunRevision,
  spans: BTreeMap<SpanId, SpanSnapshot>,
  events: Vec<EventOccurred>,
  artifacts: BTreeMap<ArtifactUri, ArtifactPublished>,
}

impl RunSnapshot {
  /// Returns the source authority identity.
  pub fn authority_id(&self) -> AuthorityId {
    self.authority_id
  }

  /// Returns the run identity.
  pub fn run_id(&self) -> RunId {
    self.run_id
  }

  /// Returns the last revision included in this snapshot.
  pub fn through_revision(&self) -> RunRevision {
    self.through_revision
  }

  /// Returns spans keyed by their local identity.
  pub fn spans(&self) -> &BTreeMap<SpanId, SpanSnapshot> {
    &self.spans
  }

  /// Returns events in commit and within-commit order.
  pub fn events(&self) -> &[EventOccurred] {
    &self.events
  }

  /// Returns artifacts keyed by canonical URI.
  pub fn artifacts(&self) -> &BTreeMap<ArtifactUri, ArtifactPublished> {
    &self.artifacts
  }
}

impl<'de> Deserialize<'de> for RunSnapshot {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      authority_id: AuthorityId,
      run_id: RunId,
      through_revision: RunRevision,
      spans: BTreeMap<SpanId, SpanSnapshot>,
      events: Vec<EventOccurred>,
      artifacts: BTreeMap<ArtifactUri, ArtifactPublished>,
    }

    let wire = Wire::deserialize(deserializer)?;
    let snapshot = Self {
      authority_id: wire.authority_id,
      run_id: wire.run_id,
      through_revision: wire.through_revision,
      spans: wire.spans,
      events: wire.events,
      artifacts: wire.artifacts,
    };
    validate_snapshot(&snapshot).map_err(de::Error::custom)?;
    Ok(snapshot)
  }
}

/// Reports a canonical history sequence that cannot produce a valid snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReduceError {
  /// A snapshot cannot identify an authority or run without a commit.
  #[error("run history is empty")]
  EmptyHistory,
  /// Revisions do not start at one and increase without gaps.
  #[error("run revisions are not contiguous")]
  NonContiguousRevision,
  /// Commits came from more than one authority.
  #[error("run history contains mixed authorities")]
  MixedAuthority,
  /// Commits belong to more than one run.
  #[error("run history contains mixed runs")]
  MixedRun,
  /// A span identity was started more than once.
  #[error("span was started more than once")]
  DuplicateSpanStart,
  /// A span identity was ended more than once.
  #[error("span was ended more than once")]
  DuplicateSpanEnd,
  /// A finish fact refers to a span that did not start.
  #[error("span end refers to an unknown span")]
  UnknownSpanEnd,
  /// A finish fact and start fact identify different spans.
  #[error("span snapshot contains a mismatched end")]
  MismatchedSpanEnd,
  /// A local parent is absent from the run.
  #[error("span refers to a missing local parent")]
  MissingLocalParent,
  /// Local parent links form a cycle.
  #[error("local span parentage is cyclic")]
  CyclicLocalParent,
  /// A span names itself as its local parent.
  #[error("span cannot be its own local parent")]
  SelfParent,
  /// A span links to its own identity as remote.
  #[error("span cannot link to itself as remote")]
  SelfRemoteLink,
  /// A local parent was repeated as the remote link.
  #[error("span cannot repeat its local parent as a remote link")]
  DuplicateParentLink,
  /// A child start was committed after its local parent ended.
  #[error("child span started after its local parent ended")]
  ParentSpanEnded,
  /// A child start timestamp precedes its local parent start timestamp.
  #[error("child span timestamp precedes its local parent")]
  ChildBeforeParent,
  /// A finish timestamp precedes its start timestamp.
  #[error("span end timestamp precedes its start")]
  EndBeforeStart,
  /// An event refers to a span that did not start.
  #[error("event refers to an unknown span")]
  UnknownEventSpan,
  /// An event was committed after its associated span ended.
  #[error("event was committed after its span ended")]
  EventAfterSpanEnd,
  /// An event timestamp precedes its associated span start.
  #[error("event timestamp precedes its span start")]
  EventBeforeSpanStart,
  /// An event identity appears more than once in a run.
  #[error("event identity is duplicated")]
  DuplicateEventId,
  /// An artifact refers to a span that did not start.
  #[error("artifact refers to an unknown span")]
  UnknownArtifactSpan,
  /// An artifact URI appears more than once in a run.
  #[error("artifact URI is duplicated")]
  DuplicateArtifactUri,
  /// An artifact URI embeds a different run identity.
  #[error("artifact URI belongs to a different run")]
  ArtifactRunMismatch,
  /// A snapshot map key does not match its span value.
  #[error("snapshot span key does not match its span identity")]
  SnapshotSpanKeyMismatch,
  /// A snapshot map key does not match its artifact value.
  #[error("snapshot artifact key does not match its artifact URI")]
  SnapshotArtifactKeyMismatch,
}

/// Replays a complete canonical commit sequence into one deterministic snapshot.
pub fn reduce_commits(commits: &[RunCommit]) -> Result<RunSnapshot, ReduceError> {
  let Some(first) = commits.first() else {
    return Err(ReduceError::EmptyHistory);
  };
  let authority_id = first.authority_id();
  let run_id = first.run_id();
  let mut snapshot = RunSnapshot {
    authority_id,
    run_id,
    through_revision: RunRevision::new(0).expect("revision zero is the valid pre-history cursor"),
    spans: BTreeMap::new(),
    events: Vec::new(),
    artifacts: BTreeMap::new(),
  };
  let mut event_ids = BTreeSet::new();

  for (index, commit) in commits.iter().enumerate() {
    let expected_revision = u64::try_from(index).ok().and_then(|value| value.checked_add(1));
    if expected_revision != Some(commit.revision().get()) {
      return Err(ReduceError::NonContiguousRevision);
    }
    if commit.authority_id() != authority_id {
      return Err(ReduceError::MixedAuthority);
    }
    if commit.run_id() != run_id {
      return Err(ReduceError::MixedRun);
    }

    apply_facts(&mut snapshot, &mut event_ids, commit.facts())?;
    snapshot.through_revision = commit.revision();
  }

  Ok(snapshot)
}

fn apply_facts(snapshot: &mut RunSnapshot, event_ids: &mut BTreeSet<EventId>, facts: &[RunFact]) -> Result<(), ReduceError> {
  let mut pending_starts = BTreeMap::<SpanId, (SpanStarted, usize)>::new();
  for (index, fact) in facts.iter().enumerate() {
    let RunFact::SpanStarted(started) = fact else {
      continue;
    };
    validate_span_links(started)?;
    if snapshot.spans.contains_key(&started.span_id()) || pending_starts.insert(started.span_id(), (started.clone(), index)).is_some() {
      return Err(ReduceError::DuplicateSpanStart);
    }
  }

  validate_pending_parentage(snapshot, &pending_starts)?;

  for (index, fact) in facts.iter().enumerate() {
    match fact {
      RunFact::SpanStarted(started) => {
        if let Some(parent_id) = started.parent_span_id() {
          let parent = snapshot.spans.get(&parent_id).ok_or(ReduceError::MissingLocalParent)?;
          if parent.ended().is_some() {
            return Err(ReduceError::ParentSpanEnded);
          }
          if started.started_at() < parent.started().started_at() {
            return Err(ReduceError::ChildBeforeParent);
          }
        }
        let (_, start_index) = pending_starts.get(&started.span_id()).expect("pending starts were collected from the same facts");
        debug_assert_eq!(*start_index, index);
        snapshot.spans.insert(started.span_id(), SpanSnapshot::new(started.clone(), None)?);
      }
      RunFact::SpanEnded(ended) => {
        let span = snapshot.spans.get_mut(&ended.span_id()).ok_or(ReduceError::UnknownSpanEnd)?;
        if span.ended.is_some() {
          return Err(ReduceError::DuplicateSpanEnd);
        }
        if ended.ended_at() < span.started.started_at() {
          return Err(ReduceError::EndBeforeStart);
        }
        if snapshot.events.iter().any(|event| event.span_id() == Some(ended.span_id()) && event.occurred_at() > ended.ended_at()) {
          return Err(ReduceError::EventAfterSpanEnd);
        }
        span.ended = Some(ended.clone());
      }
      RunFact::EventOccurred(event) => {
        if !event_ids.insert(event.event_id()) {
          return Err(ReduceError::DuplicateEventId);
        }
        if let Some(span_id) = event.span_id() {
          let span = snapshot.spans.get(&span_id).ok_or(ReduceError::UnknownEventSpan)?;
          if span.ended().is_some() {
            return Err(ReduceError::EventAfterSpanEnd);
          }
          if event.occurred_at() < span.started().started_at() {
            return Err(ReduceError::EventBeforeSpanStart);
          }
        }
        snapshot.events.push(event.clone());
      }
      RunFact::ArtifactPublished(artifact) => {
        if artifact.metadata().uri().run_id() != snapshot.run_id {
          return Err(ReduceError::ArtifactRunMismatch);
        }
        if artifact.span_id().is_some_and(|span_id| !snapshot.spans.contains_key(&span_id)) {
          return Err(ReduceError::UnknownArtifactSpan);
        }
        let uri = artifact.metadata().uri().clone();
        if snapshot.artifacts.insert(uri, artifact.clone()).is_some() {
          return Err(ReduceError::DuplicateArtifactUri);
        }
      }
    }
  }
  Ok(())
}

fn validate_span_links(started: &SpanStarted) -> Result<(), ReduceError> {
  if started.parent_span_id() == Some(started.span_id()) {
    return Err(ReduceError::SelfParent);
  }
  let remote_span_id = started.remote_link().map(SpanLink::span_id);
  if remote_span_id == Some(started.span_id()) {
    return Err(ReduceError::SelfRemoteLink);
  }
  if started.parent_span_id().is_some() && started.parent_span_id() == remote_span_id {
    return Err(ReduceError::DuplicateParentLink);
  }
  Ok(())
}

fn validate_pending_parentage(snapshot: &RunSnapshot, pending_starts: &BTreeMap<SpanId, (SpanStarted, usize)>) -> Result<(), ReduceError> {
  for (span_id, (started, _)) in pending_starts {
    if let Some(parent_id) = started.parent_span_id()
      && !snapshot.spans.contains_key(&parent_id)
      && !pending_starts.contains_key(&parent_id)
    {
      return Err(ReduceError::MissingLocalParent);
    }

    let mut seen = BTreeSet::new();
    let mut cursor = Some(*span_id);
    while let Some(current) = cursor {
      if !seen.insert(current) {
        return Err(ReduceError::CyclicLocalParent);
      }
      cursor = pending_starts
        .get(&current)
        .map(|(value, _)| value.parent_span_id())
        .unwrap_or_else(|| snapshot.spans.get(&current).and_then(|value| value.started().parent_span_id()));
    }
  }
  Ok(())
}

fn validate_snapshot(snapshot: &RunSnapshot) -> Result<(), ReduceError> {
  if snapshot.through_revision.get() == 0 {
    return Err(ReduceError::NonContiguousRevision);
  }

  let mut pending = BTreeMap::new();
  for (span_id, span) in &snapshot.spans {
    if *span_id != span.started().span_id() {
      return Err(ReduceError::SnapshotSpanKeyMismatch);
    }
    validate_span_links(span.started())?;
    pending.insert(*span_id, (span.started().clone(), 0));
  }
  validate_pending_parentage(snapshot, &pending)?;

  let mut event_ids = BTreeSet::new();
  for event in &snapshot.events {
    if !event_ids.insert(event.event_id()) {
      return Err(ReduceError::DuplicateEventId);
    }
    if let Some(span_id) = event.span_id() {
      let span = snapshot.spans.get(&span_id).ok_or(ReduceError::UnknownEventSpan)?;
      if event.occurred_at() < span.started().started_at() {
        return Err(ReduceError::EventBeforeSpanStart);
      }
      if span.ended().is_some_and(|ended| event.occurred_at() > ended.ended_at()) {
        return Err(ReduceError::EventAfterSpanEnd);
      }
    }
  }

  for (uri, artifact) in &snapshot.artifacts {
    if uri != artifact.metadata().uri() {
      return Err(ReduceError::SnapshotArtifactKeyMismatch);
    }
    if uri.run_id() != snapshot.run_id {
      return Err(ReduceError::ArtifactRunMismatch);
    }
    if artifact.span_id().is_some_and(|span_id| !snapshot.spans.contains_key(&span_id)) {
      return Err(ReduceError::UnknownArtifactSpan);
    }
  }

  Ok(())
}
