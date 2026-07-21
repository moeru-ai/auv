use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex};

use auv_tracing::{
  ArtifactId, ArtifactPurpose, ArtifactUri, AttributeKey, AttributeValue, Attributes, AuthorityId, ByteLength, ContentType, EventId,
  EventName, EventSchema, RunId, RunRevision, RustTracingProjector, Sha256Digest, SpanId, SpanName, TelemetryItem, TelemetryProjector,
  TelemetryRoutePolicy, Timestamp, configure,
};
use futures_executor::block_on;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes as TracingAttributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Metadata, Subscriber};

static TRACING_TEST_LOCK: Mutex<()> = Mutex::new(());

fn serial_tracing_test() -> std::sync::MutexGuard<'static, ()> {
  TRACING_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

const SPAN_FIELDS: &[&str] = &[
  "auv.authority.id",
  "auv.run.id",
  "auv.span.id",
  "auv.span.name",
  "auv.span.parent_id",
  "auv.span.remote_id",
  "auv.span.start_revision",
  "auv.span.end_revision",
];

const EVENT_FIELDS: &[&str] = &[
  "auv.authority.id",
  "auv.run.id",
  "auv.run.revision",
  "auv.span.id",
  "auv.event.id",
  "auv.event.schema.name",
  "auv.event.schema.version",
];

const ARTIFACT_FIELDS: &[&str] = &[
  "auv.authority.id",
  "auv.run.id",
  "auv.run.revision",
  "auv.span.id",
  "auv.artifact.uri",
  "auv.artifact.purpose",
  "auv.artifact.content_type",
  "auv.artifact.byte_length",
  "auv.artifact.sha256",
];

#[derive(Clone, Debug, PartialEq, Eq)]
enum CapturedParent {
  Contextual,
  Root,
  Explicit(u64),
}

#[derive(Clone, Debug)]
struct CapturedCallsite {
  id: Option<u64>,
  name: &'static str,
  target: &'static str,
  parent: CapturedParent,
  fields: BTreeSet<&'static str>,
  values: BTreeMap<&'static str, String>,
}

#[derive(Clone, Default)]
struct CapturingSubscriber {
  state: Arc<Mutex<CapturedState>>,
}

#[derive(Default)]
struct CapturedState {
  next_id: u64,
  callsites: Vec<CapturedCallsite>,
  spans: BTreeMap<u64, usize>,
}

impl CapturingSubscriber {
  fn callsites(&self) -> Vec<CapturedCallsite> {
    self.state.lock().unwrap().callsites.clone()
  }

  fn capture(metadata: &'static Metadata<'static>) -> CapturedCallsite {
    CapturedCallsite {
      id: None,
      name: metadata.name(),
      target: metadata.target(),
      parent: CapturedParent::Contextual,
      fields: metadata.fields().iter().map(|field| field.name()).collect(),
      values: BTreeMap::new(),
    }
  }

  fn parent(is_root: bool, is_contextual: bool, parent: Option<&Id>) -> CapturedParent {
    if let Some(parent) = parent {
      CapturedParent::Explicit(parent.into_u64())
    } else if is_root {
      CapturedParent::Root
    } else {
      assert!(is_contextual);
      CapturedParent::Contextual
    }
  }
}

impl Subscriber for CapturingSubscriber {
  fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
    true
  }

  fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> Interest {
    Interest::always()
  }

  fn new_span(&self, attributes: &TracingAttributes<'_>) -> Id {
    let mut callsite = Self::capture(attributes.metadata());
    attributes.record(&mut ValueVisitor(&mut callsite.values));

    let mut state = self.state.lock().unwrap();
    state.next_id += 1;
    let id = state.next_id;
    callsite.id = Some(id);
    callsite.parent = Self::parent(attributes.is_root(), attributes.is_contextual(), attributes.parent());
    let index = state.callsites.len();
    state.callsites.push(callsite);
    state.spans.insert(id, index);
    Id::from_u64(id)
  }

  fn record(&self, span: &Id, values: &Record<'_>) {
    let mut state = self.state.lock().unwrap();
    let index = state.spans[&span.into_u64()];
    values.record(&mut ValueVisitor(&mut state.callsites[index].values));
  }

  fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

  fn event(&self, event: &Event<'_>) {
    let mut callsite = Self::capture(event.metadata());
    event.record(&mut ValueVisitor(&mut callsite.values));
    callsite.parent = Self::parent(event.is_root(), event.is_contextual(), event.parent());
    self.state.lock().unwrap().callsites.push(callsite);
  }

  fn enter(&self, _span: &Id) {}

  fn exit(&self, _span: &Id) {}
}

struct ValueVisitor<'a>(&'a mut BTreeMap<&'static str, String>);

impl Visit for ValueVisitor<'_> {
  fn record_i64(&mut self, field: &Field, value: i64) {
    self.0.insert(field.name(), value.to_string());
  }

  fn record_u64(&mut self, field: &Field, value: u64) {
    self.0.insert(field.name(), value.to_string());
  }

  fn record_bool(&mut self, field: &Field, value: bool) {
    self.0.insert(field.name(), value.to_string());
  }

  fn record_str(&mut self, field: &Field, value: &str) {
    self.0.insert(field.name(), value.to_owned());
  }

  fn record_f64(&mut self, field: &Field, value: f64) {
    self.0.insert(field.name(), value.to_string());
  }

  fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
    self.0.insert(field.name(), format!("{value:?}"));
  }
}

#[test]
fn application_registers_rust_tracing_with_fixed_fields_policy() {
  let _serial = serial_tracing_test();
  let projector: Arc<dyn TelemetryProjector> = Arc::new(RustTracingProjector::new());
  let dispatch = configure().project_telemetry(projector, TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();

  drop(dispatch);
}

fn span_start(
  authority_id: Option<AuthorityId>,
  run_id: RunId,
  span_id: SpanId,
  parent_span_id: Option<SpanId>,
  seconds: i64,
) -> TelemetryItem {
  TelemetryItem::SpanStart {
    authority_id,
    run_id,
    span_id,
    parent_span_id,
    remote_span_id: None,
    name: SpanName::parse("auv.test.state").unwrap(),
    started_at: Timestamp::new(seconds, 0).unwrap(),
    start_revision: None,
    attributes: Attributes::empty(),
  }
}

fn span_end(authority_id: Option<AuthorityId>, run_id: RunId, span_id: SpanId, seconds: i64) -> TelemetryItem {
  TelemetryItem::SpanEnd {
    authority_id,
    run_id,
    span_id,
    ended_at: Timestamp::new(seconds, 0).unwrap(),
    end_revision: None,
  }
}

fn event(authority_id: Option<AuthorityId>, run_id: RunId, span_id: Option<SpanId>) -> TelemetryItem {
  TelemetryItem::Event {
    authority_id,
    run_id,
    span_id,
    event_id: EventId::new(),
    schema: EventSchema::new(EventName::parse("auv.test.state_event").unwrap(), 1).unwrap(),
    occurred_at: Timestamp::new(12, 0).unwrap(),
    revision: None,
  }
}

fn artifact(authority_id: AuthorityId, run_id: RunId, span_id: Option<SpanId>) -> TelemetryItem {
  TelemetryItem::Artifact {
    authority_id,
    run_id,
    span_id,
    uri: ArtifactUri::from_ids(run_id, ArtifactId::new()),
    purpose: ArtifactPurpose::parse("auv.test.state_artifact").unwrap(),
    content_type: ContentType::parse("application/octet-stream").unwrap(),
    byte_length: ByteLength::new(1).unwrap(),
    sha256: Sha256Digest::new([0; 32]),
    attributes: Attributes::empty(),
    revision: RunRevision::new(1).unwrap(),
  }
}

fn project(projector: &RustTracingProjector, item: TelemetryItem) -> Result<(), auv_tracing::TelemetryError> {
  block_on(projector.project(item))
}

#[test]
fn rust_tracing_emits_only_the_fixed_vocabulary() {
  let _serial = serial_tracing_test();
  let subscriber = CapturingSubscriber::default();
  let tracing_dispatch = tracing::Dispatch::new(subscriber.clone());
  let projector = RustTracingProjector::new();
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let span_id = SpanId::new();
  let remote_span_id = SpanId::new();
  let hidden_key = AttributeKey::parse("producer.secret").unwrap();
  let hidden_attributes = Attributes::try_from_iter([(hidden_key, AttributeValue::string("TOP_SECRET_VALUE").unwrap())]).unwrap();

  tracing::dispatcher::with_default(&tracing_dispatch, || {
    block_on(projector.project(TelemetryItem::SpanStart {
      authority_id: Some(authority_id),
      run_id,
      span_id,
      parent_span_id: None,
      remote_span_id: Some(remote_span_id),
      name: SpanName::parse("auv.test.projected_span").unwrap(),
      started_at: Timestamp::new(10, 20).unwrap(),
      start_revision: Some(RunRevision::new(3).unwrap()),
      attributes: hidden_attributes.clone(),
    }))
    .unwrap();
    block_on(projector.project(TelemetryItem::Event {
      authority_id: Some(authority_id),
      run_id,
      span_id: Some(span_id),
      event_id: EventId::new(),
      schema: EventSchema::new(EventName::parse("auv.test.projected_event").unwrap(), 2).unwrap(),
      occurred_at: Timestamp::new(12, 40).unwrap(),
      revision: Some(RunRevision::new(8).unwrap()),
    }))
    .unwrap();
    block_on(projector.project(TelemetryItem::SpanEnd {
      authority_id: Some(authority_id),
      run_id,
      span_id,
      ended_at: Timestamp::new(11, 30).unwrap(),
      end_revision: Some(RunRevision::new(7).unwrap()),
    }))
    .unwrap();
    block_on(projector.project(TelemetryItem::Artifact {
      authority_id,
      run_id,
      span_id: Some(span_id),
      uri: ArtifactUri::from_ids(run_id, ArtifactId::new()),
      purpose: ArtifactPurpose::parse("auv.test.capture").unwrap(),
      content_type: ContentType::parse("application/octet-stream").unwrap(),
      byte_length: ByteLength::new(42).unwrap(),
      sha256: Sha256Digest::new([0xab; 32]),
      attributes: hidden_attributes,
      revision: RunRevision::new(9).unwrap(),
    }))
    .unwrap();
    block_on(projector.flush()).unwrap();
  });

  let callsites = subscriber.callsites();
  assert_eq!(callsites.iter().map(|callsite| callsite.name).collect::<Vec<_>>(), ["auv.span", "auv.event", "auv.artifact.published"]);
  for callsite in &callsites {
    assert_eq!(callsite.target, "auv.telemetry.projection");
  }

  let span = &callsites[0];
  assert_eq!(span.fields, SPAN_FIELDS.iter().copied().collect());
  assert_eq!(span.values["auv.span.start_revision"], "3");
  assert_eq!(span.values["auv.span.end_revision"], "7");
  assert_eq!(span.values["auv.span.remote_id"], remote_span_id.to_string());
  let event = &callsites[1];
  assert_eq!(event.fields, EVENT_FIELDS.iter().copied().collect());
  assert_eq!(event.parent, CapturedParent::Explicit(span.id.unwrap()));
  assert_eq!(event.values["auv.run.revision"], "8");
  assert_eq!(event.values["auv.event.schema.version"], "2");
  let artifact = &callsites[2];
  assert_eq!(artifact.fields, ARTIFACT_FIELDS.iter().copied().collect());
  assert_eq!(artifact.parent, CapturedParent::Root);
  assert_eq!(artifact.values["auv.run.revision"], "9");
  assert_eq!(artifact.values["auv.artifact.byte_length"], "42");

  let all_values = callsites.iter().flat_map(|callsite| callsite.values.values()).cloned().collect::<Vec<_>>().join(" ");
  for forbidden in [
    "TOP_SECRET_VALUE",
    "event.payload",
    "artifact.bytes",
    "/tmp/",
    "content_url",
    "location",
  ] {
    assert!(!all_values.contains(forbidden), "projected forbidden value {forbidden}");
  }
}

#[test]
fn rust_tracing_run_event_ignores_ambient_span() {
  let _serial = serial_tracing_test();
  let subscriber = CapturingSubscriber::default();
  let tracing_dispatch = tracing::Dispatch::new(subscriber.clone());
  let projector = RustTracingProjector::new();

  tracing::dispatcher::with_default(&tracing_dispatch, || {
    let ambient = tracing::info_span!(target: "application.test", "ambient");
    let _entered = ambient.enter();
    project(&projector, event(Some(AuthorityId::new()), RunId::new(), None)).unwrap();
  });

  let projected_event = subscriber
    .callsites()
    .into_iter()
    .find(|callsite| callsite.target == "auv.telemetry.projection" && callsite.name == "auv.event")
    .unwrap();
  assert_eq!(projected_event.parent, CapturedParent::Root);
}

#[test]
fn rust_tracing_rejects_duplicate_start_before_and_after_end() {
  let _serial = serial_tracing_test();
  let projector = RustTracingProjector::new();
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let span_id = SpanId::new();
  let start = span_start(Some(authority_id), run_id, span_id, None, 10);

  project(&projector, start.clone()).unwrap();
  assert_eq!(project(&projector, start.clone()).unwrap_err().code().as_str(), "auv.telemetry.duplicate_span_start");
  project(&projector, span_end(Some(authority_id), run_id, span_id, 11)).unwrap();
  assert_eq!(project(&projector, start).unwrap_err().code().as_str(), "auv.telemetry.duplicate_span_start");
}

#[test]
fn rust_tracing_missing_duplicate_end_and_out_of_order_are_stable_errors() {
  let _serial = serial_tracing_test();
  let projector = RustTracingProjector::new();
  let authority_id = AuthorityId::new();
  let other_authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let span_id = SpanId::new();

  assert_eq!(
    project(&projector, span_end(Some(authority_id), run_id, span_id, 11)).unwrap_err().code().as_str(),
    "auv.telemetry.missing_span_start"
  );
  project(&projector, span_start(Some(authority_id), run_id, span_id, None, 10)).unwrap();
  assert_eq!(
    project(&projector, span_end(Some(other_authority_id), run_id, span_id, 11)).unwrap_err().code().as_str(),
    "auv.telemetry.span_authority_mismatch"
  );
  assert_eq!(
    project(&projector, span_end(Some(authority_id), run_id, span_id, 9)).unwrap_err().code().as_str(),
    "auv.telemetry.span_end_before_start"
  );
  project(&projector, span_end(Some(authority_id), run_id, span_id, 11)).unwrap();
  assert_eq!(
    project(&projector, span_end(Some(authority_id), run_id, span_id, 12)).unwrap_err().code().as_str(),
    "auv.telemetry.duplicate_span_end"
  );
}

#[test]
fn rust_tracing_enforces_run_authority_across_spans_events_and_artifacts() {
  let _serial = serial_tracing_test();
  let authority_id = AuthorityId::new();
  let other_authority_id = AuthorityId::new();

  let projector = RustTracingProjector::new();
  let run_id = RunId::new();
  project(&projector, span_start(Some(authority_id), run_id, SpanId::new(), None, 10)).unwrap();
  assert_eq!(
    project(&projector, event(Some(other_authority_id), run_id, None)).unwrap_err().code().as_str(),
    "auv.telemetry.run_authority_mismatch"
  );

  let projector = RustTracingProjector::new();
  let run_id = RunId::new();
  project(&projector, event(Some(authority_id), run_id, None)).unwrap();
  assert_eq!(
    project(&projector, artifact(other_authority_id, run_id, None)).unwrap_err().code().as_str(),
    "auv.telemetry.run_authority_mismatch"
  );

  let projector = RustTracingProjector::new();
  let run_id = RunId::new();
  project(&projector, artifact(authority_id, run_id, None)).unwrap();
  assert_eq!(
    project(&projector, span_start(Some(other_authority_id), run_id, SpanId::new(), None, 10)).unwrap_err().code().as_str(),
    "auv.telemetry.run_authority_mismatch"
  );
}

#[test]
fn rust_tracing_allows_same_span_id_in_different_runs() {
  let _serial = serial_tracing_test();
  let projector = RustTracingProjector::new();
  let authority_id = AuthorityId::new();
  let first_run_id = RunId::new();
  let second_run_id = RunId::new();
  let span_id = SpanId::new();

  project(&projector, span_start(Some(authority_id), first_run_id, span_id, None, 10)).unwrap();
  project(&projector, span_start(Some(authority_id), second_run_id, span_id, None, 10)).unwrap();
  project(&projector, span_end(Some(authority_id), first_run_id, span_id, 11)).unwrap();
  project(&projector, span_end(Some(authority_id), second_run_id, span_id, 11)).unwrap();
}

#[test]
fn rust_tracing_rejects_parent_end_until_child_ends() {
  let _serial = serial_tracing_test();
  let projector = RustTracingProjector::new();
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let parent_id = SpanId::new();
  let child_id = SpanId::new();

  project(&projector, span_start(Some(authority_id), run_id, parent_id, None, 10)).unwrap();
  project(&projector, span_start(Some(authority_id), run_id, child_id, Some(parent_id), 11)).unwrap();
  assert_eq!(
    project(&projector, span_end(Some(authority_id), run_id, parent_id, 12)).unwrap_err().code().as_str(),
    "auv.telemetry.span_has_active_children"
  );
  project(&projector, span_end(Some(authority_id), run_id, child_id, 12)).unwrap();
  project(&projector, span_end(Some(authority_id), run_id, parent_id, 13)).unwrap();
}

#[test]
fn rust_tracing_rejects_child_start_from_ended_parent() {
  let _serial = serial_tracing_test();
  let projector = RustTracingProjector::new();
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let parent_id = SpanId::new();

  project(&projector, span_start(Some(authority_id), run_id, parent_id, None, 10)).unwrap();
  project(&projector, span_end(Some(authority_id), run_id, parent_id, 11)).unwrap();
  assert_eq!(
    project(&projector, span_start(Some(authority_id), run_id, SpanId::new(), Some(parent_id), 12)).unwrap_err().code().as_str(),
    "auv.telemetry.ended_parent_span"
  );
}

#[test]
fn rust_tracing_parent_attachment_checks_authority() {
  let _serial = serial_tracing_test();
  let projector = RustTracingProjector::new();
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let parent_id = SpanId::new();

  project(&projector, span_start(Some(authority_id), run_id, parent_id, None, 10)).unwrap();
  assert_eq!(
    project(&projector, span_start(Some(AuthorityId::new()), run_id, SpanId::new(), Some(parent_id), 11),).unwrap_err().code().as_str(),
    "auv.telemetry.parent_authority_mismatch"
  );
}

#[test]
fn rust_tracing_span_event_requires_matching_live_span() {
  let _serial = serial_tracing_test();
  let projector = RustTracingProjector::new();
  let authority_id = AuthorityId::new();
  let other_authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let span_id = SpanId::new();

  project(&projector, event(Some(authority_id), run_id, None)).unwrap();
  assert_eq!(
    project(&projector, event(Some(authority_id), run_id, Some(span_id))).unwrap_err().code().as_str(),
    "auv.telemetry.missing_event_span"
  );
  project(&projector, span_start(Some(authority_id), run_id, span_id, None, 10)).unwrap();
  assert_eq!(
    project(&projector, event(Some(other_authority_id), run_id, Some(span_id))).unwrap_err().code().as_str(),
    "auv.telemetry.span_authority_mismatch"
  );
  project(&projector, span_end(Some(authority_id), run_id, span_id, 11)).unwrap();
  assert_eq!(
    project(&projector, event(Some(authority_id), run_id, Some(span_id))).unwrap_err().code().as_str(),
    "auv.telemetry.ended_event_span"
  );
}
