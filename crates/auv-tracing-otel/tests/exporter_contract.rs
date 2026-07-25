mod support;

use auv_tracing::{
  ArtifactId, ArtifactMetadata, ArtifactPurpose, ArtifactUri, Attributes, ByteLength, ContentType, EventId, EventName, EventSchema,
  JsonPayload, RunId, Sha256Digest, SpanId, SpanName, Timestamp, TraceExporter, TraceRecord,
};
use auv_tracing_otel::OtelExporter;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use support::{BoundedLogExporter, BoundedSpanExporter};

#[test]
fn span_records_export_without_authority_or_revision_vocabulary() {
  let output = BoundedSpanExporter::default();
  let tracer = SdkTracerProvider::builder().with_batch_exporter(output.clone()).build();
  let exporter = OtelExporter::new(tracer, SdkLoggerProvider::builder().build());
  let run_id = RunId::new();
  let span_id = SpanId::new();
  futures_executor::block_on(exporter.export(TraceRecord::SpanStarted {
    run_id,
    span_id,
    parent_span_id: None,
    remote_span_id: None,
    name: SpanName::parse("auv.test.operation").unwrap(),
    started_at: Timestamp::new(100, 0).unwrap(),
    attributes: Attributes::empty(),
  }))
  .unwrap();
  futures_executor::block_on(exporter.export(TraceRecord::SpanEnded {
    run_id,
    span_id,
    ended_at: Timestamp::new(101, 0).unwrap(),
  }))
  .unwrap();
  futures_executor::block_on(exporter.flush()).unwrap();

  let spans = output.spans();
  assert_eq!(spans.len(), 1);
  let keys: Vec<_> = spans[0].attributes.iter().map(|value| value.key.as_str()).collect();
  assert!(keys.contains(&"auv.run.id"));
  assert!(!keys.iter().any(|key| key.contains("authority") || key.contains("revision")));
}

#[test]
fn event_and_artifact_records_export_as_logs_without_history_vocabulary() {
  let output = BoundedLogExporter::default();
  let logger = SdkLoggerProvider::builder().with_batch_exporter(output.clone()).build();
  let exporter = OtelExporter::new(SdkTracerProvider::builder().build(), logger);
  let run_id = RunId::new();
  futures_executor::block_on(exporter.export(TraceRecord::Event {
    run_id,
    span_id: None,
    event_id: EventId::new(),
    schema: EventSchema::new(EventName::parse("auv.test.event").unwrap(), 1).unwrap(),
    occurred_at: Timestamp::new(100, 0).unwrap(),
    payload: JsonPayload::from_str(r#"{"secret":"not exported"}"#).unwrap(),
  }))
  .unwrap();
  futures_executor::block_on(exporter.export(TraceRecord::Artifact {
    run_id,
    span_id: None,
    metadata: ArtifactMetadata::new(
      ArtifactUri::from_ids(run_id, ArtifactId::new()),
      ArtifactPurpose::parse("auv.test.output").unwrap(),
      ContentType::parse("text/plain").unwrap(),
      ByteLength::new(0).unwrap(),
      Sha256Digest::new([0; 32]),
      Attributes::empty(),
    ),
  }))
  .unwrap();
  futures_executor::block_on(exporter.flush()).unwrap();

  let logs = output.logs();
  assert_eq!(logs.len(), 2);
  for log in logs {
    let keys: Vec<_> = log.attributes_iter().map(|(key, _)| key.as_str()).collect();
    assert!(!keys.iter().any(|key| key.contains("authority") || key.contains("revision")));
    assert!(!keys.contains(&"secret"));
  }
}

#[test]
fn rejected_span_records_do_not_corrupt_pairing_state() {
  let exporter = OtelExporter::new(SdkTracerProvider::builder().build(), SdkLoggerProvider::builder().build());
  let run_id = RunId::new();
  let span_id = SpanId::new();
  let start = TraceRecord::SpanStarted {
    run_id,
    span_id,
    parent_span_id: None,
    remote_span_id: None,
    name: SpanName::parse("auv.test.operation").unwrap(),
    started_at: Timestamp::new(100, 0).unwrap(),
    attributes: Attributes::empty(),
  };
  futures_executor::block_on(exporter.export(start.clone())).unwrap();
  assert!(futures_executor::block_on(exporter.export(start)).is_err());
  assert!(
    futures_executor::block_on(exporter.export(TraceRecord::SpanEnded {
      run_id,
      span_id,
      ended_at: Timestamp::new(99, 0).unwrap(),
    }))
    .is_err()
  );
  futures_executor::block_on(exporter.export(TraceRecord::SpanEnded {
    run_id,
    span_id,
    ended_at: Timestamp::new(101, 0).unwrap(),
  }))
  .unwrap();
}
