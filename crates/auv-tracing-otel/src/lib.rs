#![forbid(unsafe_code)]

//! Bounded OpenTelemetry projection for AUV run telemetry.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use auv_tracing::{
  AttributeValue, Attributes, AuthorityId, ErrorCode, RunId, RunRevision, SpanId, TelemetryError, TelemetryItem, TelemetryProjector,
  Timestamp,
};
use opentelemetry::logs::{AnyValue, LogRecord, Logger, LoggerProvider};
use opentelemetry::trace::{Span as _, TraceContextExt, Tracer, TracerProvider};
use opentelemetry::{Context, Key, KeyValue, Value};
use opentelemetry_sdk::logs::{SdkLogger, SdkLoggerProvider};
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};

const INSTRUMENTATION_SCOPE: &str = "auv-tracing";
const LOG_TARGET: &str = "auv.telemetry.projection";

/// Projects bounded AUV telemetry into application-supplied OTEL SDK providers.
#[derive(Clone)]
pub struct OtelProjector {
  inner: Arc<OtelProjectorInner>,
}

struct OtelProjectorInner {
  tracer_provider: SdkTracerProvider,
  logger_provider: SdkLoggerProvider,
  tracer: SdkTracer,
  logger: SdkLogger,
  active_spans: Mutex<BTreeMap<(RunId, SpanId), ActiveSpan>>,
}

#[derive(Clone)]
struct ActiveSpan {
  authority_id: Option<AuthorityId>,
  started_at: Timestamp,
  context: Context,
}

impl OtelProjector {
  /// Uses providers configured by the application without installing exporters
  /// or changing either provider's lifecycle.
  pub fn new(tracer_provider: SdkTracerProvider, logger_provider: SdkLoggerProvider) -> Self {
    let tracer = tracer_provider.tracer(INSTRUMENTATION_SCOPE);
    let logger = logger_provider.logger(INSTRUMENTATION_SCOPE);
    Self {
      inner: Arc::new(OtelProjectorInner {
        tracer_provider,
        logger_provider,
        tracer,
        logger,
        active_spans: Mutex::new(BTreeMap::new()),
      }),
    }
  }
}

impl TelemetryProjector for OtelProjector {
  fn project(&self, item: TelemetryItem) -> auv_tracing::BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async move { self.inner.project(item) })
  }

  fn flush(&self) -> auv_tracing::BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async move {
      let trace_result = self.inner.tracer_provider.force_flush();
      let log_result = self.inner.logger_provider.force_flush();
      if trace_result.is_err() || log_result.is_err() {
        Err(error("auv.telemetry.otel_flush_failed"))
      } else {
        Ok(())
      }
    })
  }
}

impl OtelProjectorInner {
  fn project(&self, item: TelemetryItem) -> Result<(), TelemetryError> {
    match item {
      TelemetryItem::SpanStart {
        authority_id,
        run_id,
        span_id,
        parent_span_id,
        remote_span_id,
        name,
        started_at,
        start_revision,
        attributes,
      } => self.start_span(SpanStartInput {
        authority_id,
        run_id,
        span_id,
        parent_span_id,
        remote_span_id,
        name: name.as_str().to_owned(),
        started_at,
        start_revision,
        attributes,
      }),
      TelemetryItem::SpanEnd {
        authority_id,
        run_id,
        span_id,
        ended_at,
        end_revision,
      } => self.end_span(authority_id, run_id, span_id, ended_at, end_revision),
      TelemetryItem::Event {
        authority_id,
        run_id,
        span_id,
        event_id,
        schema,
        occurred_at,
        revision,
      } => {
        let mut record = self.logger.create_log_record();
        // NOTICE: OpenTelemetry 0.32 accepts only `&'static str` event names;
        // keep the owned schema name in `auv.event.schema.name` until that API
        // accepts owned names. See `opentelemetry-0.32.0/src/logs/record.rs`.
        record.set_event_name("auv.event");
        record.set_target(LOG_TARGET);
        record.set_timestamp(system_time(occurred_at)?);
        add_optional_authority(&mut record, authority_id);
        record.add_attribute("auv.run.id", run_id.to_string());
        add_optional_revision(&mut record, revision);
        if let Some(span_id) = span_id {
          record.add_attribute("auv.span.id", span_id.to_string());
          let context = self.active_context(authority_id, run_id, span_id, "auv.telemetry.otel_missing_event_span")?;
          let span_context = context.span().span_context().clone();
          record.set_trace_context(span_context.trace_id(), span_context.span_id(), Some(span_context.trace_flags()));
        }
        record.add_attribute("auv.event.id", event_id.to_string());
        record.add_attribute("auv.event.schema.name", schema.name().as_str().to_owned());
        record.add_attribute("auv.event.schema.version", i64::from(schema.version().get()));
        self.logger.emit(record);
        Ok(())
      }
      TelemetryItem::Artifact {
        authority_id,
        run_id,
        span_id,
        uri,
        purpose,
        content_type,
        byte_length,
        sha256,
        attributes,
        revision,
      } => {
        let mut record = self.logger.create_log_record();
        record.set_event_name("auv.artifact.published");
        record.set_target(LOG_TARGET);
        record.add_attribute("auv.authority.id", authority_id.to_string());
        record.add_attribute("auv.run.id", run_id.to_string());
        record.add_attribute("auv.run.revision", revision_i64(revision));
        if let Some(span_id) = span_id {
          record.add_attribute("auv.span.id", span_id.to_string());
        }
        record.add_attribute("auv.artifact.uri", uri.to_string());
        record.add_attribute("auv.artifact.purpose", purpose.as_str().to_owned());
        record.add_attribute("auv.artifact.content_type", content_type.to_string());
        record.add_attribute("auv.artifact.byte_length", byte_length.get() as i64);
        record.add_attribute("auv.artifact.sha256", sha256.to_string());
        add_log_attributes(&mut record, attributes);
        self.logger.emit(record);
        Ok(())
      }
    }
  }

  fn start_span(&self, input: SpanStartInput) -> Result<(), TelemetryError> {
    if input.parent_span_id.is_some() && input.remote_span_id.is_some() {
      return Err(error("auv.telemetry.otel_conflicting_span_relationship"));
    }
    let start_time = system_time(input.started_at)?;
    let mut active_spans = self.active_spans.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?;
    let key = (input.run_id, input.span_id);
    if active_spans.contains_key(&key) {
      return Err(error("auv.telemetry.otel_duplicate_span_start"));
    }

    let parent_context = match input.parent_span_id {
      Some(parent_span_id) => {
        let parent = active_spans.get(&(input.run_id, parent_span_id)).ok_or_else(|| error("auv.telemetry.otel_missing_parent_span"))?;
        if parent.authority_id != input.authority_id {
          return Err(error("auv.telemetry.otel_parent_authority_mismatch"));
        }
        parent.context.clone()
      }
      None => Context::new(),
    };

    let mut attributes = vec![
      KeyValue::new("auv.run.id", input.run_id.to_string()),
      KeyValue::new("auv.span.id", input.span_id.to_string()),
      KeyValue::new("auv.span.name", input.name.clone()),
    ];
    if let Some(authority_id) = input.authority_id {
      attributes.push(KeyValue::new("auv.authority.id", authority_id.to_string()));
    }
    if let Some(parent_span_id) = input.parent_span_id {
      attributes.push(KeyValue::new("auv.span.parent_id", parent_span_id.to_string()));
    }
    if let Some(remote_span_id) = input.remote_span_id {
      attributes.push(KeyValue::new("auv.span.remote_id", remote_span_id.to_string()));
    }
    if let Some(start_revision) = input.start_revision {
      attributes.push(KeyValue::new("auv.span.start_revision", revision_i64(start_revision)));
    }
    attributes.extend(span_attributes(input.attributes));

    let span = self
      .tracer
      .span_builder(input.name)
      .with_start_time(start_time)
      .with_attributes(attributes)
      .start_with_context(&self.tracer, &parent_context);
    if !span.span_context().is_valid() {
      return Err(error("auv.telemetry.otel_invalid_span_context"));
    }
    let context = Context::new().with_span(span);
    active_spans.insert(
      key,
      ActiveSpan {
        authority_id: input.authority_id,
        started_at: input.started_at,
        context,
      },
    );
    Ok(())
  }

  fn end_span(
    &self,
    authority_id: Option<AuthorityId>,
    run_id: RunId,
    span_id: SpanId,
    ended_at: Timestamp,
    end_revision: Option<RunRevision>,
  ) -> Result<(), TelemetryError> {
    let end_time = system_time(ended_at)?;
    let mut active_spans = self.active_spans.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?;
    let key = (run_id, span_id);
    let active = active_spans.get(&key).ok_or_else(|| error("auv.telemetry.otel_missing_span_start"))?;
    if active.authority_id != authority_id {
      return Err(error("auv.telemetry.otel_span_authority_mismatch"));
    }
    if ended_at < active.started_at {
      return Err(error("auv.telemetry.otel_span_end_before_start"));
    }
    let active = active_spans.remove(&key).ok_or_else(|| error("auv.telemetry.otel_missing_span_start"))?;
    drop(active_spans);
    if let Some(end_revision) = end_revision {
      active.context.span().set_attribute(KeyValue::new("auv.span.end_revision", revision_i64(end_revision)));
    }
    active.context.span().end_with_timestamp(end_time);
    Ok(())
  }

  fn active_context(
    &self,
    authority_id: Option<AuthorityId>,
    run_id: RunId,
    span_id: SpanId,
    missing_code: &'static str,
  ) -> Result<Context, TelemetryError> {
    let active_spans = self.active_spans.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?;
    let active = active_spans.get(&(run_id, span_id)).ok_or_else(|| error(missing_code))?;
    if active.authority_id != authority_id {
      return Err(error("auv.telemetry.otel_span_authority_mismatch"));
    }
    Ok(active.context.clone())
  }
}

struct SpanStartInput {
  authority_id: Option<AuthorityId>,
  run_id: RunId,
  span_id: SpanId,
  parent_span_id: Option<SpanId>,
  remote_span_id: Option<SpanId>,
  name: String,
  started_at: Timestamp,
  start_revision: Option<RunRevision>,
  attributes: Attributes,
}

fn span_attributes(attributes: Attributes) -> Vec<KeyValue> {
  attributes
    .iter()
    .filter(|(key, _)| !fixed_field(key.as_str()))
    .map(|(key, value)| KeyValue::new(Key::new(key.as_str().to_owned()), otel_value(value)))
    .collect()
}

fn add_log_attributes(record: &mut opentelemetry_sdk::logs::SdkLogRecord, attributes: Attributes) {
  for (key, value) in attributes.iter().filter(|(key, _)| !fixed_field(key.as_str())) {
    record.add_attribute(Key::new(key.as_str().to_owned()), log_value(value));
  }
}

fn otel_value(value: &AttributeValue) -> Value {
  match value {
    AttributeValue::Bool(value) => Value::Bool(*value),
    AttributeValue::I64(value) => Value::I64(*value),
    AttributeValue::F64(value) => Value::F64(value.get()),
    AttributeValue::String(value) => Value::String(value.as_str().to_owned().into()),
  }
}

fn log_value(value: &AttributeValue) -> AnyValue {
  match value {
    AttributeValue::Bool(value) => AnyValue::Boolean(*value),
    AttributeValue::I64(value) => AnyValue::Int(*value),
    AttributeValue::F64(value) => AnyValue::Double(value.get()),
    AttributeValue::String(value) => AnyValue::String(value.as_str().to_owned().into()),
  }
}

fn add_optional_authority(record: &mut opentelemetry_sdk::logs::SdkLogRecord, authority_id: Option<AuthorityId>) {
  if let Some(authority_id) = authority_id {
    record.add_attribute("auv.authority.id", authority_id.to_string());
  }
}

fn add_optional_revision(record: &mut opentelemetry_sdk::logs::SdkLogRecord, revision: Option<RunRevision>) {
  if let Some(revision) = revision {
    record.add_attribute("auv.run.revision", revision_i64(revision));
  }
}

fn revision_i64(revision: RunRevision) -> i64 {
  revision.get() as i64
}

fn system_time(timestamp: Timestamp) -> Result<SystemTime, TelemetryError> {
  let seconds = timestamp.unix_seconds();
  let nanoseconds = timestamp.nanoseconds();
  let result = if seconds >= 0 {
    UNIX_EPOCH.checked_add(Duration::new(seconds as u64, nanoseconds))
  } else if nanoseconds == 0 {
    UNIX_EPOCH.checked_sub(Duration::new(seconds.unsigned_abs(), 0))
  } else {
    UNIX_EPOCH.checked_sub(Duration::new(seconds.unsigned_abs() - 1, 1_000_000_000 - nanoseconds))
  };
  result.ok_or_else(|| error("auv.telemetry.otel_timestamp_out_of_range"))
}

fn fixed_field(key: &str) -> bool {
  matches!(
    key,
    "auv.authority.id"
      | "auv.run.id"
      | "auv.run.revision"
      | "auv.span.id"
      | "auv.span.name"
      | "auv.span.parent_id"
      | "auv.span.remote_id"
      | "auv.span.start_revision"
      | "auv.span.end_revision"
      | "auv.event.id"
      | "auv.event.schema.name"
      | "auv.event.schema.version"
      | "auv.artifact.uri"
      | "auv.artifact.purpose"
      | "auv.artifact.content_type"
      | "auv.artifact.byte_length"
      | "auv.artifact.sha256"
  )
}

fn error(code: &'static str) -> TelemetryError {
  TelemetryError::new(ErrorCode::parse(code).expect("static OTEL telemetry error code is valid"))
}
