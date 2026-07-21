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

const ALLOWED_FIELDS: &[&str] = &[
  "auv.authority.id",
  "auv.run.id",
  "auv.run.revision",
  "auv.span.id",
  "auv.span.name",
  "auv.span.parent_id",
  "auv.span.remote_id",
  "auv.span.start_revision",
  "auv.span.end_revision",
  "auv.event.id",
  "auv.event.schema.name",
  "auv.event.schema.version",
  "auv.artifact.uri",
  "auv.artifact.purpose",
  "auv.artifact.content_type",
  "auv.artifact.byte_length",
  "auv.artifact.sha256",
];

#[derive(Clone, Debug)]
struct CapturedCallsite {
  name: &'static str,
  target: &'static str,
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
      name: metadata.name(),
      target: metadata.target(),
      fields: metadata.fields().iter().map(|field| field.name()).collect(),
      values: BTreeMap::new(),
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
  let projector: Arc<dyn TelemetryProjector> = Arc::new(RustTracingProjector::new());
  let dispatch = configure().project_telemetry(projector, TelemetryRoutePolicy::fixed_fields_only()).build().unwrap();

  drop(dispatch);
}

#[test]
fn rust_tracing_emits_only_the_fixed_vocabulary() {
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
    block_on(projector.project(TelemetryItem::SpanEnd {
      authority_id: Some(authority_id),
      run_id,
      span_id,
      ended_at: Timestamp::new(11, 30).unwrap(),
      end_revision: Some(RunRevision::new(7).unwrap()),
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
  let allowed = ALLOWED_FIELDS.iter().copied().collect::<BTreeSet<_>>();
  for callsite in &callsites {
    assert!(callsite.fields.is_subset(&allowed), "{} emitted unexpected fields: {:?}", callsite.name, callsite.fields);
    assert_eq!(callsite.target, "auv.telemetry.projection");
  }

  let span = &callsites[0];
  assert_eq!(span.values["auv.span.start_revision"], "3");
  assert_eq!(span.values["auv.span.end_revision"], "7");
  assert_eq!(span.values["auv.span.remote_id"], remote_span_id.to_string());
  let event = &callsites[1];
  assert_eq!(event.values["auv.run.revision"], "8");
  assert_eq!(event.values["auv.event.schema.version"], "2");
  let artifact = &callsites[2];
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
