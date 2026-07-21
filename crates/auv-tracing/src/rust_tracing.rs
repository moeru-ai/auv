use std::collections::BTreeMap;
use std::sync::Mutex;

use tracing::{Level, Span, field};

use crate::{AuthorityId, ErrorCode, RunId, SpanId, TelemetryError, TelemetryItem, TelemetryProjector, Timestamp};

const PROJECTION_TARGET: &str = "auv.telemetry.projection";

/// Projects bounded AUV telemetry into fixed Rust `tracing` callsites.
///
/// Applications register this projector with
/// [`crate::TelemetryRoutePolicy::fixed_fields_only`]. The projector exposes no
/// API that can expand the route policy and never ingests arbitrary tracing
/// spans or events.
pub struct RustTracingProjector {
  active_spans: Mutex<BTreeMap<(RunId, SpanId), ActiveSpan>>,
}

struct ActiveSpan {
  authority_id: Option<AuthorityId>,
  started_at: Timestamp,
  span: Span,
}

impl RustTracingProjector {
  /// Creates an empty fixed-vocabulary projector.
  pub fn new() -> Self {
    Self {
      active_spans: Mutex::new(BTreeMap::new()),
    }
  }

  fn project_now(&self, item: TelemetryItem) -> Result<(), TelemetryError> {
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
        attributes: _,
      } => {
        let mut active_spans = self.active_spans.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
        let key = (run_id, span_id);
        if active_spans.contains_key(&key) {
          return Err(error("auv.telemetry.duplicate_span_start"));
        }

        let parent = match parent_span_id {
          Some(parent_span_id) => {
            Some(active_spans.get(&(run_id, parent_span_id)).ok_or_else(|| error("auv.telemetry.missing_parent_span"))?.span.clone())
          }
          None => None,
        };
        let parent_id = parent.as_ref().and_then(Span::id);
        let authority = authority_id.as_ref().map(field::display);
        let parent_auv_id = parent_span_id.as_ref().map(field::display);
        let remote_auv_id = remote_span_id.as_ref().map(field::display);
        let start_revision = start_revision.map(crate::RunRevision::get);
        let span = tracing::span!(
          target: PROJECTION_TARGET,
          parent: parent_id,
          Level::INFO,
          "auv.span",
          "auv.authority.id" = authority,
          "auv.run.id" = %run_id,
          "auv.span.id" = %span_id,
          "auv.span.name" = %name,
          "auv.span.parent_id" = parent_auv_id,
          "auv.span.remote_id" = remote_auv_id,
          "auv.span.start_revision" = start_revision,
          "auv.span.end_revision" = field::Empty,
        );
        active_spans.insert(
          key,
          ActiveSpan {
            authority_id,
            started_at,
            span,
          },
        );
        Ok(())
      }
      TelemetryItem::SpanEnd {
        authority_id,
        run_id,
        span_id,
        ended_at,
        end_revision,
      } => {
        let mut active_spans = self.active_spans.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
        let key = (run_id, span_id);
        let active = active_spans.get(&key).ok_or_else(|| error("auv.telemetry.missing_span_start"))?;
        if active.authority_id != authority_id {
          return Err(error("auv.telemetry.span_authority_mismatch"));
        }
        if ended_at < active.started_at {
          return Err(error("auv.telemetry.span_end_before_start"));
        }
        let active = active_spans.remove(&key).ok_or_else(|| error("auv.telemetry.missing_span_start"))?;
        if let Some(end_revision) = end_revision {
          active.span.record("auv.span.end_revision", end_revision.get());
        }
        drop(active);
        Ok(())
      }
      TelemetryItem::Event {
        authority_id,
        run_id,
        span_id,
        event_id,
        schema,
        occurred_at: _,
        revision,
      } => {
        let authority = authority_id.as_ref().map(field::display);
        let span = span_id.as_ref().map(field::display);
        let revision = revision.map(crate::RunRevision::get);
        tracing::event!(
          name: "auv.event",
          target: PROJECTION_TARGET,
          Level::INFO,
          "auv.authority.id" = authority,
          "auv.run.id" = %run_id,
          "auv.run.revision" = revision,
          "auv.span.id" = span,
          "auv.event.id" = %event_id,
          "auv.event.schema.name" = %schema.name(),
          "auv.event.schema.version" = u64::from(schema.version().get()),
        );
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
        attributes: _,
        revision,
      } => {
        let span = span_id.as_ref().map(field::display);
        tracing::event!(
          name: "auv.artifact.published",
          target: PROJECTION_TARGET,
          Level::INFO,
          "auv.authority.id" = %authority_id,
          "auv.run.id" = %run_id,
          "auv.run.revision" = revision.get(),
          "auv.span.id" = span,
          "auv.artifact.uri" = %uri,
          "auv.artifact.purpose" = %purpose,
          "auv.artifact.content_type" = %content_type,
          "auv.artifact.byte_length" = byte_length.get(),
          "auv.artifact.sha256" = %sha256,
        );
        Ok(())
      }
    }
  }
}

impl Default for RustTracingProjector {
  fn default() -> Self {
    Self::new()
  }
}

impl TelemetryProjector for RustTracingProjector {
  fn project(&self, item: TelemetryItem) -> crate::BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async move { self.project_now(item) })
  }

  fn flush(&self) -> crate::BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async { Ok(()) })
  }
}

fn error(code: &'static str) -> TelemetryError {
  TelemetryError::new(ErrorCode::parse(code).expect("static telemetry error code is valid"))
}
