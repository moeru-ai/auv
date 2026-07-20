use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
  ArtifactPurpose, ArtifactUri, Attributes, AuthorityId, ByteLength, ContentType, EventId, EventSchema, IdempotencyKey, JsonPayload,
  NonEmptyVec, RunId, RunRevision, Sha256Digest, SpanId, SpanName, Timestamp, ValidationError,
};

const MAX_COMMIT_ITEMS: usize = 256;

/// A propagated span identity that correlates a new local root with remote work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// The immutable start fact for one named span.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

/// The timestamp-only finish fact for one span.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// One immutable typed point event in a run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// Durable metadata for bytes published at one canonical artifact URI.
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

/// The committed publication fact for one artifact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
  fn new(started: SpanStarted, ended: Option<SpanEnded>) -> Result<Self, SnapshotValidationError> {
    validate_span_links(&started)?;
    if let Some(ended) = &ended {
      if ended.span_id() != started.span_id() {
        return Err(SnapshotValidationError::MismatchedSpanEnd);
      }
      if ended.ended_at() < started.started_at() {
        return Err(ReduceError::EndBeforeStart.into());
      }
    }
    Ok(Self { started, ended })
  }

  /// Returns the immutable span start fact.
  pub fn started(&self) -> &SpanStarted {
    &self.started
  }

  /// Returns the optional span finish fact without inferring a reason when absent.
  pub fn ended(&self) -> Option<&SpanEnded> {
    self.ended.as_ref()
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
      #[serde(deserialize_with = "deserialize_unique_btree_map")]
      spans: BTreeMap<SpanId, SpanSnapshot>,
      events: Vec<EventOccurred>,
      #[serde(deserialize_with = "deserialize_unique_btree_map")]
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

fn deserialize_unique_btree_map<'de, D, K, V>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
  D: Deserializer<'de>,
  K: Deserialize<'de> + Ord,
  V: Deserialize<'de>,
{
  struct UniqueMapVisitor<K, V>(PhantomData<(K, V)>);

  impl<'de, K, V> Visitor<'de> for UniqueMapVisitor<K, V>
  where
    K: Deserialize<'de> + Ord,
    V: Deserialize<'de>,
  {
    type Value = BTreeMap<K, V>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
      formatter.write_str("a map with unique typed keys")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
      A: MapAccess<'de>,
    {
      let mut values = BTreeMap::new();
      while let Some(key) = map.next_key()? {
        if values.contains_key(&key) {
          return Err(de::Error::custom("map contains a duplicate typed key"));
        }
        values.insert(key, map.next_value()?);
      }
      Ok(values)
    }
  }

  deserializer.deserialize_map(UniqueMapVisitor(PhantomData))
}

#[derive(Debug, thiserror::Error)]
enum SnapshotValidationError {
  #[error(transparent)]
  History(#[from] ReduceError),
  #[error("snapshot revision is infeasible for its materialized fact count")]
  InfeasibleRevision,
  #[error("span snapshot contains a mismatched end")]
  MismatchedSpanEnd,
  #[error("snapshot span key does not match its span identity")]
  SpanKeyMismatch,
  #[error("snapshot artifact key does not match its artifact URI")]
  ArtifactKeyMismatch,
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
  /// A child start falls after its local parent's finish time or committed end.
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
}

#[derive(Default)]
struct ReducerIndexes {
  max_event_at_by_span: BTreeMap<SpanId, Timestamp>,
  max_child_start_at_by_parent: BTreeMap<SpanId, Timestamp>,
  event_ids: BTreeSet<EventId>,
}

fn record_max_timestamp(index: &mut BTreeMap<SpanId, Timestamp>, span_id: SpanId, timestamp: Timestamp) {
  index.entry(span_id).and_modify(|current| *current = (*current).max(timestamp)).or_insert(timestamp);
}

/// Incremental form of the canonical reducer used by stores that retain a
/// snapshot independently from their readable commit window.
pub(crate) struct IncrementalReducer {
  snapshot: RunSnapshot,
  indexes: ReducerIndexes,
}

impl IncrementalReducer {
  pub(crate) fn new(authority_id: AuthorityId, run_id: RunId) -> Self {
    Self {
      snapshot: RunSnapshot {
        authority_id,
        run_id,
        through_revision: RunRevision::new(0).expect("revision zero is the valid pre-history cursor"),
        spans: BTreeMap::new(),
        events: Vec::new(),
        artifacts: BTreeMap::new(),
      },
      indexes: ReducerIndexes::default(),
    }
  }

  pub(crate) fn apply(&mut self, commit: &RunCommit) -> Result<(), ReduceError> {
    self.validate_header(commit)?;
    let delta = validate_facts(&self.snapshot, &self.indexes, commit.facts())?;
    delta.apply(&mut self.snapshot, &mut self.indexes);
    self.snapshot.through_revision = commit.revision();
    Ok(())
  }

  #[cfg(feature = "memory-store")]
  pub(crate) fn validate(&self, commit: &RunCommit) -> Result<(), ReduceError> {
    self.validate_header(commit)?;
    validate_facts(&self.snapshot, &self.indexes, commit.facts())?;
    Ok(())
  }

  fn validate_header(&self, commit: &RunCommit) -> Result<(), ReduceError> {
    if self.snapshot.through_revision.get().checked_add(1) != Some(commit.revision().get()) {
      return Err(ReduceError::NonContiguousRevision);
    }
    if commit.authority_id() != self.snapshot.authority_id {
      return Err(ReduceError::MixedAuthority);
    }
    if commit.run_id() != self.snapshot.run_id {
      return Err(ReduceError::MixedRun);
    }
    Ok(())
  }

  #[cfg(any(feature = "memory-store", test))]
  pub(crate) fn snapshot(&self) -> &RunSnapshot {
    &self.snapshot
  }
}

/// Replays a complete canonical commit sequence into one deterministic snapshot.
pub fn reduce_commits(commits: &[RunCommit]) -> Result<RunSnapshot, ReduceError> {
  let Some(first) = commits.first() else {
    return Err(ReduceError::EmptyHistory);
  };
  let mut reducer = IncrementalReducer::new(first.authority_id(), first.run_id());
  for commit in commits {
    reducer.apply(commit)?;
  }
  Ok(reducer.snapshot)
}

#[derive(Default)]
struct FactDelta {
  spans: BTreeMap<SpanId, SpanSnapshot>,
  events: Vec<EventOccurred>,
  artifacts: BTreeMap<ArtifactUri, ArtifactPublished>,
  max_event_at_by_span: BTreeMap<SpanId, Timestamp>,
  max_child_start_at_by_parent: BTreeMap<SpanId, Timestamp>,
  event_ids: BTreeSet<EventId>,
}

impl FactDelta {
  fn span<'a>(&'a self, snapshot: &'a RunSnapshot, span_id: &SpanId) -> Option<&'a SpanSnapshot> {
    self.spans.get(span_id).or_else(|| snapshot.spans.get(span_id))
  }

  fn apply(self, snapshot: &mut RunSnapshot, indexes: &mut ReducerIndexes) {
    snapshot.spans.extend(self.spans);
    snapshot.events.extend(self.events);
    snapshot.artifacts.extend(self.artifacts);
    for (span_id, timestamp) in self.max_event_at_by_span {
      record_max_timestamp(&mut indexes.max_event_at_by_span, span_id, timestamp);
    }
    for (span_id, timestamp) in self.max_child_start_at_by_parent {
      record_max_timestamp(&mut indexes.max_child_start_at_by_parent, span_id, timestamp);
    }
    indexes.event_ids.extend(self.event_ids);
  }
}

fn validate_facts(snapshot: &RunSnapshot, indexes: &ReducerIndexes, facts: &[RunFact]) -> Result<FactDelta, ReduceError> {
  let mut pending_starts = BTreeMap::<SpanId, (SpanStarted, usize)>::new();
  let mut pending_order = Vec::new();
  for (index, fact) in facts.iter().enumerate() {
    let RunFact::SpanStarted(started) = fact else {
      continue;
    };
    validate_span_links(started)?;
    if snapshot.spans.contains_key(&started.span_id()) || pending_starts.contains_key(&started.span_id()) {
      return Err(ReduceError::DuplicateSpanStart);
    }
    pending_starts.insert(started.span_id(), (started.clone(), index));
    pending_order.push(started.span_id());
  }

  // Commit-level graph validity is preflighted before fact-order visibility,
  // so a forward-reference cycle remains a cycle error.
  validate_pending_parentage(snapshot, &pending_starts, &pending_order)?;

  let mut delta = FactDelta::default();
  for (index, fact) in facts.iter().enumerate() {
    match fact {
      RunFact::SpanStarted(started) => {
        if let Some(parent_id) = started.parent_span_id() {
          let parent = delta.span(snapshot, &parent_id).ok_or(ReduceError::MissingLocalParent)?;
          if parent.ended().is_some() {
            return Err(ReduceError::ParentSpanEnded);
          }
          if started.started_at() < parent.started().started_at() {
            return Err(ReduceError::ChildBeforeParent);
          }
          record_max_timestamp(&mut delta.max_child_start_at_by_parent, parent_id, started.started_at());
        }
        let (_, start_index) = pending_starts.get(&started.span_id()).expect("pending starts were collected from the same facts");
        debug_assert_eq!(*start_index, index);
        delta.spans.insert(
          started.span_id(),
          SpanSnapshot {
            started: started.clone(),
            ended: None,
          },
        );
      }
      RunFact::SpanEnded(ended) => {
        let mut span = delta.span(snapshot, &ended.span_id()).cloned().ok_or(ReduceError::UnknownSpanEnd)?;
        if span.ended.is_some() {
          return Err(ReduceError::DuplicateSpanEnd);
        }
        if ended.ended_at() < span.started.started_at() {
          return Err(ReduceError::EndBeforeStart);
        }
        if max_timestamp(indexes.max_event_at_by_span.get(&ended.span_id()), delta.max_event_at_by_span.get(&ended.span_id()))
          .is_some_and(|occurred_at| occurred_at > ended.ended_at())
        {
          return Err(ReduceError::EventAfterSpanEnd);
        }
        if max_timestamp(
          indexes.max_child_start_at_by_parent.get(&ended.span_id()),
          delta.max_child_start_at_by_parent.get(&ended.span_id()),
        )
        .is_some_and(|started_at| started_at > ended.ended_at())
        {
          return Err(ReduceError::ParentSpanEnded);
        }
        span.ended = Some(ended.clone());
        delta.spans.insert(ended.span_id(), span);
      }
      RunFact::EventOccurred(event) => {
        if indexes.event_ids.contains(&event.event_id()) || !delta.event_ids.insert(event.event_id()) {
          return Err(ReduceError::DuplicateEventId);
        }
        if let Some(span_id) = event.span_id() {
          let span = delta.span(snapshot, &span_id).ok_or(ReduceError::UnknownEventSpan)?;
          if span.ended().is_some() {
            return Err(ReduceError::EventAfterSpanEnd);
          }
          if event.occurred_at() < span.started().started_at() {
            return Err(ReduceError::EventBeforeSpanStart);
          }
          record_max_timestamp(&mut delta.max_event_at_by_span, span_id, event.occurred_at());
        }
        delta.events.push(event.clone());
      }
      RunFact::ArtifactPublished(artifact) => {
        if artifact.metadata().uri().run_id() != snapshot.run_id {
          return Err(ReduceError::ArtifactRunMismatch);
        }
        if artifact.span_id().is_some_and(|span_id| delta.span(snapshot, &span_id).is_none()) {
          return Err(ReduceError::UnknownArtifactSpan);
        }
        let uri = artifact.metadata().uri().clone();
        if snapshot.artifacts.contains_key(&uri) || delta.artifacts.insert(uri, artifact.clone()).is_some() {
          return Err(ReduceError::DuplicateArtifactUri);
        }
      }
    }
  }
  Ok(delta)
}

fn max_timestamp(first: Option<&Timestamp>, second: Option<&Timestamp>) -> Option<Timestamp> {
  match (first, second) {
    (Some(first), Some(second)) => Some((*first).max(*second)),
    (Some(timestamp), None) | (None, Some(timestamp)) => Some(*timestamp),
    (None, None) => None,
  }
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState {
  Visiting,
  Done,
}

// Iterative path marking completes each supplied graph node once.
fn validate_pending_parentage(
  snapshot: &RunSnapshot,
  pending_starts: &BTreeMap<SpanId, (SpanStarted, usize)>,
  pending_order: &[SpanId],
) -> Result<(), ReduceError> {
  let mut states = BTreeMap::<SpanId, VisitState>::new();
  for root in pending_order {
    if states.get(root) == Some(&VisitState::Done) {
      continue;
    }

    let mut path = Vec::new();
    let mut current = *root;
    loop {
      match states.get(&current) {
        Some(VisitState::Done) => break,
        Some(VisitState::Visiting) => return Err(ReduceError::CyclicLocalParent),
        None => {
          states.insert(current, VisitState::Visiting);
          path.push(current);
        }
      }

      let parent_id = pending_starts.get(&current).expect("the traversal contains only pending spans").0.parent_span_id();
      match parent_id {
        None => break,
        Some(parent_id) if pending_starts.contains_key(&parent_id) => current = parent_id,
        Some(parent_id) if snapshot.spans.contains_key(&parent_id) => break,
        Some(_) => return Err(ReduceError::MissingLocalParent),
      }
    }

    for span_id in path {
      states.insert(span_id, VisitState::Done);
    }
  }
  Ok(())
}

fn validate_snapshot(snapshot: &RunSnapshot) -> Result<(), SnapshotValidationError> {
  if snapshot.through_revision.get() == 0 {
    return Err(ReduceError::NonContiguousRevision.into());
  }
  let fact_count = snapshot
    .spans
    .len()
    .checked_add(snapshot.spans.values().filter(|span| span.ended().is_some()).count())
    .and_then(|count| count.checked_add(snapshot.events.len()))
    .and_then(|count| count.checked_add(snapshot.artifacts.len()))
    .ok_or(SnapshotValidationError::InfeasibleRevision)?;
  let fact_count = u64::try_from(fact_count).map_err(|_| SnapshotValidationError::InfeasibleRevision)?;
  let minimum_revision = fact_count.div_ceil(MAX_COMMIT_ITEMS as u64);
  if fact_count == 0 || snapshot.through_revision.get() < minimum_revision || snapshot.through_revision.get() > fact_count {
    return Err(SnapshotValidationError::InfeasibleRevision);
  }

  let mut pending = BTreeMap::new();
  let mut pending_order = Vec::new();
  for (span_id, span) in &snapshot.spans {
    if *span_id != span.started().span_id() {
      return Err(SnapshotValidationError::SpanKeyMismatch);
    }
    validate_span_links(span.started())?;
    pending.insert(*span_id, (span.started().clone(), 0));
    pending_order.push(*span_id);
  }
  validate_pending_parentage(snapshot, &pending, &pending_order)?;
  for span in snapshot.spans.values() {
    if let Some(parent_id) = span.started().parent_span_id() {
      let parent = snapshot.spans.get(&parent_id).ok_or(ReduceError::MissingLocalParent)?;
      if span.started().started_at() < parent.started().started_at() {
        return Err(ReduceError::ChildBeforeParent.into());
      }
      if parent.ended().is_some_and(|ended| span.started().started_at() > ended.ended_at()) {
        return Err(ReduceError::ParentSpanEnded.into());
      }
    }
  }

  let mut event_ids = BTreeSet::new();
  for event in &snapshot.events {
    if !event_ids.insert(event.event_id()) {
      return Err(ReduceError::DuplicateEventId.into());
    }
    if let Some(span_id) = event.span_id() {
      let span = snapshot.spans.get(&span_id).ok_or(ReduceError::UnknownEventSpan)?;
      if event.occurred_at() < span.started().started_at() {
        return Err(ReduceError::EventBeforeSpanStart.into());
      }
      if span.ended().is_some_and(|ended| event.occurred_at() > ended.ended_at()) {
        return Err(ReduceError::EventAfterSpanEnd.into());
      }
    }
  }

  for (uri, artifact) in &snapshot.artifacts {
    if uri != artifact.metadata().uri() {
      return Err(SnapshotValidationError::ArtifactKeyMismatch);
    }
    if uri.run_id() != snapshot.run_id {
      return Err(ReduceError::ArtifactRunMismatch.into());
    }
    if artifact.span_id().is_some_and(|span_id| !snapshot.spans.contains_key(&span_id)) {
      return Err(ReduceError::UnknownArtifactSpan.into());
    }
  }

  Ok(())
}

#[cfg(test)]
mod incremental_tests {
  use super::*;
  use crate::{EventName, JsonPayload};

  #[test]
  fn incremental_reduction_matches_complete_history_replay() {
    let authority_id = AuthorityId::new();
    let run_id = RunId::new();
    let commits = vec![
      event_commit(authority_id, run_id, 1, "first"),
      event_commit(authority_id, run_id, 2, "second"),
    ];
    let expected = reduce_commits(&commits).expect("complete history is valid");

    let mut reducer = IncrementalReducer::new(authority_id, run_id);
    for commit in &commits {
      reducer.apply(commit).expect("each incremental commit is valid");
    }

    assert_eq!(reducer.snapshot(), &expected);
  }

  #[test]
  fn rejected_incremental_commit_leaves_snapshot_unchanged() {
    let authority_id = AuthorityId::new();
    let run_id = RunId::new();
    let mut reducer = IncrementalReducer::new(authority_id, run_id);
    reducer.apply(&event_commit(authority_id, run_id, 1, "first")).unwrap();
    let before = reducer.snapshot().clone();
    let event_id = EventId::new();
    let duplicate = RunCommit::new(
      authority_id,
      run_id,
      RunRevision::new(2).unwrap(),
      IdempotencyKey::new(),
      Timestamp::new(2, 0).unwrap(),
      vec![
        event_fact(event_id, 2, "accepted first"),
        event_fact(event_id, 2, "duplicate"),
      ],
    )
    .unwrap();

    assert_eq!(reducer.apply(&duplicate), Err(ReduceError::DuplicateEventId));
    assert_eq!(reducer.snapshot(), &before);
  }

  fn event_commit(authority_id: AuthorityId, run_id: RunId, revision: u64, value: &str) -> RunCommit {
    RunCommit::new(
      authority_id,
      run_id,
      RunRevision::new(revision).unwrap(),
      IdempotencyKey::new(),
      Timestamp::new(revision as i64, 0).unwrap(),
      vec![event_fact(EventId::new(), revision, value)],
    )
    .unwrap()
  }

  fn event_fact(event_id: EventId, revision: u64, value: &str) -> RunFact {
    let schema = EventSchema::new(EventName::parse("auv.test.incremental").unwrap(), 1).unwrap();
    RunFact::EventOccurred(EventOccurred::new(
      event_id,
      None,
      Timestamp::new(revision as i64, 0).unwrap(),
      schema,
      JsonPayload::encode(&serde_json::json!({ "value": value })).unwrap(),
    ))
  }
}
