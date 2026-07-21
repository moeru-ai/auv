#![cfg(feature = "memory-store")]

mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Barrier};
use std::task::Context as TaskContext;
use std::time::{Duration, Instant};

use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, ArtifactWriteError, Attributes, ByteLength, ContentType, DispatchStage, ErrorCode, EventPayload,
  NewArtifact, RunId, RunStore, Sha256Digest, SpanSpec, TelemetryItem, TelemetryRoutePolicy, configure, dispatcher,
};
use sha2::{Digest, Sha256};
use support::{
  ArtifactStore, CommitUnknownStore, ControlledStore, CursorStore, DropFirstTaskSpawner, DropNthTaskSpawner, IntegrityFault, IntegrityStore,
  ProbeReader, ReadGate, RecordingProjector, RecordingReporter, TrackingTaskSpawner, UnknownLookup, WAIT_TIMEOUT, block_on_timeout,
};

#[derive(serde::Serialize)]
struct TestEvent {
  value: u32,
}

impl EventPayload for TestEvent {
  const NAME: &'static str = "auv.test.artifact_event";
  const VERSION: u32 = 1;
}

struct TestSpan;

impl SpanSpec for TestSpan {
  const NAME: &'static str = "auv.test.artifact_span";

  fn attributes(&self) -> Attributes {
    Attributes::empty()
  }
}

fn test_artifact<R>(body: R, bytes: &[u8]) -> NewArtifact<R> {
  NewArtifact::new(
    ArtifactPurpose::parse("auv.test.capture").unwrap(),
    ContentType::parse("application/octet-stream").unwrap(),
    ByteLength::new(bytes.len() as u64).unwrap(),
    Sha256Digest::new(Sha256::digest(bytes).into()),
    Attributes::empty(),
    body,
  )
}

fn ready_artifact(bytes: &[u8]) -> NewArtifact<futures_util::io::Cursor<Vec<u8>>> {
  test_artifact(futures_util::io::Cursor::new(bytes.to_vec()), bytes)
}

fn context(dispatch: &auv_tracing::Dispatch, run_id: RunId) -> auv_tracing::Context {
  dispatcher::with_default(dispatch, || auv_tracing::Context::root(run_id))
}

fn wait_for_revision(store: &dyn RunStore, run_id: RunId, expected: u64) {
  let deadline = Instant::now() + WAIT_TIMEOUT;
  loop {
    let revision = block_on_timeout(store.load_snapshot(run_id)).unwrap().map(|snapshot| snapshot.through_revision().get()).unwrap_or(0);
    if revision >= expected {
      return;
    }
    assert!(Instant::now() < deadline, "timed out waiting for authority revision {expected}; observed {revision}");
    std::thread::yield_now();
  }
}

fn wait_for_projected(projector: &RecordingProjector, expected: usize) {
  let deadline = Instant::now() + WAIT_TIMEOUT;
  while projector.item_count() < expected {
    assert!(Instant::now() < deadline, "timed out waiting for {expected} projected facts");
    std::thread::yield_now();
  }
}

fn projected_revisions(projector: &RecordingProjector) -> Vec<u64> {
  projector
    .items()
    .iter()
    .map(|item| match item {
      TelemetryItem::SpanStart { start_revision, .. } => start_revision.unwrap().get(),
      TelemetryItem::SpanEnd { end_revision, .. } => end_revision.unwrap().get(),
      TelemetryItem::Event { revision, .. } => revision.unwrap().get(),
      TelemetryItem::Artifact { revision, .. } => revision.get(),
    })
    .collect()
}

fn projected_kinds(projector: &RecordingProjector) -> Vec<&'static str> {
  projector
    .items()
    .iter()
    .map(|item| match item {
      TelemetryItem::SpanStart { .. } => "span_started",
      TelemetryItem::SpanEnd { .. } => "span_ended",
      TelemetryItem::Event { .. } => "event_occurred",
      TelemetryItem::Artifact { .. } => "artifact_published",
    })
    .collect()
}

#[test]
fn disabled_artifact_does_not_poll_body() {
  let polled = Arc::new(AtomicBool::new(false));

  let result = block_on_timeout(auv_tracing::emit_artifact(test_artifact(ProbeReader::new(polled.clone()), &[]))).unwrap();

  assert!(result.is_none());
  assert!(!polled.load(Ordering::SeqCst));
}

#[test]
fn telemetry_only_artifact_does_not_poll_body() {
  let projector = RecordingProjector::new();
  let dispatch = configure().project_telemetry(projector, TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let root = context(&dispatch, RunId::new());
  let polled = Arc::new(AtomicBool::new(false));

  let result = root.in_scope(|| block_on_timeout(auv_tracing::emit_artifact(test_artifact(ProbeReader::new(polled.clone()), &[])))).unwrap();

  assert!(result.is_none());
  assert!(!polled.load(Ordering::SeqCst));
}

#[test]
fn referenced_span_start_is_committed_before_artifact_body_polling() {
  let store = ArtifactStore::new();
  let start_gate = store.block_next_commit();
  let dispatch = configure().run_store(store.clone()).build().unwrap();
  let root = context(&dispatch, RunId::new());
  let body_gate = ReadGate::new(b"fenced".to_vec());

  let (span, receipt) = root.in_scope(|| {
    let span = auv_tracing::start_span!(TestSpan);
    let receipt = span.in_scope(|| auv_tracing::emit_artifact(test_artifact(body_gate.reader(), b"fenced")));
    (span, receipt)
  });
  assert_eq!(store.write_call_count(), 0, "artifact write must wait behind its preceding span-start fence");

  start_gate.release();
  body_gate.wait_until_polled();
  body_gate.release();
  assert!(block_on_timeout(receipt).unwrap().is_some());
  drop(span);
  block_on_timeout(dispatch.flush()).unwrap();
}

#[test]
fn slow_artifact_does_not_block_later_facts_and_projects_by_revision() {
  let store = ArtifactStore::new();
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let run_id = RunId::new();
  let root = context(&dispatch, run_id);
  let body_gate = ReadGate::new(b"slow".to_vec());

  root.in_scope(|| {
    let span = auv_tracing::start_span!(TestSpan);
    let receipt = span.in_scope(|| auv_tracing::emit_artifact(test_artifact(body_gate.reader(), b"slow")));
    span.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 7 }));
    drop(span);
    drop(receipt);
  });

  body_gate.wait_until_polled();
  wait_for_revision(store.as_ref(), run_id, 3);
  wait_for_projected(projector.as_ref(), 3);
  assert_eq!(projected_revisions(projector.as_ref()), [1, 2, 3]);
  assert_eq!(projected_kinds(projector.as_ref()), ["span_started", "event_occurred", "span_ended"]);
  assert!(block_on_timeout(store.load_snapshot(run_id)).unwrap().unwrap().artifacts().is_empty());

  body_gate.release();
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(projected_revisions(projector.as_ref()), [1, 2, 3, 4]);
  assert_eq!(
    projected_kinds(projector.as_ref()),
    [
      "span_started",
      "event_occurred",
      "span_ended",
      "artifact_published"
    ]
  );
}

#[test]
fn fast_artifact_projects_before_later_admitted_but_uncommitted_fact() {
  let store = ArtifactStore::new();
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let run_id = RunId::new();
  let root = context(&dispatch, run_id);
  let span = root.in_scope(|| auv_tracing::start_span!(TestSpan));
  block_on_timeout(dispatch.flush()).unwrap();
  let event_gate = store.block_next_commit();

  let receipt = span.in_scope(|| {
    let receipt = auv_tracing::emit_artifact(ready_artifact(b"fast"));
    auv_tracing::emit_event!(TestEvent { value: 9 });
    receipt
  });

  event_gate.wait_until_entered();
  wait_for_revision(store.as_ref(), run_id, 2);
  assert!(block_on_timeout(receipt).unwrap().is_some());
  event_gate.release();
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(projected_revisions(projector.as_ref()), [1, 2, 3]);
  assert_eq!(projected_kinds(projector.as_ref()), ["span_started", "artifact_published", "event_occurred"]);
  drop(span);
  block_on_timeout(dispatch.flush()).unwrap();
}

#[test]
fn dropping_receipt_does_not_cancel_accepted_artifact_job() {
  let store = ArtifactStore::new();
  let dispatch = configure().run_store(store.clone()).build().unwrap();
  let run_id = RunId::new();
  let root = context(&dispatch, run_id);
  let body_gate = ReadGate::new(b"detached".to_vec());

  let receipt = root.in_scope(|| auv_tracing::emit_artifact(test_artifact(body_gate.reader(), b"detached")));
  drop(receipt);
  body_gate.wait_until_polled();
  body_gate.release();
  block_on_timeout(dispatch.flush()).unwrap();

  assert_eq!(block_on_timeout(store.load_snapshot(run_id)).unwrap().unwrap().artifacts().len(), 1);
}

#[test]
fn flush_barrier_waits_for_previously_admitted_artifact_and_excludes_later_emission() {
  let store = ArtifactStore::new();
  let dispatch = configure().run_store(store.clone()).build().unwrap();
  let run_id = RunId::new();
  let root = context(&dispatch, run_id);
  let body_gate = ReadGate::new(b"barrier".to_vec());

  let receipt = root.in_scope(|| auv_tracing::emit_artifact(test_artifact(body_gate.reader(), b"barrier")));
  body_gate.wait_until_polled();
  let mut flush = dispatch.flush();
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 11 }));
  let waker = futures_util::task::noop_waker();
  let mut task_context = TaskContext::from_waker(&waker);
  assert!(flush.as_mut().poll(&mut task_context).is_pending());

  body_gate.release();
  assert!(block_on_timeout(receipt).unwrap().is_some());
  block_on_timeout(flush).unwrap();
  block_on_timeout(dispatch.flush()).unwrap();
  let snapshot = block_on_timeout(store.load_snapshot(run_id)).unwrap().unwrap();
  assert_eq!(snapshot.artifacts().len(), 1);
  assert_eq!(snapshot.events().len(), 1);
}

#[test]
fn pre_barrier_artifact_flush_does_not_wait_for_blocked_post_barrier_commit() {
  let store = ArtifactStore::new();
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let root = context(&dispatch, RunId::new());
  let body_gate = ReadGate::new(b"pre-barrier".to_vec());
  let receipt = root.in_scope(|| auv_tracing::emit_artifact(test_artifact(body_gate.reader(), b"pre-barrier")));
  body_gate.wait_until_polled();

  let flush_dispatch = dispatch.clone();
  let (barrier_sender, barrier_receiver) = sync_channel(1);
  let (completion_sender, completion_receiver) = sync_channel(1);
  let flush_thread = std::thread::spawn(move || {
    let flush = flush_dispatch.flush();
    barrier_sender.send(()).unwrap();
    completion_sender.send(block_on_timeout(flush)).unwrap();
  });
  barrier_receiver.recv_timeout(WAIT_TIMEOUT).unwrap();

  let later_gate = store.block_next_commit();
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 12 }));
  later_gate.wait_until_entered();
  body_gate.release();
  assert!(block_on_timeout(receipt).unwrap().is_some());

  let first_flush = completion_receiver.recv_timeout(Duration::from_secs(1));
  if first_flush.is_err() {
    later_gate.release();
    flush_thread.join().unwrap();
    panic!("pre-barrier artifact flush waited for a blocked post-barrier ordinary commit");
  }
  first_flush.unwrap().unwrap();
  assert_eq!(projected_kinds(projector.as_ref()), ["artifact_published"]);

  later_gate.release();
  flush_thread.join().unwrap();
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(projected_kinds(projector.as_ref()), ["artifact_published", "event_occurred"]);
}

#[test]
fn pre_barrier_flush_projects_an_earlier_post_barrier_revision_first() {
  let store = ArtifactStore::new();
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let run_id = RunId::new();
  let root = context(&dispatch, run_id);
  let body_gate = ReadGate::new(b"later-revision".to_vec());
  let receipt = root.in_scope(|| auv_tracing::emit_artifact(test_artifact(body_gate.reader(), b"later-revision")));
  body_gate.wait_until_polled();
  let flush = dispatch.flush();

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 13 }));
  wait_for_revision(store.as_ref(), run_id, 1);
  wait_for_projected(projector.as_ref(), 1);
  assert_eq!(projected_kinds(projector.as_ref()), ["event_occurred"]);

  body_gate.release();
  assert!(block_on_timeout(receipt).unwrap().is_some());
  block_on_timeout(flush).unwrap();
  assert_eq!(projected_revisions(projector.as_ref()), [1, 2]);
  assert_eq!(projected_kinds(projector.as_ref()), ["event_occurred", "artifact_published"]);
  block_on_timeout(dispatch.flush()).unwrap();
}

#[test]
fn higher_revision_target_prunes_an_already_observed_queued_target() {
  let store = ArtifactStore::new();
  let response_gate = store.store_then_wait_response_next();
  let page_gate = store.block_next_observation_page();
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let root = context(&dispatch, RunId::new());
  let receipt = root.in_scope(|| auv_tracing::emit_artifact(ready_artifact(b"target-inversion")));
  response_gate.wait_until_entered();

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 14 }));
  page_gate.wait_until_entered();
  response_gate.release();
  assert!(block_on_timeout(receipt).unwrap().is_some());
  page_gate.release();

  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(projected_revisions(projector.as_ref()), [1, 2]);
  assert_eq!(projected_kinds(projector.as_ref()), ["artifact_published", "event_occurred"]);
  block_on_timeout(dispatch.flush()).unwrap();
}

#[test]
fn cursor_proof_overrides_a_later_artifact_publication_unknown() {
  let store = ArtifactStore::new();
  store.store_then_unknown_next(ErrorCode::parse("auv.test.artifact_response_lost").unwrap());
  let lookup_gate = store.block_lookup_then_none();
  let projector = RecordingProjector::new();
  let reporter = RecordingReporter::new();
  let dispatch = configure()
    .run_store(store.clone())
    .project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only())
    .on_error(reporter.clone())
    .build()
    .unwrap();
  let run_id = RunId::new();
  let root = context(&dispatch, run_id);
  let receipt = root.in_scope(|| auv_tracing::emit_artifact(ready_artifact(b"cursor-proof")));
  lookup_gate.wait_until_entered();

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 15 }));
  wait_for_revision(store.as_ref(), run_id, 2);
  assert_eq!(projector.item_count(), 0, "cursor proof remains staged until the ambiguous artifact response resolves");
  lookup_gate.release();

  assert!(block_on_timeout(receipt).unwrap().is_some());
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(store.write_call_count(), 1);
  assert_eq!(store.lookup_call_count(), 1);
  assert!(reporter.failures().is_empty());
  assert_eq!(projected_revisions(projector.as_ref()), [1, 2]);
}

#[test]
fn cursor_proof_overrides_a_later_ordinary_commit_unknown() {
  let store = CommitUnknownStore::new(UnknownLookup::CommittedButMissing);
  let lookup_gate = store.block_lookup();
  let projector = RecordingProjector::new();
  let spawner = TrackingTaskSpawner::new();
  let dispatch = configure()
    .run_store(store.clone())
    .project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only())
    .task_spawner(spawner.clone())
    .build()
    .unwrap();
  let run_id = RunId::new();
  let root = context(&dispatch, run_id);
  let body_gate = ReadGate::new(b"ordinary-proof".to_vec());
  let receipt = root.in_scope(|| auv_tracing::emit_artifact(test_artifact(body_gate.reader(), b"ordinary-proof")));
  body_gate.wait_until_polled();

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 16 }));
  lookup_gate.wait_until_entered();
  body_gate.release();
  assert!(block_on_timeout(receipt).unwrap().is_some());
  spawner.wait_for_active(1, 4);
  assert_eq!(projector.item_count(), 0, "cursor proof remains staged until the ambiguous ordinary response resolves");
  lookup_gate.release();

  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(store.commit_calls(), 1);
  assert_eq!(store.lookup_calls(), 1);
  assert_eq!(projected_revisions(projector.as_ref()), [1, 2]);
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 17 }));
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(store.commit_calls(), 2, "cursor proof must keep the run lane usable");
}

#[test]
fn recovered_target_projects_when_resubscribe_fails_and_the_next_target_reestablishes() {
  let store = CursorStore::pending_then_resubscribe_failure();
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let root = context(&dispatch, RunId::new());

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 18 }));
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(projected_revisions(projector.as_ref()), [1]);
  assert_eq!(store.commits_after_call_count(), 1);
  assert_eq!(store.subscribe_call_count(), 2, "the failed replacement subscription is attempted once");

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 19 }));
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(projected_revisions(projector.as_ref()), [1, 2]);
  assert!(store.subscribe_call_count() >= 3, "the next target must establish a replacement cursor");
}

#[test]
fn artifact_direct_contradiction_after_cursor_proof_fails_without_lookup_or_projection() {
  let store = ArtifactStore::new();
  let response_gate = store.store_then_wait_mismatch_next();
  let page_gate = store.block_next_observation_page();
  let projector = RecordingProjector::new();
  let reporter = RecordingReporter::new();
  let spawner = TrackingTaskSpawner::new();
  let dispatch = configure()
    .run_store(store.clone())
    .project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only())
    .on_error(reporter.clone())
    .task_spawner(spawner.clone())
    .build()
    .unwrap();
  let run_id = RunId::new();
  let root = context(&dispatch, run_id);

  let receipt = root.in_scope(|| auv_tracing::emit_artifact(ready_artifact(b"artifact-contradiction")));
  response_gate.wait_until_entered();
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 20 }));
  page_gate.wait_until_entered();
  page_gate.release();
  spawner.wait_for_active(1, 4);
  assert_eq!(projector.item_count(), 0, "cursor proof must remain staged until the artifact response is classified");

  response_gate.release();
  let error = block_on_timeout(receipt).unwrap_err();
  assert_eq!(error, ArtifactWriteError::Integrity(ErrorCode::parse("auv.dispatch.commit_response_mismatch").unwrap()));
  let flush = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(flush.failure_count().get(), 1);
  assert_eq!(flush.first().stage(), DispatchStage::ArtifactWrite);
  assert_eq!(store.write_call_count(), 1);
  assert_eq!(store.lookup_call_count(), 0, "a contradictory direct response must never enter publication lookup");
  assert_eq!(block_on_timeout(store.load_snapshot(run_id)).unwrap().unwrap().artifacts().len(), 1);
  assert_eq!(projected_kinds(projector.as_ref()), ["event_occurred"]);
  assert!(reporter.failures().is_empty(), "the awaited receipt owns failure observation");
  block_on_timeout(dispatch.flush()).unwrap();

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 21 }));
  let quarantined = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(quarantined.first().code().as_str(), "auv.dispatch.run_lane_indeterminate");
}

#[test]
fn artifact_body_integrity_failure_does_not_quarantine_the_run_lane() {
  let store = ArtifactStore::new();
  let dispatch = configure().run_store(store.clone()).build().unwrap();
  let run_id = RunId::new();
  let root = context(&dispatch, run_id);
  let artifact = test_artifact(futures_util::io::Cursor::new(b"actual".to_vec()), b"expect");

  let result = root.in_scope(|| block_on_timeout(auv_tracing::emit_artifact(artifact)));
  assert!(matches!(result, Err(ArtifactWriteError::Integrity(_))));
  let failed = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(failed.failure_count().get(), 1);
  assert_eq!(failed.first().stage(), DispatchStage::ArtifactWrite);

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 24 }));
  block_on_timeout(dispatch.flush()).unwrap();
  let snapshot = block_on_timeout(store.load_snapshot(run_id)).unwrap().unwrap();
  assert!(snapshot.artifacts().is_empty());
  assert_eq!(snapshot.events().len(), 1);
}

#[test]
fn ordinary_direct_contradiction_after_cursor_proof_skips_that_projection_and_quarantines() {
  let run_id = RunId::new();
  let store = IntegrityStore::new(run_id, IntegrityFault::DirectResponseMismatch);
  let response_gate = store.block_direct_response();
  let page_gate = store.block_observation_page();
  let projector = RecordingProjector::new();
  let reporter = RecordingReporter::new();
  let spawner = TrackingTaskSpawner::new();
  let dispatch = configure()
    .run_store(store.clone())
    .project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only())
    .on_error(reporter.clone())
    .task_spawner(spawner.clone())
    .build()
    .unwrap();
  let root = context(&dispatch, run_id);
  let body_gate = ReadGate::new(b"ordinary-contradiction".to_vec());
  let receipt = root.in_scope(|| auv_tracing::emit_artifact(test_artifact(body_gate.reader(), b"ordinary-contradiction")));
  body_gate.wait_until_polled();

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 22 }));
  response_gate.wait_until_entered();
  body_gate.release();
  assert!(block_on_timeout(receipt).unwrap().is_some());
  page_gate.wait_until_entered();
  page_gate.release();
  spawner.wait_for_active(1, 4);
  assert_eq!(projector.item_count(), 0, "cursor proof must remain staged until the ordinary response is classified");

  response_gate.release();
  let flush = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(flush.failure_count().get(), 1);
  assert_eq!(flush.first().stage(), DispatchStage::AuthorityCommit);
  assert_eq!(flush.first().code().as_str(), "auv.dispatch.commit_response_mismatch");
  assert_eq!(projected_kinds(projector.as_ref()), ["artifact_published"]);
  assert_eq!(reporter.failures(), [flush.first().clone()]);
  block_on_timeout(dispatch.flush()).unwrap();

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 23 }));
  let quarantined = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(quarantined.first().code().as_str(), "auv.dispatch.run_lane_indeterminate");
  assert_eq!(store.commit_call_count(run_id), 1);
}

#[test]
fn receipt_polling_under_another_context_does_not_change_artifact_ownership() {
  let first_store = ArtifactStore::new();
  let first_dispatch = configure().run_store(first_store.clone()).build().unwrap();
  let first_run = RunId::new();
  let first = context(&first_dispatch, first_run);
  let second_store = ArtifactStore::new();
  let second_dispatch = configure().run_store(second_store.clone()).build().unwrap();
  let second_run = RunId::new();
  let second = context(&second_dispatch, second_run);
  let body_gate = ReadGate::new(b"owned".to_vec());

  let receipt = first.in_scope(|| auv_tracing::emit_artifact(test_artifact(body_gate.reader(), b"owned")));
  body_gate.wait_until_polled();
  body_gate.release();
  let metadata = second.in_scope(|| block_on_timeout(receipt)).unwrap().unwrap();

  assert_eq!(metadata.uri().run_id(), first_run);
  block_on_timeout(first_dispatch.flush()).unwrap();
  assert_eq!(block_on_timeout(first_store.load_snapshot(first_run)).unwrap().unwrap().artifacts().len(), 1);
  assert!(block_on_timeout(second_store.load_snapshot(second_run)).unwrap().is_none());
}

#[test]
fn committed_publication_unknown_performs_one_lookup_without_reuploading() {
  let store = ArtifactStore::new();
  store.store_then_unknown_next(ErrorCode::parse("auv.test.response_lost").unwrap());
  let dispatch = configure().run_store(store.clone()).build().unwrap();
  let run_id = RunId::new();
  let root = context(&dispatch, run_id);

  let result = root.in_scope(|| block_on_timeout(auv_tracing::emit_artifact(ready_artifact(b"committed"))));

  assert!(result.unwrap().is_some());
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(store.write_call_count(), 1);
  assert_eq!(store.lookup_call_count(), 1);
}

#[test]
fn unresolved_publication_unknown_performs_one_lookup_and_returns_unknown() {
  let store = ArtifactStore::new();
  let code = ErrorCode::parse("auv.test.publication_unknown").unwrap();
  store.unknown_next(code.clone());
  let dispatch = configure().run_store(store.clone()).build().unwrap();
  let root = context(&dispatch, RunId::new());

  let result = root.in_scope(|| block_on_timeout(auv_tracing::emit_artifact(ready_artifact(b"unknown"))));

  assert_eq!(result.unwrap_err(), ArtifactWriteError::PublicationUnknown(code));
  assert_eq!(store.write_call_count(), 1);
  assert_eq!(store.lookup_call_count(), 1);
  let flush = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(flush.failure_count().get(), 1);
  assert_eq!(flush.first().stage(), DispatchStage::ArtifactWrite);
  block_on_timeout(dispatch.flush()).unwrap();
}

#[test]
fn panicking_artifact_write_performs_one_lookup_without_reuploading() {
  let store = ArtifactStore::new();
  store.panic_call_next();
  let dispatch = configure().run_store(store.clone()).build().unwrap();
  let root = context(&dispatch, RunId::new());

  let result = root.in_scope(|| block_on_timeout(auv_tracing::emit_artifact(ready_artifact(b"panic"))));

  assert!(matches!(result, Err(ArtifactWriteError::PublicationUnknown(_))));
  assert_eq!(store.write_call_count(), 1);
  assert_eq!(store.lookup_call_count(), 1);
  assert_eq!(block_on_timeout(dispatch.flush()).unwrap_err().failure_count().get(), 1);
}

#[test]
fn canceled_started_transfer_performs_one_lookup_without_reuploading() {
  let store = ArtifactStore::new();
  let spawner = DropNthTaskSpawner::new(1);
  let dispatch = configure().run_store(store.clone()).task_spawner(spawner).build().unwrap();
  let root = context(&dispatch, RunId::new());
  let body_gate = ReadGate::new(b"canceled".to_vec());

  let receipt = root.in_scope(|| auv_tracing::emit_artifact(test_artifact(body_gate.reader(), b"canceled")));
  body_gate.wait_until_polled();
  let result = block_on_timeout(receipt);

  assert!(matches!(result, Err(ArtifactWriteError::PublicationUnknown(_))));
  assert_eq!(store.write_call_count(), 1);
  assert_eq!(store.lookup_call_count(), 1);
  assert_eq!(block_on_timeout(dispatch.flush()).unwrap_err().failure_count().get(), 1);
}

#[test]
fn canceled_cursor_establishment_terminalizes_receipt_without_polling_body() {
  let store = ControlledStore::new();
  store.keep_snapshot_reads_pending();
  let projector = RecordingProjector::new();
  let dispatch = configure()
    .run_store(store)
    .project_telemetry(projector, TelemetryRoutePolicy::fixed_fields_only())
    .task_spawner(DropFirstTaskSpawner::new())
    .build()
    .unwrap();
  let root = context(&dispatch, RunId::new());
  let polled = Arc::new(AtomicBool::new(false));

  let result = root.in_scope(|| block_on_timeout(auv_tracing::emit_artifact(test_artifact(ProbeReader::new(polled.clone()), &[]))));

  assert!(matches!(result, Err(ArtifactWriteError::Unavailable(_))));
  assert!(!polled.load(Ordering::SeqCst));
  let flush = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(flush.failure_count().get(), 1);
  assert_eq!(flush.first().stage(), DispatchStage::AuthorityRead);
}

#[test]
fn observed_confirmed_failure_reaches_receipt_and_one_flush_interval() {
  let store = ArtifactStore::new();
  let code = ErrorCode::parse("auv.test.artifact_rejected").unwrap();
  store.fail_next(ArtifactWriteError::Rejected(code.clone()));
  let reporter = RecordingReporter::new();
  let dispatch = configure().run_store(store).on_error(reporter.clone()).build().unwrap();
  let root = context(&dispatch, RunId::new());

  let result = root.in_scope(|| block_on_timeout(auv_tracing::emit_artifact(ready_artifact(b"rejected"))));

  assert_eq!(result.unwrap_err(), ArtifactWriteError::Rejected(code.clone()));
  assert!(reporter.failures().is_empty(), "an observed artifact failure must not also use the unobserved reporter path");
  let flush = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(flush.failure_count().get(), 1);
  assert_eq!(flush.first().code(), &code);
  block_on_timeout(dispatch.flush()).unwrap();
}

#[test]
fn dropped_failure_receipt_reports_and_flushes_exactly_once() {
  let store = ArtifactStore::new();
  let code = ErrorCode::parse("auv.test.dropped_artifact_failure").unwrap();
  let failure_gate = store.block_then_fail_next(ArtifactWriteError::Rejected(code.clone()));
  let reporter = RecordingReporter::new();
  let dispatch = configure().run_store(store.clone()).on_error(reporter.clone()).build().unwrap();
  let root = context(&dispatch, RunId::new());

  let receipt = root.in_scope(|| auv_tracing::emit_artifact(ready_artifact(b"failure")));
  failure_gate.wait_until_entered();
  drop(receipt);
  failure_gate.release();
  store.wait_for_write_calls(1);
  let flush = block_on_timeout(dispatch.flush()).unwrap_err();

  assert_eq!(flush.failure_count().get(), 1);
  assert_eq!(flush.first().code(), &code);
  assert_eq!(reporter.failures().len(), 1);
  assert_eq!(reporter.failures()[0].code(), &code);
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(reporter.failures().len(), 1);
}

#[test]
fn synchronized_receipt_send_drop_races_report_each_unobserved_failure_once() {
  const ATTEMPTS: usize = 128;

  let store = ArtifactStore::new();
  let code = ErrorCode::parse("auv.test.receipt_send_drop_race").unwrap();
  let reporter = RecordingReporter::new();
  let dispatch = configure().run_store(store.clone()).on_error(reporter.clone()).build().unwrap();
  let root = context(&dispatch, RunId::new());

  for attempt in 0..ATTEMPTS {
    let failure_gate = store.block_then_fail_next(ArtifactWriteError::Rejected(code.clone()));
    let receipt = root.in_scope(|| auv_tracing::emit_artifact(ready_artifact(format!("race-{attempt}").as_bytes())));
    failure_gate.wait_until_entered();
    let start = Arc::new(Barrier::new(2));
    let drop_start = start.clone();
    let drop_thread = std::thread::spawn(move || {
      drop_start.wait();
      drop(receipt);
    });
    start.wait();
    failure_gate.release();
    drop_thread.join().unwrap();
  }

  let flush = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(flush.failure_count().get(), ATTEMPTS);
  assert_eq!(flush.first().code(), &code);
  assert_eq!(reporter.failures().len(), ATTEMPTS);
  assert!(reporter.failures().iter().all(|failure| failure.code() == &code));
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(reporter.failures().len(), ATTEMPTS);
}

#[test]
fn canceled_failure_completion_does_not_duplicate_unobserved_reporting() {
  let store = ArtifactStore::new();
  let code = ErrorCode::parse("auv.test.canceled_failure_completion").unwrap();
  let failure_gate = store.block_then_fail_next(ArtifactWriteError::Rejected(code.clone()));
  let reporter = RecordingReporter::new();
  let dispatch = configure().run_store(store).on_error(reporter.clone()).task_spawner(DropNthTaskSpawner::new(2)).build().unwrap();
  let root = context(&dispatch, RunId::new());

  let receipt = root.in_scope(|| auv_tracing::emit_artifact(ready_artifact(b"failure")));
  failure_gate.wait_until_entered();
  drop(receipt);
  failure_gate.release();
  let flush = block_on_timeout(dispatch.flush()).unwrap_err();

  assert_eq!(flush.failure_count().get(), 1);
  assert_eq!(flush.first().code(), &code);
  assert_eq!(reporter.failures().len(), 1);
  assert_eq!(reporter.failures()[0].code(), &code);
  block_on_timeout(dispatch.flush()).unwrap();
}

#[test]
fn artifact_telemetry_contains_metadata_but_never_body_bytes() {
  const SECRET: &[u8] = b"artifact-body-must-not-reach-telemetry";
  let store = ArtifactStore::new();
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let root = context(&dispatch, RunId::new());

  let metadata: ArtifactMetadata = root.in_scope(|| block_on_timeout(auv_tracing::emit_artifact!(ready_artifact(SECRET)))).unwrap().unwrap();
  block_on_timeout(dispatch.flush()).unwrap();

  let items = projector.items();
  assert_eq!(items.len(), 1);
  let TelemetryItem::Artifact {
    uri,
    byte_length,
    sha256,
    ..
  } = &items[0]
  else {
    panic!("artifact publication must project one metadata-only artifact item");
  };
  assert_eq!(uri, metadata.uri());
  assert_eq!(*byte_length, metadata.byte_length());
  assert_eq!(*sha256, metadata.sha256());
  assert!(!format!("{:?}", items[0]).contains(std::str::from_utf8(SECRET).unwrap()));
}
