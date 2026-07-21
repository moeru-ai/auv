use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::sync::Mutex;

use tracing::span::Id;
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
  state: Mutex<ProjectorState>,
}

#[derive(Default)]
struct ProjectorState {
  // TODO(run-ended-v1): Reclaim retained run identities and span tombstones
  // when TelemetryItem gains a validated RunEnded signal.
  runs: BTreeMap<RunId, RunState>,
}

struct RunState {
  authority_id: Option<AuthorityId>,
  spans: BTreeMap<SpanId, SpanState>,
}

enum SpanState {
  Active(ActiveSpan),
  Ended,
}

struct ActiveSpan {
  authority_id: Option<AuthorityId>,
  started_at: Timestamp,
  parent_span_id: Option<SpanId>,
  active_children: usize,
  span: Span,
}

impl RustTracingProjector {
  /// Creates an empty fixed-vocabulary projector.
  pub fn new() -> Self {
    Self {
      state: Mutex::new(ProjectorState::default()),
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
        let mut state = self.state.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
        let run = match state.runs.entry(run_id) {
          Entry::Vacant(entry) => {
            if parent_span_id.is_some() {
              return Err(error("auv.telemetry.missing_parent_span"));
            }
            entry.insert(RunState {
              authority_id,
              spans: BTreeMap::new(),
            })
          }
          Entry::Occupied(entry) => entry.into_mut(),
        };
        if run.spans.contains_key(&span_id) {
          return Err(error("auv.telemetry.duplicate_span_start"));
        }

        let parent = match parent_span_id {
          Some(parent_span_id) => match run.spans.get(&parent_span_id) {
            Some(SpanState::Active(parent)) => {
              if parent.authority_id != authority_id {
                return Err(error("auv.telemetry.parent_authority_mismatch"));
              }
              Some(parent.span.clone())
            }
            Some(SpanState::Ended) => return Err(error("auv.telemetry.ended_parent_span")),
            None => return Err(error("auv.telemetry.missing_parent_span")),
          },
          None => None,
        };
        if run.authority_id != authority_id {
          return Err(error("auv.telemetry.run_authority_mismatch"));
        }
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
        if let Some(parent_span_id) = parent_span_id {
          match run.spans.get_mut(&parent_span_id) {
            Some(SpanState::Active(parent)) => parent.active_children += 1,
            Some(SpanState::Ended) => return Err(error("auv.telemetry.ended_parent_span")),
            None => return Err(error("auv.telemetry.missing_parent_span")),
          }
        }
        run.spans.insert(
          span_id,
          SpanState::Active(ActiveSpan {
            authority_id,
            started_at,
            parent_span_id,
            active_children: 0,
            span,
          }),
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
        let mut state = self.state.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
        let run = state.runs.get_mut(&run_id).ok_or_else(|| error("auv.telemetry.missing_span_start"))?;
        let active = match run.spans.get(&span_id) {
          Some(SpanState::Active(active)) => active,
          Some(SpanState::Ended) => return Err(error("auv.telemetry.duplicate_span_end")),
          None => return Err(error("auv.telemetry.missing_span_start")),
        };
        if active.authority_id != authority_id {
          return Err(error("auv.telemetry.span_authority_mismatch"));
        }
        if run.authority_id != authority_id {
          return Err(error("auv.telemetry.run_authority_mismatch"));
        }
        if ended_at < active.started_at {
          return Err(error("auv.telemetry.span_end_before_start"));
        }
        if active.active_children != 0 {
          return Err(error("auv.telemetry.span_has_active_children"));
        }
        let parent_span_id = active.parent_span_id;
        if let Some(parent_span_id) = parent_span_id {
          match run.spans.get_mut(&parent_span_id) {
            Some(SpanState::Active(parent)) => {
              parent.active_children =
                parent.active_children.checked_sub(1).ok_or_else(|| error("auv.telemetry.invalid_parent_child_count"))?;
            }
            Some(SpanState::Ended) => return Err(error("auv.telemetry.ended_parent_span")),
            None => return Err(error("auv.telemetry.missing_parent_span")),
          }
        }
        let previous = run.spans.insert(span_id, SpanState::Ended).ok_or_else(|| error("auv.telemetry.missing_span_start"))?;
        let SpanState::Active(active) = previous else {
          return Err(error("auv.telemetry.duplicate_span_end"));
        };
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
        let parent_id = {
          let mut state = self.state.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
          match span_id {
            Some(span_id) => {
              let run = state.runs.get(&run_id).ok_or_else(|| error("auv.telemetry.missing_event_span"))?;
              let active = match run.spans.get(&span_id) {
                Some(SpanState::Active(active)) => active,
                Some(SpanState::Ended) => return Err(error("auv.telemetry.ended_event_span")),
                None => return Err(error("auv.telemetry.missing_event_span")),
              };
              if active.authority_id != authority_id {
                return Err(error("auv.telemetry.span_authority_mismatch"));
              }
              if run.authority_id != authority_id {
                return Err(error("auv.telemetry.run_authority_mismatch"));
              }
              active.span.id()
            }
            None => {
              ensure_run_authority(&mut state, run_id, authority_id)?;
              None
            }
          }
        };
        let authority = authority_id.as_ref().map(field::display);
        let span = span_id.as_ref().map(field::display);
        let revision = revision.map(crate::RunRevision::get);
        tracing::event!(
          name: "auv.event",
          target: PROJECTION_TARGET,
          parent: parent_id,
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
        {
          let mut state = self.state.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
          ensure_run_authority(&mut state, run_id, Some(authority_id))?;
        }
        let span = span_id.as_ref().map(field::display);
        tracing::event!(
          name: "auv.artifact.published",
          target: PROJECTION_TARGET,
          parent: Option::<Id>::None,
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

fn ensure_run_authority(state: &mut ProjectorState, run_id: RunId, authority_id: Option<AuthorityId>) -> Result<(), TelemetryError> {
  match state.runs.entry(run_id) {
    Entry::Vacant(entry) => {
      entry.insert(RunState {
        authority_id,
        spans: BTreeMap::new(),
      });
      Ok(())
    }
    Entry::Occupied(entry) if entry.get().authority_id == authority_id => Ok(()),
    Entry::Occupied(_) => Err(error("auv.telemetry.run_authority_mismatch")),
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
