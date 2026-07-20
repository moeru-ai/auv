//! Typed operation definitions and payload codecs.

use std::{future::Future, pin::Pin, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::{EncodedPayload, ExecutionId, ExecutionResult, FailureCode, OperationFailure, OperationName, PayloadSchema, RunId};

/// A sendable future returned by object-safe operation contracts.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The operation-scoped context supplied by the runtime.
#[derive(Debug)]
pub struct ExecutionContext {
  run_id: RunId,
  execution_id: ExecutionId,
}

impl ExecutionContext {
  /// Creates a direct-call context with fresh run and execution identities.
  ///
  /// In this slice the context allocates identities only.
  pub fn detached() -> Self {
    Self {
      run_id: RunId::new(),
      execution_id: ExecutionId::new(),
    }
  }

  pub const fn run_id(&self) -> RunId {
    self.run_id
  }

  pub const fn execution_id(&self) -> ExecutionId {
    self.execution_id
  }

  // TODO(auv-run-v1-runtime-context): Add typed instrumentation, cancellation,
  // close semantics, and accepted-fact accounting in Task 9 after Task 4 lands
  // canonical fact types; temporary instrumentation payloads stay out of Task 3.
}

/// Converts typed values to and from one declared payload schema.
pub trait PayloadCodec<T>: Send + Sync {
  fn schema(&self) -> &PayloadSchema;
  fn encode(&self, value: &T) -> Result<EncodedPayload, EncodeError>;
  fn decode(&self, value: &EncodedPayload) -> Result<T, DecodeError>;
}

/// A stable payload encoding failure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(deny_unknown_fields)]
#[error("{code}")]
pub struct EncodeError {
  code: FailureCode,
}

impl EncodeError {
  pub const fn new(code: FailureCode) -> Self {
    Self { code }
  }

  pub fn code(&self) -> &FailureCode {
    &self.code
  }
}

/// A stable payload decoding failure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(deny_unknown_fields)]
#[error("{code}")]
pub struct DecodeError {
  code: FailureCode,
}

impl DecodeError {
  pub const fn new(code: FailureCode) -> Self {
    Self { code }
  }

  pub fn schema_mismatch() -> Self {
    Self::new(FailureCode::parse("auv.payload.schema_mismatch").expect("static failure code must be valid"))
  }

  pub fn code(&self) -> &FailureCode {
    &self.code
  }
}

/// A stable failure raised before an erased operation starts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(deny_unknown_fields)]
#[error("{code}")]
pub struct DispatchError {
  code: FailureCode,
}

impl DispatchError {
  pub const fn new(code: FailureCode) -> Self {
    Self { code }
  }

  pub fn code(&self) -> &FailureCode {
    &self.code
  }
}

impl From<DecodeError> for DispatchError {
  fn from(error: DecodeError) -> Self {
    Self::new(error.code)
  }
}

/// A reusable typed application operation.
pub trait Operation: Send + Sync {
  type Input: Send + Sync + 'static;
  type Output: Send + Sync + 'static;

  fn name(&self) -> &OperationName;
  fn input_codec(&self) -> &dyn PayloadCodec<Self::Input>;
  fn output_codec(&self) -> &dyn PayloadCodec<Self::Output>;
  fn execute<'a>(&'a self, context: &'a ExecutionContext, input: Self::Input) -> BoxFuture<'a, ExecutionResult<Self::Output>>;
}

/// An object-safe operation boundary for dynamic frontends.
pub trait ErasedOperation: Send + Sync {
  fn name(&self) -> &OperationName;
  fn input_schema(&self) -> &PayloadSchema;
  fn execute_encoded<'a>(
    &'a self,
    context: &'a ExecutionContext,
    input: &'a EncodedPayload,
  ) -> BoxFuture<'a, Result<ExecutionResult<EncodedPayload>, DispatchError>>;
}

/// Resolves dynamically named operations.
pub trait OperationCatalog: Send + Sync {
  fn resolve(&self, name: &OperationName) -> Option<Arc<dyn ErasedOperation>>;
}

/// Adapts one typed operation to the object-safe encoded boundary.
pub struct ErasedAdapter<O> {
  operation: Arc<O>,
}

impl<O> ErasedAdapter<O> {
  pub const fn new(operation: Arc<O>) -> Self {
    Self { operation }
  }
}

impl<O> ErasedOperation for ErasedAdapter<O>
where
  O: Operation + 'static,
{
  fn name(&self) -> &OperationName {
    self.operation.name()
  }

  fn input_schema(&self) -> &PayloadSchema {
    self.operation.input_codec().schema()
  }

  fn execute_encoded<'a>(
    &'a self,
    context: &'a ExecutionContext,
    input: &'a EncodedPayload,
  ) -> BoxFuture<'a, Result<ExecutionResult<EncodedPayload>, DispatchError>> {
    Box::pin(async move {
      let input = self.operation.input_codec().decode(input)?;
      let result = self.operation.execute(context, input).await;
      Ok(match result {
        ExecutionResult::Completed { output } => match self.operation.output_codec().encode(&output) {
          Ok(output) => ExecutionResult::Completed { output },
          Err(_) => ExecutionResult::Failed {
            failure: OperationFailure::new(
              FailureCode::parse("auv.runtime.output_encoding_failed").expect("static failure code must be valid"),
              None,
            ),
          },
        },
        ExecutionResult::Failed { failure } => ExecutionResult::Failed { failure },
        ExecutionResult::Cancelled { reason } => ExecutionResult::Cancelled { reason },
      })
    })
  }
}
