//! Authoritative commit history and snapshot reduction.

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
  Artifact, ArtifactScope, Attributes, EncodeError, EncodedPayload, EventId, EventName, ExecutionId, ExecutionResult, IdempotencyKey,
  NonEmptyVec, OperationName, Revision, RunId, RunSealReason, Sha256Digest, SpanId, SpanName, Timestamp, VerificationRequest,
  VerificationResult, stable_json_bytes,
};

/// A non-empty idempotent proposal to append run changes.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunCommitRequest {
  run_id: RunId,
  expected_revision: Revision,
  idempotency_key: IdempotencyKey,
  changes: NonEmptyVec<RunChange>,
}

impl RunCommitRequest {
  pub fn new(
    run_id: RunId,
    expected_revision: Revision,
    idempotency_key: IdempotencyKey,
    changes: Vec<RunChange>,
  ) -> Result<Self, HistoryContractError> {
    let changes = NonEmptyVec::new(changes).map_err(|_| HistoryContractError::EmptyCommitRequest)?;
    Ok(Self {
      run_id,
      expected_revision,
      idempotency_key,
      changes,
    })
  }

  pub const fn run_id(&self) -> RunId {
    self.run_id
  }

  pub const fn expected_revision(&self) -> Revision {
    self.expected_revision
  }

  pub const fn idempotency_key(&self) -> IdempotencyKey {
    self.idempotency_key
  }

  pub fn changes(&self) -> &NonEmptyVec<RunChange> {
    &self.changes
  }

  // TODO(auv-run-v1-store): This becomes a live production call from the
  // idempotency table when the owner-approved Task 5 store slice lands.
  #[allow(dead_code)]
  fn without_expected_revision(&self) -> StableRunCommitRequest<'_> {
    StableRunCommitRequest {
      request_type: "run_commit",
      run_id: self.run_id,
      idempotency_key: self.idempotency_key,
      changes: &self.changes,
    }
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
      run_id: RunId,
      expected_revision: Revision,
      idempotency_key: IdempotencyKey,
      changes: Vec<RunChange>,
    }

    let wire = Wire::deserialize(deserializer)?;
    Self::new(wire.run_id, wire.expected_revision, wire.idempotency_key, wire.changes).map_err(de::Error::custom)
  }
}

// TODO(auv-run-v1-store): This wire view is consumed with request_fingerprint
// by the Task 5 idempotency table; keep it private until that call site lands.
#[allow(dead_code)]
#[derive(Serialize)]
struct StableRunCommitRequest<'a> {
  #[serde(rename = "type")]
  request_type: &'static str,
  run_id: RunId,
  idempotency_key: IdempotencyKey,
  changes: &'a NonEmptyVec<RunChange>,
}

/// Computes the stable idempotency fingerprint without the retry cursor.
// TODO(auv-run-v1-store): Remove the dead-code allowance when Task 5 wires
// this into the store's idempotency table.
#[allow(dead_code)]
pub(crate) fn request_fingerprint(request: &RunCommitRequest) -> Result<Sha256Digest, EncodeError> {
  let stable = stable_json_bytes(&request.without_expected_revision()).map_err(EncodeError::stable_json)?;
  Ok(Sha256Digest::of_bytes(&stable))
}

impl EncodeError {
  // TODO(auv-run-v1-store): This mapper becomes live with the Task 5
  // request_fingerprint call site.
  #[allow(dead_code)]
  fn stable_json(_error: serde_json::Error) -> Self {
    Self::new(crate::FailureCode::stable_json_failed())
  }
}

/// One authoritative accepted revision and its committed artifacts.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunCommit {
  run_id: RunId,
  revision: Revision,
  idempotency_key: IdempotencyKey,
  committed_at: Timestamp,
  changes: Vec<RunChange>,
  artifacts: Vec<Artifact>,
}

impl RunCommit {
  pub fn new(
    run_id: RunId,
    revision: Revision,
    idempotency_key: IdempotencyKey,
    committed_at: Timestamp,
    changes: Vec<RunChange>,
    artifacts: Vec<Artifact>,
  ) -> Result<Self, HistoryContractError> {
    if revision == Revision::ZERO {
      return Err(HistoryContractError::ZeroCommitRevision);
    }
    if changes.is_empty() && artifacts.is_empty() {
      return Err(HistoryContractError::EmptyCommit);
    }
    Ok(Self {
      run_id,
      revision,
      idempotency_key,
      committed_at,
      changes,
      artifacts,
    })
  }

  pub const fn run_id(&self) -> RunId {
    self.run_id
  }

  pub const fn revision(&self) -> Revision {
    self.revision
  }

  pub const fn idempotency_key(&self) -> IdempotencyKey {
    self.idempotency_key
  }

  pub const fn committed_at(&self) -> Timestamp {
    self.committed_at
  }

  pub fn changes(&self) -> &[RunChange] {
    &self.changes
  }

  pub fn artifacts(&self) -> &[Artifact] {
    &self.artifacts
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
      run_id: RunId,
      revision: Revision,
      idempotency_key: IdempotencyKey,
      committed_at: Timestamp,
      changes: Vec<RunChange>,
      artifacts: Vec<Artifact>,
    }

    let wire = Wire::deserialize(deserializer)?;
    Self::new(wire.run_id, wire.revision, wire.idempotency_key, wire.committed_at, wire.changes, wire.artifacts).map_err(de::Error::custom)
  }
}

/// One accepted fact in the canonical run history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunChange {
  RunOpened(RunOpened),
  ExecutionPrepared(ExecutionPrepared),
  ExecutionFinished(ExecutionFinished),
  SpanStarted(SpanStarted),
  SpanFinished(SpanFinished),
  EventAdded(EventAdded),
  RunSealed(RunSealed),
}

/// The first accepted fact for a run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunOpened {
  opened_at: Timestamp,
}

impl RunOpened {
  pub const fn new(opened_at: Timestamp) -> Self {
    Self { opened_at }
  }

  pub const fn opened_at(self) -> Timestamp {
    self.opened_at
  }
}

/// The durable intent to invoke one operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPrepared {
  execution_id: ExecutionId,
  operation: OperationName,
  input: EncodedPayload,
  verification_request: Option<VerificationRequest>,
  prepared_at: Timestamp,
}

impl ExecutionPrepared {
  pub const fn new(
    execution_id: ExecutionId,
    operation: OperationName,
    input: EncodedPayload,
    verification_request: Option<VerificationRequest>,
    prepared_at: Timestamp,
  ) -> Self {
    Self {
      execution_id,
      operation,
      input,
      verification_request,
      prepared_at,
    }
  }

  pub const fn execution_id(&self) -> ExecutionId {
    self.execution_id
  }

  pub fn operation(&self) -> &OperationName {
    &self.operation
  }

  pub fn input(&self) -> &EncodedPayload {
    &self.input
  }

  pub fn verification_request(&self) -> Option<&VerificationRequest> {
    self.verification_request.as_ref()
  }

  pub const fn prepared_at(&self) -> Timestamp {
    self.prepared_at
  }
}

/// The terminal result of one prepared execution.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionFinished {
  execution_id: ExecutionId,
  started_at: Timestamp,
  finished_at: Timestamp,
  result: ExecutionResult<EncodedPayload>,
  verification_result: Option<VerificationResult>,
}

impl ExecutionFinished {
  pub fn new(
    execution_id: ExecutionId,
    started_at: Timestamp,
    finished_at: Timestamp,
    result: ExecutionResult<EncodedPayload>,
    verification_result: Option<VerificationResult>,
  ) -> Result<Self, HistoryContractError> {
    if finished_at < started_at {
      return Err(HistoryContractError::ExecutionFinishedBeforeStarted);
    }
    Ok(Self {
      execution_id,
      started_at,
      finished_at,
      result,
      verification_result,
    })
  }

  pub const fn execution_id(&self) -> ExecutionId {
    self.execution_id
  }

  pub const fn started_at(&self) -> Timestamp {
    self.started_at
  }

  pub const fn finished_at(&self) -> Timestamp {
    self.finished_at
  }

  pub fn result(&self) -> &ExecutionResult<EncodedPayload> {
    &self.result
  }

  pub fn verification_result(&self) -> Option<&VerificationResult> {
    self.verification_result.as_ref()
  }
}

impl<'de> Deserialize<'de> for ExecutionFinished {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      execution_id: ExecutionId,
      started_at: Timestamp,
      finished_at: Timestamp,
      result: ExecutionResult<EncodedPayload>,
      verification_result: Option<VerificationResult>,
    }

    let wire = Wire::deserialize(deserializer)?;
    Self::new(wire.execution_id, wire.started_at, wire.finished_at, wire.result, wire.verification_result).map_err(de::Error::custom)
  }
}

/// The accepted start of a diagnostic span.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpanStarted {
  span_id: SpanId,
  execution_id: ExecutionId,
  parent_span_id: Option<SpanId>,
  name: SpanName,
  started_at: Timestamp,
  attributes: Attributes,
}

impl SpanStarted {
  pub const fn new(
    span_id: SpanId,
    execution_id: ExecutionId,
    parent_span_id: Option<SpanId>,
    name: SpanName,
    started_at: Timestamp,
    attributes: Attributes,
  ) -> Self {
    Self {
      span_id,
      execution_id,
      parent_span_id,
      name,
      started_at,
      attributes,
    }
  }

  pub const fn span_id(&self) -> SpanId {
    self.span_id
  }

  pub const fn execution_id(&self) -> ExecutionId {
    self.execution_id
  }

  pub const fn parent_span_id(&self) -> Option<SpanId> {
    self.parent_span_id
  }

  pub fn name(&self) -> &SpanName {
    &self.name
  }

  pub const fn started_at(&self) -> Timestamp {
    self.started_at
  }

  pub fn attributes(&self) -> &Attributes {
    &self.attributes
  }
}

/// The accepted finish of a previously started span.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpanFinished {
  span_id: SpanId,
  finished_at: Timestamp,
  attributes: Attributes,
}

impl SpanFinished {
  pub const fn new(span_id: SpanId, finished_at: Timestamp, attributes: Attributes) -> Self {
    Self {
      span_id,
      finished_at,
      attributes,
    }
  }

  pub const fn span_id(&self) -> SpanId {
    self.span_id
  }

  pub const fn finished_at(&self) -> Timestamp {
    self.finished_at
  }

  pub fn attributes(&self) -> &Attributes {
    &self.attributes
  }
}

/// One committed execution-scoped diagnostic event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventAdded {
  event_id: EventId,
  execution_id: ExecutionId,
  span_id: Option<SpanId>,
  name: EventName,
  occurred_at: Timestamp,
  attributes: Attributes,
}

impl EventAdded {
  pub const fn new(
    event_id: EventId,
    execution_id: ExecutionId,
    span_id: Option<SpanId>,
    name: EventName,
    occurred_at: Timestamp,
    attributes: Attributes,
  ) -> Self {
    Self {
      event_id,
      execution_id,
      span_id,
      name,
      occurred_at,
      attributes,
    }
  }

  pub const fn event_id(&self) -> EventId {
    self.event_id
  }

  pub const fn execution_id(&self) -> ExecutionId {
    self.execution_id
  }

  pub const fn span_id(&self) -> Option<SpanId> {
    self.span_id
  }

  pub fn name(&self) -> &EventName {
    &self.name
  }

  pub const fn occurred_at(&self) -> Timestamp {
    self.occurred_at
  }

  pub fn attributes(&self) -> &Attributes {
    &self.attributes
  }
}

/// The terminal accepted fact for a run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSealed {
  sealed_at: Timestamp,
  reason: RunSealReason,
}

impl RunSealed {
  pub const fn new(sealed_at: Timestamp, reason: RunSealReason) -> Self {
    Self { sealed_at, reason }
  }

  pub const fn sealed_at(&self) -> Timestamp {
    self.sealed_at
  }

  pub fn reason(&self) -> &RunSealReason {
    &self.reason
  }
}

/// A derived read model through one contiguous committed revision.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunSnapshot {
  run_id: RunId,
  through_revision: Revision,
  opened_at: Timestamp,
  status: RunStatus,
  executions: Vec<ExecutionSnapshot>,
  artifacts: Vec<Artifact>,
  spans: Vec<SpanSnapshot>,
  events: Vec<EventAdded>,
}

impl RunSnapshot {
  pub const fn run_id(&self) -> RunId {
    self.run_id
  }

  pub const fn through_revision(&self) -> Revision {
    self.through_revision
  }

  pub const fn opened_at(&self) -> Timestamp {
    self.opened_at
  }

  pub fn status(&self) -> &RunStatus {
    &self.status
  }

  pub fn executions(&self) -> &[ExecutionSnapshot] {
    &self.executions
  }

  pub fn artifacts(&self) -> &[Artifact] {
    &self.artifacts
  }

  pub fn spans(&self) -> &[SpanSnapshot] {
    &self.spans
  }

  pub fn events(&self) -> &[EventAdded] {
    &self.events
  }
}

/// The derived state of one prepared execution.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSnapshot {
  execution_id: ExecutionId,
  operation: OperationName,
  input: EncodedPayload,
  verification_request: Option<VerificationRequest>,
  state: ExecutionState,
}

impl ExecutionSnapshot {
  pub const fn execution_id(&self) -> ExecutionId {
    self.execution_id
  }

  pub fn operation(&self) -> &OperationName {
    &self.operation
  }

  pub fn input(&self) -> &EncodedPayload {
    &self.input
  }

  pub fn verification_request(&self) -> Option<&VerificationRequest> {
    self.verification_request.as_ref()
  }

  pub fn state(&self) -> &ExecutionState {
    &self.state
  }
}

/// Whether a prepared execution has a committed terminal result.
// NOTICE: The variants intentionally preserve the accepted public contract;
// boxing only the finished state would add an ownership distinction absent
// from the run model and complicate callers that pattern-match snapshots.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionState {
  Prepared {
    prepared_at: Timestamp,
  },
  Finished {
    prepared_at: Timestamp,
    started_at: Timestamp,
    finished_at: Timestamp,
    result: ExecutionResult<EncodedPayload>,
    verification_result: Option<VerificationResult>,
  },
}

impl ExecutionState {
  pub const fn prepared_at(&self) -> Timestamp {
    match self {
      Self::Prepared { prepared_at } | Self::Finished { prepared_at, .. } => *prepared_at,
    }
  }

  pub const fn finished_at(&self) -> Option<Timestamp> {
    match self {
      Self::Prepared { .. } => None,
      Self::Finished { finished_at, .. } => Some(*finished_at),
    }
  }
}

impl<'de> Deserialize<'de> for ExecutionState {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[allow(clippy::large_enum_variant)]
    #[derive(Deserialize)]
    #[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
    enum Wire {
      Prepared {
        prepared_at: Timestamp,
      },
      Finished {
        prepared_at: Timestamp,
        started_at: Timestamp,
        finished_at: Timestamp,
        result: ExecutionResult<EncodedPayload>,
        verification_result: Option<VerificationResult>,
      },
    }

    match Wire::deserialize(deserializer)? {
      Wire::Prepared { prepared_at } => Ok(Self::Prepared { prepared_at }),
      Wire::Finished {
        prepared_at,
        started_at,
        finished_at,
        result,
        verification_result,
      } => {
        if started_at < prepared_at {
          return Err(de::Error::custom(ReduceError::ExecutionStartedBeforePrepared));
        }
        if finished_at < started_at {
          return Err(de::Error::custom(HistoryContractError::ExecutionFinishedBeforeStarted));
        }
        Ok(Self::Finished {
          prepared_at,
          started_at,
          finished_at,
          result,
          verification_result,
        })
      }
    }
  }
}

/// Whether the canonical history accepts more facts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunStatus {
  Open,
  Sealed {
    sealed_at: Timestamp,
    reason: RunSealReason,
  },
}

impl<'de> Deserialize<'de> for RunStatus {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
    enum Wire {
      Open {},
      Sealed {
        sealed_at: Timestamp,
        reason: RunSealReason,
      },
    }

    Ok(match Wire::deserialize(deserializer)? {
      Wire::Open {} => Self::Open,
      Wire::Sealed { sealed_at, reason } => Self::Sealed { sealed_at, reason },
    })
  }
}

/// A derived span with its start metadata retained after finish.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpanSnapshot {
  span_id: SpanId,
  execution_id: ExecutionId,
  parent_span_id: Option<SpanId>,
  name: SpanName,
  state: SpanState,
}

impl SpanSnapshot {
  pub const fn span_id(&self) -> SpanId {
    self.span_id
  }

  pub const fn execution_id(&self) -> ExecutionId {
    self.execution_id
  }

  pub const fn parent_span_id(&self) -> Option<SpanId> {
    self.parent_span_id
  }

  pub fn name(&self) -> &SpanName {
    &self.name
  }

  pub fn state(&self) -> &SpanState {
    &self.state
  }
}

/// Whether a committed span has a finish fact.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum SpanState {
  Started {
    started_at: Timestamp,
    start_attributes: Attributes,
  },
  Finished {
    started_at: Timestamp,
    finished_at: Timestamp,
    start_attributes: Attributes,
    finish_attributes: Attributes,
  },
}

impl SpanState {
  pub const fn started_at(&self) -> Timestamp {
    match self {
      Self::Started { started_at, .. } | Self::Finished { started_at, .. } => *started_at,
    }
  }

  pub const fn finished_at(&self) -> Option<Timestamp> {
    match self {
      Self::Started { .. } => None,
      Self::Finished { finished_at, .. } => Some(*finished_at),
    }
  }
}

impl<'de> Deserialize<'de> for SpanState {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
    enum Wire {
      Started {
        started_at: Timestamp,
        start_attributes: Attributes,
      },
      Finished {
        started_at: Timestamp,
        finished_at: Timestamp,
        start_attributes: Attributes,
        finish_attributes: Attributes,
      },
    }

    match Wire::deserialize(deserializer)? {
      Wire::Started {
        started_at,
        start_attributes,
      } => Ok(Self::Started {
        started_at,
        start_attributes,
      }),
      Wire::Finished {
        started_at,
        finished_at,
        start_attributes,
        finish_attributes,
      } => {
        if finished_at < started_at {
          return Err(de::Error::custom(ReduceError::SpanFinishedBeforeStarted));
        }
        Ok(Self::Finished {
          started_at,
          finished_at,
          start_attributes,
          finish_attributes,
        })
      }
    }
  }
}

/// Stable local construction failures for canonical history values.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum HistoryContractError {
  #[error("auv.run.commit_request_empty")]
  EmptyCommitRequest,
  #[error("auv.run.commit_empty")]
  EmptyCommit,
  #[error("auv.run.commit_revision_zero")]
  ZeroCommitRevision,
  #[error("auv.run.execution_finished_before_started")]
  ExecutionFinishedBeforeStarted,
}

impl HistoryContractError {
  pub const fn code(self) -> &'static str {
    match self {
      Self::EmptyCommitRequest => "auv.run.commit_request_empty",
      Self::EmptyCommit => "auv.run.commit_empty",
      Self::ZeroCommitRevision => "auv.run.commit_revision_zero",
      Self::ExecutionFinishedBeforeStarted => "auv.run.execution_finished_before_started",
    }
  }
}

/// Stable validation failures produced while reducing canonical commits.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ReduceError {
  #[error("auv.run.empty_history")]
  EmptyHistory,
  #[error("auv.run.first_revision_not_open")]
  FirstRevisionNotOpen,
  #[error("auv.run.revision_not_contiguous")]
  RevisionNotContiguous,
  #[error("auv.run.commit_scope_mismatch")]
  CommitScopeMismatch,
  #[error("auv.run.already_opened")]
  AlreadyOpened,
  #[error("auv.run.execution_already_prepared")]
  ExecutionAlreadyPrepared,
  #[error("auv.run.execution_prepared_before_open")]
  ExecutionPreparedBeforeOpen,
  #[error("auv.run.execution_not_prepared")]
  ExecutionNotPrepared,
  #[error("auv.run.execution_already_finished")]
  ExecutionAlreadyFinished,
  #[error("auv.run.execution_started_before_prepared")]
  ExecutionStartedBeforePrepared,
  #[error("auv.run.execution_finished_before_scoped_fact")]
  ExecutionFinishedBeforeScopedFact,
  #[error("auv.run.verification_presence_mismatch")]
  VerificationPresenceMismatch,
  #[error("auv.run.verification_cardinality_mismatch")]
  VerificationCardinalityMismatch,
  #[error("auv.run.artifact_scope_mismatch")]
  ArtifactScopeMismatch,
  #[error("auv.run.artifact_already_committed")]
  ArtifactAlreadyCommitted,
  #[error("auv.run.artifact_execution_not_prepared")]
  ArtifactExecutionNotPrepared,
  #[error("auv.run.artifact_created_before_open")]
  ArtifactCreatedBeforeOpen,
  #[error("auv.run.artifact_created_before_execution")]
  ArtifactCreatedBeforeExecution,
  #[error("auv.run.span_execution_not_prepared")]
  SpanExecutionNotPrepared,
  #[error("auv.run.span_already_started")]
  SpanAlreadyStarted,
  #[error("auv.run.span_parent_not_started")]
  SpanParentNotStarted,
  #[error("auv.run.span_parent_scope_mismatch")]
  SpanParentScopeMismatch,
  #[error("auv.run.span_started_before_execution")]
  SpanStartedBeforeExecution,
  #[error("auv.run.span_started_before_parent")]
  SpanStartedBeforeParent,
  #[error("auv.run.span_started_after_execution")]
  SpanStartedAfterExecution,
  #[error("auv.run.span_not_started")]
  SpanNotStarted,
  #[error("auv.run.span_already_finished")]
  SpanAlreadyFinished,
  #[error("auv.run.span_finished_before_started")]
  SpanFinishedBeforeStarted,
  #[error("auv.run.span_finished_after_execution")]
  SpanFinishedAfterExecution,
  #[error("auv.run.span_finished_before_event")]
  SpanFinishedBeforeEvent,
  #[error("auv.run.event_execution_not_prepared")]
  EventExecutionNotPrepared,
  #[error("auv.run.event_already_added")]
  EventAlreadyAdded,
  #[error("auv.run.event_span_not_started")]
  EventSpanNotStarted,
  #[error("auv.run.event_span_scope_mismatch")]
  EventSpanScopeMismatch,
  #[error("auv.run.event_before_execution")]
  EventBeforeExecution,
  #[error("auv.run.event_before_span")]
  EventBeforeSpan,
  #[error("auv.run.event_after_span")]
  EventAfterSpan,
  #[error("auv.run.event_after_execution")]
  EventAfterExecution,
  #[error("auv.run.sealed_before_history")]
  SealedBeforeHistory,
  #[error("auv.run.sealed")]
  Sealed,
}

impl ReduceError {
  pub const fn code(self) -> &'static str {
    match self {
      Self::EmptyHistory => "auv.run.empty_history",
      Self::FirstRevisionNotOpen => "auv.run.first_revision_not_open",
      Self::RevisionNotContiguous => "auv.run.revision_not_contiguous",
      Self::CommitScopeMismatch => "auv.run.commit_scope_mismatch",
      Self::AlreadyOpened => "auv.run.already_opened",
      Self::ExecutionAlreadyPrepared => "auv.run.execution_already_prepared",
      Self::ExecutionPreparedBeforeOpen => "auv.run.execution_prepared_before_open",
      Self::ExecutionNotPrepared => "auv.run.execution_not_prepared",
      Self::ExecutionAlreadyFinished => "auv.run.execution_already_finished",
      Self::ExecutionStartedBeforePrepared => "auv.run.execution_started_before_prepared",
      Self::ExecutionFinishedBeforeScopedFact => "auv.run.execution_finished_before_scoped_fact",
      Self::VerificationPresenceMismatch => "auv.run.verification_presence_mismatch",
      Self::VerificationCardinalityMismatch => "auv.run.verification_cardinality_mismatch",
      Self::ArtifactScopeMismatch => "auv.run.artifact_scope_mismatch",
      Self::ArtifactAlreadyCommitted => "auv.run.artifact_already_committed",
      Self::ArtifactExecutionNotPrepared => "auv.run.artifact_execution_not_prepared",
      Self::ArtifactCreatedBeforeOpen => "auv.run.artifact_created_before_open",
      Self::ArtifactCreatedBeforeExecution => "auv.run.artifact_created_before_execution",
      Self::SpanExecutionNotPrepared => "auv.run.span_execution_not_prepared",
      Self::SpanAlreadyStarted => "auv.run.span_already_started",
      Self::SpanParentNotStarted => "auv.run.span_parent_not_started",
      Self::SpanParentScopeMismatch => "auv.run.span_parent_scope_mismatch",
      Self::SpanStartedBeforeExecution => "auv.run.span_started_before_execution",
      Self::SpanStartedBeforeParent => "auv.run.span_started_before_parent",
      Self::SpanStartedAfterExecution => "auv.run.span_started_after_execution",
      Self::SpanNotStarted => "auv.run.span_not_started",
      Self::SpanAlreadyFinished => "auv.run.span_already_finished",
      Self::SpanFinishedBeforeStarted => "auv.run.span_finished_before_started",
      Self::SpanFinishedAfterExecution => "auv.run.span_finished_after_execution",
      Self::SpanFinishedBeforeEvent => "auv.run.span_finished_before_event",
      Self::EventExecutionNotPrepared => "auv.run.event_execution_not_prepared",
      Self::EventAlreadyAdded => "auv.run.event_already_added",
      Self::EventSpanNotStarted => "auv.run.event_span_not_started",
      Self::EventSpanScopeMismatch => "auv.run.event_span_scope_mismatch",
      Self::EventBeforeExecution => "auv.run.event_before_execution",
      Self::EventBeforeSpan => "auv.run.event_before_span",
      Self::EventAfterSpan => "auv.run.event_after_span",
      Self::EventAfterExecution => "auv.run.event_after_execution",
      Self::SealedBeforeHistory => "auv.run.sealed_before_history",
      Self::Sealed => "auv.run.sealed",
    }
  }
}

/// Reduces a contiguous run history into its derived snapshot.
///
/// Each commit is applied to a clone and published only after every change and
/// artifact in that revision validates.
pub fn reduce_commits<'a>(commits: impl IntoIterator<Item = &'a RunCommit>) -> Result<RunSnapshot, ReduceError> {
  let mut commits = commits.into_iter();
  let first = commits.next().ok_or(ReduceError::EmptyHistory)?;
  if first.revision != Revision::new(1) || first.changes.len() != 1 || !matches!(first.changes.first(), Some(RunChange::RunOpened(_))) {
    return Err(ReduceError::FirstRevisionNotOpen);
  }

  let RunChange::RunOpened(opened) = &first.changes[0] else {
    return Err(ReduceError::FirstRevisionNotOpen);
  };
  let mut snapshot = RunSnapshot {
    run_id: first.run_id,
    through_revision: Revision::new(1),
    opened_at: opened.opened_at,
    status: RunStatus::Open,
    executions: Vec::new(),
    artifacts: Vec::new(),
    spans: Vec::new(),
    events: Vec::new(),
  };
  let mut first_candidate = snapshot.clone();
  for artifact in &first.artifacts {
    apply_artifact(&mut first_candidate, artifact)?;
  }
  snapshot = first_candidate;

  for commit in commits {
    if commit.run_id != snapshot.run_id {
      return Err(ReduceError::CommitScopeMismatch);
    }
    let expected_revision = snapshot.through_revision.next().map_err(|_| ReduceError::RevisionNotContiguous)?;
    if commit.revision != expected_revision {
      return Err(ReduceError::RevisionNotContiguous);
    }

    let mut candidate = snapshot.clone();
    for change in &commit.changes {
      apply_change(&mut candidate, change)?;
    }
    for artifact in &commit.artifacts {
      ensure_open(&candidate)?;
      apply_artifact(&mut candidate, artifact)?;
    }
    candidate.through_revision = commit.revision;
    snapshot = candidate;
  }

  Ok(snapshot)
}

fn ensure_open(snapshot: &RunSnapshot) -> Result<(), ReduceError> {
  if matches!(snapshot.status, RunStatus::Sealed { .. }) {
    return Err(ReduceError::Sealed);
  }
  Ok(())
}

fn apply_change(snapshot: &mut RunSnapshot, change: &RunChange) -> Result<(), ReduceError> {
  ensure_open(snapshot)?;
  match change {
    RunChange::RunOpened(_) => Err(ReduceError::AlreadyOpened),
    RunChange::ExecutionPrepared(prepared) => apply_execution_prepared(snapshot, prepared),
    RunChange::ExecutionFinished(finished) => apply_execution_finished(snapshot, finished),
    RunChange::SpanStarted(started) => apply_span_started(snapshot, started),
    RunChange::SpanFinished(finished) => apply_span_finished(snapshot, finished),
    RunChange::EventAdded(event) => apply_event(snapshot, event),
    RunChange::RunSealed(sealed) => {
      if sealed.sealed_at < latest_history_timestamp(snapshot) {
        return Err(ReduceError::SealedBeforeHistory);
      }
      snapshot.status = RunStatus::Sealed {
        sealed_at: sealed.sealed_at,
        reason: sealed.reason.clone(),
      };
      Ok(())
    }
  }
}

fn apply_execution_prepared(snapshot: &mut RunSnapshot, prepared: &ExecutionPrepared) -> Result<(), ReduceError> {
  if snapshot.executions.iter().any(|execution| execution.execution_id == prepared.execution_id) {
    return Err(ReduceError::ExecutionAlreadyPrepared);
  }
  if prepared.prepared_at < snapshot.opened_at {
    return Err(ReduceError::ExecutionPreparedBeforeOpen);
  }
  snapshot.executions.push(ExecutionSnapshot {
    execution_id: prepared.execution_id,
    operation: prepared.operation.clone(),
    input: prepared.input.clone(),
    verification_request: prepared.verification_request.clone(),
    state: ExecutionState::Prepared {
      prepared_at: prepared.prepared_at,
    },
  });
  Ok(())
}

fn apply_execution_finished(snapshot: &mut RunSnapshot, finished: &ExecutionFinished) -> Result<(), ReduceError> {
  let execution_index = snapshot
    .executions
    .iter()
    .position(|execution| execution.execution_id == finished.execution_id)
    .ok_or(ReduceError::ExecutionNotPrepared)?;
  let ExecutionState::Prepared { prepared_at } = &snapshot.executions[execution_index].state else {
    return Err(ReduceError::ExecutionAlreadyFinished);
  };
  let prepared_at = *prepared_at;
  if finished.started_at < prepared_at {
    return Err(ReduceError::ExecutionStartedBeforePrepared);
  }
  match (&snapshot.executions[execution_index].verification_request, &finished.verification_result) {
    (None, None) => {}
    (Some(request), Some(VerificationResult::Evaluated { outcomes })) => {
      if request.assertions().as_slice().len() != outcomes.as_slice().len() {
        return Err(ReduceError::VerificationCardinalityMismatch);
      }
    }
    (Some(_), Some(VerificationResult::EvaluationFailed { .. })) => {}
    _ => return Err(ReduceError::VerificationPresenceMismatch),
  }
  let span_outlives_execution = snapshot
    .spans
    .iter()
    .any(|span| span.execution_id == finished.execution_id && span_latest_timestamp(&span.state) > finished.finished_at);
  let event_outlives_execution =
    snapshot.events.iter().any(|event| event.execution_id == finished.execution_id && event.occurred_at > finished.finished_at);
  if span_outlives_execution || event_outlives_execution {
    return Err(ReduceError::ExecutionFinishedBeforeScopedFact);
  }
  snapshot.executions[execution_index].state = ExecutionState::Finished {
    prepared_at,
    started_at: finished.started_at,
    finished_at: finished.finished_at,
    result: finished.result.clone(),
    verification_result: finished.verification_result.clone(),
  };
  Ok(())
}

fn apply_artifact(snapshot: &mut RunSnapshot, artifact: &Artifact) -> Result<(), ReduceError> {
  if artifact.artifact_ref().run_id() != snapshot.run_id {
    return Err(ReduceError::ArtifactScopeMismatch);
  }
  if artifact.created_at() < snapshot.opened_at {
    return Err(ReduceError::ArtifactCreatedBeforeOpen);
  }
  if snapshot.artifacts.iter().any(|existing| existing.artifact_ref().artifact_id() == artifact.artifact_ref().artifact_id()) {
    return Err(ReduceError::ArtifactAlreadyCommitted);
  }
  let execution_id = match artifact.scope() {
    ArtifactScope::Run => None,
    ArtifactScope::Execution { execution_id } | ArtifactScope::Verification { execution_id } => Some(execution_id),
  };
  if let Some(execution_id) = execution_id {
    let execution = execution(snapshot, execution_id).ok_or(ReduceError::ArtifactExecutionNotPrepared)?;
    if artifact.created_at() < execution.state.prepared_at() {
      return Err(ReduceError::ArtifactCreatedBeforeExecution);
    }
  }
  snapshot.artifacts.push(artifact.clone());
  Ok(())
}

fn apply_span_started(snapshot: &mut RunSnapshot, started: &SpanStarted) -> Result<(), ReduceError> {
  let execution = execution(snapshot, started.execution_id).ok_or(ReduceError::SpanExecutionNotPrepared)?;
  if started.started_at < execution.state.prepared_at() {
    return Err(ReduceError::SpanStartedBeforeExecution);
  }
  if execution.state.finished_at().is_some_and(|finished_at| started.started_at > finished_at) {
    return Err(ReduceError::SpanStartedAfterExecution);
  }
  if snapshot.spans.iter().any(|span| span.span_id == started.span_id) {
    return Err(ReduceError::SpanAlreadyStarted);
  }
  if let Some(parent_span_id) = started.parent_span_id {
    let parent = snapshot.spans.iter().find(|span| span.span_id == parent_span_id).ok_or(ReduceError::SpanParentNotStarted)?;
    if parent.execution_id != started.execution_id {
      return Err(ReduceError::SpanParentScopeMismatch);
    }
    if started.started_at < parent.state.started_at() {
      return Err(ReduceError::SpanStartedBeforeParent);
    }
  }
  snapshot.spans.push(SpanSnapshot {
    span_id: started.span_id,
    execution_id: started.execution_id,
    parent_span_id: started.parent_span_id,
    name: started.name.clone(),
    state: SpanState::Started {
      started_at: started.started_at,
      start_attributes: started.attributes.clone(),
    },
  });
  Ok(())
}

fn apply_span_finished(snapshot: &mut RunSnapshot, finished: &SpanFinished) -> Result<(), ReduceError> {
  let span_index = snapshot.spans.iter().position(|span| span.span_id == finished.span_id).ok_or(ReduceError::SpanNotStarted)?;
  let SpanState::Started {
    started_at,
    start_attributes,
  } = &snapshot.spans[span_index].state
  else {
    return Err(ReduceError::SpanAlreadyFinished);
  };
  let started_at = *started_at;
  let start_attributes = start_attributes.clone();
  if finished.finished_at < started_at {
    return Err(ReduceError::SpanFinishedBeforeStarted);
  }
  let execution_id = snapshot.spans[span_index].execution_id;
  let execution = execution(snapshot, execution_id).ok_or(ReduceError::SpanExecutionNotPrepared)?;
  if execution.state.finished_at().is_some_and(|execution_finished_at| finished.finished_at > execution_finished_at) {
    return Err(ReduceError::SpanFinishedAfterExecution);
  }
  if snapshot.events.iter().any(|event| event.span_id == Some(finished.span_id) && event.occurred_at > finished.finished_at) {
    return Err(ReduceError::SpanFinishedBeforeEvent);
  }
  snapshot.spans[span_index].state = SpanState::Finished {
    started_at,
    finished_at: finished.finished_at,
    start_attributes,
    finish_attributes: finished.attributes.clone(),
  };
  Ok(())
}

fn apply_event(snapshot: &mut RunSnapshot, event: &EventAdded) -> Result<(), ReduceError> {
  let execution = execution(snapshot, event.execution_id).ok_or(ReduceError::EventExecutionNotPrepared)?;
  if event.occurred_at < execution.state.prepared_at() {
    return Err(ReduceError::EventBeforeExecution);
  }
  if execution.state.finished_at().is_some_and(|finished_at| event.occurred_at > finished_at) {
    return Err(ReduceError::EventAfterExecution);
  }
  if snapshot.events.iter().any(|existing| existing.event_id == event.event_id) {
    return Err(ReduceError::EventAlreadyAdded);
  }
  if let Some(span_id) = event.span_id {
    let span = snapshot.spans.iter().find(|span| span.span_id == span_id).ok_or(ReduceError::EventSpanNotStarted)?;
    if span.execution_id != event.execution_id {
      return Err(ReduceError::EventSpanScopeMismatch);
    }
    if event.occurred_at < span.state.started_at() {
      return Err(ReduceError::EventBeforeSpan);
    }
    if span.state.finished_at().is_some_and(|finished_at| event.occurred_at > finished_at) {
      return Err(ReduceError::EventAfterSpan);
    }
  }
  snapshot.events.push(event.clone());
  Ok(())
}

fn execution(snapshot: &RunSnapshot, execution_id: ExecutionId) -> Option<&ExecutionSnapshot> {
  snapshot.executions.iter().find(|execution| execution.execution_id == execution_id)
}

fn span_latest_timestamp(state: &SpanState) -> Timestamp {
  state.finished_at().unwrap_or_else(|| state.started_at())
}

fn latest_history_timestamp(snapshot: &RunSnapshot) -> Timestamp {
  let mut latest = snapshot.opened_at;
  for execution in &snapshot.executions {
    let timestamp = match &execution.state {
      ExecutionState::Prepared { prepared_at } => *prepared_at,
      ExecutionState::Finished { finished_at, .. } => *finished_at,
    };
    latest = latest.max(timestamp);
  }
  for artifact in &snapshot.artifacts {
    latest = latest.max(artifact.created_at());
  }
  for span in &snapshot.spans {
    let timestamp = match &span.state {
      SpanState::Started { started_at, .. } => *started_at,
      SpanState::Finished { finished_at, .. } => *finished_at,
    };
    latest = latest.max(timestamp);
  }
  for event in &snapshot.events {
    latest = latest.max(event.occurred_at);
  }
  latest
}

#[cfg(test)]
mod request_fingerprint_tests {
  use super::*;
  use crate::{
    EncodedPayload, ExecutionId, ExecutionPrepared, IdempotencyKey, OperationName, PayloadSchema, Revision, RunChange, RunCommitRequest,
    RunId, Sha256Digest, Timestamp, stable_json_bytes,
  };

  fn request_with_data(data: serde_json::Value) -> RunCommitRequest {
    RunCommitRequest::new(
      RunId::parse("018f0000-0000-7000-8000-000000000001").unwrap(),
      Revision::new(41),
      IdempotencyKey::parse("018f0000-0000-7000-8000-000000000002").unwrap(),
      vec![RunChange::ExecutionPrepared(ExecutionPrepared::new(
        ExecutionId::parse("018f0000-0000-7000-8000-000000000003").unwrap(),
        OperationName::parse("test.operation").unwrap(),
        EncodedPayload::new(PayloadSchema::parse("test.payload", 1).unwrap(), data).unwrap(),
        None,
        Timestamp::new(1_700_000_000, 0).unwrap(),
      ))],
    )
    .unwrap()
  }

  #[test]
  fn request_fingerprint_is_stable_across_map_insertion_order() {
    let mut first_nested = serde_json::Map::new();
    first_nested.insert("b".to_owned(), serde_json::json!(1));
    first_nested.insert("a".to_owned(), serde_json::json!(2));
    let mut first_data = serde_json::Map::new();
    first_data.insert("z".to_owned(), first_nested.into());
    first_data.insert("a".to_owned(), serde_json::json!(3));

    let mut second_nested = serde_json::Map::new();
    second_nested.insert("a".to_owned(), serde_json::json!(2));
    second_nested.insert("b".to_owned(), serde_json::json!(1));
    let mut second_data = serde_json::Map::new();
    second_data.insert("a".to_owned(), serde_json::json!(3));
    second_data.insert("z".to_owned(), second_nested.into());

    let first = request_with_data(first_data.into());
    let second = request_with_data(second_data.into());

    assert_eq!(
      stable_json_bytes(&first.without_expected_revision()).unwrap(),
      br#"{"changes":[{"execution_id":"018f0000-0000-7000-8000-000000000003","input":{"data":{"a":3,"z":{"a":2,"b":1}},"schema":{"name":"test.payload","version":1}},"operation":"test.operation","prepared_at":"2023-11-14T22:13:20Z","type":"execution_prepared","verification_request":null}],"idempotency_key":"018f0000-0000-7000-8000-000000000002","run_id":"018f0000-0000-7000-8000-000000000001","type":"run_commit"}"#
    );
    assert_eq!(request_fingerprint(&first).unwrap(), request_fingerprint(&second).unwrap());
    assert_eq!(
      request_fingerprint(&first).unwrap(),
      Sha256Digest::parse_hex("10650dc1b250403958f1c777f8ef3371c2cee90d2b7a5d51b7ec91711f0d6295").unwrap()
    );
  }

  #[test]
  fn request_fingerprint_excludes_only_expected_revision() {
    let first = request_with_data(serde_json::json!({ "value": 1 }));
    let retry =
      RunCommitRequest::new(first.run_id(), Revision::new(99), first.idempotency_key(), first.changes().as_slice().to_vec()).unwrap();

    assert_eq!(request_fingerprint(&first).unwrap(), request_fingerprint(&retry).unwrap());
  }

  #[test]
  fn stable_json_and_fingerprints_preserve_large_exact_numbers() {
    let raw_numbers = [
      "9007199254740992",
      "9007199254740993",
      "9223372036854775807",
    ];
    let mut stable_numbers = Vec::new();
    let mut fingerprints = Vec::new();

    for raw in raw_numbers {
      let number: serde_json::Value = serde_json::from_str(raw).unwrap();
      stable_numbers.push(stable_json_bytes(&number).unwrap());
      fingerprints.push(request_fingerprint(&request_with_data(number)).unwrap());
    }

    assert_eq!(stable_numbers[0], b"9007199254740992");
    assert_eq!(stable_numbers[1], b"9007199254740993");
    assert_eq!(stable_numbers[2], b"9223372036854775807");
    assert_ne!(fingerprints[0], fingerprints[1]);
    assert_ne!(fingerprints[1], fingerprints[2]);
    assert_ne!(fingerprints[0], fingerprints[2]);
  }

  #[test]
  fn stable_json_and_fingerprints_accept_distinct_large_exponents() {
    let first: serde_json::Value = serde_json::from_str("1e400").unwrap();
    let second: serde_json::Value = serde_json::from_str("2e400").unwrap();

    assert_eq!(stable_json_bytes(&first).unwrap(), b"1e+400");
    assert_eq!(stable_json_bytes(&second).unwrap(), b"2e+400");
    assert_ne!(request_fingerprint(&request_with_data(first)).unwrap(), request_fingerprint(&request_with_data(second)).unwrap());
  }
}
