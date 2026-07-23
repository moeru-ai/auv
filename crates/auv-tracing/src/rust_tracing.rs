use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};

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
  Ended { retain_on_flush: bool },
}

struct ActiveSpan {
  authority_id: Option<AuthorityId>,
  started_at: Timestamp,
  latest_event_at: Option<Timestamp>,
  latest_child_started_at: Option<Timestamp>,
  tracing_id: Option<Id>,
  span: Arc<Span>,
}

struct ProjectionReservation<'a> {
  projector: &'a RustTracingProjector,
  owner: ThreadId,
  active: bool,
}

impl RustTracingProjector {
  /// Creates an empty fixed-vocabulary projector.
  pub fn new() -> Self {
    Self {
      state: Mutex::new(ProjectorState::default()),
    }
  }

  fn reserve(&self) -> Result<ProjectionReservation<'_>, TelemetryError> {
    let owner = thread::current().id();
    let mut state = self.state.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
    match state.in_flight.as_ref() {
      None => {
        state.in_flight = Some(owner);
        Ok(ProjectionReservation {
          projector: self,
          owner,
          active: true,
        })
      }
      Some(active_owner) if *active_owner == owner => Err(error("auv.telemetry.rust_tracing_reentrant_projection")),
      Some(_) => Err(error("auv.telemetry.rust_tracing_concurrent_projection")),
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
        if parent_span_id.is_some() && remote_span_id.is_some() {
          return Err(error("auv.telemetry.conflicting_span_relationship"));
        }
        let reservation = self.reserve()?;
        let parent_id = {
          let mut state = self.state.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
          match state.runs.get(&run_id) {
            None => {
              if parent_span_id.is_some() {
                return Err(error("auv.telemetry.missing_parent_span"));
              }
            }
            Some(run) => {
              if run.spans.contains_key(&span_id) {
                return Err(error("auv.telemetry.duplicate_span_start"));
              }
              if let Some(parent_span_id) = parent_span_id {
                match run.spans.get(&parent_span_id) {
                  Some(SpanState::Active(parent)) => {
                    if parent.authority_id != authority_id {
                      return Err(error("auv.telemetry.parent_authority_mismatch"));
                    }
                    if started_at < parent.started_at {
                      return Err(error("auv.telemetry.child_before_parent"));
                    }
                  }
                  Some(SpanState::Starting) | None => return Err(error("auv.telemetry.missing_parent_span")),
                  Some(SpanState::Ended { .. }) => return Err(error("auv.telemetry.ended_parent_span")),
                }
              }
              if run.authority_id != authority_id {
                return Err(error("auv.telemetry.run_authority_mismatch"));
              }
            }
          }

          let parent_id = parent_span_id.and_then(|parent_span_id| match state.runs.get(&run_id) {
            Some(run) => match run.spans.get(&parent_span_id) {
              Some(SpanState::Active(parent)) => parent.tracing_id.clone(),
              _ => None,
            },
            None => None,
          });
          commit_run_authority(&mut state, run_id, authority_id)?;
          let run = state.runs.get_mut(&run_id).expect("committed run authority creates run state");
          if let Some(parent_span_id) = parent_span_id {
            let Some(SpanState::Active(parent)) = run.spans.get_mut(&parent_span_id) else {
              return Err(error("auv.telemetry.missing_parent_span"));
            };
            parent.latest_child_started_at = Some(parent.latest_child_started_at.map_or(started_at, |current| current.max(started_at)));
          }
          match run.spans.entry(span_id) {
            Entry::Vacant(entry) => {
              entry.insert(SpanState::Starting);
            }
            Entry::Occupied(_) => return Err(error("auv.telemetry.duplicate_span_start")),
          }
          parent_id
        };
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
        let tracing_id = span.id();
        let active = ActiveSpan {
          authority_id,
          started_at,
          latest_event_at: None,
          latest_child_started_at: None,
          tracing_id,
          span: Arc::new(span),
        };
        {
          let mut state = self.state.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
          let run = state.runs.get_mut(&run_id).expect("reserved span start retains run state");
          match run.spans.insert(span_id, SpanState::Active(active)) {
            Some(SpanState::Starting) => {}
            _ => return Err(error("auv.telemetry.duplicate_span_start")),
          }
        }
        reservation.finish()
      }
      TelemetryItem::SpanEnd {
        authority_id,
        run_id,
        span_id,
        ended_at,
        end_revision,
      } => {
        let reservation = self.reserve()?;
        let active = {
          let mut state = self.state.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
          let run = state.runs.get_mut(&run_id).ok_or_else(|| error("auv.telemetry.missing_span_start"))?;
          let active = match run.spans.get(&span_id) {
            Some(SpanState::Active(active)) => active,
            Some(SpanState::Starting) | None => return Err(error("auv.telemetry.missing_span_start")),
            Some(SpanState::Ended { .. }) => return Err(error("auv.telemetry.duplicate_span_end")),
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
          if active.latest_event_at.is_some_and(|occurred_at| ended_at < occurred_at) {
            return Err(error("auv.telemetry.span_end_before_event"));
          }
          if active.latest_child_started_at.is_some_and(|started_at| ended_at < started_at) {
            return Err(error("auv.telemetry.span_end_before_child_start"));
          }
          let previous = run
            .spans
            .insert(
              span_id,
              SpanState::Ended {
                retain_on_flush: true,
              },
            )
            .ok_or_else(|| error("auv.telemetry.missing_span_start"))?;
          let SpanState::Active(active) = previous else {
            return Err(error("auv.telemetry.duplicate_span_end"));
          };
          active
        };
        let record_panic = end_revision
          .and_then(|end_revision| catch_unwind(AssertUnwindSafe(|| active.span.record("auv.span.end_revision", end_revision.get()))).err());
        let close_panic = catch_unwind(AssertUnwindSafe(|| drop(active))).err();
        if let Some(payload) = record_panic.or(close_panic) {
          resume_unwind(payload);
        }
        {
          let mut state = self.state.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
          mark_ended_prunable(&mut state, run_id, span_id)?;
        }
        reservation.finish()
      }
      TelemetryItem::Event {
        authority_id,
        run_id,
        span_id,
        event_id,
        schema,
        occurred_at,
        revision,
      } => {
        let reservation = self.reserve()?;
        let authority = authority_id.as_ref().map(field::display);
        let span = span_id.as_ref().map(field::display);
        let revision = revision.map(crate::RunRevision::get);
        let emit = |parent_id| {
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
        };
        match span_id {
          Some(span_id) => {
            let parent_id = {
              let mut state = self.state.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
              let run = state.runs.get_mut(&run_id).ok_or_else(|| error("auv.telemetry.missing_event_span"))?;
              let active = match run.spans.get(&span_id) {
                Some(SpanState::Active(active)) => active,
                Some(SpanState::Starting) | None => return Err(error("auv.telemetry.missing_event_span")),
                Some(SpanState::Ended { .. }) => return Err(error("auv.telemetry.ended_event_span")),
              };
              if active.authority_id != authority_id {
                return Err(error("auv.telemetry.span_authority_mismatch"));
              }
              if run.authority_id != authority_id {
                return Err(error("auv.telemetry.run_authority_mismatch"));
              }
              if occurred_at < active.started_at {
                return Err(error("auv.telemetry.event_before_span_start"));
              }
              let Some(SpanState::Active(active)) = run.spans.get_mut(&span_id) else {
                return Err(error("auv.telemetry.missing_event_span"));
              };
              active.latest_event_at = Some(active.latest_event_at.map_or(occurred_at, |current| current.max(occurred_at)));
              active.tracing_id.clone()
            };
            emit(parent_id);
          }
          None => {
            {
              let mut state = self.state.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
              commit_run_authority(&mut state, run_id, authority_id)?;
            }
            emit(None);
          }
        }
        reservation.finish()
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
        let reservation = self.reserve()?;
        {
          let mut state = self.state.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
          commit_run_authority(&mut state, run_id, Some(authority_id))?;
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
        reservation.finish()
      }
    }
  }
}

impl ProjectionReservation<'_> {
  fn finish(mut self) -> Result<(), TelemetryError> {
    let mut state = self.projector.state.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
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
    Box::pin(async move {
      let reservation = self.reserve()?;
      {
        let mut state = self.state.lock().map_err(|_| error("auv.telemetry.rust_tracing_state_poisoned"))?;
        prune_ordinary_ended_spans(&mut state);
      }
      reservation.finish()
    })
  }
}

fn mark_ended_prunable(state: &mut ProjectorState, run_id: RunId, span_id: SpanId) -> Result<(), TelemetryError> {
  let run = state.runs.get_mut(&run_id).ok_or_else(|| error("auv.telemetry.missing_span_start"))?;
  let Some(SpanState::Ended { retain_on_flush }) = run.spans.get_mut(&span_id) else {
    return Err(error("auv.telemetry.missing_span_start"));
  };
  *retain_on_flush = false;
  Ok(())
}

fn prune_ordinary_ended_spans(state: &mut ProjectorState) {
  for run in state.runs.values_mut() {
    run.spans.retain(|_, span| {
      !matches!(
        span,
        SpanState::Ended {
          retain_on_flush: false
        }
      )
    });
  }
}

fn error(code: &'static str) -> TelemetryError {
  TelemetryError::new(ErrorCode::parse(code).expect("static telemetry error code is valid"))
}
