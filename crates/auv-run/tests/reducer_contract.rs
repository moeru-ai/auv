use auv_run::{
  Artifact, ArtifactContent, ArtifactId, ArtifactPurpose, ArtifactRef, ArtifactScope, AssertionOutcome, Attributes, ByteLength, ContentType,
  EncodedPayload, EventAdded, EventId, EventName, ExecutionFinished, ExecutionId, ExecutionPrepared, ExecutionResult, ExecutionState,
  IdempotencyKey, OperationName, PayloadSchema, Revision, RunChange, RunCommit, RunCommitRequest, RunId, RunOpened, RunSealReason,
  RunSealed, Sha256Digest, SpanFinished, SpanId, SpanName, SpanStarted, Timestamp, VerificationAssertion, VerificationRequest,
  VerificationResult, reduce_commits,
};

fn timestamp(seconds: i64) -> Timestamp {
  Timestamp::new(seconds, 0).unwrap()
}

fn payload(data: serde_json::Value) -> EncodedPayload {
  EncodedPayload::new(PayloadSchema::parse("test.payload", 1).unwrap(), data).unwrap()
}

fn verification_request(assertion_count: usize) -> VerificationRequest {
  VerificationRequest::new(
    (0..assertion_count).map(|index| VerificationAssertion::new(true, payload(serde_json::json!({ "index": index })))).collect(),
  )
  .unwrap()
}

fn opened() -> RunOpened {
  RunOpened::new(timestamp(10))
}

fn prepared(execution_id: ExecutionId) -> ExecutionPrepared {
  ExecutionPrepared::new(
    execution_id,
    OperationName::parse("test.operation").unwrap(),
    payload(serde_json::json!({ "input": 1 })),
    None,
    timestamp(20),
  )
}

fn prepared_with_verification(execution_id: ExecutionId, assertion_count: usize) -> ExecutionPrepared {
  ExecutionPrepared::new(
    execution_id,
    OperationName::parse("test.operation").unwrap(),
    payload(serde_json::json!({ "input": 1 })),
    Some(verification_request(assertion_count)),
    timestamp(20),
  )
}

fn finished(execution_id: ExecutionId) -> ExecutionFinished {
  ExecutionFinished::new(
    execution_id,
    timestamp(21),
    timestamp(22),
    ExecutionResult::Completed {
      output: payload(serde_json::json!({ "output": 2 })),
    },
    None,
  )
  .unwrap()
}

fn commit(run_id: RunId, revision: u64, changes: Vec<RunChange>) -> RunCommit {
  RunCommit::new(run_id, Revision::new(revision), IdempotencyKey::new(), timestamp(100 + revision as i64), changes, Vec::new()).unwrap()
}

fn artifact(run_id: RunId, scope: ArtifactScope) -> Artifact {
  Artifact::new(
    ArtifactRef::new(run_id, ArtifactId::new()),
    scope,
    ArtifactPurpose::parse("test.evidence").unwrap(),
    ArtifactContent::new(
      ContentType::parse("application/json").unwrap(),
      Some(PayloadSchema::parse("test.artifact", 1).unwrap()),
      Sha256Digest::of_bytes(b"{}"),
      ByteLength::new(2),
    ),
    timestamp(30),
    Attributes::default(),
  )
}

fn artifact_commit(run_id: RunId, revision: u64, artifact: Artifact) -> RunCommit {
  RunCommit::new(run_id, Revision::new(revision), IdempotencyKey::new(), timestamp(100 + revision as i64), Vec::new(), vec![artifact])
    .unwrap()
}

fn reduce(commits: &[RunCommit]) -> Result<auv_run::RunSnapshot, auv_run::ReduceError> {
  reduce_commits(commits)
}

#[test]
fn prepared_execution_survives_without_claiming_it_is_running() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let snapshot = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(run_id, 2, vec![RunChange::ExecutionPrepared(prepared(execution_id))]),
  ])
  .unwrap();

  assert_eq!(snapshot.through_revision(), Revision::new(2));
  assert!(matches!(
    snapshot.executions()[0].state(),
    ExecutionState::Prepared { prepared_at } if *prepared_at == timestamp(20)
  ));
}

#[test]
fn revision_one_requires_exactly_one_open_change() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let missing_open = reduce(&[commit(
    run_id,
    1,
    vec![RunChange::ExecutionPrepared(prepared(execution_id))],
  )])
  .unwrap_err();
  assert_eq!(missing_open.code(), "auv.run.first_revision_not_open");

  let duplicate_open = reduce(&[commit(
    run_id,
    1,
    vec![
      RunChange::RunOpened(opened()),
      RunChange::RunOpened(opened()),
    ],
  )])
  .unwrap_err();
  assert_eq!(duplicate_open.code(), "auv.run.first_revision_not_open");

  let empty = reduce(&[]).unwrap_err();
  assert_eq!(empty.code(), "auv.run.empty_history");
}

#[test]
fn revisions_are_contiguous_and_belong_to_one_run() {
  let run_id = RunId::new();
  let skipped = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(run_id, 3, vec![RunChange::ExecutionPrepared(prepared(ExecutionId::new()))]),
  ])
  .unwrap_err();
  assert_eq!(skipped.code(), "auv.run.revision_not_contiguous");

  let other_run = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(RunId::new(), 2, vec![RunChange::ExecutionPrepared(prepared(ExecutionId::new()))]),
  ])
  .unwrap_err();
  assert_eq!(other_run.code(), "auv.run.commit_scope_mismatch");
}

#[test]
fn execution_is_prepared_once_and_finished_at_most_once() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let duplicate_prepare = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![
        RunChange::ExecutionPrepared(prepared(execution_id)),
        RunChange::ExecutionPrepared(prepared(execution_id)),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(duplicate_prepare.code(), "auv.run.execution_already_prepared");

  let duplicate_finish = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(run_id, 2, vec![RunChange::ExecutionPrepared(prepared(execution_id))]),
    commit(
      run_id,
      3,
      vec![
        RunChange::ExecutionFinished(finished(execution_id)),
        RunChange::ExecutionFinished(finished(execution_id)),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(duplicate_finish.code(), "auv.run.execution_already_finished");
}

#[test]
fn finish_requires_a_preparation_and_ordered_timestamps() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let missing_prepare = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(run_id, 2, vec![RunChange::ExecutionFinished(finished(execution_id))]),
  ])
  .unwrap_err();
  assert_eq!(missing_prepare.code(), "auv.run.execution_not_prepared");

  let started_before_prepared = ExecutionFinished::new(
    execution_id,
    timestamp(19),
    timestamp(22),
    ExecutionResult::Completed {
      output: payload(serde_json::json!({ "output": 2 })),
    },
    None,
  )
  .unwrap();
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(run_id, 2, vec![RunChange::ExecutionPrepared(prepared(execution_id))]),
    commit(run_id, 3, vec![RunChange::ExecutionFinished(started_before_prepared)]),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.execution_started_before_prepared");

  let local_error = ExecutionFinished::new(
    execution_id,
    timestamp(22),
    timestamp(21),
    ExecutionResult::Completed {
      output: payload(serde_json::json!({ "output": 2 })),
    },
    None,
  )
  .unwrap_err();
  assert_eq!(local_error.code(), "auv.run.execution_finished_before_started");
}

#[test]
fn finish_rejects_verification_presence_mismatch() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![RunChange::ExecutionPrepared(prepared_with_verification(
        execution_id,
        1,
      ))],
    ),
    commit(run_id, 3, vec![RunChange::ExecutionFinished(finished(execution_id))]),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.verification_presence_mismatch");
}

#[test]
fn finish_rejects_verification_cardinality_mismatch() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let verification_result = VerificationResult::evaluated(vec![AssertionOutcome::Passed]).unwrap();
  let finish = ExecutionFinished::new(
    execution_id,
    timestamp(21),
    timestamp(22),
    ExecutionResult::Completed {
      output: payload(serde_json::json!({ "output": 2 })),
    },
    Some(verification_result),
  )
  .unwrap();
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![RunChange::ExecutionPrepared(prepared_with_verification(
        execution_id,
        2,
      ))],
    ),
    commit(run_id, 3, vec![RunChange::ExecutionFinished(finish)]),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.verification_cardinality_mismatch");
}

#[test]
fn artifacts_must_belong_to_the_run_and_reference_its_execution() {
  let run_id = RunId::new();
  let foreign_artifact = artifact(RunId::new(), ArtifactScope::Run);
  let scope_error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    artifact_commit(run_id, 2, foreign_artifact),
  ])
  .unwrap_err();
  assert_eq!(scope_error.code(), "auv.run.artifact_scope_mismatch");

  let missing_execution = artifact(
    run_id,
    ArtifactScope::Execution {
      execution_id: ExecutionId::new(),
    },
  );
  let execution_error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    artifact_commit(run_id, 2, missing_execution),
  ])
  .unwrap_err();
  assert_eq!(execution_error.code(), "auv.run.artifact_execution_not_prepared");
}

#[test]
fn artifact_timestamps_follow_their_run_and_execution() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let before_open = Artifact::new(
    ArtifactRef::new(run_id, ArtifactId::new()),
    ArtifactScope::Run,
    ArtifactPurpose::parse("test.evidence").unwrap(),
    ArtifactContent::new(ContentType::parse("application/json").unwrap(), None, Sha256Digest::of_bytes(b"{}"), ByteLength::new(2)),
    timestamp(9),
    Attributes::default(),
  );
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    artifact_commit(run_id, 2, before_open),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.artifact_created_before_open");

  let before_execution = Artifact::new(
    ArtifactRef::new(run_id, ArtifactId::new()),
    ArtifactScope::Execution { execution_id },
    ArtifactPurpose::parse("test.evidence").unwrap(),
    ArtifactContent::new(ContentType::parse("application/json").unwrap(), None, Sha256Digest::of_bytes(b"{}"), ByteLength::new(2)),
    timestamp(19),
    Attributes::default(),
  );
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(run_id, 2, vec![RunChange::ExecutionPrepared(prepared(execution_id))]),
    artifact_commit(run_id, 3, before_execution),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.artifact_created_before_execution");
}

#[test]
fn execution_span_and_event_timestamps_follow_causal_starts() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let prepared_before_open = ExecutionPrepared::new(
    execution_id,
    OperationName::parse("test.operation").unwrap(),
    payload(serde_json::json!({ "input": 1 })),
    None,
    timestamp(9),
  );
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(run_id, 2, vec![RunChange::ExecutionPrepared(prepared_before_open)]),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.execution_prepared_before_open");

  let span_id = SpanId::new();
  let span_before_execution =
    SpanStarted::new(span_id, execution_id, None, SpanName::parse("test.span").unwrap(), timestamp(19), Attributes::default());
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![
        RunChange::ExecutionPrepared(prepared(execution_id)),
        RunChange::SpanStarted(span_before_execution),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.span_started_before_execution");

  let event_before_execution =
    EventAdded::new(EventId::new(), execution_id, None, EventName::parse("test.event").unwrap(), timestamp(19), Attributes::default());
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![
        RunChange::ExecutionPrepared(prepared(execution_id)),
        RunChange::EventAdded(event_before_execution),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.event_before_execution");
}

#[test]
fn child_spans_and_spanned_events_follow_their_parent_start() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let parent_id = SpanId::new();
  let child_id = SpanId::new();
  let parent =
    SpanStarted::new(parent_id, execution_id, None, SpanName::parse("test.parent").unwrap(), timestamp(23), Attributes::default());
  let child =
    SpanStarted::new(child_id, execution_id, Some(parent_id), SpanName::parse("test.child").unwrap(), timestamp(22), Attributes::default());
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![
        RunChange::ExecutionPrepared(prepared(execution_id)),
        RunChange::SpanStarted(parent.clone()),
        RunChange::SpanStarted(child),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.span_started_before_parent");

  let event = EventAdded::new(
    EventId::new(),
    execution_id,
    Some(parent_id),
    EventName::parse("test.event").unwrap(),
    timestamp(22),
    Attributes::default(),
  );
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![
        RunChange::ExecutionPrepared(prepared(execution_id)),
        RunChange::SpanStarted(parent),
        RunChange::EventAdded(event),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.event_before_span");
}

#[test]
fn span_and_event_timestamps_do_not_outlive_a_finished_execution() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let span_after_finish =
    SpanStarted::new(SpanId::new(), execution_id, None, SpanName::parse("test.span").unwrap(), timestamp(23), Attributes::default());
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![
        RunChange::ExecutionPrepared(prepared(execution_id)),
        RunChange::ExecutionFinished(finished(execution_id)),
        RunChange::SpanStarted(span_after_finish),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.span_started_after_execution");

  let event_after_finish =
    EventAdded::new(EventId::new(), execution_id, None, EventName::parse("test.event").unwrap(), timestamp(23), Attributes::default());
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![
        RunChange::ExecutionPrepared(prepared(execution_id)),
        RunChange::ExecutionFinished(finished(execution_id)),
        RunChange::EventAdded(event_after_finish),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.event_after_execution");
}

#[test]
fn execution_and_span_finishes_follow_already_accepted_scoped_facts() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let late_event =
    EventAdded::new(EventId::new(), execution_id, None, EventName::parse("test.event").unwrap(), timestamp(23), Attributes::default());
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![
        RunChange::ExecutionPrepared(prepared(execution_id)),
        RunChange::EventAdded(late_event),
        RunChange::ExecutionFinished(finished(execution_id)),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.execution_finished_before_scoped_fact");

  let span_id = SpanId::new();
  let start = SpanStarted::new(span_id, execution_id, None, SpanName::parse("test.span").unwrap(), timestamp(21), Attributes::default());
  let event = EventAdded::new(
    EventId::new(),
    execution_id,
    Some(span_id),
    EventName::parse("test.event").unwrap(),
    timestamp(23),
    Attributes::default(),
  );
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![
        RunChange::ExecutionPrepared(prepared(execution_id)),
        RunChange::SpanStarted(start),
        RunChange::EventAdded(event),
        RunChange::SpanFinished(SpanFinished::new(span_id, timestamp(22), Attributes::default())),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.span_finished_before_event");
}

#[test]
fn a_spanned_event_does_not_outlive_its_finished_span() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let span_id = SpanId::new();
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![
        RunChange::ExecutionPrepared(prepared(execution_id)),
        RunChange::SpanStarted(SpanStarted::new(
          span_id,
          execution_id,
          None,
          SpanName::parse("test.span").unwrap(),
          timestamp(21),
          Attributes::default(),
        )),
        RunChange::SpanFinished(SpanFinished::new(span_id, timestamp(22), Attributes::default())),
        RunChange::EventAdded(EventAdded::new(
          EventId::new(),
          execution_id,
          Some(span_id),
          EventName::parse("test.event").unwrap(),
          timestamp(23),
          Attributes::default(),
        )),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.event_after_span");
}

#[test]
fn span_parent_must_exist_first_and_belong_to_the_same_execution() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let child = SpanStarted::new(
    SpanId::new(),
    execution_id,
    Some(SpanId::new()),
    SpanName::parse("test.child").unwrap(),
    timestamp(23),
    Attributes::default(),
  );
  let missing_parent = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(run_id, 2, vec![RunChange::ExecutionPrepared(prepared(execution_id))]),
    commit(run_id, 3, vec![RunChange::SpanStarted(child)]),
  ])
  .unwrap_err();
  assert_eq!(missing_parent.code(), "auv.run.span_parent_not_started");

  let other_execution_id = ExecutionId::new();
  let parent_id = SpanId::new();
  let parent =
    SpanStarted::new(parent_id, execution_id, None, SpanName::parse("test.parent").unwrap(), timestamp(23), Attributes::default());
  let child = SpanStarted::new(
    SpanId::new(),
    other_execution_id,
    Some(parent_id),
    SpanName::parse("test.child").unwrap(),
    timestamp(24),
    Attributes::default(),
  );
  let wrong_execution = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![
        RunChange::ExecutionPrepared(prepared(execution_id)),
        RunChange::ExecutionPrepared(prepared(other_execution_id)),
        RunChange::SpanStarted(parent),
        RunChange::SpanStarted(child),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(wrong_execution.code(), "auv.run.span_parent_scope_mismatch");
}

#[test]
fn spans_finish_once_and_not_before_they_start() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let span_id = SpanId::new();
  let start = SpanStarted::new(span_id, execution_id, None, SpanName::parse("test.span").unwrap(), timestamp(23), Attributes::default());
  let early_finish = SpanFinished::new(span_id, timestamp(22), Attributes::default());
  let ordering = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![
        RunChange::ExecutionPrepared(prepared(execution_id)),
        RunChange::SpanStarted(start.clone()),
        RunChange::SpanFinished(early_finish),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(ordering.code(), "auv.run.span_finished_before_started");

  let finish = SpanFinished::new(span_id, timestamp(24), Attributes::default());
  let duplicate = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![
        RunChange::ExecutionPrepared(prepared(execution_id)),
        RunChange::SpanStarted(start),
        RunChange::SpanFinished(finish.clone()),
        RunChange::SpanFinished(finish),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(duplicate.code(), "auv.run.span_already_finished");
}

#[test]
fn spans_and_events_are_scoped_to_prepared_executions() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let span_error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![RunChange::SpanStarted(SpanStarted::new(
        SpanId::new(),
        execution_id,
        None,
        SpanName::parse("test.span").unwrap(),
        timestamp(23),
        Attributes::default(),
      ))],
    ),
  ])
  .unwrap_err();
  assert_eq!(span_error.code(), "auv.run.span_execution_not_prepared");

  let event_error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![RunChange::EventAdded(EventAdded::new(
        EventId::new(),
        execution_id,
        None,
        EventName::parse("test.event").unwrap(),
        timestamp(23),
        Attributes::default(),
      ))],
    ),
  ])
  .unwrap_err();
  assert_eq!(event_error.code(), "auv.run.event_execution_not_prepared");
}

#[test]
fn event_span_must_belong_to_the_event_execution() {
  let run_id = RunId::new();
  let span_execution_id = ExecutionId::new();
  let event_execution_id = ExecutionId::new();
  let span_id = SpanId::new();
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(
      run_id,
      2,
      vec![
        RunChange::ExecutionPrepared(prepared(span_execution_id)),
        RunChange::ExecutionPrepared(prepared(event_execution_id)),
        RunChange::SpanStarted(SpanStarted::new(
          span_id,
          span_execution_id,
          None,
          SpanName::parse("test.span").unwrap(),
          timestamp(23),
          Attributes::default(),
        )),
        RunChange::EventAdded(EventAdded::new(
          EventId::new(),
          event_execution_id,
          Some(span_id),
          EventName::parse("test.event").unwrap(),
          timestamp(24),
          Attributes::default(),
        )),
      ],
    ),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.event_span_scope_mismatch");
}

#[test]
fn seal_rejects_every_later_change() {
  let run_id = RunId::new();
  let sealed = RunSealed::new(timestamp(40), RunSealReason::parse("test.complete").unwrap());
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(run_id, 2, vec![RunChange::RunSealed(sealed)]),
    commit(run_id, 3, vec![RunChange::ExecutionPrepared(prepared(ExecutionId::new()))]),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.sealed");

  let sealed = RunSealed::new(timestamp(40), RunSealReason::parse("test.complete").unwrap());
  let artifact_after_seal = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(run_id, 2, vec![RunChange::RunSealed(sealed)]),
    artifact_commit(run_id, 3, artifact(run_id, ArtifactScope::Run)),
  ])
  .unwrap_err();
  assert_eq!(artifact_after_seal.code(), "auv.run.sealed");
}

#[test]
fn seal_rejects_a_same_commit_artifact_and_precedes_no_accepted_timestamp() {
  let run_id = RunId::new();
  let same_commit = RunCommit::new(
    run_id,
    Revision::new(2),
    IdempotencyKey::new(),
    timestamp(102),
    vec![RunChange::RunSealed(RunSealed::new(
      timestamp(40),
      RunSealReason::parse("test.complete").unwrap(),
    ))],
    vec![artifact(run_id, ArtifactScope::Run)],
  )
  .unwrap();
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    same_commit,
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.sealed");

  let execution_id = ExecutionId::new();
  let error = reduce(&[
    commit(run_id, 1, vec![RunChange::RunOpened(opened())]),
    commit(run_id, 2, vec![RunChange::ExecutionPrepared(prepared(execution_id))]),
    commit(
      run_id,
      3,
      vec![RunChange::RunSealed(RunSealed::new(
        timestamp(19),
        RunSealReason::parse("test.complete").unwrap(),
      ))],
    ),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.run.sealed_before_history");
}

#[test]
fn a_commit_is_reduced_as_one_atomic_candidate() {
  let run_id = RunId::new();
  let execution_id = ExecutionId::new();
  let open = commit(run_id, 1, vec![RunChange::RunOpened(opened())]);
  let invalid_batch = commit(
    run_id,
    2,
    vec![
      RunChange::ExecutionPrepared(prepared(execution_id)),
      RunChange::ExecutionPrepared(prepared(execution_id)),
    ],
  );

  assert_eq!(reduce(&[open.clone(), invalid_batch]).unwrap_err().code(), "auv.run.execution_already_prepared");
  let unchanged = reduce(&[open]).unwrap();
  assert!(unchanged.executions().is_empty());
  assert_eq!(unchanged.through_revision(), Revision::new(1));
}

#[test]
fn commit_constructors_and_deserializers_reject_empty_batches() {
  let run_id = RunId::new();
  let commit_error = RunCommit::new(run_id, Revision::new(1), IdempotencyKey::new(), timestamp(100), Vec::new(), Vec::new()).unwrap_err();
  assert_eq!(commit_error.code(), "auv.run.commit_empty");

  let request_error = RunCommitRequest::new(run_id, Revision::ZERO, IdempotencyKey::new(), Vec::new()).unwrap_err();
  assert_eq!(request_error.code(), "auv.run.commit_request_empty");

  let invalid_commit = serde_json::json!({
    "run_id": run_id,
    "revision": 1,
    "idempotency_key": IdempotencyKey::new(),
    "committed_at": timestamp(100),
    "changes": [],
    "artifacts": []
  });
  assert!(serde_json::from_value::<RunCommit>(invalid_commit).is_err());

  let invalid_request = serde_json::json!({
    "run_id": run_id,
    "expected_revision": 0,
    "idempotency_key": IdempotencyKey::new(),
    "changes": []
  });
  assert!(serde_json::from_value::<RunCommitRequest>(invalid_request).is_err());
}

#[test]
fn history_deserializers_reject_invalid_local_state_and_unknown_fields() {
  let invalid_finish = serde_json::json!({
    "execution_id": ExecutionId::new(),
    "started_at": timestamp(22),
    "finished_at": timestamp(21),
    "result": {
      "status": "completed",
      "output": payload(serde_json::json!({ "output": 2 }))
    },
    "verification_result": null
  });
  assert!(serde_json::from_value::<ExecutionFinished>(invalid_finish).is_err());

  let unknown_open_field = serde_json::json!({
    "opened_at": timestamp(10),
    "unexpected": true
  });
  assert!(serde_json::from_value::<RunOpened>(unknown_open_field).is_err());

  let unknown_scope_field = serde_json::json!({
    "type": "run",
    "unexpected": true
  });
  assert!(serde_json::from_value::<ArtifactScope>(unknown_scope_field).is_err());
}
