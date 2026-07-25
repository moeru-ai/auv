#![forbid(unsafe_code)]

//! Lossy OpenTelemetry export for full-fidelity AUV trace records.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use auv_tracing::{AttributeValue, ErrorCode, ExportError, RunId, SpanId, Timestamp, TraceExporter, TraceRecord};
use opentelemetry::logs::{AnyValue, LogRecord, Logger, LoggerProvider};
use opentelemetry::trace::{TraceContextExt, Tracer, TracerProvider};
use opentelemetry::{Context, Key, KeyValue, Value};
use opentelemetry_sdk::logs::{SdkLogger, SdkLoggerProvider};
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};

const SCOPE: &str = "auv-tracing";
const LOG_TARGET: &str = "auv.telemetry.export";

/// Exports AUV records through application-owned OpenTelemetry providers.
#[derive(Clone)]
pub struct OtelExporter {
  inner: Arc<Inner>,
}

struct Inner {
  tracer_provider: SdkTracerProvider,
  logger_provider: SdkLoggerProvider,
  tracer: SdkTracer,
  logger: SdkLogger,
  spans: Mutex<BTreeMap<(RunId, SpanId), ActiveSpan>>,
}

struct ActiveSpan {
  started_at: Timestamp,
  context: Context,
}

impl OtelExporter {
  /// Creates an exporter without installing globals or owning provider shutdown.
  pub fn new(tracer_provider: SdkTracerProvider, logger_provider: SdkLoggerProvider) -> Self {
    let tracer = tracer_provider.tracer(SCOPE);
    let logger = logger_provider.logger(SCOPE);
    Self {
      inner: Arc::new(Inner {
        tracer_provider,
        logger_provider,
        tracer,
        logger,
        spans: Mutex::new(BTreeMap::new()),
      }),
    }
  }
}

impl TraceExporter for OtelExporter {
  fn export(&self, record: TraceRecord) -> auv_tracing::BoxFuture<'_, Result<(), ExportError>> {
    Box::pin(async move { self.inner.export(record) })
  }

  fn flush(&self) -> auv_tracing::BoxFuture<'_, Result<(), ExportError>> {
    Box::pin(async move {
      let trace_failed = self.inner.tracer_provider.force_flush().is_err();
      let log_failed = self.inner.logger_provider.force_flush().is_err();
      if trace_failed || log_failed {
        Err(error("auv.telemetry.otel_flush_failed"))
      } else {
        Ok(())
      }
    })
  }
}

impl Inner {
  fn export(&self, record: TraceRecord) -> Result<(), ExportError> {
    match record {
      TraceRecord::SpanStarted {
        run_id,
        span_id,
        parent_span_id,
        remote_span_id,
        name,
        started_at,
        attributes,
      } => {
        let mut values = vec![
          KeyValue::new("auv.run.id", run_id.to_string()),
          KeyValue::new("auv.span.id", span_id.to_string()),
          KeyValue::new("auv.span.name", name.as_str().to_owned()),
        ];
        if let Some(parent) = parent_span_id {
          values.push(KeyValue::new("auv.span.parent_id", parent.to_string()));
        }
        if let Some(remote) = remote_span_id {
          values.push(KeyValue::new("auv.span.remote_id", remote.to_string()));
        }
        values.extend(attributes.iter().map(|(key, value)| KeyValue::new(Key::new(key.as_str().to_owned()), otel_value(value))));

        let parent = {
          let spans = self.spans.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?;
          if spans.contains_key(&(run_id, span_id)) {
            return Err(error("auv.telemetry.otel_duplicate_span_start"));
          }
          match parent_span_id {
            Some(id) => {
              spans.get(&(run_id, id)).map(|active| active.context.clone()).ok_or_else(|| error("auv.telemetry.otel_missing_parent_span"))?
            }
            None => Context::default(),
          }
        };
        let span = self
          .tracer
          .span_builder(name.as_str().to_owned())
          .with_start_time(system_time(started_at)?)
          .with_attributes(values)
          .start_with_context(&self.tracer, &parent);
        self.spans.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?.insert(
          (run_id, span_id),
          ActiveSpan {
            started_at,
            context: Context::new().with_span(span),
          },
        );
        Ok(())
      }
      TraceRecord::SpanEnded {
        run_id,
        span_id,
        ended_at,
      } => {
        let end_time = system_time(ended_at)?;
        let active = {
          let mut spans = self.spans.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?;
          let active = spans.get(&(run_id, span_id)).ok_or_else(|| error("auv.telemetry.otel_missing_span_start"))?;
          if ended_at < active.started_at {
            return Err(error("auv.telemetry.otel_span_end_before_start"));
          }
          spans.remove(&(run_id, span_id)).expect("validated span remains present")
        };
        active.context.span().end_with_timestamp(end_time);
        Ok(())
      }
      TraceRecord::Event {
        run_id,
        span_id,
        event_id,
        schema,
        occurred_at,
        ..
      } => {
        let attributes = vec![
          KeyValue::new("auv.run.id", run_id.to_string()),
          KeyValue::new("auv.event.id", event_id.to_string()),
          KeyValue::new("auv.event.schema.name", schema.name().as_str().to_owned()),
          KeyValue::new("auv.event.schema.version", i64::from(schema.version().get())),
        ];
        if let Some(span_id) = span_id {
          let context = self
            .spans
            .lock()
            .map_err(|_| error("auv.telemetry.otel_state_poisoned"))?
            .get(&(run_id, span_id))
            .map(|active| active.context.clone())
            .ok_or_else(|| error("auv.telemetry.otel_missing_event_span"))?;
          context.span().add_event_with_timestamp(schema.name().as_str().to_owned(), system_time(occurred_at)?, attributes);
        } else {
          let mut record = self.logger.create_log_record();
          record.set_event_name("auv.event");
          record.set_target(LOG_TARGET);
          record.set_timestamp(system_time(occurred_at)?);
          record.add_attribute("auv.run.id", run_id.to_string());
          record.add_attribute("auv.event.id", event_id.to_string());
          record.add_attribute("auv.event.schema.name", schema.name().as_str().to_owned());
          record.add_attribute("auv.event.schema.version", i64::from(schema.version().get()));
          self.emit_log(record);
        }
        Ok(())
      }
      TraceRecord::Artifact {
        run_id,
        span_id,
        metadata,
      } => {
        let mut record = self.logger.create_log_record();
        record.set_event_name("auv.artifact.published");
        record.set_target(LOG_TARGET);
        record.add_attribute("auv.run.id", run_id.to_string());
        if let Some(span_id) = span_id {
          record.add_attribute("auv.span.id", span_id.to_string());
        }
        record.add_attribute("auv.artifact.uri", metadata.uri().to_string());
        record.add_attribute("auv.artifact.purpose", metadata.purpose().as_str().to_owned());
        record.add_attribute("auv.artifact.content_type", metadata.content_type().to_string());
        record.add_attribute("auv.artifact.byte_length", metadata.byte_length().get() as i64);
        record.add_attribute("auv.artifact.sha256", metadata.sha256().to_string());
        for (key, value) in metadata.attributes().iter() {
          record.add_attribute(Key::new(key.as_str().to_owned()), log_value(value));
        }
        self.emit_log(record);
        Ok(())
      }
    }
  }

  fn emit_log(&self, record: opentelemetry_sdk::logs::SdkLogRecord) {
    let context = Context::new();
    let _guard = context.attach();
    self.logger.emit(record);
  }
}

fn otel_value(value: &AttributeValue) -> Value {
  match value {
    AttributeValue::Bool(v) => Value::Bool(*v),
    AttributeValue::I64(v) => Value::I64(*v),
    AttributeValue::F64(v) => Value::F64(v.get()),
    AttributeValue::String(v) => Value::String(v.as_str().to_owned().into()),
  }
}
fn log_value(value: &AttributeValue) -> AnyValue {
  match value {
    AttributeValue::Bool(v) => AnyValue::Boolean(*v),
    AttributeValue::I64(v) => AnyValue::Int(*v),
    AttributeValue::F64(v) => AnyValue::Double(v.get()),
    AttributeValue::String(v) => AnyValue::String(v.as_str().to_owned().into()),
  }
}
fn system_time(timestamp: Timestamp) -> Result<SystemTime, ExportError> {
  if timestamp.unix_seconds() >= 0 {
    UNIX_EPOCH
      .checked_add(Duration::new(timestamp.unix_seconds() as u64, timestamp.nanoseconds()))
      .ok_or_else(|| error("auv.telemetry.otel_timestamp_out_of_range"))
  } else {
    UNIX_EPOCH
      .checked_sub(Duration::from_secs(timestamp.unix_seconds().unsigned_abs()))
      .and_then(|value| value.checked_add(Duration::from_nanos(u64::from(timestamp.nanoseconds()))))
      .ok_or_else(|| error("auv.telemetry.otel_timestamp_out_of_range"))
  }
}
fn error(code: &'static str) -> ExportError {
  ExportError::new(ErrorCode::parse(code).expect("static OTEL error code"))
}
