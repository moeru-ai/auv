//! Operation execution lifecycle and verification records.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::{
  BoxFuture, CancellationReason, EncodedPayload, ExecutionContext, ExecutionId, FailureCode, IdempotencyKey, NonEmptyVec, OperationName,
  ReasonCode, RunId, Timestamp,
};

/// One invocation that started and reached a terminal execution result.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperationExecution<T> {
  run_id: RunId,
  execution_id: ExecutionId,
  operation: OperationName,
  started_at: Timestamp,
  finished_at: Timestamp,
  result: ExecutionResult<T>,
  verification: Option<Box<VerificationEvaluation>>,
}

impl<T> OperationExecution<T> {
  pub fn new(
    run_id: RunId,
    execution_id: ExecutionId,
    operation: OperationName,
    started_at: Timestamp,
    finished_at: Timestamp,
    result: ExecutionResult<T>,
    verification: Option<VerificationEvaluation>,
  ) -> Result<Self, ExecutionContractError> {
    if finished_at < started_at {
      return Err(ExecutionContractError::FinishedBeforeStarted);
    }
    Ok(Self {
      run_id,
      execution_id,
      operation,
      started_at,
      finished_at,
      result,
      verification: verification.map(Box::new),
    })
  }

  pub const fn run_id(&self) -> RunId {
    self.run_id
  }

  pub const fn execution_id(&self) -> ExecutionId {
    self.execution_id
  }

  pub fn operation(&self) -> &OperationName {
    &self.operation
  }

  pub const fn started_at(&self) -> Timestamp {
    self.started_at
  }

  pub const fn finished_at(&self) -> Timestamp {
    self.finished_at
  }

  pub fn result(&self) -> &ExecutionResult<T> {
    &self.result
  }

  pub fn verification(&self) -> Option<&VerificationEvaluation> {
    self.verification.as_deref()
  }
}

impl<'de, T> Deserialize<'de> for OperationExecution<T>
where
  T: Deserialize<'de>,
{
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire<T> {
      run_id: RunId,
      execution_id: ExecutionId,
      operation: OperationName,
      started_at: Timestamp,
      finished_at: Timestamp,
      result: ExecutionResult<T>,
      verification: Option<VerificationEvaluation>,
    }

    let wire = Wire::deserialize(deserializer)?;
    Self::new(wire.run_id, wire.execution_id, wire.operation, wire.started_at, wire.finished_at, wire.result, wire.verification)
      .map_err(de::Error::custom)
  }
}

/// Stable construction failures for operation execution records.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ExecutionContractError {
  #[error("auv.execution.finished_before_started")]
  FinishedBeforeStarted,
}

impl ExecutionContractError {
  pub const fn code(self) -> &'static str {
    match self {
      Self::FinishedBeforeStarted => "auv.execution.finished_before_started",
    }
  }
}

/// The local result of calling an operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionResult<T> {
  Completed { output: T },
  Failed { failure: OperationFailure },
  Cancelled { reason: CancellationReason },
}

/// A bounded operation failure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationFailure {
  code: FailureCode,
  details: Option<EncodedPayload>,
}

impl OperationFailure {
  pub const fn new(code: FailureCode, details: Option<EncodedPayload>) -> Self {
    Self { code, details }
  }

  pub fn code(&self) -> &FailureCode {
    &self.code
  }

  pub fn details(&self) -> Option<&EncodedPayload> {
    self.details.as_ref()
  }
}

/// A resolved verification request paired with its result.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationEvaluation {
  request: VerificationRequest,
  result: VerificationResult,
}

impl VerificationEvaluation {
  pub fn new(request: VerificationRequest, result: VerificationResult) -> Result<Self, VerificationContractError> {
    if let VerificationResult::Evaluated { outcomes } = &result
      && request.assertions.as_slice().len() != outcomes.as_slice().len()
    {
      return Err(VerificationContractError::OutcomeCardinalityMismatch);
    }
    Ok(Self { request, result })
  }

  pub fn request(&self) -> &VerificationRequest {
    &self.request
  }

  pub fn result(&self) -> &VerificationResult {
    &self.result
  }

  /// Returns the highest-priority non-passing required assertion outcome.
  ///
  /// `Ok(None)` means all required assertions passed. A verifier-wide failure
  /// is returned separately because it has no per-assertion outcomes.
  pub fn required_outcome(&self) -> Result<Option<&AssertionOutcome>, &VerificationFailure> {
    match &self.result {
      VerificationResult::Evaluated { outcomes } => {
        let required = self.request.assertions.as_slice().iter().zip(outcomes.as_slice()).filter(|(assertion, _)| assertion.required);

        for priority in [
          OutcomePriority::Failed,
          OutcomePriority::Error,
          OutcomePriority::Inconclusive,
        ] {
          if let Some((_, outcome)) = required.clone().find(|(_, outcome)| priority.matches(outcome)) {
            return Ok(Some(outcome));
          }
        }
        Ok(None)
      }
      VerificationResult::EvaluationFailed { failure } => Err(failure),
    }
  }
}

impl<'de> Deserialize<'de> for VerificationEvaluation {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      request: VerificationRequest,
      result: VerificationResult,
    }

    let wire = Wire::deserialize(deserializer)?;
    Self::new(wire.request, wire.result).map_err(de::Error::custom)
  }
}

#[derive(Clone, Copy)]
enum OutcomePriority {
  Failed,
  Error,
  Inconclusive,
}

impl OutcomePriority {
  fn matches(self, outcome: &AssertionOutcome) -> bool {
    matches!(
      (self, outcome),
      (Self::Failed, AssertionOutcome::Failed { .. })
        | (Self::Error, AssertionOutcome::Error { .. })
        | (Self::Inconclusive, AssertionOutcome::Inconclusive { .. })
    )
  }
}

/// An ordered, non-empty set of verification assertions.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationRequest {
  assertions: NonEmptyVec<VerificationAssertion>,
}

impl VerificationRequest {
  pub fn new(assertions: Vec<VerificationAssertion>) -> Result<Self, VerificationContractError> {
    let assertions = NonEmptyVec::new(assertions).map_err(|_| VerificationContractError::EmptyAssertions)?;
    if !assertions.as_slice().iter().any(|assertion| assertion.required) {
      return Err(VerificationContractError::NoRequiredAssertion);
    }
    Ok(Self { assertions })
  }

  pub fn assertions(&self) -> &NonEmptyVec<VerificationAssertion> {
    &self.assertions
  }
}

impl<'de> Deserialize<'de> for VerificationRequest {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      assertions: Vec<VerificationAssertion>,
    }

    Self::new(Wire::deserialize(deserializer)?.assertions).map_err(de::Error::custom)
  }
}

/// One schema-bound assertion in a verification request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationAssertion {
  required: bool,
  assertion: EncodedPayload,
}

impl VerificationAssertion {
  pub const fn new(required: bool, assertion: EncodedPayload) -> Self {
    Self {
      required,
      assertion,
    }
  }

  pub const fn required(&self) -> bool {
    self.required
  }

  pub fn assertion(&self) -> &EncodedPayload {
    &self.assertion
  }
}

/// The result of evaluating a verification request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum VerificationResult {
  Evaluated {
    outcomes: NonEmptyVec<AssertionOutcome>,
  },
  EvaluationFailed {
    failure: VerificationFailure,
  },
}

impl VerificationResult {
  /// Constructs a non-empty evaluated result.
  ///
  /// Request/result cardinality remains validated by `VerificationEvaluation`.
  pub fn evaluated(outcomes: Vec<AssertionOutcome>) -> Result<Self, VerificationContractError> {
    let outcomes = NonEmptyVec::new(outcomes).map_err(|_| VerificationContractError::EmptyOutcomes)?;
    Ok(Self::Evaluated { outcomes })
  }

  pub const fn evaluation_failed(failure: VerificationFailure) -> Self {
    Self::EvaluationFailed { failure }
  }
}

/// One assertion's verification outcome.
#[derive(Clone, Debug, PartialEq)]
pub enum AssertionOutcome {
  Passed,
  Failed { reason: ReasonCode },
  Inconclusive { reason: ReasonCode },
  Error { failure: VerificationFailure },
}

// Unit variants use struct-shaped wire variants so unknown fields are rejected
// instead of silently discarded by Serde.
#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum AssertionOutcomeWire<R, F> {
  Passed {},
  Failed { reason: R },
  Inconclusive { reason: R },
  Error { failure: F },
}

impl<'a> From<&'a AssertionOutcome> for AssertionOutcomeWire<&'a ReasonCode, &'a VerificationFailure> {
  fn from(outcome: &'a AssertionOutcome) -> Self {
    match outcome {
      AssertionOutcome::Passed => Self::Passed {},
      AssertionOutcome::Failed { reason } => Self::Failed { reason },
      AssertionOutcome::Inconclusive { reason } => Self::Inconclusive { reason },
      AssertionOutcome::Error { failure } => Self::Error { failure },
    }
  }
}

impl From<AssertionOutcomeWire<ReasonCode, VerificationFailure>> for AssertionOutcome {
  fn from(outcome: AssertionOutcomeWire<ReasonCode, VerificationFailure>) -> Self {
    match outcome {
      AssertionOutcomeWire::Passed {} => Self::Passed,
      AssertionOutcomeWire::Failed { reason } => Self::Failed { reason },
      AssertionOutcomeWire::Inconclusive { reason } => Self::Inconclusive { reason },
      AssertionOutcomeWire::Error { failure } => Self::Error { failure },
    }
  }
}

impl Serialize for AssertionOutcome {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    let wire: AssertionOutcomeWire<&ReasonCode, &VerificationFailure> = self.into();
    wire.serialize(serializer)
  }
}

impl<'de> Deserialize<'de> for AssertionOutcome {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    Ok(AssertionOutcomeWire::<ReasonCode, VerificationFailure>::deserialize(deserializer)?.into())
  }
}

/// A bounded verification failure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationFailure {
  code: FailureCode,
  details: Option<EncodedPayload>,
}

impl VerificationFailure {
  pub const fn new(code: FailureCode, details: Option<EncodedPayload>) -> Self {
    Self { code, details }
  }

  pub fn code(&self) -> &FailureCode {
    &self.code
  }

  pub fn details(&self) -> Option<&EncodedPayload> {
    self.details.as_ref()
  }
}

/// A synchronous post-operation verifier.
pub trait VerificationEvaluator: Send + Sync {
  fn request(&self) -> &VerificationRequest;
  fn evaluate<'a>(&'a self, context: &'a ExecutionContext) -> BoxFuture<'a, VerificationResult>;
}

/// A failure that prevents an operation from starting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StartFailure {
  InvalidRequest { code: FailureCode },
  OperationUnavailable { code: FailureCode },
  Persistence { failure: PersistenceFailure },
}

/// A bounded authority persistence failure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistenceFailure {
  code: FailureCode,
  request_key: IdempotencyKey,
  details: Option<EncodedPayload>,
}

impl PersistenceFailure {
  pub const fn new(code: FailureCode, request_key: IdempotencyKey, details: Option<EncodedPayload>) -> Self {
    Self {
      code,
      request_key,
      details,
    }
  }

  pub fn code(&self) -> &FailureCode {
    &self.code
  }

  pub const fn request_key(&self) -> IdempotencyKey {
    self.request_key
  }

  pub fn details(&self) -> Option<&EncodedPayload> {
    self.details.as_ref()
  }
}

/// The result of attempting to invoke an operation through the runtime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperationInvocation<T> {
  Rejected {
    run_id: RunId,
    operation: OperationName,
    failure: StartFailure,
  },
  Executed {
    execution: OperationExecution<T>,
    persistence: PersistenceStatus,
  },
}

/// Whether an executed operation's intended persistence set was committed.
#[derive(Clone, Debug, PartialEq)]
pub enum PersistenceStatus {
  Disabled,
  Stored,
  NotStored { failure: PersistenceFailure },
  Incomplete { failure: PersistenceFailure },
}

// Unit variants use struct-shaped wire variants so unknown fields are rejected
// instead of silently discarded by Serde.
#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum PersistenceStatusWire<F> {
  Disabled {},
  Stored {},
  NotStored { failure: F },
  Incomplete { failure: F },
}

impl<'a> From<&'a PersistenceStatus> for PersistenceStatusWire<&'a PersistenceFailure> {
  fn from(status: &'a PersistenceStatus) -> Self {
    match status {
      PersistenceStatus::Disabled => Self::Disabled {},
      PersistenceStatus::Stored => Self::Stored {},
      PersistenceStatus::NotStored { failure } => Self::NotStored { failure },
      PersistenceStatus::Incomplete { failure } => Self::Incomplete { failure },
    }
  }
}

impl From<PersistenceStatusWire<PersistenceFailure>> for PersistenceStatus {
  fn from(status: PersistenceStatusWire<PersistenceFailure>) -> Self {
    match status {
      PersistenceStatusWire::Disabled {} => Self::Disabled,
      PersistenceStatusWire::Stored {} => Self::Stored,
      PersistenceStatusWire::NotStored { failure } => Self::NotStored { failure },
      PersistenceStatusWire::Incomplete { failure } => Self::Incomplete { failure },
    }
  }
}

impl Serialize for PersistenceStatus {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    let wire: PersistenceStatusWire<&PersistenceFailure> = self.into();
    wire.serialize(serializer)
  }
}

impl<'de> Deserialize<'de> for PersistenceStatus {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    Ok(PersistenceStatusWire::<PersistenceFailure>::deserialize(deserializer)?.into())
  }
}

/// Stable verification aggregate validation failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum VerificationContractError {
  #[error("verification request must contain at least one assertion")]
  EmptyAssertions,
  #[error("verification request must contain at least one required assertion")]
  NoRequiredAssertion,
  #[error("evaluated verification must contain at least one outcome")]
  EmptyOutcomes,
  #[error("evaluated verification must contain one outcome per assertion")]
  OutcomeCardinalityMismatch,
}

#[cfg(test)]
mod tests {
  use super::*;

  // ROOT CAUSE:
  //
  // If an assertion outcome carried a reason or failure, constructing its
  // serialization wire value cloned that field because the wire enum owned it.
  //
  // Before the fix, serializing could deep-clone an encoded payload.
  // The fix keeps serialization wire fields borrowed from the public value.
  #[test]
  fn assertion_outcome_serialization_wire_borrows_variant_fields() {
    let failed = AssertionOutcome::Failed {
      reason: ReasonCode::parse("test.assertion.failed").unwrap(),
    };
    let AssertionOutcome::Failed { reason } = &failed else {
      unreachable!();
    };
    let wire: AssertionOutcomeWire<&ReasonCode, &VerificationFailure> = (&failed).into();
    let AssertionOutcomeWire::Failed {
      reason: wire_reason,
    } = wire
    else {
      panic!("failed outcome must use the failed wire variant");
    };
    assert!(std::ptr::eq(wire_reason, reason));

    let inconclusive = AssertionOutcome::Inconclusive {
      reason: ReasonCode::parse("test.assertion.inconclusive").unwrap(),
    };
    let AssertionOutcome::Inconclusive { reason } = &inconclusive else {
      unreachable!();
    };
    let wire: AssertionOutcomeWire<&ReasonCode, &VerificationFailure> = (&inconclusive).into();
    let AssertionOutcomeWire::Inconclusive {
      reason: wire_reason,
    } = wire
    else {
      panic!("inconclusive outcome must use the inconclusive wire variant");
    };
    assert!(std::ptr::eq(wire_reason, reason));

    let error = AssertionOutcome::Error {
      failure: VerificationFailure::new(FailureCode::parse("test.verification.failed").unwrap(), None),
    };
    let AssertionOutcome::Error { failure } = &error else {
      unreachable!();
    };
    let wire: AssertionOutcomeWire<&ReasonCode, &VerificationFailure> = (&error).into();
    let AssertionOutcomeWire::Error {
      failure: wire_failure,
    } = wire
    else {
      panic!("error outcome must use the error wire variant");
    };
    assert!(std::ptr::eq(wire_failure, failure));
  }

  // ROOT CAUSE:
  //
  // If a persistence status carried a failure, constructing its serialization
  // wire value cloned the failure because the wire enum owned it.
  //
  // Before the fix, serializing could deep-clone an encoded payload.
  // The fix keeps serialization wire fields borrowed from the public value.
  #[test]
  fn persistence_status_serialization_wire_borrows_variant_fields() {
    let not_stored = PersistenceStatus::NotStored {
      failure: PersistenceFailure::new(FailureCode::parse("test.persistence.not_stored").unwrap(), IdempotencyKey::new(), None),
    };
    let PersistenceStatus::NotStored { failure } = &not_stored else {
      unreachable!();
    };
    let wire: PersistenceStatusWire<&PersistenceFailure> = (&not_stored).into();
    let PersistenceStatusWire::NotStored {
      failure: wire_failure,
    } = wire
    else {
      panic!("not-stored status must use the not-stored wire variant");
    };
    assert!(std::ptr::eq(wire_failure, failure));

    let incomplete = PersistenceStatus::Incomplete {
      failure: PersistenceFailure::new(FailureCode::parse("test.persistence.incomplete").unwrap(), IdempotencyKey::new(), None),
    };
    let PersistenceStatus::Incomplete { failure } = &incomplete else {
      unreachable!();
    };
    let wire: PersistenceStatusWire<&PersistenceFailure> = (&incomplete).into();
    let PersistenceStatusWire::Incomplete {
      failure: wire_failure,
    } = wire
    else {
      panic!("incomplete status must use the incomplete wire variant");
    };
    assert!(std::ptr::eq(wire_failure, failure));
  }
}
