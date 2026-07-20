use std::sync::{
  Arc, OnceLock,
  atomic::{AtomicUsize, Ordering},
};

use auv_run::{
  AssertionOutcome, BoxFuture, CancellationReason, DecodeError, EncodeError, EncodedPayload, ErasedAdapter, ErasedOperation,
  ExecutionContext, ExecutionResult, FailureCode, IdempotencyKey, Operation, OperationExecution, OperationFailure, OperationInvocation,
  OperationName, PayloadCodec, PayloadSchema, PersistenceFailure, PersistenceStatus, ReasonCode, StartFailure, Timestamp,
  VerificationAssertion, VerificationEvaluation, VerificationFailure, VerificationRequest, VerificationResult,
};
use serde::de::DeserializeOwned;

struct IntegerCodec {
  schema: PayloadSchema,
}

impl IntegerCodec {
  fn new(name: &str) -> Self {
    Self {
      schema: PayloadSchema::parse(name, 1).unwrap(),
    }
  }
}

impl PayloadCodec<i32> for IntegerCodec {
  fn schema(&self) -> &PayloadSchema {
    &self.schema
  }

  fn encode(&self, value: &i32) -> Result<EncodedPayload, EncodeError> {
    EncodedPayload::new(self.schema.clone(), serde_json::json!(value))
      .map_err(|_| EncodeError::new(FailureCode::parse("auv.payload.encoding_failed").unwrap()))
  }

  fn decode(&self, value: &EncodedPayload) -> Result<i32, DecodeError> {
    if value.schema() != &self.schema {
      return Err(DecodeError::schema_mismatch());
    }
    value
      .data()
      .as_i64()
      .and_then(|value| i32::try_from(value).ok())
      .ok_or_else(|| DecodeError::new(FailureCode::parse("auv.payload.invalid_integer").unwrap()))
  }
}

struct AddOneCodec;

impl AddOneCodec {
  fn input() -> &'static IntegerCodec {
    static INPUT: OnceLock<IntegerCodec> = OnceLock::new();
    INPUT.get_or_init(|| IntegerCodec::new("test.add_one.input"))
  }

  fn output() -> &'static IntegerCodec {
    static OUTPUT: OnceLock<IntegerCodec> = OnceLock::new();
    OUTPUT.get_or_init(|| IntegerCodec::new("test.add_one.output"))
  }
}

struct AddOneOperation {
  name: OperationName,
}

impl AddOneOperation {
  fn new() -> Self {
    Self {
      name: OperationName::parse("test.add_one").unwrap(),
    }
  }
}

impl Operation for AddOneOperation {
  type Input = i32;
  type Output = i32;

  fn name(&self) -> &OperationName {
    &self.name
  }

  fn input_codec(&self) -> &dyn PayloadCodec<Self::Input> {
    AddOneCodec::input()
  }

  fn output_codec(&self) -> &dyn PayloadCodec<Self::Output> {
    AddOneCodec::output()
  }

  fn execute<'a>(&'a self, _context: &'a ExecutionContext, input: Self::Input) -> BoxFuture<'a, ExecutionResult<Self::Output>> {
    Box::pin(async move { ExecutionResult::Completed { output: input + 1 } })
  }
}

struct FailingOutputCodec {
  schema: PayloadSchema,
  encodes: AtomicUsize,
}

impl PayloadCodec<i32> for FailingOutputCodec {
  fn schema(&self) -> &PayloadSchema {
    &self.schema
  }

  fn encode(&self, _value: &i32) -> Result<EncodedPayload, EncodeError> {
    self.encodes.fetch_add(1, Ordering::SeqCst);
    Err(EncodeError::new(FailureCode::parse("test.output.rejected").unwrap()))
  }

  fn decode(&self, _value: &EncodedPayload) -> Result<i32, DecodeError> {
    unreachable!("the failing output codec is never used to decode")
  }
}

struct OutputEncodingFailureOperation {
  name: OperationName,
  output: FailingOutputCodec,
  calls: AtomicUsize,
}

impl OutputEncodingFailureOperation {
  fn new() -> Self {
    Self {
      name: OperationName::parse("test.output_encoding_failure").unwrap(),
      output: FailingOutputCodec {
        schema: PayloadSchema::parse("test.output_encoding_failure.output", 1).unwrap(),
        encodes: AtomicUsize::new(0),
      },
      calls: AtomicUsize::new(0),
    }
  }
}

impl Operation for OutputEncodingFailureOperation {
  type Input = i32;
  type Output = i32;

  fn name(&self) -> &OperationName {
    &self.name
  }

  fn input_codec(&self) -> &dyn PayloadCodec<Self::Input> {
    AddOneCodec::input()
  }

  fn output_codec(&self) -> &dyn PayloadCodec<Self::Output> {
    &self.output
  }

  fn execute<'a>(&'a self, _context: &'a ExecutionContext, input: Self::Input) -> BoxFuture<'a, ExecutionResult<Self::Output>> {
    self.calls.fetch_add(1, Ordering::SeqCst);
    Box::pin(async move { ExecutionResult::Completed { output: input + 1 } })
  }
}

struct CountingCodec {
  schema: PayloadSchema,
  decodes: AtomicUsize,
  encodes: AtomicUsize,
}

impl CountingCodec {
  fn new(name: &str) -> Self {
    Self {
      schema: PayloadSchema::parse(name, 1).unwrap(),
      decodes: AtomicUsize::new(0),
      encodes: AtomicUsize::new(0),
    }
  }
}

impl PayloadCodec<i32> for CountingCodec {
  fn schema(&self) -> &PayloadSchema {
    &self.schema
  }

  fn encode(&self, value: &i32) -> Result<EncodedPayload, EncodeError> {
    self.encodes.fetch_add(1, Ordering::SeqCst);
    EncodedPayload::new(self.schema.clone(), serde_json::json!(value))
      .map_err(|_| EncodeError::new(FailureCode::parse("auv.payload.encoding_failed").unwrap()))
  }

  fn decode(&self, value: &EncodedPayload) -> Result<i32, DecodeError> {
    self.decodes.fetch_add(1, Ordering::SeqCst);
    if value.schema() != &self.schema {
      return Err(DecodeError::schema_mismatch());
    }
    value
      .data()
      .as_i64()
      .and_then(|value| i32::try_from(value).ok())
      .ok_or_else(|| DecodeError::new(FailureCode::parse("auv.payload.invalid_integer").unwrap()))
  }
}

struct CountingOperation {
  name: OperationName,
  input: CountingCodec,
  output: CountingCodec,
  result: ExecutionResult<i32>,
  calls: AtomicUsize,
}

impl CountingOperation {
  fn new(result: ExecutionResult<i32>) -> Self {
    Self {
      name: OperationName::parse("test.counting").unwrap(),
      input: CountingCodec::new("test.counting.input"),
      output: CountingCodec::new("test.counting.output"),
      result,
      calls: AtomicUsize::new(0),
    }
  }

  fn encoded_input(&self, data: serde_json::Value) -> EncodedPayload {
    EncodedPayload::new(self.input.schema.clone(), data).unwrap()
  }
}

impl Operation for CountingOperation {
  type Input = i32;
  type Output = i32;

  fn name(&self) -> &OperationName {
    &self.name
  }

  fn input_codec(&self) -> &dyn PayloadCodec<Self::Input> {
    &self.input
  }

  fn output_codec(&self) -> &dyn PayloadCodec<Self::Output> {
    &self.output
  }

  fn execute<'a>(&'a self, _context: &'a ExecutionContext, _input: Self::Input) -> BoxFuture<'a, ExecutionResult<Self::Output>> {
    self.calls.fetch_add(1, Ordering::SeqCst);
    let result = self.result.clone();
    Box::pin(async move { result })
  }
}

fn assertion(required: bool, value: i32) -> VerificationAssertion {
  VerificationAssertion::new(required, AddOneCodec::output().encode(&value).unwrap())
}

fn verification_request_with_two_assertions() -> VerificationRequest {
  VerificationRequest::new(vec![assertion(true, 1), assertion(false, 2)]).unwrap()
}

fn evaluation(outcomes: Vec<AssertionOutcome>) -> VerificationEvaluation {
  VerificationEvaluation::new(
    VerificationRequest::new(vec![
      assertion(false, 0),
      assertion(true, 1),
      assertion(true, 2),
      assertion(true, 3),
    ])
    .unwrap(),
    VerificationResult::evaluated(outcomes).unwrap(),
  )
  .unwrap()
}

fn assert_rejects_field<T>(mut value: serde_json::Value, field: &str)
where
  T: DeserializeOwned,
{
  value.as_object_mut().unwrap().insert(field.to_owned(), serde_json::json!(true));
  assert!(serde_json::from_value::<T>(value).is_err(), "{field} must be rejected");
}

fn rejected_invocation_json() -> serde_json::Value {
  serde_json::json!({
    "status": "rejected",
    "run_id": auv_run::RunId::new(),
    "operation": "test.add_one",
    "failure": {
      "type": "invalid_request",
      "code": "test.invalid_request"
    }
  })
}

#[tokio::test]
async fn erased_and_typed_operations_return_the_same_output() {
  let operation = Arc::new(AddOneOperation::new());
  let context = ExecutionContext::detached();
  let typed = operation.execute(&context, 41).await;
  let erased = ErasedAdapter::new(operation).execute_encoded(&context, &AddOneCodec::input().encode(&41).unwrap()).await.unwrap();
  assert_eq!(typed, ExecutionResult::Completed { output: 42 });
  let ExecutionResult::Completed { output } = erased else {
    panic!("erased operation should complete");
  };
  assert_eq!(AddOneCodec::output().decode(&output).unwrap(), 42);
}

#[tokio::test]
async fn erased_operations_reject_mismatched_input_schemas() {
  let mismatched = EncodedPayload::new(PayloadSchema::parse("test.other.input", 1).unwrap(), serde_json::json!(41)).unwrap();
  let direct_error = AddOneCodec::input().decode(&mismatched).unwrap_err();
  assert_eq!(direct_error.code().as_str(), "auv.payload.schema_mismatch");

  let error =
    ErasedAdapter::new(Arc::new(AddOneOperation::new())).execute_encoded(&ExecutionContext::detached(), &mismatched).await.unwrap_err();
  assert_eq!(error.code().as_str(), "auv.payload.schema_mismatch");
}

#[tokio::test]
async fn output_encode_failure_fails_without_reexecuting_the_operation() {
  let operation = Arc::new(OutputEncodingFailureOperation::new());
  let result = ErasedAdapter::new(operation.clone())
    .execute_encoded(&ExecutionContext::detached(), &AddOneCodec::input().encode(&41).unwrap())
    .await
    .unwrap();

  let ExecutionResult::Failed { failure } = result else {
    panic!("output encoding failure should fail the execution");
  };
  assert_eq!(failure.code().as_str(), "auv.runtime.output_encoding_failed");
  assert_eq!(operation.calls.load(Ordering::SeqCst), 1);
  assert_eq!(operation.output.encodes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn decode_failure_calls_neither_operation_nor_output_encoder() {
  let operation = Arc::new(CountingOperation::new(ExecutionResult::Completed { output: 42 }));
  let input = operation.encoded_input(serde_json::json!("not an integer"));

  let error = ErasedAdapter::new(operation.clone()).execute_encoded(&ExecutionContext::detached(), &input).await.unwrap_err();

  assert_eq!(error.code().as_str(), "auv.payload.invalid_integer");
  assert_eq!(operation.input.decodes.load(Ordering::SeqCst), 1);
  assert_eq!(operation.calls.load(Ordering::SeqCst), 0);
  assert_eq!(operation.output.encodes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn successful_erased_execution_decodes_executes_and_encodes_once() {
  let operation = Arc::new(CountingOperation::new(ExecutionResult::Completed { output: 42 }));
  let input = operation.encoded_input(serde_json::json!(41));

  let result = ErasedAdapter::new(operation.clone()).execute_encoded(&ExecutionContext::detached(), &input).await.unwrap();

  let ExecutionResult::Completed { output } = result else {
    panic!("counting operation should complete");
  };
  assert_eq!(output.data(), &serde_json::json!(42));
  assert_eq!(operation.input.decodes.load(Ordering::SeqCst), 1);
  assert_eq!(operation.calls.load(Ordering::SeqCst), 1);
  assert_eq!(operation.output.encodes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_erased_execution_preserves_failure_without_output_encoding() {
  let failure = OperationFailure::new(FailureCode::parse("test.operation.failed").unwrap(), None);
  let operation = Arc::new(CountingOperation::new(ExecutionResult::Failed {
    failure: failure.clone(),
  }));
  let input = operation.encoded_input(serde_json::json!(41));

  let result = ErasedAdapter::new(operation.clone()).execute_encoded(&ExecutionContext::detached(), &input).await.unwrap();

  assert_eq!(result, ExecutionResult::Failed { failure });
  assert_eq!(operation.input.decodes.load(Ordering::SeqCst), 1);
  assert_eq!(operation.calls.load(Ordering::SeqCst), 1);
  assert_eq!(operation.output.encodes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cancelled_erased_execution_preserves_reason_without_output_encoding() {
  let reason = CancellationReason::parse("test.cancelled").unwrap();
  let operation = Arc::new(CountingOperation::new(ExecutionResult::Cancelled {
    reason: reason.clone(),
  }));
  let input = operation.encoded_input(serde_json::json!(41));

  let result = ErasedAdapter::new(operation.clone()).execute_encoded(&ExecutionContext::detached(), &input).await.unwrap();

  assert_eq!(result, ExecutionResult::Cancelled { reason });
  assert_eq!(operation.input.decodes.load(Ordering::SeqCst), 1);
  assert_eq!(operation.calls.load(Ordering::SeqCst), 1);
  assert_eq!(operation.output.encodes.load(Ordering::SeqCst), 0);
}

#[test]
fn evaluated_verification_requires_one_outcome_per_assertion() {
  let request = verification_request_with_two_assertions();
  let result = VerificationResult::evaluated(vec![AssertionOutcome::Passed]).unwrap();
  assert!(VerificationEvaluation::new(request, result).is_err());
}

#[test]
fn evaluated_verification_rejects_empty_outcomes_without_panicking() {
  let result = VerificationResult::evaluated(Vec::new());
  assert!(result.is_err());
}

#[test]
fn verification_requests_require_at_least_one_required_assertion() {
  assert!(VerificationRequest::new(vec![assertion(false, 1)]).is_err());
}

#[test]
fn verification_request_wire_rejects_no_required_assertion() {
  let request = VerificationRequest::new(vec![assertion(true, 1)]).unwrap();
  let mut encoded = serde_json::to_value(request).unwrap();
  encoded["assertions"][0]["required"] = serde_json::json!(false);

  let error = serde_json::from_value::<VerificationRequest>(encoded).unwrap_err();
  assert!(error.to_string().contains("verification request must contain at least one required assertion"));
}

#[test]
fn verification_evaluation_wire_rejects_request_result_cardinality_mismatch() {
  let evaluation = VerificationEvaluation::new(
    verification_request_with_two_assertions(),
    VerificationResult::evaluated(vec![AssertionOutcome::Passed, AssertionOutcome::Passed]).unwrap(),
  )
  .unwrap();
  let mut encoded = serde_json::to_value(evaluation).unwrap();
  encoded["result"]["outcomes"].as_array_mut().unwrap().pop();

  let error = serde_json::from_value::<VerificationEvaluation>(encoded).unwrap_err();
  assert!(error.to_string().contains("evaluated verification must contain one outcome per assertion"));
}

#[test]
fn required_assertion_aggregation_uses_failure_error_inconclusive_order() {
  let failure = VerificationFailure::new(FailureCode::parse("test.verification.error").unwrap(), None);
  let failed_evaluation = evaluation(vec![
    AssertionOutcome::Failed {
      reason: ReasonCode::parse("test.optional.failed").unwrap(),
    },
    AssertionOutcome::Inconclusive {
      reason: ReasonCode::parse("test.required.inconclusive").unwrap(),
    },
    AssertionOutcome::Error { failure },
    AssertionOutcome::Failed {
      reason: ReasonCode::parse("test.required.failed").unwrap(),
    },
  ]);
  assert!(matches!(
    failed_evaluation.required_outcome().unwrap(),
    Some(AssertionOutcome::Failed { reason }) if reason.as_str() == "test.required.failed"
  ));

  let failure = VerificationFailure::new(FailureCode::parse("test.verification.error").unwrap(), None);
  let error_evaluation = evaluation(vec![
    AssertionOutcome::Failed {
      reason: ReasonCode::parse("test.optional.failed").unwrap(),
    },
    AssertionOutcome::Inconclusive {
      reason: ReasonCode::parse("test.required.inconclusive").unwrap(),
    },
    AssertionOutcome::Error { failure },
    AssertionOutcome::Passed,
  ]);
  assert!(matches!(
    error_evaluation.required_outcome().unwrap(),
    Some(AssertionOutcome::Error { failure }) if failure.code().as_str() == "test.verification.error"
  ));

  let inconclusive_evaluation = evaluation(vec![
    AssertionOutcome::Failed {
      reason: ReasonCode::parse("test.optional.failed").unwrap(),
    },
    AssertionOutcome::Inconclusive {
      reason: ReasonCode::parse("test.required.inconclusive").unwrap(),
    },
    AssertionOutcome::Passed,
    AssertionOutcome::Passed,
  ]);
  assert!(matches!(
    inconclusive_evaluation.required_outcome().unwrap(),
    Some(AssertionOutcome::Inconclusive { reason }) if reason.as_str() == "test.required.inconclusive"
  ));

  let passing_evaluation = evaluation(vec![
    AssertionOutcome::Failed {
      reason: ReasonCode::parse("test.optional.failed").unwrap(),
    },
    AssertionOutcome::Passed,
    AssertionOutcome::Passed,
    AssertionOutcome::Passed,
  ]);
  assert_eq!(passing_evaluation.required_outcome().unwrap(), None);
}

#[test]
fn no_verification_is_represented_by_none() {
  let execution = OperationExecution::new(
    auv_run::RunId::new(),
    auv_run::ExecutionId::new(),
    OperationName::parse("test.add_one").unwrap(),
    Timestamp::new(1_700_000_000, 0).unwrap(),
    Timestamp::new(1_700_000_001, 0).unwrap(),
    ExecutionResult::Completed { output: 42 },
    None,
  )
  .unwrap();
  assert!(execution.verification().is_none());
}

#[test]
fn verification_can_accompany_failed_and_cancelled_execution() {
  let verification = VerificationEvaluation::new(
    VerificationRequest::new(vec![assertion(true, 1)]).unwrap(),
    VerificationResult::evaluated(vec![AssertionOutcome::Passed]).unwrap(),
  )
  .unwrap();
  let results = [
    ExecutionResult::Failed {
      failure: OperationFailure::new(FailureCode::parse("test.operation.failed").unwrap(), None),
    },
    ExecutionResult::Cancelled {
      reason: CancellationReason::parse("test.cancelled").unwrap(),
    },
  ];

  for result in results {
    let execution = OperationExecution::new(
      auv_run::RunId::new(),
      auv_run::ExecutionId::new(),
      OperationName::parse("test.add_one").unwrap(),
      Timestamp::new(1_700_000_000, 0).unwrap(),
      Timestamp::new(1_700_000_001, 0).unwrap(),
      result.clone(),
      Some(verification.clone()),
    )
    .unwrap();
    let encoded = serde_json::to_value(&execution).unwrap();
    let decoded = serde_json::from_value::<OperationExecution<i32>>(encoded).unwrap();

    assert_eq!(decoded.result(), &result);
    assert_eq!(decoded.verification(), Some(&verification));
  }
}

#[test]
fn assertion_outcomes_roundtrip_every_variant_with_exact_json() {
  let failure = VerificationFailure::new(FailureCode::parse("test.verification.failed").unwrap(), None);
  let cases = [
    (AssertionOutcome::Passed, serde_json::json!({ "status": "passed" })),
    (
      AssertionOutcome::Failed {
        reason: ReasonCode::parse("test.assertion.failed").unwrap(),
      },
      serde_json::json!({ "status": "failed", "reason": "test.assertion.failed" }),
    ),
    (
      AssertionOutcome::Inconclusive {
        reason: ReasonCode::parse("test.assertion.inconclusive").unwrap(),
      },
      serde_json::json!({ "status": "inconclusive", "reason": "test.assertion.inconclusive" }),
    ),
    (
      AssertionOutcome::Error { failure },
      serde_json::json!({
        "status": "error",
        "failure": { "code": "test.verification.failed", "details": null }
      }),
    ),
  ];

  for (outcome, expected) in cases {
    let encoded = serde_json::to_value(&outcome).unwrap();
    assert_eq!(encoded, expected);
    assert_eq!(serde_json::from_value::<AssertionOutcome>(encoded).unwrap(), outcome);
  }
}

#[test]
fn persistence_statuses_roundtrip_every_variant_with_exact_json() {
  let request_key = IdempotencyKey::new();
  let failure = PersistenceFailure::new(FailureCode::parse("test.persistence.failed").unwrap(), request_key, None);
  let encoded_failure = serde_json::json!({
    "code": "test.persistence.failed",
    "request_key": request_key,
    "details": null
  });
  let cases = [
    (PersistenceStatus::Disabled, serde_json::json!({ "status": "disabled" })),
    (PersistenceStatus::Stored, serde_json::json!({ "status": "stored" })),
    (
      PersistenceStatus::NotStored {
        failure: failure.clone(),
      },
      serde_json::json!({ "status": "not_stored", "failure": encoded_failure.clone() }),
    ),
    (PersistenceStatus::Incomplete { failure }, serde_json::json!({ "status": "incomplete", "failure": encoded_failure })),
  ];

  for (status, expected) in cases {
    let encoded = serde_json::to_value(&status).unwrap();
    assert_eq!(encoded, expected);
    assert_eq!(serde_json::from_value::<PersistenceStatus>(encoded).unwrap(), status);
  }
}

#[test]
fn failures_have_no_retry_boolean_or_recursive_cause() {
  let operation = OperationFailure::new(FailureCode::parse("test.operation.failed").unwrap(), None);
  let verification = VerificationFailure::new(FailureCode::parse("test.verification.failed").unwrap(), None);
  let persistence = PersistenceFailure::new(FailureCode::parse("test.persistence.failed").unwrap(), IdempotencyKey::new(), None);
  let encoded = serde_json::to_value((operation, verification, persistence)).unwrap();
  let encoded = serde_json::to_string(&encoded).unwrap();

  assert!(!encoded.contains("retry"));
  assert!(!encoded.contains("cause"));
}

#[test]
fn execution_results_use_semantic_status_tags() {
  let completed = serde_json::to_value(ExecutionResult::Completed { output: 42 }).unwrap();
  assert_eq!(completed, serde_json::json!({ "status": "completed", "output": 42 }));

  let cancelled = serde_json::to_value(ExecutionResult::<i32>::Cancelled {
    reason: CancellationReason::parse("test.cancelled").unwrap(),
  })
  .unwrap();
  assert_eq!(cancelled, serde_json::json!({ "status": "cancelled", "reason": "test.cancelled" }));
}

#[test]
fn operation_execution_rejects_finished_before_started() {
  let error = OperationExecution::new(
    auv_run::RunId::new(),
    auv_run::ExecutionId::new(),
    OperationName::parse("test.add_one").unwrap(),
    Timestamp::new(1_700_000_001, 0).unwrap(),
    Timestamp::new(1_700_000_000, 0).unwrap(),
    ExecutionResult::Completed { output: 42 },
    None,
  )
  .unwrap_err();
  assert_eq!(error.code(), "auv.execution.finished_before_started");
}

#[test]
fn operation_execution_wire_rejects_finished_before_started() {
  let encoded = serde_json::json!({
    "run_id": auv_run::RunId::new(),
    "execution_id": auv_run::ExecutionId::new(),
    "operation": "test.add_one",
    "started_at": "2023-11-14T22:13:21Z",
    "finished_at": "2023-11-14T22:13:20Z",
    "result": { "status": "completed", "output": 42 },
    "verification": null
  });
  let error = serde_json::from_value::<OperationExecution<i32>>(encoded).unwrap_err();
  assert!(error.to_string().contains("auv.execution.finished_before_started"));
}

#[test]
fn internally_tagged_enums_reject_arbitrary_unknown_fields() {
  assert_rejects_field::<ExecutionResult<i32>>(serde_json::json!({ "status": "completed", "output": 42 }), "unexpected");
  assert_rejects_field::<VerificationResult>(
    serde_json::json!({ "status": "evaluated", "outcomes": [{ "status": "passed" }] }),
    "unexpected",
  );
  assert_rejects_field::<AssertionOutcome>(serde_json::json!({ "status": "passed" }), "unexpected");
  assert_rejects_field::<StartFailure>(serde_json::json!({ "type": "invalid_request", "code": "test.invalid_request" }), "unexpected");
  assert_rejects_field::<OperationInvocation<i32>>(rejected_invocation_json(), "unexpected");
  assert_rejects_field::<PersistenceStatus>(serde_json::json!({ "status": "disabled" }), "unexpected");
}

#[test]
fn internally_tagged_enums_reject_forbidden_legacy_fields() {
  assert_rejects_field::<ExecutionResult<i32>>(serde_json::json!({ "status": "completed", "output": 42 }), "retryable");
  assert_rejects_field::<VerificationResult>(
    serde_json::json!({ "status": "evaluated", "outcomes": [{ "status": "passed" }] }),
    "verification_status",
  );
  assert_rejects_field::<AssertionOutcome>(serde_json::json!({ "status": "passed" }), "summary");
  assert_rejects_field::<StartFailure>(serde_json::json!({ "type": "invalid_request", "code": "test.invalid_request" }), "cause");
  assert_rejects_field::<OperationInvocation<i32>>(rejected_invocation_json(), "recommended_next_action");
  assert_rejects_field::<PersistenceStatus>(serde_json::json!({ "status": "disabled" }), "backend");
}
