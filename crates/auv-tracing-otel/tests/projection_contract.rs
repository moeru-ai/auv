mod support;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use auv_tracing::{
  ArtifactId, ArtifactPurpose, ArtifactUri, AttributeKey, AttributeValue, Attributes, AuthorityId, ByteLength, ContentType, EventId,
  EventName, EventSchema, RunId, RunRevision, Sha256Digest, SpanId as AuvSpanId, SpanName, SpanSpec, TelemetryItem, TelemetryProjector,
  TelemetryRoutePolicy, Timestamp, configure, dispatcher,
};
use auv_tracing_otel::OtelProjector;
use futures_executor::block_on;
use opentelemetry::Value;
use opentelemetry::logs::AnyValue;
use opentelemetry::trace::{SpanId, Status, TraceId};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::trace::{SdkTracerProvider, SpanData};
use support::{BoundedLogExporter, BoundedSpanExporter, FlushProbeLogProcessor, FlushProbeSpanProcessor};

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
    event_id: EventId::new(),
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
  let remote_attributes = span_attributes(remote_root);
  assert_eq!(remote_attributes["auv.span.remote_id"], Value::String(remote_link_id.to_string().into()));

  let logs = log_exporter.logs();
  assert_eq!(logs.len(), 3);
  let span_event = logs
    .iter()
    .find(|record| log_attributes(record).get("auv.event.schema.name") == Some(&AnyValue::String("auv.test.span_event".into())))
    .unwrap();
  let run_event = logs
    .iter()
    .find(|record| log_attributes(record).get("auv.event.schema.name") == Some(&AnyValue::String("auv.test.run_event".into())))
    .unwrap();
  let artifact = logs.iter().find(|record| record.event_name() == Some("auv.artifact.published")).unwrap();
  assert_eq!(span_event.event_name(), Some("auv.event"));
  assert_eq!(span_event.timestamp(), Some(system_time(13, 400)));
  assert_eq!(span_event.trace_context().unwrap().trace_id, child.span_context.trace_id());
  assert_eq!(span_event.trace_context().unwrap().span_id, child.span_context.span_id());
  assert_eq!(log_attributes(span_event)["auv.run.revision"], AnyValue::Int(5));
  assert!(run_event.trace_context().is_none());
  assert_eq!(log_attributes(run_event)["auv.run.revision"], AnyValue::Int(6));
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
  let duplicate = block_on(projector.project(start)).unwrap_err();
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
  block_on(projector.project(TelemetryItem::SpanEnd {
    authority_id: Some(authority_id),
    run_id,
    span_id,
    ended_at: timestamp(21, 0),
    end_revision: None,
  }))
  .unwrap();
  let duplicate_end = block_on(projector.project(TelemetryItem::SpanEnd {
    authority_id: Some(authority_id),
    run_id,
    span_id,
    ended_at: timestamp(22, 0),
    end_revision: None,
  }))
  .unwrap_err();
  assert_eq!(duplicate_end.code().as_str(), "auv.telemetry.otel_missing_span_start");
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
