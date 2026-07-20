#![cfg(feature = "memory-store")]

mod support;

use std::sync::Arc;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::task::{Context as TaskContext, Poll};

use auv_tracing::{
  AttributeKey, AttributeValue, Attributes, AuthorityId, DispatchErrorReporter, DispatchFailure, DispatchStage, ErrorCode, EventId,
  EventName, EventOccurred, EventPayload, EventSchema, IdempotencyKey, JsonPayload, MemoryRunStore, RunCommitRequest, RunId, RunMutation,
  RunStore, SpanSpec, TelemetryError, TelemetryItem, TelemetryProjector, TelemetryRoutePolicy, Timestamp, configure, dispatcher,
};
use support::{
  AsyncDropRaceSpawner, AuthorityCall, BlockingProjector, CommitUnknownStore, ControlledStore, CursorStore, DiscardFirstTaskSpawner,
  DropFirstTaskSpawner, FailFirstSpawner, FirstThreadThenInlineSpawner, FirstThreadThenPollDropSpawner, IntegrityFault, IntegrityStore,
  ManualTaskSpawner, PanicFirstSpawner, ProjectorCall, RacingSpawnResult, RecordingProjector, RecordingReporter, SubscriptionTrackingStore,
  UnknownLookup, block_on_timeout,
};

struct SignalWake(SyncSender<()>);

impl futures_util::task::ArcWake for SignalWake {
  fn wake_by_ref(wake: &Arc<Self>) {
    let _ = wake.0.try_send(());
  }
}

#[derive(Clone, Debug, serde::Serialize)]
struct TestEvent {
  value: u32,
}

impl EventPayload for TestEvent {
  const NAME: &'static str = "auv.test.dispatch_event";
  const VERSION: u32 = 1;
}

#[derive(serde::Serialize)]
struct OversizedEvent {
  value: String,
}

impl OversizedEvent {
  fn new(length: usize) -> Self {
    Self {
      value: "x".repeat(length),
    }
  }
}

impl EventPayload for OversizedEvent {
  const NAME: &'static str = "auv.test.oversized_event";
  const VERSION: u32 = 1;
}

#[derive(serde::Serialize)]
struct PanickingDropEvent {
  value: u32,
}

impl EventPayload for PanickingDropEvent {
  const NAME: &'static str = "auv.test.panicking_drop_after_quarantine";
  const VERSION: u32 = 1;
}

impl Drop for PanickingDropEvent {
  fn drop(&mut self) {
    panic!("panicking event destructor after run-lane quarantine")
  }
}

struct TestSpan {
  attributes: Attributes,
}

impl SpanSpec for TestSpan {
  const NAME: &'static str = "auv.test.dispatch_span";

  fn attributes(&self) -> Attributes {
    self.attributes.clone()
  }
}

fn test_span() -> TestSpan {
  TestSpan {
    attributes: Attributes::empty(),
  }
}

struct NoopProjector;

impl TelemetryProjector for NoopProjector {
  fn project(&self, _item: TelemetryItem) -> auv_tracing::BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async { Ok(()) })
  }

  fn flush(&self) -> auv_tracing::BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async { Ok(()) })
  }
}

struct NoopReporter;

impl DispatchErrorReporter for NoopReporter {
  fn report(&self, _failure: &DispatchFailure) {}
}

struct PendingProjector;

impl TelemetryProjector for PendingProjector {
  fn project(&self, _item: TelemetryItem) -> auv_tracing::BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(std::future::pending())
  }

  fn flush(&self) -> auv_tracing::BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async { Ok(()) })
  }
}

struct PendingFlushProjector;

impl TelemetryProjector for PendingFlushProjector {
  fn project(&self, _item: TelemetryItem) -> auv_tracing::BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async { Ok(()) })
  }

  fn flush(&self) -> auv_tracing::BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(std::future::pending())
  }
}

struct PanickingProjector;

impl TelemetryProjector for PanickingProjector {
  fn project(&self, _item: TelemetryItem) -> auv_tracing::BoxFuture<'_, Result<(), TelemetryError>> {
    panic!("projector panicked before returning its future")
  }

  fn flush(&self) -> auv_tracing::BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async { Ok(()) })
  }
}

struct PanickingReporter;

impl DispatchErrorReporter for PanickingReporter {
  fn report(&self, _failure: &DispatchFailure) {
    panic!("reporter panicked")
  }
}

#[test]
fn telemetry_ports_are_object_safe_and_errors_expose_stable_codes() {
  let projector: Arc<dyn TelemetryProjector> = Arc::new(NoopProjector);
  let reporter: Arc<dyn DispatchErrorReporter> = Arc::new(NoopReporter);
  let error = TelemetryError::new(ErrorCode::parse("auv.test.telemetry").unwrap());

  assert_eq!(error.code().as_str(), "auv.test.telemetry");
  drop((projector, reporter));
}

#[test]
fn telemetry_event_contains_only_fixed_bounded_fields() {
  let run_id = RunId::new();
  let event_id = EventId::new();
  let schema = EventSchema::new(EventName::parse("auv.test.event").unwrap(), 1).unwrap();
  let occurred_at = Timestamp::new(1, 2).unwrap();
  let item = TelemetryItem::Event {
    authority_id: None,
    run_id,
    span_id: None,
    event_id,
    schema: schema.clone(),
    occurred_at,
    revision: None,
  };

  assert_eq!(
    item,
    TelemetryItem::Event {
      authority_id: None,
      run_id,
      span_id: None,
      event_id,
      schema,
      occurred_at,
      revision: None,
    }
  );

  let _policy = TelemetryRoutePolicy::fixed_fields_only()
    .allow_span_attribute(AttributeKey::parse("auv.test.span").unwrap())
    .allow_artifact_attribute(AttributeKey::parse("auv.test.artifact").unwrap());
}

#[test]
fn telemetry_only_dispatch_projects_volatile_facts_without_authority_revisions() {
  let projector = RecordingProjector::new();
  let dispatch = configure().project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(run_id));

  assert!(root.is_enabled());
  assert!(root.authority_id().is_none());
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  block_on_timeout(dispatch.flush()).unwrap();

  let items = projector.items();
  assert_eq!(items.len(), 1);
  let TelemetryItem::Event {
    authority_id,
    run_id: projected_run_id,
    revision,
    ..
  } = &items[0]
  else {
    panic!("expected a projected event")
  };
  assert_eq!(*authority_id, None);
  assert_eq!(*projected_run_id, run_id);
  assert_eq!(*revision, None);
}

#[test]
fn route_policies_filter_span_attributes_before_each_projector() {
  let hidden = AttributeKey::parse("auv.test.hidden").unwrap();
  let allowed = AttributeKey::parse("auv.test.allowed").unwrap();
  let attributes = Attributes::try_from_iter([
    (hidden, AttributeValue::boolean(true)),
    (allowed.clone(), AttributeValue::integer(7).unwrap()),
  ])
  .unwrap();
  let fixed = RecordingProjector::new();
  let allowlisted = RecordingProjector::new();
  let dispatch = configure()
    .project_telemetry(fixed.clone(), TelemetryRoutePolicy::fixed_fields_only())
    .project_telemetry(allowlisted.clone(), TelemetryRoutePolicy::fixed_fields_only().allow_span_attribute(allowed.clone()))
    .build()
    .unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

  let span = root.in_scope(|| auv_tracing::start_span!(TestSpan { attributes }));
  drop(span);
  block_on_timeout(dispatch.flush()).unwrap();

  let TelemetryItem::SpanStart { attributes, .. } = &fixed.items()[0] else {
    panic!("expected fixed-route span start")
  };
  assert!(attributes.is_empty());
  let TelemetryItem::SpanStart { attributes, .. } = &allowlisted.items()[0] else {
    panic!("expected allowlisted-route span start")
  };
  assert_eq!(attributes.len(), 1);
  assert_eq!(attributes.get(&allowed), Some(&AttributeValue::integer(7).unwrap()));
}

#[test]
fn flush_waits_for_pre_barrier_items_and_projector_flush() {
  let projector = BlockingProjector::new();
  let dispatch = configure().project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  projector.wait_until_project_entered();
  let mut flush = dispatch.flush();
  let waker = futures_util::task::noop_waker();
  let mut task_context = TaskContext::from_waker(&waker);
  assert!(matches!(flush.as_mut().poll(&mut task_context), Poll::Pending));
  assert_eq!(projector.item_count(), 0);

  projector.release_project();
  projector.wait_until_flush_entered();
  assert!(matches!(flush.as_mut().poll(&mut task_context), Poll::Pending));
  projector.release_flush();
  block_on_timeout(flush).unwrap();
  assert_eq!(projector.calls(), [ProjectorCall::Project, ProjectorCall::Flush]);
}

#[test]
fn projector_flush_barrier_does_not_wait_for_later_authority_submissions() {
  let store = ControlledStore::new();
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));
  let run_id = *root.run_id().unwrap();

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  store.wait_until_committed(run_id);
  let first_flush = dispatch.flush();
  let second_commit = store.block_next_commit(run_id);
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  second_commit.wait_until_entered();

  block_on_timeout(first_flush).unwrap();
  assert_eq!(projector.item_count(), 1);
  assert_eq!(projector.calls(), [ProjectorCall::Project, ProjectorCall::Flush]);

  second_commit.release();
  block_on_timeout(dispatch.flush()).unwrap();
}

#[test]
fn failed_projector_flush_advances_its_reported_interval() {
  let projector = RecordingProjector::new();
  projector.fail_next_flush(ErrorCode::parse("auv.test.flush_failure").unwrap());
  let reporter = RecordingReporter::new();
  let dispatch =
    configure().project_telemetry(projector, TelemetryRoutePolicy::fixed_fields_only()).on_error(reporter.clone()).build().unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));

  let error = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::ProjectorFlush);
  assert_eq!(error.first().code().as_str(), "auv.test.flush_failure");
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(reporter.failures(), [error.first().clone()]);
}

#[test]
fn oversized_event_reports_encode_failure_and_reaches_no_authority_or_projector() {
  let store = ControlledStore::new();
  let projector = RecordingProjector::new();
  let reporter = RecordingReporter::new();
  let dispatch = configure()
    .run_store(store.clone())
    .project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only())
    .on_error(reporter.clone())
    .build()
    .unwrap();
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(run_id));

  root.in_scope(|| auv_tracing::emit_event!(OversizedEvent::new(70 * 1024)));
  let error = block_on_timeout(dispatch.flush()).unwrap_err();

  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::Encode);
  assert_eq!(store.commit_call_count(run_id), 0);
  assert_eq!(projector.item_count(), 0);
  assert_eq!(reporter.failures(), [error.first().clone()]);
}

#[test]
fn flush_does_not_end_spans_and_later_events_are_accepted() {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(run_id));
  let span = root.in_scope(|| auv_tracing::start_span!(test_span()));

  block_on_timeout(dispatch.flush()).unwrap();
  assert!(block_on_timeout(store.load_snapshot(run_id)).unwrap().unwrap().spans().values().all(|span| span.ended().is_none()));
  span.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(block_on_timeout(store.load_snapshot(run_id)).unwrap().unwrap().events().len(), 1);
  drop(span);
  block_on_timeout(dispatch.flush()).unwrap();
}

fn external_event_request(store: &MemoryRunStore, run_id: RunId, event_id: EventId, value: u32) -> RunCommitRequest {
  let event = EventOccurred::new(
    event_id,
    None,
    Timestamp::new(10, 0).unwrap(),
    EventSchema::for_payload::<TestEvent>().unwrap(),
    JsonPayload::encode(&TestEvent { value }).unwrap(),
  );
  RunCommitRequest::new(store.authority_id(), run_id, IdempotencyKey::new(), vec![RunMutation::EmitEvent(event)]).unwrap()
}

#[test]
fn authority_cursor_starts_snapshot_and_subscription_before_first_commit() {
  let store = CursorStore::normal();
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector, TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  block_on_timeout(dispatch.flush()).unwrap();

  assert_eq!(
    &store.calls()[..3],
    [
      AuthorityCall::LoadSnapshot,
      AuthorityCall::Subscribe,
      AuthorityCall::Commit
    ]
  );
}

#[test]
fn authority_backed_projector_never_receives_a_precommit_mutation() {
  let store = ControlledStore::new();
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));
  let gate = store.block_next_commit(*root.run_id().unwrap());

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  gate.wait_until_entered();
  assert_eq!(projector.item_count(), 0);

  gate.release();
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(projector.item_count(), 1);
}

// ROOT CAUSE:
//
// If a commit future accepted its mutation and then remained pending, canceling
// the lane task terminalized its ticket without quarantining the run.
//
// Before the fix, a later same-run mutation reached the authority after an
// unknown write outcome. The fix arms quarantine for the entire commit future.
#[test]
fn canceled_commit_after_authority_acceptance_quarantines_only_that_run() {
  let store = ControlledStore::new();
  let affected_run_id = RunId::new();
  let independent_run_id = RunId::new();
  let gate = store.commit_then_pending_next(affected_run_id);
  gate.release();
  let dispatch = configure().run_store(store.clone()).task_spawner(DropFirstTaskSpawner::new()).build().unwrap();
  let affected = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(affected_run_id));
  let independent = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(independent_run_id));

  affected.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  assert_eq!(block_on_timeout(store.load_snapshot(affected_run_id)).unwrap().unwrap().events().len(), 1);
  affected.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  independent.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 3 }));

  let error = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 2);
  assert_eq!(error.first().code().as_str(), "auv.dispatch.task_unwind");
  assert_eq!(store.commit_call_count(affected_run_id), 1);
  assert_eq!(store.commit_call_count(independent_run_id), 1);
  assert_eq!(block_on_timeout(store.load_snapshot(affected_run_id)).unwrap().unwrap().events().len(), 1);
}

#[test]
fn authority_cursor_projects_local_revisions_and_skips_external_writers() {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(run_id));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  block_on_timeout(dispatch.flush()).unwrap();
  let external_id = EventId::new();
  block_on_timeout(store.commit(external_event_request(&store, run_id, external_id, 2))).unwrap();
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 3 }));
  block_on_timeout(dispatch.flush()).unwrap();

  let projected = projector
    .items()
    .into_iter()
    .filter_map(|item| match item {
      TelemetryItem::Event {
        event_id, revision, ..
      } => Some((event_id, revision.unwrap().get())),
      _ => None,
    })
    .collect::<Vec<_>>();
  assert_eq!(projected.iter().map(|(_, revision)| *revision).collect::<Vec<_>>(), [1, 3]);
  assert!(projected.iter().all(|(event_id, _)| *event_id != external_id));
}

#[test]
fn subscription_gap_recovers_from_revisioned_pages_and_resumes() {
  let store = CursorStore::gap_once();
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  block_on_timeout(dispatch.flush()).unwrap();

  assert_eq!(projector.item_count(), 1);
  assert_eq!(store.commits_after_call_count(), 1);
  assert_eq!(store.subscribe_call_count(), 2, "gap recovery must resume the subscription");
}

#[test]
fn permanently_waiting_subscription_does_not_block_observing_a_known_local_revision() {
  let store = CursorStore::pending_once();
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  block_on_timeout(dispatch.flush()).unwrap();

  assert_eq!(projector.item_count(), 1);
  assert_eq!(store.commits_after_call_count(), 1);
  assert_eq!(store.subscribe_call_count(), 2);
}

#[test]
fn completed_unique_runs_release_all_authority_subscriptions() {
  const RUN_COUNT: usize = 64;

  let store = SubscriptionTrackingStore::new();
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector, TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();

  for value in 0..RUN_COUNT {
    let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));
    root.in_scope(|| {
      auv_tracing::emit_event!(TestEvent {
        value: value as u32
      })
    });
  }
  block_on_timeout(dispatch.flush()).unwrap();

  assert_eq!(store.active_subscription_count(), 0, "idle successful lanes must not retain authority subscriptions");
}

#[test]
fn history_gap_at_revision_zero_is_a_stable_read_failure_without_quarantine() {
  let store = CursorStore::zero_history_gap_once();
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  let gap = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(gap.first().stage(), DispatchStage::AuthorityRead);
  assert_eq!(gap.first().code().as_str(), "auv.dispatch.authority_history_gap");

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(projector.item_count(), 1);
}

#[test]
fn cursor_establishment_failure_reaches_no_commit_or_projector_and_reports_once() {
  let store = CursorStore::snapshot_failure();
  let projector = RecordingProjector::new();
  let reporter = RecordingReporter::new();
  let dispatch = configure()
    .run_store(store.clone())
    .project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only())
    .on_error(reporter.clone())
    .build()
    .unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  let error = block_on_timeout(dispatch.flush()).unwrap_err();

  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::AuthorityRead);
  assert_eq!(error.first().code().as_str(), "auv.test.snapshot_failed");
  assert!(!store.calls().contains(&AuthorityCall::Commit));
  assert_eq!(projector.item_count(), 0);
  assert_eq!(reporter.failures(), [error.first().clone()]);
  block_on_timeout(dispatch.flush()).unwrap();
}

#[test]
fn post_commit_cursor_read_failure_preserves_commit_and_skips_projection() {
  let store = CursorStore::page_failure();
  let projector = RecordingProjector::new();
  let reporter = RecordingReporter::new();
  let dispatch = configure()
    .run_store(store.clone())
    .project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only())
    .on_error(reporter.clone())
    .build()
    .unwrap();
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(run_id));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  let error = block_on_timeout(dispatch.flush()).unwrap_err();

  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::AuthorityRead);
  assert_eq!(error.first().code().as_str(), "auv.test.page_failed");
  assert!(block_on_timeout(store.load_snapshot(run_id)).unwrap().is_some());
  assert_eq!(projector.item_count(), 0);
  assert_eq!(reporter.failures(), [error.first().clone()]);
  block_on_timeout(dispatch.flush()).unwrap();

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  block_on_timeout(dispatch.flush()).unwrap();
  let items = projector.items();
  let TelemetryItem::Event { revision, .. } = &items[0] else {
    panic!("the recovered cursor must project the later event")
  };
  assert_eq!(revision.unwrap().get(), 2);
}

#[test]
fn mismatched_direct_commit_response_quarantines_only_that_run() {
  let affected_run_id = RunId::new();
  let independent_run_id = RunId::new();
  let store = IntegrityStore::new(affected_run_id, IntegrityFault::DirectResponseMismatch);
  let dispatch = configure().run_store(store.clone()).build().unwrap();
  let affected = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(affected_run_id));
  let independent = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(independent_run_id));

  affected.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  let mismatch = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(mismatch.first().stage(), DispatchStage::AuthorityCommit);
  assert_eq!(mismatch.first().code().as_str(), "auv.dispatch.commit_response_mismatch");

  affected.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  independent.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 3 }));
  let quarantined = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(quarantined.first().code().as_str(), "auv.dispatch.run_lane_indeterminate");
  assert_eq!(store.commit_call_count(affected_run_id), 1);
  assert_eq!(store.commit_call_count(independent_run_id), 1);
}

#[test]
fn cursor_integrity_contradictions_quarantine_only_the_affected_run() {
  for fault in [
    IntegrityFault::CursorAuthorityMismatch,
    IntegrityFault::CursorRunMismatch,
    IntegrityFault::CursorRevisionMismatch,
    IntegrityFault::CursorOwnedRequestMismatch,
    IntegrityFault::CursorMissingCurrentTicket,
  ] {
    let affected_run_id = RunId::new();
    let independent_run_id = RunId::new();
    let store = IntegrityStore::new(affected_run_id, fault);
    let projector = RecordingProjector::new();
    let dispatch =
      configure().run_store(store.clone()).project_telemetry(projector, TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
    let affected = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(affected_run_id));
    let independent = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(independent_run_id));

    affected.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
    let mismatch = block_on_timeout(dispatch.flush()).unwrap_err();
    assert_eq!(mismatch.first().stage(), DispatchStage::AuthorityRead, "fault: {fault:?}");

    affected.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
    independent.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 3 }));
    let quarantined = block_on_timeout(dispatch.flush()).unwrap_err();
    assert_eq!(quarantined.first().code().as_str(), "auv.dispatch.run_lane_indeterminate", "fault: {fault:?}");
    assert_eq!(store.commit_call_count(affected_run_id), 1, "fault: {fault:?}");
    assert_eq!(store.commit_call_count(independent_run_id), 1, "fault: {fault:?}");
  }
}

#[test]
fn unknown_commit_is_looked_up_once_and_never_resubmitted() {
  let store = CommitUnknownStore::new(UnknownLookup::Committed);
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  block_on_timeout(dispatch.flush()).unwrap();

  assert_eq!(store.commit_calls(), 1);
  assert_eq!(store.lookup_calls(), 1);
  assert_eq!(projector.item_count(), 1);
}

#[test]
fn unresolved_unknown_commit_quarantines_only_the_affected_run_lane() {
  let store = CommitUnknownStore::new(UnknownLookup::None);
  let projector = RecordingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let affected = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

  affected.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  let error = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(error.first().stage(), DispatchStage::AuthorityCommit);
  assert_eq!(error.first().code().as_str(), "auv.test.commit_unknown");

  affected.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  let quarantined = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(quarantined.first().code().as_str(), "auv.dispatch.run_lane_indeterminate");
  assert_eq!(store.commit_calls(), 1);
  assert_eq!(store.lookup_calls(), 1);

  affected.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 4 }));
  let still_quarantined = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(still_quarantined.first().code().as_str(), "auv.dispatch.run_lane_indeterminate");
  assert_eq!(store.commit_calls(), 1, "quarantine must survive local terminalization");

  let independent = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));
  independent.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 3 }));
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(store.commit_calls(), 2);
  assert_eq!(projector.item_count(), 1);
}

#[test]
fn lookup_read_failure_and_structural_mismatch_each_quarantine_without_resubmission() {
  for mode in [UnknownLookup::ReadFailure, UnknownLookup::Mismatch] {
    let store = CommitUnknownStore::new(mode);
    let dispatch = configure().run_store(store.clone()).build().unwrap();
    let affected = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

    affected.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
    let error = block_on_timeout(dispatch.flush()).unwrap_err();
    assert_eq!(error.first().stage(), DispatchStage::AuthorityCommit);
    affected.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
    let quarantined = block_on_timeout(dispatch.flush()).unwrap_err();

    assert_eq!(quarantined.first().code().as_str(), "auv.dispatch.run_lane_indeterminate");
    assert_eq!(store.commit_calls(), 1);
    assert_eq!(store.lookup_calls(), 1);
  }
}

#[test]
fn quarantined_lane_preserves_the_original_payload_destructor_panic_without_double_panicking() {
  let store = CommitUnknownStore::new(UnknownLookup::None);
  let dispatch = configure().run_store(store.clone()).build().unwrap();
  let affected = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));
  affected.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  block_on_timeout(dispatch.flush()).unwrap_err();

  let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    affected.in_scope(|| auv_tracing::emit_event!(PanickingDropEvent { value: 2 }));
  }));

  assert!(panic.is_err(), "the payload's original destructor panic remains observable");
  let quarantined = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(quarantined.first().code().as_str(), "auv.dispatch.run_lane_indeterminate");
  assert_eq!(store.commit_calls(), 1);
}

#[test]
fn multiple_projector_failures_for_one_ticket_are_retained_and_reported_once() {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let first = RecordingProjector::new();
  let second = RecordingProjector::new();
  first.fail_next_project(ErrorCode::parse("auv.test.project_one").unwrap());
  second.fail_next_project(ErrorCode::parse("auv.test.project_two").unwrap());
  let reporter = RecordingReporter::new();
  let dispatch = configure()
    .run_store(store.clone())
    .project_telemetry(first, TelemetryRoutePolicy::fixed_fields_only())
    .project_telemetry(second, TelemetryRoutePolicy::fixed_fields_only())
    .on_error(reporter.clone())
    .build()
    .unwrap();
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(run_id));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  let error = block_on_timeout(dispatch.flush()).unwrap_err();

  assert_eq!(error.failure_count().get(), 2);
  assert!(block_on_timeout(store.load_snapshot(run_id)).unwrap().is_some(), "projection cannot roll back the commit");
  assert_eq!(reporter.failures().len(), 2);
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(reporter.failures().len(), 2, "flush must not report retained failures again");
}

#[test]
fn canceled_flush_cannot_consume_its_projector_flush_failure() {
  let projector = RecordingProjector::new();
  projector.fail_next_flush(ErrorCode::parse("auv.test.canceled_flush").unwrap());
  let reporter = RecordingReporter::new();
  let dispatch =
    configure().project_telemetry(projector, TelemetryRoutePolicy::fixed_fields_only()).on_error(reporter.clone()).build().unwrap();

  drop(dispatch.flush());
  let error = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::ProjectorFlush);
  assert_eq!(error.first().code().as_str(), "auv.test.canceled_flush");
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(reporter.failures().len(), 1);
}

// ROOT CAUSE:
//
// If a completed non-front flush was canceled, popping the preceding live
// flush exposed an ownerless canceled registration because `poll_flush` did
// not revisit canceled-front draining.
//
// Before the fix, every later flush remained stranded behind that registration.
// The fix drains newly exposed completed cancellations and carries their
// failures into the next live interval.
#[test]
fn completing_front_flush_drains_canceled_middle_and_wakes_third() {
  let projector = RecordingProjector::new();
  projector.fail_next_project(ErrorCode::parse("auv.test.first_ticket").unwrap());
  projector.fail_next_project(ErrorCode::parse("auv.test.middle_ticket").unwrap());
  projector.fail_next_flush(ErrorCode::parse("auv.test.first_flush").unwrap());
  projector.fail_next_flush(ErrorCode::parse("auv.test.middle_flush").unwrap());
  let reporter = RecordingReporter::new();
  let spawner = ManualTaskSpawner::new();
  let dispatch = configure()
    .project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only())
    .on_error(reporter.clone())
    .task_spawner(spawner.clone())
    .build()
    .unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  let first = dispatch.flush();
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  let middle = dispatch.flush();
  let mut third = dispatch.flush();

  let (wake_sender, wake_receiver) = sync_channel(1);
  let wake = Arc::new(SignalWake(wake_sender));
  let waker = futures_util::task::waker_ref(&wake);
  let mut task_context = TaskContext::from_waker(&waker);
  assert!(third.as_mut().poll(&mut task_context).is_pending());
  drop(middle);

  spawner.run_all();
  assert_eq!(
    projector.calls(),
    [
      ProjectorCall::Project,
      ProjectorCall::Flush,
      ProjectorCall::Project,
      ProjectorCall::Flush,
      ProjectorCall::Flush,
    ]
  );

  let first_error = block_on_timeout(first).unwrap_err();
  assert_eq!(first_error.failure_count().get(), 2);
  assert_eq!(first_error.first().code().as_str(), "auv.test.first_ticket");
  wake_receiver.try_recv().expect("popping the first flush must wake the third waiter through the canceled middle flush");

  let third_error = block_on_timeout(third).unwrap_err();
  assert_eq!(third_error.failure_count().get(), 2);
  assert_eq!(third_error.first().code().as_str(), "auv.test.middle_ticket");
  assert_eq!(
    reporter.failures().iter().map(|failure| failure.code().as_str()).collect::<Vec<_>>(),
    [
      "auv.test.first_ticket",
      "auv.test.first_flush",
      "auv.test.middle_ticket",
      "auv.test.middle_flush",
    ]
  );

  let fourth = dispatch.flush();
  spawner.run_all();
  block_on_timeout(fourth).unwrap();
  assert_eq!(reporter.failures().len(), 4, "carried failures must not be reported twice");
}

#[test]
fn blocked_projector_does_not_block_later_authority_commits() {
  let store = ControlledStore::new();
  let projector = BlockingProjector::new();
  let dispatch =
    configure().run_store(store.clone()).project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));
  let run_id = *root.run_id().unwrap();

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  projector.wait_until_project_entered();
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  store.wait_for_commit_calls(run_id, 2);
  store.wait_for_committed_count(run_id, 2);
  assert_eq!(store.committed_revisions(run_id), [1, 2]);
  assert_eq!(projector.item_count(), 0, "the second project call must wait for the first call to finish");

  let flush = dispatch.flush();
  projector.release_project();
  projector.wait_until_flush_entered();
  projector.release_flush();
  block_on_timeout(flush).unwrap();
}

// ROOT CAUSE:
//
// If a projection action completed inside an inline spawner, its completion
// called `wake_projection` before the preceding spawn call returned.
//
// Before the fix, queued actions nested one spawn frame per item. The fix keeps
// one iterative projection drainer across synchronous completions.
#[test]
fn hybrid_spawner_drains_inline_projection_backlog_at_bounded_depth() {
  const BACKLOG: usize = 128;

  let projector = BlockingProjector::new();
  let spawner = FirstThreadThenInlineSpawner::new();
  let dispatch = configure()
    .project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only())
    .task_spawner(spawner.clone())
    .build()
    .unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 0 }));
  projector.wait_until_project_entered();
  for value in 1..=BACKLOG {
    root.in_scope(|| {
      auv_tracing::emit_event!(TestEvent {
        value: value as u32
      })
    });
  }
  let flush = dispatch.flush();

  projector.release_project();
  projector.wait_until_flush_entered();
  projector.release_flush();
  block_on_timeout(flush).unwrap();

  assert_eq!(projector.item_count(), BACKLOG + 1);
  assert_eq!(spawner.max_inline_depth(), 1, "inline completions must return to one iterative drain frame");
}

// ROOT CAUSE:
//
// If a started authority task was dropped during cursor establishment,
// `LaneDrainGuard` recovered by synchronously waking the same run lane. Before
// the fix, an inline spawner nested one recovery and spawn frame per queued
// ticket. The fix keeps one iterative wake owner until async work takes over.
#[test]
fn authority_recovery_drains_inline_cancellation_backlog_at_bounded_depth() {
  const TICKET_COUNT: usize = 256;

  let store = ControlledStore::new();
  store.keep_snapshot_reads_pending();
  let projector = RecordingProjector::new();
  let reporter = RecordingReporter::new();
  let spawner = FirstThreadThenPollDropSpawner::new(TICKET_COUNT);
  let dispatch = configure()
    .run_store(store.clone())
    .project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only())
    .on_error(reporter.clone())
    .task_spawner(spawner.clone())
    .build()
    .unwrap();
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(run_id));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 0 }));
  spawner.wait_until_first_pending();
  for value in 1..TICKET_COUNT {
    root.in_scope(|| {
      auv_tracing::emit_event!(TestEvent {
        value: value as u32
      })
    });
  }

  spawner.release_first();
  spawner.wait_for_completed(TICKET_COUNT);
  let error = block_on_timeout(dispatch.flush()).unwrap_err();
  let failures = reporter.failures();

  assert_eq!(error.failure_count().get(), TICKET_COUNT);
  assert_eq!(failures.len(), TICKET_COUNT, "each accepted ticket must terminalize and report exactly once");
  assert!(failures.iter().all(|failure| failure.stage() == DispatchStage::AuthorityRead));
  assert!(failures.iter().all(|failure| failure.code().as_str() == "auv.dispatch.task_unwind"));
  assert_eq!(store.commit_call_count(run_id), 0);
  assert_eq!(projector.item_count(), 0);
  assert!(spawner.max_depth() <= 2, "authority wake recovery nested to depth {}", spawner.max_depth());
  block_on_timeout(dispatch.flush()).unwrap();
}

#[test]
fn projection_spawn_rejection_and_panic_terminalize_without_stranding_flush() {
  for spawner in [
    FailFirstSpawner::new() as Arc<dyn auv_tracing::TaskSpawner>,
    PanicFirstSpawner::new() as Arc<dyn auv_tracing::TaskSpawner>,
  ] {
    let projector = RecordingProjector::new();
    let reporter = RecordingReporter::new();
    let dispatch = configure()
      .project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only())
      .on_error(reporter.clone())
      .task_spawner(spawner)
      .build()
      .unwrap();
    let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

    root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
    let error = block_on_timeout(dispatch.flush()).unwrap_err();

    assert_eq!(error.failure_count().get(), 1);
    assert_eq!(error.first().stage(), DispatchStage::Spawn);
    assert_eq!(projector.item_count(), 0);
    assert_eq!(reporter.failures(), [error.first().clone()]);
    block_on_timeout(dispatch.flush()).unwrap();
  }
}

// ROOT CAUSE:
//
// If an unpolled task drop raced the spawner's return, independent atomics
// could let each side observe that the other side had not yet published its
// recovery flag. Before the fix, neither side then owned recovery. The fix
// makes spawn admission one locked state transition with exactly one owner.
#[test]
fn asynchronous_task_drop_racing_spawn_return_never_strands_dispatch() {
  const ATTEMPTS: usize = 256;

  for spawn_result in [RacingSpawnResult::Ok, RacingSpawnResult::Err] {
    let spawner = AsyncDropRaceSpawner::new(spawn_result);
    let reporter = RecordingReporter::new();
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).on_error(reporter.clone()).task_spawner(spawner.clone()).build().unwrap();

    for value in 0..ATTEMPTS {
      let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));
      root.in_scope(|| {
        auv_tracing::emit_event!(TestEvent {
          value: value as u32
        })
      });
    }
    spawner.wait_for_drops(ATTEMPTS);
    let error = block_on_timeout(dispatch.flush()).unwrap_err();
    assert_eq!(error.failure_count().get(), ATTEMPTS);
    assert_eq!(reporter.failures().len(), ATTEMPTS);

    let spawner = AsyncDropRaceSpawner::new(spawn_result);
    let reporter = RecordingReporter::new();
    let dispatch = configure()
      .project_telemetry(RecordingProjector::new(), TelemetryRoutePolicy::fixed_fields_only())
      .on_error(reporter.clone())
      .task_spawner(spawner.clone())
      .build()
      .unwrap();
    let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

    for value in 0..ATTEMPTS {
      root.in_scope(|| {
        auv_tracing::emit_event!(TestEvent {
          value: value as u32
        })
      });
    }
    spawner.wait_for_drops(ATTEMPTS);
    let flush = dispatch.flush();
    spawner.wait_for_drops(ATTEMPTS + 1);
    let error = block_on_timeout(flush).unwrap_err();
    assert_eq!(error.failure_count().get(), ATTEMPTS + 1);
    assert_eq!(reporter.failures().len(), ATTEMPTS + 1);
  }
}

#[test]
fn canceled_projection_task_terminalizes_its_ticket_and_releases_flush() {
  let dispatch = configure()
    .project_telemetry(Arc::new(PendingProjector), TelemetryRoutePolicy::fixed_fields_only())
    .task_spawner(DropFirstTaskSpawner::new())
    .build()
    .unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  let error = block_on_timeout(dispatch.flush()).unwrap_err();

  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::Project);
  assert_eq!(error.first().code().as_str(), "auv.dispatch.task_unwind");
}

#[test]
fn unpolled_projection_task_drop_terminalizes_its_ticket_and_releases_flush() {
  let dispatch = configure()
    .project_telemetry(RecordingProjector::new(), TelemetryRoutePolicy::fixed_fields_only())
    .task_spawner(DiscardFirstTaskSpawner::new())
    .build()
    .unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  let error = block_on_timeout(dispatch.flush()).unwrap_err();

  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::Project);
  assert_eq!(error.first().code().as_str(), "auv.dispatch.task_unwind");
}

#[test]
fn unpolled_authority_task_drop_terminalizes_its_ticket_and_releases_the_run_lane() {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let dispatch = configure().run_store(store.clone()).task_spawner(DiscardFirstTaskSpawner::new()).build().unwrap();
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(run_id));

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  let error = block_on_timeout(dispatch.flush()).unwrap_err();
  assert_eq!(error.first().stage(), DispatchStage::AuthorityCommit);
  assert_eq!(error.first().code().as_str(), "auv.dispatch.task_unwind");

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  block_on_timeout(dispatch.flush()).unwrap();
  assert_eq!(block_on_timeout(store.load_snapshot(run_id)).unwrap().unwrap().events().len(), 1);
}

#[test]
fn unpolled_projector_flush_task_completes_its_flush_interval() {
  let dispatch = configure()
    .project_telemetry(RecordingProjector::new(), TelemetryRoutePolicy::fixed_fields_only())
    .task_spawner(DiscardFirstTaskSpawner::new())
    .build()
    .unwrap();

  let error = block_on_timeout(dispatch.flush()).unwrap_err();

  assert_eq!(error.first().stage(), DispatchStage::ProjectorFlush);
  assert_eq!(error.first().code().as_str(), "auv.dispatch.task_unwind");
  block_on_timeout(dispatch.flush()).unwrap();
}

#[test]
fn canceled_projector_flush_task_completes_its_exact_flush_interval() {
  let dispatch = configure()
    .project_telemetry(Arc::new(PendingFlushProjector), TelemetryRoutePolicy::fixed_fields_only())
    .task_spawner(DropFirstTaskSpawner::new())
    .build()
    .unwrap();

  let error = block_on_timeout(dispatch.flush()).unwrap_err();

  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::ProjectorFlush);
  assert_eq!(error.first().code().as_str(), "auv.dispatch.task_unwind");
}

#[test]
fn projector_and_reporter_panics_cannot_escape_or_undo_the_commit() {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let dispatch = configure()
    .run_store(store.clone())
    .project_telemetry(Arc::new(PanickingProjector), TelemetryRoutePolicy::fixed_fields_only())
    .on_error(Arc::new(PanickingReporter))
    .build()
    .unwrap();
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(run_id));

  let emission = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  }));
  assert!(emission.is_ok());
  let error = block_on_timeout(dispatch.flush()).unwrap_err();

  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::Project);
  assert_eq!(error.first().code().as_str(), "auv.dispatch.projector_panic");
  assert!(block_on_timeout(store.load_snapshot(run_id)).unwrap().is_some());
}

#[test]
fn every_projector_flush_failure_is_retained_in_route_order() {
  let first = RecordingProjector::new();
  let second = RecordingProjector::new();
  first.fail_next_flush(ErrorCode::parse("auv.test.flush_one").unwrap());
  second.fail_next_flush(ErrorCode::parse("auv.test.flush_two").unwrap());
  let dispatch = configure()
    .project_telemetry(first, TelemetryRoutePolicy::fixed_fields_only())
    .project_telemetry(second, TelemetryRoutePolicy::fixed_fields_only())
    .build()
    .unwrap();

  let error = block_on_timeout(dispatch.flush()).unwrap_err();

  assert_eq!(error.failure_count().get(), 2);
  assert_eq!(error.first().code().as_str(), "auv.test.flush_one");
}
