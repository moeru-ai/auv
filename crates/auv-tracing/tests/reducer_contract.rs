use auv_tracing::*;

fn timestamp(seconds: i64) -> Timestamp {
  Timestamp::new(seconds, 0).unwrap()
}

fn span_started(span_id: SpanId) -> SpanStarted {
  SpanStarted::new(span_id, None, None, SpanName::parse("auv.test.operation").unwrap(), timestamp(10), Attributes::empty())
}

#[test]
fn span_end_contains_no_outcome_or_status() {
  let json = serde_json::to_value(SpanEnded::new(SpanId::new(), timestamp(11))).unwrap();
  assert_eq!(json.as_object().unwrap().len(), 2);
  assert!(json.get("span_id").is_some());
  assert!(json.get("ended_at").is_some());
}

#[test]
fn missing_span_end_remains_open_without_inferred_reason() {
  let authority = AuthorityId::new();
  let run = RunId::new();
  let span = SpanId::new();
  let commit = RunCommit::new(
    authority,
    run,
    RunRevision::new(1).unwrap(),
    IdempotencyKey::new(),
    timestamp(10),
    vec![RunFact::SpanStarted(span_started(span))],
  )
  .unwrap();
  let snapshot = reduce_commits(&[commit]).unwrap();
  assert!(snapshot.spans().get(&span).unwrap().ended().is_none());
}

#[test]
fn reducer_rejects_event_after_span_end() {
  let authority = AuthorityId::new();
  let run = RunId::new();
  let span = SpanId::new();
  let event = EventOccurred::new(
    EventId::new(),
    Some(span),
    timestamp(12),
    EventSchema::new(EventName::parse("auv.test.event").unwrap(), 1).unwrap(),
    JsonPayload::from_str(r#"{"value":1}"#).unwrap(),
  );
  let commits = vec![
    RunCommit::new(
      authority,
      run,
      RunRevision::new(1).unwrap(),
      IdempotencyKey::new(),
      timestamp(10),
      vec![RunFact::SpanStarted(span_started(span))],
    )
    .unwrap(),
    RunCommit::new(
      authority,
      run,
      RunRevision::new(2).unwrap(),
      IdempotencyKey::new(),
      timestamp(11),
      vec![RunFact::SpanEnded(SpanEnded::new(span, timestamp(11)))],
    )
    .unwrap(),
    RunCommit::new(authority, run, RunRevision::new(3).unwrap(), IdempotencyKey::new(), timestamp(12), vec![RunFact::EventOccurred(event)])
      .unwrap(),
  ];
  assert_eq!(reduce_commits(&commits).unwrap_err(), ReduceError::EventAfterSpanEnd);
}

#[test]
fn typed_wire_records_reject_unknown_fields() {
  let event = format!(
    r#"{{"event_id":"{}","span_id":null,"occurred_at":{{"unix_seconds":1,"nanoseconds":0}},"schema":{{"name":"auv.test.event","version":1}},"payload":{{"value":1}},"surprise":true}}"#,
    EventId::new(),
  );
  assert!(serde_json::from_str::<EventOccurred>(&event).is_err());
}

#[test]
fn ordinary_commit_batches_are_bounded() {
  let mutations = (0..257).map(|_| RunMutation::EndSpan(SpanEnded::new(SpanId::new(), timestamp(11)))).collect::<Vec<_>>();
  assert!(RunCommitRequest::new(AuthorityId::new(), RunId::new(), IdempotencyKey::new(), mutations,).is_err());
}

fn span_started_with(span_id: SpanId, parent_span_id: Option<SpanId>, remote_link: Option<SpanLink>, started_at: i64) -> SpanStarted {
  SpanStarted::new(
    span_id,
    parent_span_id,
    remote_link,
    SpanName::parse("auv.test.operation").unwrap(),
    timestamp(started_at),
    Attributes::empty(),
  )
}

fn event_occurred(event_id: EventId, span_id: Option<SpanId>, occurred_at: i64) -> EventOccurred {
  EventOccurred::new(
    event_id,
    span_id,
    timestamp(occurred_at),
    EventSchema::new(EventName::parse("auv.test.event").unwrap(), 1).unwrap(),
    JsonPayload::from_str(r#"{"value":1}"#).unwrap(),
  )
}

fn artifact_published(run_id: RunId, artifact_id: ArtifactId, span_id: Option<SpanId>) -> ArtifactPublished {
  ArtifactPublished::new(
    span_id,
    ArtifactMetadata::new(
      ArtifactUri::from_ids(run_id, artifact_id),
      ArtifactPurpose::parse("auv.test.capture").unwrap(),
      ContentType::parse("application/octet-stream").unwrap(),
      ByteLength::new(0).unwrap(),
      Sha256Digest::new([0; 32]),
      Attributes::empty(),
    ),
  )
}

fn commit(authority: AuthorityId, run: RunId, revision: u64, facts: Vec<RunFact>) -> RunCommit {
  RunCommit::new(authority, run, RunRevision::new(revision).unwrap(), IdempotencyKey::new(), timestamp(20 + revision as i64), facts).unwrap()
}

#[test]
fn history_unions_are_externally_tagged_and_strict_inside_payloads() {
  let span_id = SpanId::new();
  let mutation = serde_json::to_value(RunMutation::StartSpan(span_started(span_id))).unwrap();
  assert_eq!(mutation.as_object().unwrap().len(), 1);
  assert!(mutation.get("start_span").is_some());

  let mut payload = mutation.get("start_span").unwrap().clone();
  payload.as_object_mut().unwrap().insert("surprise".into(), true.into());
  let invalid = serde_json::json!({ "start_span": payload });
  assert!(serde_json::from_value::<RunMutation>(invalid).is_err());
  assert!(serde_json::from_value::<RunMutation>(serde_json::json!({ "kind": "start_span" })).is_err());
}

#[test]
fn commit_constructors_and_deserializers_enforce_batch_bounds() {
  let authority = AuthorityId::new();
  let run = RunId::new();
  assert!(RunCommitRequest::new(authority, run, IdempotencyKey::new(), Vec::new()).is_err());
  assert!(RunCommit::new(authority, run, RunRevision::new(1).unwrap(), IdempotencyKey::new(), timestamp(1), Vec::new(),).is_err());

  let request = RunCommitRequest::new(
    authority,
    run,
    IdempotencyKey::new(),
    vec![RunMutation::EndSpan(SpanEnded::new(
      SpanId::new(),
      timestamp(1),
    ))],
  )
  .unwrap();
  let mut request_json = serde_json::to_value(request).unwrap();
  request_json["mutations"] = serde_json::Value::Array(Vec::new());
  assert!(serde_json::from_value::<RunCommitRequest>(request_json.clone()).is_err());
  request_json["mutations"] = serde_json::Value::Array(
    (0..257).map(|_| serde_json::to_value(RunMutation::EndSpan(SpanEnded::new(SpanId::new(), timestamp(1)))).unwrap()).collect(),
  );
  assert!(serde_json::from_value::<RunCommitRequest>(request_json).is_err());

  let mut commit_json = serde_json::to_value(commit(authority, run, 1, vec![RunFact::SpanStarted(span_started(SpanId::new()))])).unwrap();
  commit_json["facts"] = serde_json::Value::Array(Vec::new());
  assert!(serde_json::from_value::<RunCommit>(commit_json.clone()).is_err());
  commit_json["facts"] = serde_json::Value::Array(
    (0..257).map(|_| serde_json::to_value(RunFact::SpanEnded(SpanEnded::new(SpanId::new(), timestamp(1)))).unwrap()).collect(),
  );
  assert!(serde_json::from_value::<RunCommit>(commit_json).is_err());
}

#[test]
fn commit_accessors_expose_the_validated_wire_values() {
  let authority = AuthorityId::new();
  let run = RunId::new();
  let key = IdempotencyKey::new();
  let request = RunCommitRequest::new(
    authority,
    run,
    key,
    vec![RunMutation::EndSpan(SpanEnded::new(
      SpanId::new(),
      timestamp(1),
    ))],
  )
  .unwrap();
  assert_eq!(request.authority_id(), authority);
  assert_eq!(request.run_id(), run);
  assert_eq!(request.idempotency_key(), key);
  assert_eq!(request.mutations().len(), 1);

  let revision = RunRevision::new(1).unwrap();
  let committed_at = timestamp(2);
  let committed = RunCommit::new(
    authority,
    run,
    revision,
    key,
    committed_at,
    vec![RunFact::SpanEnded(SpanEnded::new(
      SpanId::new(),
      timestamp(1),
    ))],
  )
  .unwrap();
  assert_eq!(committed.authority_id(), authority);
  assert_eq!(committed.run_id(), run);
  assert_eq!(committed.revision(), revision);
  assert_eq!(committed.idempotency_key(), key);
  assert_eq!(committed.committed_at(), committed_at);
  assert_eq!(committed.facts().len(), 1);
}

#[test]
fn reducer_requires_nonempty_history_starting_at_revision_one_without_gaps() {
  assert_eq!(reduce_commits(&[]).unwrap_err(), ReduceError::EmptyHistory);

  let authority = AuthorityId::new();
  let run = RunId::new();
  let span = SpanId::new();
  assert_eq!(
    reduce_commits(&[commit(
      authority,
      run,
      2,
      vec![RunFact::SpanStarted(span_started(span))]
    )])
    .unwrap_err(),
    ReduceError::NonContiguousRevision,
  );
  assert_eq!(
    reduce_commits(&[
      commit(authority, run, 1, vec![RunFact::SpanStarted(span_started(span))]),
      commit(authority, run, 3, vec![RunFact::SpanEnded(SpanEnded::new(span, timestamp(11)))]),
    ])
    .unwrap_err(),
    ReduceError::NonContiguousRevision,
  );
}

#[test]
fn reducer_rejects_mixed_authority_and_run_histories() {
  let authority = AuthorityId::new();
  let run = RunId::new();
  let span = SpanId::new();
  let first = commit(authority, run, 1, vec![RunFact::SpanStarted(span_started(span))]);
  assert_eq!(
    reduce_commits(&[
      first.clone(),
      commit(AuthorityId::new(), run, 2, vec![RunFact::SpanEnded(SpanEnded::new(span, timestamp(11)))]),
    ])
    .unwrap_err(),
    ReduceError::MixedAuthority,
  );
  assert_eq!(
    reduce_commits(&[
      first,
      commit(authority, RunId::new(), 2, vec![RunFact::SpanEnded(SpanEnded::new(span, timestamp(11)))]),
    ])
    .unwrap_err(),
    ReduceError::MixedRun,
  );
}

#[test]
fn reducer_rejects_duplicate_or_unmatched_span_lifecycle_facts() {
  let authority = AuthorityId::new();
  let run = RunId::new();
  let span = SpanId::new();
  assert_eq!(
    reduce_commits(&[commit(
      authority,
      run,
      1,
      vec![
        RunFact::SpanStarted(span_started(span)),
        RunFact::SpanStarted(span_started(span))
      ],
    )])
    .unwrap_err(),
    ReduceError::DuplicateSpanStart,
  );
  assert_eq!(
    reduce_commits(&[commit(
      authority,
      run,
      1,
      vec![RunFact::SpanEnded(SpanEnded::new(span, timestamp(11)))],
    )])
    .unwrap_err(),
    ReduceError::UnknownSpanEnd,
  );
  assert_eq!(
    reduce_commits(&[
      commit(authority, run, 1, vec![RunFact::SpanStarted(span_started(span))]),
      commit(
        authority,
        run,
        2,
        vec![
          RunFact::SpanEnded(SpanEnded::new(span, timestamp(11))),
          RunFact::SpanEnded(SpanEnded::new(span, timestamp(12))),
        ],
      ),
    ])
    .unwrap_err(),
    ReduceError::DuplicateSpanEnd,
  );
}

#[test]
fn reducer_rejects_invalid_local_parent_graphs() {
  let authority = AuthorityId::new();
  let run = RunId::new();
  let parent = SpanId::new();
  let child = SpanId::new();
  assert_eq!(
    reduce_commits(&[commit(
      authority,
      run,
      1,
      vec![RunFact::SpanStarted(span_started_with(
        child,
        Some(parent),
        None,
        10
      ))],
    )])
    .unwrap_err(),
    ReduceError::MissingLocalParent,
  );
  assert_eq!(
    reduce_commits(&[commit(
      authority,
      run,
      1,
      vec![
        RunFact::SpanStarted(span_started_with(parent, Some(child), None, 10)),
        RunFact::SpanStarted(span_started_with(child, Some(parent), None, 10)),
      ],
    )])
    .unwrap_err(),
    ReduceError::CyclicLocalParent,
  );
  assert_eq!(
    reduce_commits(&[commit(
      authority,
      run,
      1,
      vec![RunFact::SpanStarted(span_started_with(
        parent,
        Some(parent),
        None,
        10
      ))],
    )])
    .unwrap_err(),
    ReduceError::SelfParent,
  );
  assert_eq!(
    reduce_commits(&[commit(
      authority,
      run,
      1,
      vec![RunFact::SpanStarted(span_started_with(
        parent,
        None,
        Some(SpanLink::new(parent)),
        10
      ))],
    )])
    .unwrap_err(),
    ReduceError::SelfRemoteLink,
  );
  assert_eq!(
    reduce_commits(&[commit(
      authority,
      run,
      1,
      vec![
        RunFact::SpanStarted(span_started(parent)),
        RunFact::SpanStarted(span_started_with(child, Some(parent), Some(SpanLink::new(parent)), 10)),
      ],
    )])
    .unwrap_err(),
    ReduceError::DuplicateParentLink,
  );
}

#[test]
fn reducer_rejects_invalid_span_and_event_time_order() {
  let authority = AuthorityId::new();
  let run = RunId::new();
  let span = SpanId::new();
  assert_eq!(
    reduce_commits(&[commit(
      authority,
      run,
      1,
      vec![
        RunFact::SpanStarted(span_started(span)),
        RunFact::SpanEnded(SpanEnded::new(span, timestamp(9))),
      ],
    )])
    .unwrap_err(),
    ReduceError::EndBeforeStart,
  );
  assert_eq!(
    reduce_commits(&[commit(
      authority,
      run,
      1,
      vec![
        RunFact::SpanStarted(span_started(span)),
        RunFact::EventOccurred(event_occurred(EventId::new(), Some(span), 9)),
      ],
    )])
    .unwrap_err(),
    ReduceError::EventBeforeSpanStart,
  );
  assert_eq!(
    reduce_commits(&[commit(
      authority,
      run,
      1,
      vec![
        RunFact::SpanStarted(span_started(span)),
        RunFact::EventOccurred(event_occurred(EventId::new(), Some(span), 12)),
        RunFact::SpanEnded(SpanEnded::new(span, timestamp(11))),
      ],
    )])
    .unwrap_err(),
    ReduceError::EventAfterSpanEnd,
  );
}

#[test]
fn reducer_rejects_unknown_and_duplicate_events() {
  let authority = AuthorityId::new();
  let run = RunId::new();
  let span = SpanId::new();
  let event_id = EventId::new();
  assert_eq!(
    reduce_commits(&[commit(
      authority,
      run,
      1,
      vec![RunFact::EventOccurred(event_occurred(
        event_id,
        Some(span),
        10
      ))],
    )])
    .unwrap_err(),
    ReduceError::UnknownEventSpan,
  );
  assert_eq!(
    reduce_commits(&[
      commit(authority, run, 1, vec![RunFact::EventOccurred(event_occurred(event_id, None, 10))]),
      commit(authority, run, 2, vec![RunFact::EventOccurred(event_occurred(event_id, None, 11))]),
    ])
    .unwrap_err(),
    ReduceError::DuplicateEventId,
  );
}

#[test]
fn reducer_rejects_invalid_artifacts_but_allows_publication_after_span_end() {
  let authority = AuthorityId::new();
  let run = RunId::new();
  let span = SpanId::new();
  let artifact_id = ArtifactId::new();
  assert_eq!(
    reduce_commits(&[commit(
      authority,
      run,
      1,
      vec![RunFact::ArtifactPublished(artifact_published(
        run,
        artifact_id,
        Some(span)
      ))],
    )])
    .unwrap_err(),
    ReduceError::UnknownArtifactSpan,
  );
  assert_eq!(
    reduce_commits(&[commit(
      authority,
      run,
      1,
      vec![
        RunFact::ArtifactPublished(artifact_published(run, artifact_id, None)),
        RunFact::ArtifactPublished(artifact_published(run, artifact_id, None)),
      ],
    )])
    .unwrap_err(),
    ReduceError::DuplicateArtifactUri,
  );

  let snapshot = reduce_commits(&[
    commit(authority, run, 1, vec![RunFact::SpanStarted(span_started(span))]),
    commit(authority, run, 2, vec![RunFact::SpanEnded(SpanEnded::new(span, timestamp(11)))]),
    commit(
      authority,
      run,
      3,
      vec![RunFact::ArtifactPublished(artifact_published(
        run,
        artifact_id,
        Some(span),
      ))],
    ),
  ])
  .unwrap();
  assert_eq!(snapshot.artifacts().len(), 1);
  assert_eq!(snapshot.events().len(), 0);
  assert_eq!(snapshot.through_revision(), RunRevision::new(3).unwrap());
}
