#![forbid(unsafe_code)]

//! Bounded OpenTelemetry projection for AUV run telemetry.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};
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
  state: Mutex<ProjectorState>,
}

#[derive(Default)]
struct ProjectorState {
  in_flight: Option<ThreadId>,
  // TODO(run-ended-v1): Reclaim retained run identities and span tombstones
  // when TelemetryItem gains a validated RunEnded signal.
  runs: BTreeMap<RunId, RunState>,
}

struct RunState {
  authority_id: Option<AuthorityId>,
  spans: BTreeMap<SpanId, SpanState>,
}

enum SpanState {
  // A start callback may publish before panicking, so its reserved identity is
  // retained as a tombstone instead of being made available for reuse.
  Starting,
  Active(ActiveSpan),
  Ended,
}

struct ActiveSpan {
  authority_id: Option<AuthorityId>,
  started_at: Timestamp,
  latest_event_at: Option<Timestamp>,
  latest_child_started_at: Option<Timestamp>,
  context: Context,
}

struct ProjectionReservation<'a> {
  projector: &'a OtelProjectorInner,
  owner: ThreadId,
  active: bool,
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
        state: Mutex::new(ProjectorState::default()),
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
  fn reserve(&self) -> Result<ProjectionReservation<'_>, TelemetryError> {
    let owner = thread::current().id();
    let mut state = self.state.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?;
    match state.in_flight.as_ref() {
      None => {
        state.in_flight = Some(owner);
        Ok(ProjectionReservation {
          projector: self,
          owner,
          active: true,
        })
      }
      Some(active_owner) if *active_owner == owner => Err(error("auv.telemetry.otel_reentrant_projection")),
      Some(_) => Err(error("auv.telemetry.otel_concurrent_projection")),
    }
  }

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
      } => match span_id {
        Some(span_id) => {
          let timestamp = system_time(occurred_at)?;
          let attributes = event_attributes(authority_id, run_id, Some(span_id), event_id, &schema, revision);
          let reservation = self.reserve()?;
          let context = {
            let mut state = self.state.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?;
            let run = state.runs.get_mut(&run_id).ok_or_else(|| error("auv.telemetry.otel_missing_event_span"))?;
            let active = match run.spans.get(&span_id) {
              Some(SpanState::Active(active)) => active,
              Some(SpanState::Starting) | None => return Err(error("auv.telemetry.otel_missing_event_span")),
              Some(SpanState::Ended) => return Err(error("auv.telemetry.otel_ended_event_span")),
            };
            if active.authority_id != authority_id {
              return Err(error("auv.telemetry.otel_span_authority_mismatch"));
            }
            if run.authority_id != authority_id {
              return Err(error("auv.telemetry.otel_run_authority_mismatch"));
            }
            if occurred_at < active.started_at {
              return Err(error("auv.telemetry.otel_event_before_span_start"));
            }
            let Some(SpanState::Active(active)) = run.spans.get_mut(&span_id) else {
              return Err(error("auv.telemetry.otel_missing_event_span"));
            };
            active.latest_event_at = Some(active.latest_event_at.map_or(occurred_at, |current| current.max(occurred_at)));
            active.context.clone()
          };
          context.span().add_event_with_timestamp(schema.name().as_str().to_owned(), timestamp, attributes);
          reservation.finish()
        }
        None => {
          let timestamp = system_time(occurred_at)?;
          let reservation = self.reserve()?;
          let mut record = self.logger.create_log_record();
          // NOTICE: OpenTelemetry 0.32 accepts only `&'static str` LogRecord
          // event names. Keep the exact bounded schema in
          // `auv.event.schema.name` until the API accepts owned names; leaking
          // or interning producer strings would create unbounded process state.
          // See `opentelemetry-0.32.0/src/logs/record.rs`.
          record.set_event_name("auv.event");
          record.set_target(LOG_TARGET);
          record.set_timestamp(timestamp);
          add_optional_authority(&mut record, authority_id);
          record.add_attribute("auv.run.id", run_id.to_string());
          add_optional_revision(&mut record, revision);
          record.add_attribute("auv.event.id", event_id.to_string());
          record.add_attribute("auv.event.schema.name", schema.name().as_str().to_owned());
          record.add_attribute("auv.event.schema.version", i64::from(schema.version().get()));
          {
            let mut state = self.state.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?;
            commit_run_authority(&mut state, run_id, authority_id)?;
          }
          self.logger.emit(record);
          reservation.finish()
        }
      },
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
        let reservation = self.reserve()?;
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
        {
          let mut state = self.state.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?;
          commit_run_authority(&mut state, run_id, Some(authority_id))?;
        }
        self.logger.emit(record);
        reservation.finish()
      }
    }
  }

  fn start_span(&self, input: SpanStartInput) -> Result<(), TelemetryError> {
    if input.parent_span_id.is_some() && input.remote_span_id.is_some() {
      return Err(error("auv.telemetry.otel_conflicting_span_relationship"));
    }
    let start_time = system_time(input.started_at)?;
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

    let reservation = self.reserve()?;
    let (parent_context, run_was_new, previous_parent_latest_child_started_at) = {
      let mut state = self.state.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?;
      let run_was_new = !state.runs.contains_key(&input.run_id);
      match state.runs.get(&input.run_id) {
        None => {
          if input.parent_span_id.is_some() {
            return Err(error("auv.telemetry.otel_missing_parent_span"));
          }
        }
        Some(run) => {
          if run.spans.contains_key(&input.span_id) {
            return Err(error("auv.telemetry.otel_duplicate_span_start"));
          }
          if let Some(parent_span_id) = input.parent_span_id {
            match run.spans.get(&parent_span_id) {
              Some(SpanState::Active(parent)) => {
                if parent.authority_id != input.authority_id {
                  return Err(error("auv.telemetry.otel_parent_authority_mismatch"));
                }
                if input.started_at < parent.started_at {
                  return Err(error("auv.telemetry.otel_child_before_parent"));
                }
              }
              Some(SpanState::Starting) | None => return Err(error("auv.telemetry.otel_missing_parent_span")),
              Some(SpanState::Ended) => return Err(error("auv.telemetry.otel_ended_parent_span")),
            }
          }
          if run.authority_id != input.authority_id {
            return Err(error("auv.telemetry.otel_run_authority_mismatch"));
          }
        }
      }

      let parent_context = input.parent_span_id.and_then(|parent_span_id| match state.runs.get(&input.run_id) {
        Some(run) => match run.spans.get(&parent_span_id) {
          Some(SpanState::Active(parent)) => Some(parent.context.clone()),
          _ => None,
        },
        None => None,
      });
      commit_run_authority(&mut state, input.run_id, input.authority_id)?;
      let run = state.runs.get_mut(&input.run_id).expect("committed run authority creates run state");
      let previous_parent_latest_child_started_at = if let Some(parent_span_id) = input.parent_span_id {
        let Some(SpanState::Active(parent)) = run.spans.get_mut(&parent_span_id) else {
          return Err(error("auv.telemetry.otel_missing_parent_span"));
        };
        let previous = parent.latest_child_started_at;
        parent.latest_child_started_at =
          Some(parent.latest_child_started_at.map_or(input.started_at, |current| current.max(input.started_at)));
        previous
      } else {
        None
      };
      match run.spans.entry(input.span_id) {
        Entry::Vacant(entry) => {
          entry.insert(SpanState::Starting);
        }
        Entry::Occupied(_) => return Err(error("auv.telemetry.otel_duplicate_span_start")),
      }
      (parent_context.unwrap_or_default(), run_was_new, previous_parent_latest_child_started_at)
    };

    let span = self
      .tracer
      .span_builder(input.name)
      .with_start_time(start_time)
      .with_attributes(attributes)
      .start_with_context(&self.tracer, &parent_context);
    if !span.span_context().is_valid() {
      // NOTICE: OpenTelemetry SDK 0.32 returns an invalid span before invoking
      // `SpanProcessor::on_start` when its provider is shut down. Revisit this
      // rollback against `opentelemetry_sdk/src/trace/tracer.rs` on SDK upgrade.
      drop(span);
      let mut state = self.state.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?;
      let remove_run = {
        let run = state.runs.get_mut(&input.run_id).expect("reserved span start retains run state");
        match run.spans.remove(&input.span_id) {
          Some(SpanState::Starting) => {}
          _ => return Err(error("auv.telemetry.otel_duplicate_span_start")),
        }
        if let Some(parent_span_id) = input.parent_span_id {
          let Some(SpanState::Active(parent)) = run.spans.get_mut(&parent_span_id) else {
            return Err(error("auv.telemetry.otel_missing_parent_span"));
          };
          parent.latest_child_started_at = previous_parent_latest_child_started_at;
        }
        run_was_new && run.spans.is_empty()
      };
      if remove_run {
        state.runs.remove(&input.run_id);
      }
      return Err(error("auv.telemetry.otel_invalid_span_context"));
    }
    let context = Context::new().with_span(span);
    {
      let mut state = self.state.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?;
      let run = state.runs.get_mut(&input.run_id).expect("reserved span start retains run state");
      let active = SpanState::Active(ActiveSpan {
        authority_id: input.authority_id,
        started_at: input.started_at,
        latest_event_at: None,
        latest_child_started_at: None,
        context,
      });
      match run.spans.insert(input.span_id, active) {
        Some(SpanState::Starting) => {}
        _ => return Err(error("auv.telemetry.otel_duplicate_span_start")),
      }
    }
    reservation.finish()
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
    let reservation = self.reserve()?;
    let active = {
      let mut state = self.state.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?;
      let run = state.runs.get_mut(&run_id).ok_or_else(|| error("auv.telemetry.otel_missing_span_start"))?;
      let active = match run.spans.get(&span_id) {
        Some(SpanState::Active(active)) => active,
        Some(SpanState::Starting) | None => return Err(error("auv.telemetry.otel_missing_span_start")),
        Some(SpanState::Ended) => return Err(error("auv.telemetry.otel_duplicate_span_end")),
      };
      if active.authority_id != authority_id {
        return Err(error("auv.telemetry.otel_span_authority_mismatch"));
      }
      if run.authority_id != authority_id {
        return Err(error("auv.telemetry.otel_run_authority_mismatch"));
      }
      if ended_at < active.started_at {
        return Err(error("auv.telemetry.otel_span_end_before_start"));
      }
      if active.latest_event_at.is_some_and(|occurred_at| ended_at < occurred_at) {
        return Err(error("auv.telemetry.otel_span_end_before_event"));
      }
      if active.latest_child_started_at.is_some_and(|started_at| ended_at < started_at) {
        return Err(error("auv.telemetry.otel_span_end_before_child_start"));
      }
      let previous = run.spans.insert(span_id, SpanState::Ended).ok_or_else(|| error("auv.telemetry.otel_missing_span_start"))?;
      let SpanState::Active(active) = previous else {
        return Err(error("auv.telemetry.otel_duplicate_span_end"));
      };
      active
    };
    if let Some(end_revision) = end_revision {
      active.context.span().set_attribute(KeyValue::new("auv.span.end_revision", revision_i64(end_revision)));
    }
    active.context.span().end_with_timestamp(end_time);
    drop(active);
    reservation.finish()
  }
}

impl ProjectionReservation<'_> {
  fn finish(mut self) -> Result<(), TelemetryError> {
    let mut state = self.projector.state.lock().map_err(|_| error("auv.telemetry.otel_state_poisoned"))?;
    state.in_flight = None;
    self.active = false;
    Ok(())
  }
}

impl Drop for ProjectionReservation<'_> {
  fn drop(&mut self) {
    if !self.active {
      return;
    }
    let mut state = match self.projector.state.lock() {
      Ok(state) => state,
      Err(poisoned) => poisoned.into_inner(),
    };
    if state.in_flight.as_ref() == Some(&self.owner) {
      state.in_flight = None;
    }
  }
}

fn commit_run_authority(state: &mut ProjectorState, run_id: RunId, authority_id: Option<AuthorityId>) -> Result<(), TelemetryError> {
  match state.runs.entry(run_id) {
    Entry::Vacant(entry) => {
      entry.insert(RunState {
        authority_id,
        spans: BTreeMap::new(),
      });
      Ok(())
    }
    Entry::Occupied(entry) if entry.get().authority_id == authority_id => Ok(()),
    Entry::Occupied(_) => Err(error("auv.telemetry.otel_run_authority_mismatch")),
  }
}

fn event_attributes(
  authority_id: Option<AuthorityId>,
  run_id: RunId,
  span_id: Option<SpanId>,
  event_id: auv_tracing::EventId,
  schema: &auv_tracing::EventSchema,
  revision: Option<RunRevision>,
) -> Vec<KeyValue> {
  let mut attributes = vec![
    KeyValue::new("auv.run.id", run_id.to_string()),
    KeyValue::new("auv.event.id", event_id.to_string()),
    KeyValue::new("auv.event.schema.name", schema.name().as_str().to_owned()),
    KeyValue::new("auv.event.schema.version", i64::from(schema.version().get())),
  ];
  if let Some(authority_id) = authority_id {
    attributes.push(KeyValue::new("auv.authority.id", authority_id.to_string()));
  }
  if let Some(revision) = revision {
    attributes.push(KeyValue::new("auv.run.revision", revision_i64(revision)));
  }
  if let Some(span_id) = span_id {
    attributes.push(KeyValue::new("auv.span.id", span_id.to_string()));
  }
  attributes
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
