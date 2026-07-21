mod support;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use auv_tracing::{
  ArtifactId, ArtifactPurpose, ArtifactUri, AttributeKey, AttributeValue, Attributes, AuthorityId, ByteLength, ContentType, EventId,
  EventName, EventSchema, MemoryRunStore, NewArtifact, RunId, RunRevision, Sha256Digest, SpanId as AuvSpanId, SpanName, SpanSpec,
  TelemetryItem, TelemetryProjector, TelemetryRoutePolicy, Timestamp, configure, dispatcher,
};
use auv_tracing_otel::OtelProjector;
use futures_executor::block_on;
use futures_util::FutureExt;
use opentelemetry::Value;
use opentelemetry::logs::AnyValue;
use opentelemetry::trace::{Event as OtelEvent, SpanId, Status, TraceId};
use opentelemetry_sdk::logs::{BatchConfigBuilder as LogBatchConfigBuilder, BatchLogProcessor, SdkLoggerProvider};
use opentelemetry_sdk::trace::{BatchConfigBuilder as SpanBatchConfigBuilder, BatchSpanProcessor, SdkTracerProvider, SpanData};
use support::{
  BoundedLogExporter, BoundedSpanExporter, CallbackLogProcessor, CallbackSpanProcessor, FlushProbeLogProcessor, FlushProbeSpanProcessor,
  MAX_EXPORTED_ITEMS,
};

fn providers() -> (SdkTracerProvider, SdkLoggerProvider, BoundedSpanExporter, BoundedLogExporter) {
  let span_exporter = BoundedSpanExporter::default();
  let log_exporter = BoundedLogExporter::default();
  let tracer_provider = SdkTracerProvider::builder().with_batch_exporter(span_exporter.clone()).build();
  let logger_provider = SdkLoggerProvider::builder().with_batch_exporter(log_exporter.clone()).build();
  (tracer_provider, logger_provider, span_exporter, log_exporter)
}

fn timestamp(seconds: i64, nanoseconds: u32) -> Timestamp {
  Timestamp::new(seconds, nanoseconds).unwrap()
}

fn system_time(seconds: u64, nanoseconds: u32) -> SystemTime {
  UNIX_EPOCH + Duration::new(seconds, nanoseconds)
}

fn scalar_attributes(prefix: &str) -> Attributes {
  Attributes::try_from_iter([
    (AttributeKey::parse(format!("{prefix}.bool")).unwrap(), AttributeValue::boolean(true)),
    (AttributeKey::parse(format!("{prefix}.i64")).unwrap(), AttributeValue::integer(-17).unwrap()),
    (AttributeKey::parse(format!("{prefix}.f64")).unwrap(), AttributeValue::float(1.25).unwrap()),
    (AttributeKey::parse(format!("{prefix}.string")).unwrap(), AttributeValue::string("bounded-text").unwrap()),
  ])
  .unwrap()
}

fn span_attributes(span: &SpanData) -> BTreeMap<String, Value> {
  span.attributes.iter().map(|attribute| (attribute.key.as_str().to_owned(), attribute.value.clone())).collect()
}

fn event_attributes(event: &OtelEvent) -> BTreeMap<String, Value> {
  event.attributes.iter().map(|attribute| (attribute.key.as_str().to_owned(), attribute.value.clone())).collect()
}

fn log_attributes(record: &opentelemetry_sdk::logs::SdkLogRecord) -> BTreeMap<String, AnyValue> {
  record.attributes_iter().map(|(key, value)| (key.as_str().to_owned(), value.clone())).collect()
}

fn projected_span<'a>(spans: &'a [SpanData], name: &str) -> &'a SpanData {
  spans.iter().find(|span| span.name == name).unwrap_or_else(|| panic!("missing exported span {name}"))
}

#[test]
fn exported_sdk_data_preserves_bounded_auv_semantics() {
  let (tracer_provider, logger_provider, span_exporter, log_exporter) = providers();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let root_id = AuvSpanId::new();
  let child_id = AuvSpanId::new();
  let remote_root_id = AuvSpanId::new();
  let remote_link_id = AuvSpanId::new();
  let span_event_id = EventId::new();

  block_on(projector.project(TelemetryItem::SpanStart {
    authority_id: Some(authority_id),
    run_id,
    span_id: root_id,
    parent_span_id: None,
    remote_span_id: None,
    name: SpanName::parse("auv.test.otel_root").unwrap(),
    started_at: timestamp(10, 100),
    start_revision: Some(RunRevision::new(2).unwrap()),
    attributes: scalar_attributes("producer.span"),
  }))
  .unwrap();
  block_on(projector.project(TelemetryItem::SpanStart {
    authority_id: Some(authority_id),
    run_id,
    span_id: child_id,
    parent_span_id: Some(root_id),
    remote_span_id: None,
    name: SpanName::parse("auv.test.otel_child").unwrap(),
    started_at: timestamp(11, 200),
    start_revision: Some(RunRevision::new(3).unwrap()),
    attributes: Attributes::empty(),
  }))
  .unwrap();
  block_on(projector.project(TelemetryItem::SpanStart {
    authority_id: Some(authority_id),
    run_id,
    span_id: remote_root_id,
    parent_span_id: None,
    remote_span_id: Some(remote_link_id),
    name: SpanName::parse("auv.test.otel_remote_root").unwrap(),
    started_at: timestamp(12, 300),
    start_revision: Some(RunRevision::new(4).unwrap()),
    attributes: Attributes::empty(),
  }))
  .unwrap();

  block_on(projector.project(TelemetryItem::Event {
    authority_id: Some(authority_id),
    run_id,
    span_id: Some(child_id),
    event_id: span_event_id,
    schema: EventSchema::new(EventName::parse("auv.test.span_event").unwrap(), 2).unwrap(),
    occurred_at: timestamp(13, 400),
    revision: Some(RunRevision::new(5).unwrap()),
  }))
  .unwrap();
  block_on(projector.project(TelemetryItem::Event {
    authority_id: Some(authority_id),
    run_id,
    span_id: None,
    event_id: EventId::new(),
    schema: EventSchema::new(EventName::parse("auv.test.run_event").unwrap(), 3).unwrap(),
    occurred_at: timestamp(14, 500),
    revision: Some(RunRevision::new(6).unwrap()),
  }))
  .unwrap();
  for (span_id, seconds, revision) in [
    (child_id, 15, 8),
    (root_id, 16, 9),
    (remote_root_id, 17, 10),
  ] {
    block_on(projector.project(TelemetryItem::SpanEnd {
      authority_id: Some(authority_id),
      run_id,
      span_id,
      ended_at: timestamp(seconds, 600),
      end_revision: Some(RunRevision::new(revision).unwrap()),
    }))
    .unwrap();
  }
  let artifact_uri = ArtifactUri::from_ids(run_id, ArtifactId::new());
  block_on(projector.project(TelemetryItem::Artifact {
    authority_id,
    run_id,
    span_id: Some(child_id),
    uri: artifact_uri.clone(),
    purpose: ArtifactPurpose::parse("auv.test.capture").unwrap(),
    content_type: ContentType::parse("image/png").unwrap(),
    byte_length: ByteLength::new(42).unwrap(),
    sha256: Sha256Digest::new([0xab; 32]),
    attributes: scalar_attributes("producer.artifact"),
    revision: RunRevision::new(7).unwrap(),
  }))
  .unwrap();
  block_on(projector.flush()).unwrap();

  let spans = span_exporter.spans();
  assert_eq!(spans.len(), 3);
  let root = projected_span(&spans, "auv.test.otel_root");
  let child = projected_span(&spans, "auv.test.otel_child");
  let remote_root = projected_span(&spans, "auv.test.otel_remote_root");
  assert_eq!(root.status, Status::Unset);
  assert_eq!(child.status, Status::Unset);
  assert_eq!(remote_root.status, Status::Unset);
  assert_eq!(root.start_time, system_time(10, 100));
  assert_eq!(root.end_time, system_time(16, 600));
  assert_eq!(root.parent_span_id, SpanId::INVALID);
  assert_eq!(remote_root.parent_span_id, SpanId::INVALID);
  assert!(remote_root.links.is_empty());
  assert_eq!(child.span_context.trace_id(), root.span_context.trace_id());
  assert_eq!(child.parent_span_id, root.span_context.span_id());
  assert_ne!(remote_root.span_context.trace_id(), root.span_context.trace_id());
  assert_ne!(root.span_context.trace_id(), TraceId::from_bytes(*run_id.as_uuid().as_bytes()));
  let root_auv_bytes = root_id.as_uuid().as_bytes();
  assert_ne!(root.span_context.span_id(), SpanId::from_bytes(root_auv_bytes[8..].try_into().unwrap()));

  let root_attributes = span_attributes(root);
  assert_eq!(root_attributes["auv.span.start_revision"], Value::I64(2));
  assert_eq!(root_attributes["auv.span.end_revision"], Value::I64(9));
  assert_eq!(root_attributes["producer.span.bool"], Value::Bool(true));
  assert_eq!(root_attributes["producer.span.i64"], Value::I64(-17));
  assert_eq!(root_attributes["producer.span.f64"], Value::F64(1.25));
  assert_eq!(root_attributes["producer.span.string"], Value::String("bounded-text".into()));
  let child_attributes = span_attributes(child);
  assert_eq!(child_attributes["auv.span.parent_id"], Value::String(root_id.to_string().into()));
  assert_eq!(child.events.len(), 1);
  let span_event = &child.events[0];
  assert_eq!(span_event.name.as_ref(), "auv.test.span_event");
  assert_eq!(span_event.timestamp, system_time(13, 400));
  let span_event_attributes = event_attributes(span_event);
  assert_eq!(span_event_attributes.len(), 7);
  assert_eq!(span_event_attributes["auv.authority.id"], Value::String(authority_id.to_string().into()));
  assert_eq!(span_event_attributes["auv.run.id"], Value::String(run_id.to_string().into()));
  assert_eq!(span_event_attributes["auv.run.revision"], Value::I64(5));
  assert_eq!(span_event_attributes["auv.span.id"], Value::String(child_id.to_string().into()));
  assert_eq!(span_event_attributes["auv.event.id"], Value::String(span_event_id.to_string().into()));
  assert_eq!(span_event_attributes["auv.event.schema.name"], Value::String("auv.test.span_event".into()));
  assert_eq!(span_event_attributes["auv.event.schema.version"], Value::I64(2));
  assert!(span_event_attributes.keys().all(|key| !key.contains("payload") && !key.contains("json")));
  let remote_attributes = span_attributes(remote_root);
  assert_eq!(remote_attributes["auv.span.remote_id"], Value::String(remote_link_id.to_string().into()));

  let logs = log_exporter.logs();
  assert_eq!(logs.len(), 2);
  let run_event = logs
    .iter()
    .find(|record| log_attributes(record).get("auv.event.schema.name") == Some(&AnyValue::String("auv.test.run_event".into())))
    .unwrap();
  let artifact = logs.iter().find(|record| record.event_name() == Some("auv.artifact.published")).unwrap();
  // NOTICE: OpenTelemetry 0.32 LogRecord event names accept only &'static str.
  // The bounded dynamic schema remains exact in the schema attribute instead
  // of being leaked or interned into an unbounded process-lifetime registry.
  assert_eq!(run_event.event_name(), Some("auv.event"));
  assert!(run_event.trace_context().is_none());
  let run_event_attributes = log_attributes(run_event);
  assert_eq!(run_event_attributes["auv.run.revision"], AnyValue::Int(6));
  assert_eq!(run_event_attributes["auv.event.schema.name"], AnyValue::String("auv.test.run_event".into()));
  let artifact_attributes = log_attributes(artifact);
  assert_eq!(artifact_attributes["auv.run.revision"], AnyValue::Int(7));
  assert_eq!(artifact_attributes["auv.artifact.uri"], AnyValue::String(artifact_uri.to_string().into()));
  assert_eq!(artifact_attributes["producer.artifact.bool"], AnyValue::Boolean(true));
  assert_eq!(artifact_attributes["producer.artifact.i64"], AnyValue::Int(-17));
  assert_eq!(artifact_attributes["producer.artifact.f64"], AnyValue::Double(1.25));
  assert_eq!(artifact_attributes["producer.artifact.string"], AnyValue::String("bounded-text".into()));
  assert!(artifact.trace_context().is_none());

  for record in &logs {
    assert!(record.body().is_none());
    let attributes = log_attributes(record);
    assert!(
      attributes.values().all(|value| matches!(value, AnyValue::Boolean(_) | AnyValue::Int(_) | AnyValue::Double(_) | AnyValue::String(_)))
    );
    assert!(!attributes.contains_key("auv.span.start_revision"));
    assert!(!attributes.contains_key("auv.span.end_revision"));
    for forbidden in [
      "payload",
      "json",
      "bytes",
      "path",
      "location",
      "content_url",
    ] {
      assert!(attributes.keys().all(|key| !key.contains(forbidden)), "exported forbidden log field containing {forbidden}");
    }
  }
}

fn span_start(
  authority_id: Option<AuthorityId>,
  run_id: RunId,
  span_id: AuvSpanId,
  parent_span_id: Option<AuvSpanId>,
  seconds: i64,
) -> TelemetryItem {
  TelemetryItem::SpanStart {
    authority_id,
    run_id,
    span_id,
    parent_span_id,
    remote_span_id: None,
    name: SpanName::parse("auv.test.state").unwrap(),
    started_at: timestamp(seconds, 0),
    start_revision: None,
    attributes: Attributes::empty(),
  }
}

fn span_end(authority_id: Option<AuthorityId>, run_id: RunId, span_id: AuvSpanId, seconds: i64) -> TelemetryItem {
  TelemetryItem::SpanEnd {
    authority_id,
    run_id,
    span_id,
    ended_at: timestamp(seconds, 0),
    end_revision: None,
  }
}

fn event_at(authority_id: Option<AuthorityId>, run_id: RunId, span_id: Option<AuvSpanId>, seconds: i64) -> TelemetryItem {
  TelemetryItem::Event {
    authority_id,
    run_id,
    span_id,
    event_id: EventId::new(),
    schema: EventSchema::new(EventName::parse("auv.test.state_event").unwrap(), 1).unwrap(),
    occurred_at: timestamp(seconds, 0),
    revision: None,
  }
}

fn event(authority_id: Option<AuthorityId>, run_id: RunId, span_id: Option<AuvSpanId>) -> TelemetryItem {
  event_at(authority_id, run_id, span_id, 12)
}

fn artifact(authority_id: AuthorityId, run_id: RunId, span_id: Option<AuvSpanId>) -> TelemetryItem {
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

fn project(projector: &OtelProjector, item: TelemetryItem) -> Result<(), auv_tracing::TelemetryError> {
  block_on(projector.project(item))
}

fn project_from_callback_thread(projector: Arc<OtelProjector>, item: TelemetryItem) -> Result<(), auv_tracing::TelemetryError> {
  let (done_tx, done_rx) = mpsc::channel();
  std::thread::spawn(move || {
    let result = project(&projector, item);
    done_tx.send(result).unwrap();
  });
  done_rx.recv_timeout(Duration::from_secs(2)).expect("cross-thread OTEL callback projection did not return")
}

struct RouteSpan {
  attributes: Attributes,
}

impl SpanSpec for RouteSpan {
  const NAME: &'static str = "auv.test.otel_route";

  fn attributes(&self) -> Attributes {
    self.attributes.clone()
  }
}

#[test]
fn application_route_controls_otel_attribute_allowlist() {
  let (tracer_provider, logger_provider, span_exporter, _log_exporter) = providers();
  let projector = Arc::new(OtelProjector::new(tracer_provider, logger_provider));
  let allowed = AttributeKey::parse("producer.allowed").unwrap();
  let hidden = AttributeKey::parse("producer.hidden").unwrap();
  let dispatch = configure()
    .project_telemetry(projector, TelemetryRoutePolicy::fixed_fields_only().allow_span_attribute(allowed.clone()))
    .build()
    .unwrap();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(RunId::new()));
  let attributes = Attributes::try_from_iter([
    (allowed, AttributeValue::boolean(true)),
    (hidden, AttributeValue::string("must-not-export").unwrap()),
  ])
  .unwrap();

  let span = root.in_scope(|| auv_tracing::start_span!(RouteSpan { attributes }));
  drop(span);
  block_on(dispatch.flush()).unwrap();

  let spans = span_exporter.spans();
  assert_eq!(spans.len(), 1);
  let attributes = span_attributes(&spans[0]);
  assert_eq!(attributes["producer.allowed"], Value::Bool(true));
  assert!(!attributes.contains_key("producer.hidden"));
  assert!(attributes.values().all(|value| value != &Value::String("must-not-export".into())));
}

#[test]
fn artifact_route_filters_attributes_from_public_emission() {
  let (tracer_provider, logger_provider, _span_exporter, log_exporter) = providers();
  let projector = Arc::new(OtelProjector::new(tracer_provider, logger_provider));
  let authority_id = AuthorityId::new();
  let store = Arc::new(MemoryRunStore::new(authority_id));
  let allowed = AttributeKey::parse("producer.artifact.allowed").unwrap();
  let hidden = AttributeKey::parse("producer.artifact.hidden").unwrap();
  let dispatch = configure()
    .run_store(store)
    .project_telemetry(projector, TelemetryRoutePolicy::fixed_fields_only().allow_artifact_attribute(allowed.clone()))
    .build()
    .unwrap();
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || auv_tracing::Context::root(run_id));
  let attributes = Attributes::try_from_iter([
    (allowed, AttributeValue::boolean(true)),
    (hidden, AttributeValue::string("must-not-export").unwrap()),
  ])
  .unwrap();
  let artifact = NewArtifact::new(
    ArtifactPurpose::parse("auv.test.filtered_artifact").unwrap(),
    ContentType::parse("application/octet-stream").unwrap(),
    ByteLength::new(0).unwrap(),
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".parse().unwrap(),
    attributes,
    futures_util::io::Cursor::new(Vec::new()),
  );

  let metadata = root.in_scope(|| block_on(auv_tracing::emit_artifact(artifact))).unwrap().unwrap();
  block_on(dispatch.flush()).unwrap();

  assert_eq!(metadata.uri().run_id(), run_id);
  let logs = log_exporter.logs();
  assert_eq!(logs.len(), 1);
  let attributes = log_attributes(&logs[0]);
  assert_eq!(attributes["producer.artifact.allowed"], AnyValue::Boolean(true));
  assert!(!attributes.contains_key("producer.artifact.hidden"));
  assert!(attributes.values().all(|value| value != &AnyValue::String("must-not-export".into())));
}

#[test]
fn missing_duplicate_and_out_of_order_span_state_are_stable_errors() {
  let (tracer_provider, logger_provider, _span_exporter, _log_exporter) = providers();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let span_id = AuvSpanId::new();
  let missing_parent_id = AuvSpanId::new();
  let start = TelemetryItem::SpanStart {
    authority_id: Some(authority_id),
    run_id,
    span_id,
    parent_span_id: None,
    remote_span_id: None,
    name: SpanName::parse("auv.test.state").unwrap(),
    started_at: timestamp(20, 0),
    start_revision: None,
    attributes: Attributes::empty(),
  };

  let missing = block_on(projector.project(TelemetryItem::SpanEnd {
    authority_id: Some(authority_id),
    run_id,
    span_id,
    ended_at: timestamp(21, 0),
    end_revision: None,
  }))
  .unwrap_err();
  assert_eq!(missing.code().as_str(), "auv.telemetry.otel_missing_span_start");
  let missing_parent = block_on(projector.project(TelemetryItem::SpanStart {
    authority_id: Some(authority_id),
    run_id,
    span_id: AuvSpanId::new(),
    parent_span_id: Some(missing_parent_id),
    remote_span_id: None,
    name: SpanName::parse("auv.test.missing_parent").unwrap(),
    started_at: timestamp(20, 0),
    start_revision: None,
    attributes: Attributes::empty(),
  }))
  .unwrap_err();
  assert_eq!(missing_parent.code().as_str(), "auv.telemetry.otel_missing_parent_span");
  block_on(projector.project(start.clone())).unwrap();
  let duplicate = block_on(projector.project(start.clone())).unwrap_err();
  assert_eq!(duplicate.code().as_str(), "auv.telemetry.otel_duplicate_span_start");
  let out_of_order = block_on(projector.project(TelemetryItem::SpanEnd {
    authority_id: Some(authority_id),
    run_id,
    span_id,
    ended_at: timestamp(19, 0),
    end_revision: None,
  }))
  .unwrap_err();
  assert_eq!(out_of_order.code().as_str(), "auv.telemetry.otel_span_end_before_start");
  let wrong_authority = project(&projector, span_end(Some(AuthorityId::new()), run_id, span_id, 21)).unwrap_err();
  assert_eq!(wrong_authority.code().as_str(), "auv.telemetry.otel_span_authority_mismatch");
  block_on(projector.project(TelemetryItem::SpanEnd {
    authority_id: Some(authority_id),
    run_id,
    span_id,
    ended_at: timestamp(21, 0),
    end_revision: None,
  }))
  .unwrap();
  let duplicate_after_end = block_on(projector.project(start)).unwrap_err();
  assert_eq!(duplicate_after_end.code().as_str(), "auv.telemetry.otel_duplicate_span_start");
  let duplicate_end = block_on(projector.project(TelemetryItem::SpanEnd {
    authority_id: Some(authority_id),
    run_id,
    span_id,
    ended_at: timestamp(22, 0),
    end_revision: None,
  }))
  .unwrap_err();
  assert_eq!(duplicate_end.code().as_str(), "auv.telemetry.otel_duplicate_span_end");
}

#[test]
fn run_authority_is_enforced_across_otel_spans_events_and_artifacts() {
  let authority_id = AuthorityId::new();
  let other_authority_id = AuthorityId::new();

  let (tracer_provider, logger_provider, _, _) = providers();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let run_id = RunId::new();
  project(&projector, span_start(Some(authority_id), run_id, AuvSpanId::new(), None, 10)).unwrap();
  assert_eq!(
    project(&projector, event(Some(other_authority_id), run_id, None)).unwrap_err().code().as_str(),
    "auv.telemetry.otel_run_authority_mismatch"
  );

  let (tracer_provider, logger_provider, _, _) = providers();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let run_id = RunId::new();
  project(&projector, event(Some(authority_id), run_id, None)).unwrap();
  assert_eq!(
    project(&projector, artifact(other_authority_id, run_id, None)).unwrap_err().code().as_str(),
    "auv.telemetry.otel_run_authority_mismatch"
  );

  let (tracer_provider, logger_provider, _, _) = providers();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let run_id = RunId::new();
  project(&projector, artifact(authority_id, run_id, None)).unwrap();
  assert_eq!(
    project(&projector, span_start(Some(other_authority_id), run_id, AuvSpanId::new(), None, 10)).unwrap_err().code().as_str(),
    "auv.telemetry.otel_run_authority_mismatch"
  );
}

#[test]
fn same_auv_span_id_is_valid_in_different_otel_runs() {
  let (tracer_provider, logger_provider, _, _) = providers();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let authority_id = AuthorityId::new();
  let first_run_id = RunId::new();
  let second_run_id = RunId::new();
  let span_id = AuvSpanId::new();

  project(&projector, span_start(Some(authority_id), first_run_id, span_id, None, 10)).unwrap();
  project(&projector, span_start(Some(authority_id), second_run_id, span_id, None, 10)).unwrap();
  project(&projector, span_end(Some(authority_id), first_run_id, span_id, 11)).unwrap();
  project(&projector, span_end(Some(authority_id), second_run_id, span_id, 11)).unwrap();
}

#[test]
fn otel_parent_end_before_child_exports_both_spans() {
  let (tracer_provider, logger_provider, span_exporter, _) = providers();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let parent_id = AuvSpanId::new();
  let child_id = AuvSpanId::new();

  project(&projector, span_start(Some(authority_id), run_id, parent_id, None, 10)).unwrap();
  project(&projector, span_start(Some(authority_id), run_id, child_id, Some(parent_id), 11)).unwrap();
  project(&projector, span_end(Some(authority_id), run_id, parent_id, 12)).unwrap();
  project(&projector, span_end(Some(authority_id), run_id, child_id, 13)).unwrap();
  block_on(projector.flush()).unwrap();

  let spans = span_exporter.spans();
  assert_eq!(spans.len(), 2);
  let parent = spans.iter().find(|span| span.parent_span_id == SpanId::INVALID).unwrap();
  let child = spans.iter().find(|span| span.parent_span_id == parent.span_context.span_id()).unwrap();
  assert_eq!(parent.end_time, system_time(12, 0));
  assert_eq!(child.end_time, system_time(13, 0));
}

#[test]
fn otel_rejects_conflicting_local_and_remote_parentage() {
  let (tracer_provider, logger_provider, _, _) = providers();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let parent_id = AuvSpanId::new();
  let child_id = AuvSpanId::new();
  project(&projector, span_start(Some(authority_id), run_id, parent_id, None, 10)).unwrap();
  let mut child = span_start(Some(authority_id), run_id, child_id, Some(parent_id), 11);
  let TelemetryItem::SpanStart { remote_span_id, .. } = &mut child else {
    unreachable!();
  };
  *remote_span_id = Some(AuvSpanId::new());

  assert_eq!(project(&projector, child).unwrap_err().code().as_str(), "auv.telemetry.otel_conflicting_span_relationship");
}

#[test]
fn otel_temporal_order_errors_are_stable() {
  let (tracer_provider, logger_provider, _, _) = providers();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let authority_id = AuthorityId::new();

  let run_id = RunId::new();
  let parent_id = AuvSpanId::new();
  project(&projector, span_start(Some(authority_id), run_id, parent_id, None, 10)).unwrap();
  assert_eq!(
    project(&projector, span_start(Some(authority_id), run_id, AuvSpanId::new(), Some(parent_id), 9)).unwrap_err().code().as_str(),
    "auv.telemetry.otel_child_before_parent"
  );

  let run_id = RunId::new();
  let span_id = AuvSpanId::new();
  project(&projector, span_start(Some(authority_id), run_id, span_id, None, 10)).unwrap();
  assert_eq!(
    project(&projector, event_at(Some(authority_id), run_id, Some(span_id), 9)).unwrap_err().code().as_str(),
    "auv.telemetry.otel_event_before_span_start"
  );
  project(&projector, event_at(Some(authority_id), run_id, Some(span_id), 14)).unwrap();
  project(&projector, event_at(Some(authority_id), run_id, Some(span_id), 12)).unwrap();
  assert_eq!(
    project(&projector, span_end(Some(authority_id), run_id, span_id, 13)).unwrap_err().code().as_str(),
    "auv.telemetry.otel_span_end_before_event"
  );

  let run_id = RunId::new();
  let parent_id = AuvSpanId::new();
  project(&projector, span_start(Some(authority_id), run_id, parent_id, None, 10)).unwrap();
  project(&projector, span_start(Some(authority_id), run_id, AuvSpanId::new(), Some(parent_id), 14)).unwrap();
  project(&projector, span_start(Some(authority_id), run_id, AuvSpanId::new(), Some(parent_id), 12)).unwrap();
  assert_eq!(
    project(&projector, span_end(Some(authority_id), run_id, parent_id, 13)).unwrap_err().code().as_str(),
    "auv.telemetry.otel_span_end_before_child_start"
  );
}

#[test]
fn otel_child_cannot_start_from_ended_parent() {
  let (tracer_provider, logger_provider, _, _) = providers();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let parent_id = AuvSpanId::new();

  project(&projector, span_start(Some(authority_id), run_id, parent_id, None, 10)).unwrap();
  project(&projector, span_end(Some(authority_id), run_id, parent_id, 11)).unwrap();
  assert_eq!(
    project(&projector, span_start(Some(authority_id), run_id, AuvSpanId::new(), Some(parent_id), 12)).unwrap_err().code().as_str(),
    "auv.telemetry.otel_ended_parent_span"
  );
}

#[test]
fn otel_parent_attachment_checks_authority() {
  let (tracer_provider, logger_provider, _, _) = providers();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let parent_id = AuvSpanId::new();

  project(&projector, span_start(Some(authority_id), run_id, parent_id, None, 10)).unwrap();
  assert_eq!(
    project(&projector, span_start(Some(AuthorityId::new()), run_id, AuvSpanId::new(), Some(parent_id), 11),).unwrap_err().code().as_str(),
    "auv.telemetry.otel_parent_authority_mismatch"
  );
}

#[test]
fn otel_span_event_requires_matching_live_span() {
  let (tracer_provider, logger_provider, _, _) = providers();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let authority_id = AuthorityId::new();
  let other_authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let span_id = AuvSpanId::new();

  project(&projector, event(Some(authority_id), run_id, None)).unwrap();
  assert_eq!(
    project(&projector, event(Some(authority_id), run_id, Some(span_id))).unwrap_err().code().as_str(),
    "auv.telemetry.otel_missing_event_span"
  );
  project(&projector, span_start(Some(authority_id), run_id, span_id, None, 10)).unwrap();
  assert_eq!(
    project(&projector, event(Some(other_authority_id), run_id, Some(span_id))).unwrap_err().code().as_str(),
    "auv.telemetry.otel_span_authority_mismatch"
  );
  project(&projector, span_end(Some(authority_id), run_id, span_id, 11)).unwrap();
  assert_eq!(
    project(&projector, event(Some(authority_id), run_id, Some(span_id))).unwrap_err().code().as_str(),
    "auv.telemetry.otel_ended_event_span"
  );
}

#[test]
fn concurrent_direct_span_event_and_end_are_linearized() {
  const ATTEMPTS: usize = 32;

  let (tracer_provider, logger_provider, span_exporter, _) = providers();
  let projector = Arc::new(OtelProjector::new(tracer_provider, logger_provider));
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let mut outcomes = Vec::with_capacity(ATTEMPTS);

  for _ in 0..ATTEMPTS {
    let span_id = AuvSpanId::new();
    let event_id = EventId::new();
    project(&projector, span_start(Some(authority_id), run_id, span_id, None, 10)).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let event_projector = Arc::clone(&projector);
    let event_barrier = Arc::clone(&barrier);
    let end_projector = Arc::clone(&projector);
    let end_barrier = Arc::clone(&barrier);

    let event_item = TelemetryItem::Event {
      authority_id: Some(authority_id),
      run_id,
      span_id: Some(span_id),
      event_id,
      schema: EventSchema::new(EventName::parse("auv.test.concurrent_event").unwrap(), 1).unwrap(),
      occurred_at: timestamp(11, 0),
      revision: None,
    };
    let retry_event = event_item.clone();
    let (event_tx, event_rx) = mpsc::channel();
    std::thread::spawn(move || {
      event_barrier.wait();
      event_tx.send(project(&event_projector, event_item)).unwrap();
    });
    let end_item = span_end(Some(authority_id), run_id, span_id, 12);
    let retry_end = end_item.clone();
    let (end_tx, end_rx) = mpsc::channel();
    std::thread::spawn(move || {
      end_barrier.wait();
      end_tx.send(project(&end_projector, end_item)).unwrap();
    });
    barrier.wait();

    let mut event_result = event_rx.recv_timeout(Duration::from_secs(2)).expect("concurrent OTEL event did not return");
    let mut end_result = end_rx.recv_timeout(Duration::from_secs(2)).expect("concurrent OTEL end did not return");
    if event_result.as_ref().is_err_and(|error| error.code().as_str() == "auv.telemetry.otel_concurrent_projection") {
      event_result = project(&projector, retry_event);
    }
    if end_result.as_ref().is_err_and(|error| error.code().as_str() == "auv.telemetry.otel_concurrent_projection") {
      end_result = project(&projector, retry_end);
    }

    end_result.unwrap();
    if let Err(error) = &event_result {
      assert_eq!(error.code().as_str(), "auv.telemetry.otel_ended_event_span");
    }
    outcomes.push((span_id, event_id, event_result.is_ok()));
  }
  block_on(projector.flush()).unwrap();

  let spans = span_exporter.spans();
  assert_eq!(spans.len(), ATTEMPTS);
  for (span_id, event_id, event_succeeded) in outcomes {
    let span_id = Value::String(span_id.to_string().into());
    let span = spans.iter().find(|span| span_attributes(span).get("auv.span.id") == Some(&span_id)).unwrap();
    if event_succeeded {
      assert_eq!(span.events.len(), 1);
      assert_eq!(event_attributes(&span.events[0])["auv.event.id"], Value::String(event_id.to_string().into()));
    } else {
      assert!(span.events.is_empty());
    }
  }
}

#[test]
fn flush_delegates_to_both_providers_without_shutdown() {
  let span_exporter = BoundedSpanExporter::default();
  let log_exporter = BoundedLogExporter::default();
  let span_probe = FlushProbeSpanProcessor::failing();
  let log_probe = FlushProbeLogProcessor::failing();
  let tracer_provider =
    SdkTracerProvider::builder().with_batch_exporter(span_exporter.clone()).with_span_processor(span_probe.clone()).build();
  let logger_provider = SdkLoggerProvider::builder().with_batch_exporter(log_exporter.clone()).with_log_processor(log_probe.clone()).build();
  let projector = OtelProjector::new(tracer_provider, logger_provider);

  let error = block_on(projector.flush()).unwrap_err();

  assert_eq!(error.code().as_str(), "auv.telemetry.otel_flush_failed");
  assert_eq!(span_probe.force_flush_count(), 1);
  assert_eq!(log_probe.force_flush_count(), 1);
  assert_eq!(span_exporter.shutdown_count(), 0);
  assert_eq!(log_exporter.shutdown_count(), 0);
  assert_eq!(span_probe.shutdown_count(), 0);
  assert_eq!(log_probe.shutdown_count(), 0);
}

#[test]
fn bounded_span_exporter_overflow_is_reported_by_flush() {
  let span_exporter = BoundedSpanExporter::default();
  let processor = BatchSpanProcessor::builder(span_exporter.clone())
    .with_batch_config(
      SpanBatchConfigBuilder::default()
        .with_max_queue_size(MAX_EXPORTED_ITEMS * 2)
        .with_max_export_batch_size(MAX_EXPORTED_ITEMS * 2)
        .with_scheduled_delay(Duration::from_secs(24 * 60 * 60))
        .build(),
    )
    .build();
  let tracer_provider = SdkTracerProvider::builder().with_span_processor(processor).build();
  let logger_provider = SdkLoggerProvider::builder().with_batch_exporter(BoundedLogExporter::default()).build();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();

  for _ in 0..=MAX_EXPORTED_ITEMS {
    let span_id = AuvSpanId::new();
    project(&projector, span_start(Some(authority_id), run_id, span_id, None, 10)).unwrap();
    project(&projector, span_end(Some(authority_id), run_id, span_id, 11)).unwrap();
  }

  let error = block_on(projector.flush()).unwrap_err();
  assert_eq!(error.code().as_str(), "auv.telemetry.otel_flush_failed");
  assert!(span_exporter.spans().is_empty());
}

#[test]
fn bounded_log_exporter_overflow_is_reported_by_flush() {
  let log_exporter = BoundedLogExporter::default();
  let processor = BatchLogProcessor::builder(log_exporter.clone())
    .with_batch_config(
      LogBatchConfigBuilder::default()
        .with_max_queue_size(MAX_EXPORTED_ITEMS * 2)
        .with_max_export_batch_size(MAX_EXPORTED_ITEMS * 2)
        .with_scheduled_delay(Duration::from_secs(24 * 60 * 60))
        .build(),
    )
    .build();
  let tracer_provider = SdkTracerProvider::builder().with_batch_exporter(BoundedSpanExporter::default()).build();
  let logger_provider = SdkLoggerProvider::builder().with_log_processor(processor).build();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();

  for _ in 0..=MAX_EXPORTED_ITEMS {
    project(&projector, event(Some(authority_id), run_id, None)).unwrap();
  }

  let error = block_on(projector.flush()).unwrap_err();
  assert_eq!(error.code().as_str(), "auv.telemetry.otel_flush_failed");
  assert!(log_exporter.logs().is_empty());
}

#[test]
fn otel_reentrant_processor_projection_fails_without_hanging() {
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let span_id = AuvSpanId::new();
  let projector_slot = Arc::new(Mutex::new(None::<std::sync::Weak<OtelProjector>>));
  let callback_slot = Arc::clone(&projector_slot);
  let (reentrant_tx, reentrant_rx) = mpsc::channel();
  let processor = CallbackSpanProcessor::new(
    Arc::new(move || {
      let projector = callback_slot.lock().unwrap().as_ref().unwrap().upgrade().unwrap();
      let result = projector.project(event(Some(authority_id), run_id, None)).now_or_never().expect("projector future is immediately ready");
      reentrant_tx.send(result).unwrap();
    }),
    Arc::new(|| {}),
  );
  let tracer_provider = SdkTracerProvider::builder().with_span_processor(processor).build();
  let logger_provider = SdkLoggerProvider::builder().build();
  let projector = Arc::new(OtelProjector::new(tracer_provider, logger_provider));
  *projector_slot.lock().unwrap() = Some(Arc::downgrade(&projector));
  let (done_tx, done_rx) = mpsc::channel();
  let worker_projector = Arc::clone(&projector);
  std::thread::spawn(move || {
    done_tx.send(project(&worker_projector, span_start(Some(authority_id), run_id, span_id, None, 10))).unwrap();
  });

  done_rx.recv_timeout(Duration::from_secs(2)).expect("OTEL processor callback deadlocked").unwrap();
  let reentrant = reentrant_rx.recv_timeout(Duration::from_secs(2)).expect("reentrant projection did not return").unwrap_err();
  assert_eq!(reentrant.code().as_str(), "auv.telemetry.otel_reentrant_projection");
}

#[test]
fn otel_cross_thread_processor_projection_is_promptly_busy() {
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let other_run_id = RunId::new();
  let span_id = AuvSpanId::new();
  let retry_item = event(Some(authority_id), other_run_id, None);
  let callback_item = retry_item.clone();
  let emit_count = Arc::new(AtomicUsize::new(0));
  let emit_observer = Arc::clone(&emit_count);
  let projector_slot = Arc::new(Mutex::new(None::<std::sync::Weak<OtelProjector>>));
  let callback_slot = Arc::clone(&projector_slot);
  let (callback_tx, callback_rx) = mpsc::channel();
  let processor = CallbackSpanProcessor::new(
    Arc::new(move || {
      let projector = callback_slot.lock().unwrap().as_ref().unwrap().upgrade().unwrap();
      callback_tx.send(project_from_callback_thread(projector, callback_item.clone())).unwrap();
    }),
    Arc::new(|| {}),
  );
  let tracer_provider = SdkTracerProvider::builder().with_span_processor(processor).build();
  let logger_provider = SdkLoggerProvider::builder()
    .with_log_processor(CallbackLogProcessor::new(Arc::new(move || {
      emit_observer.fetch_add(1, Ordering::SeqCst);
    })))
    .build();
  let projector = Arc::new(OtelProjector::new(tracer_provider, logger_provider));
  *projector_slot.lock().unwrap() = Some(Arc::downgrade(&projector));

  project(&projector, span_start(Some(authority_id), run_id, span_id, None, 10)).unwrap();
  let busy = callback_rx.recv_timeout(Duration::from_secs(2)).expect("OTEL processor callback did not complete").unwrap_err();
  assert_eq!(busy.code().as_str(), "auv.telemetry.otel_concurrent_projection");
  project(&projector, retry_item).unwrap();
  assert_eq!(emit_count.load(Ordering::SeqCst), 1);
}

#[test]
fn otel_cross_thread_log_projection_is_promptly_busy() {
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let other_run_id = RunId::new();
  let retry_item = event(Some(authority_id), other_run_id, None);
  let callback_item = retry_item.clone();
  let projector_slot = Arc::new(Mutex::new(None::<std::sync::Weak<OtelProjector>>));
  let callback_slot = Arc::clone(&projector_slot);
  let first_emit = Arc::new(AtomicBool::new(true));
  let emit_flag = Arc::clone(&first_emit);
  let emit_count = Arc::new(AtomicUsize::new(0));
  let emit_observer = Arc::clone(&emit_count);
  let (callback_tx, callback_rx) = mpsc::channel();
  let processor = CallbackLogProcessor::new(Arc::new(move || {
    emit_observer.fetch_add(1, Ordering::SeqCst);
    if emit_flag.swap(false, Ordering::SeqCst) {
      let projector = callback_slot.lock().unwrap().as_ref().unwrap().upgrade().unwrap();
      callback_tx.send(project_from_callback_thread(projector, callback_item.clone())).unwrap();
    }
  }));
  let logger_provider = SdkLoggerProvider::builder().with_log_processor(processor).build();
  let projector = Arc::new(OtelProjector::new(SdkTracerProvider::builder().build(), logger_provider));
  *projector_slot.lock().unwrap() = Some(Arc::downgrade(&projector));

  project(&projector, event(Some(authority_id), run_id, None)).unwrap();
  let busy = callback_rx.recv_timeout(Duration::from_secs(2)).expect("OTEL log callback did not complete").unwrap_err();
  assert_eq!(busy.code().as_str(), "auv.telemetry.otel_concurrent_projection");
  project(&projector, retry_item).unwrap();
  assert_eq!(emit_count.load(Ordering::SeqCst), 2);
}

#[test]
fn otel_start_panic_retains_run_authority_and_start_identity() {
  let panic_start = Arc::new(AtomicBool::new(true));
  let start_flag = Arc::clone(&panic_start);
  let start_processor = CallbackSpanProcessor::new(
    Arc::new(move || {
      if start_flag.swap(false, Ordering::SeqCst) {
        panic!("test OTEL on_start panic");
      }
    }),
    Arc::new(|| {}),
  );
  let tracer_provider = SdkTracerProvider::builder().with_span_processor(start_processor).build();
  let projector = OtelProjector::new(tracer_provider, SdkLoggerProvider::builder().build());
  let first_authority_id = AuthorityId::new();
  let second_authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let failed_span_id = AuvSpanId::new();
  let panic = catch_unwind(AssertUnwindSafe(|| project(&projector, span_start(Some(first_authority_id), run_id, failed_span_id, None, 10))));
  assert!(panic.is_err());
  assert_eq!(
    project(&projector, span_start(Some(second_authority_id), run_id, AuvSpanId::new(), None, 10)).unwrap_err().code().as_str(),
    "auv.telemetry.otel_run_authority_mismatch"
  );
  assert_eq!(
    project(&projector, span_start(Some(first_authority_id), run_id, failed_span_id, None, 10)).unwrap_err().code().as_str(),
    "auv.telemetry.otel_duplicate_span_start"
  );
  let span_id = AuvSpanId::new();
  project(&projector, span_start(Some(first_authority_id), run_id, span_id, None, 10)).unwrap();
  project(&projector, span_end(Some(first_authority_id), run_id, span_id, 11)).unwrap();
}

#[test]
fn otel_end_panic_keeps_end_terminal_and_exports_once() {
  let panic_end = Arc::new(AtomicBool::new(true));
  let end_flag = Arc::clone(&panic_end);
  let end_count = Arc::new(AtomicUsize::new(0));
  let count_observer = Arc::clone(&end_count);
  let end_processor = CallbackSpanProcessor::new(
    Arc::new(|| {}),
    Arc::new(move || {
      count_observer.fetch_add(1, Ordering::SeqCst);
      if end_flag.swap(false, Ordering::SeqCst) {
        panic!("test OTEL on_end panic");
      }
    }),
  );
  let span_exporter = BoundedSpanExporter::default();
  let tracer_provider = SdkTracerProvider::builder().with_batch_exporter(span_exporter.clone()).with_span_processor(end_processor).build();
  let projector = OtelProjector::new(tracer_provider, SdkLoggerProvider::builder().build());
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let span_id = AuvSpanId::new();
  project(&projector, span_start(Some(authority_id), run_id, span_id, None, 10)).unwrap();
  let panic = catch_unwind(AssertUnwindSafe(|| project(&projector, span_end(Some(authority_id), run_id, span_id, 11))));
  assert!(panic.is_err());
  block_on(projector.flush()).unwrap();
  assert_eq!(end_count.load(Ordering::SeqCst), 1);
  assert_eq!(span_exporter.spans().len(), 1);
  assert_eq!(
    project(&projector, span_end(Some(authority_id), run_id, span_id, 12)).unwrap_err().code().as_str(),
    "auv.telemetry.otel_duplicate_span_end"
  );
  assert_eq!(end_count.load(Ordering::SeqCst), 1);
  assert_eq!(span_exporter.spans().len(), 1);
}

#[test]
fn otel_invalid_child_context_restores_parent_child_maximum() {
  let tracer_provider = SdkTracerProvider::builder().build();
  let projector = OtelProjector::new(tracer_provider.clone(), SdkLoggerProvider::builder().build());
  let authority_id = AuthorityId::new();
  let run_id = RunId::new();
  let parent_id = AuvSpanId::new();

  project(&projector, span_start(Some(authority_id), run_id, parent_id, None, 10)).unwrap();
  project(&projector, span_start(Some(authority_id), run_id, AuvSpanId::new(), Some(parent_id), 14)).unwrap();
  tracer_provider.shutdown().unwrap();

  // ROOT CAUSE:
  //
  // If the SDK rejected a child before `on_start`, rollback removed its
  // reservation but left its timestamp as the parent's child maximum.
  //
  // Before the fix, the failed child at 20 prevented the parent ending at 15.
  // The fix restores the previous maximum at 14 instead of clearing it.
  let failed_child = project(&projector, span_start(Some(authority_id), run_id, AuvSpanId::new(), Some(parent_id), 20)).unwrap_err();
  assert_eq!(failed_child.code().as_str(), "auv.telemetry.otel_invalid_span_context");
  assert_eq!(
    project(&projector, span_end(Some(authority_id), run_id, parent_id, 13)).unwrap_err().code().as_str(),
    "auv.telemetry.otel_span_end_before_child_start"
  );
  project(&projector, span_end(Some(authority_id), run_id, parent_id, 15)).unwrap();
}

#[test]
fn otel_invalid_root_context_does_not_claim_run_authority() {
  let tracer_provider = SdkTracerProvider::builder().build();
  let projector = OtelProjector::new(tracer_provider.clone(), SdkLoggerProvider::builder().build());
  tracer_provider.shutdown().unwrap();
  let run_id = RunId::new();

  let first = project(&projector, span_start(Some(AuthorityId::new()), run_id, AuvSpanId::new(), None, 10)).unwrap_err();
  assert_eq!(first.code().as_str(), "auv.telemetry.otel_invalid_span_context");
  let second = project(&projector, span_start(Some(AuthorityId::new()), run_id, AuvSpanId::new(), None, 10)).unwrap_err();
  assert_eq!(second.code().as_str(), "auv.telemetry.otel_invalid_span_context");
}

#[test]
fn otel_run_emission_panic_retains_authority() {
  for artifact_projection in [false, true] {
    let panic_emit = Arc::new(AtomicBool::new(true));
    let emit_flag = Arc::clone(&panic_emit);
    let processor = CallbackLogProcessor::new(Arc::new(move || {
      if emit_flag.swap(false, Ordering::SeqCst) {
        panic!("test OTEL log processor panic");
      }
    }));
    let logger_provider = SdkLoggerProvider::builder().with_log_processor(processor).build();
    let projector = OtelProjector::new(SdkTracerProvider::builder().build(), logger_provider);
    let first_authority_id = AuthorityId::new();
    let second_authority_id = AuthorityId::new();
    let run_id = RunId::new();

    let first = if artifact_projection {
      artifact(first_authority_id, run_id, None)
    } else {
      event(Some(first_authority_id), run_id, None)
    };
    let panic = catch_unwind(AssertUnwindSafe(|| project(&projector, first)));
    assert!(panic.is_err());

    let wrong_authority = if artifact_projection {
      artifact(second_authority_id, run_id, None)
    } else {
      event(Some(second_authority_id), run_id, None)
    };
    assert_eq!(project(&projector, wrong_authority).unwrap_err().code().as_str(), "auv.telemetry.otel_run_authority_mismatch");

    let same_authority = if artifact_projection {
      artifact(first_authority_id, run_id, None)
    } else {
      event(Some(first_authority_id), run_id, None)
    };
    project(&projector, same_authority).unwrap();
  }
}

#[test]
fn otel_precallback_validation_error_does_not_claim_authority() {
  let (tracer_provider, logger_provider, _, _) = providers();
  let projector = OtelProjector::new(tracer_provider, logger_provider);
  let run_id = RunId::new();

  assert_eq!(
    project(&projector, span_start(Some(AuthorityId::new()), run_id, AuvSpanId::new(), Some(AuvSpanId::new()), 10),)
      .unwrap_err()
      .code()
      .as_str(),
    "auv.telemetry.otel_missing_parent_span"
  );
  project(&projector, span_start(Some(AuthorityId::new()), run_id, AuvSpanId::new(), None, 10)).unwrap();
}
