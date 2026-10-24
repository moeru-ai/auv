//! Trace correlation that the daemon forwards with routed Runner calls.
//!
//! A Runner records a call only when it carries `auv-trace-*` fields, and the
//! daemon decides which calls do:
//!
//! - A call routed to a Run carries that Run's trace id. A client may add its
//!   own span (the CLI sends its command span) but may not name another Run,
//!   since the Runner would write records into a Run the caller was not
//!   admitted to. A client that sent nothing gets the Run's id.
//! - A call without a Run forwards no trace fields, so the Runner records
//!   nothing: records belong to a Run.
//!
//! See `docs/ai/references/inspect/2026-10-21-trace-and-log-responsibility-split.md`.

use axum::http::{HeaderMap, HeaderValue};
use tonic::Status;

/// Rewrites `headers` so they carry exactly the trace correlation the routed
/// Run allows.
pub(crate) fn forward(headers: &mut HeaderMap, routed_run_id: Option<&str>) -> Result<(), Status> {
  let Some(routed_run_id) = routed_run_id else {
    strip(headers);
    return Ok(());
  };
  let Ok(run_id) = auv_tracing::RunId::from_daemon_run_id(routed_run_id) else {
    // NOTICE(trace-run-id-projection): Run ids are UUIDs, the same projection
    // `auv invoke` uses for its tracing Run. Any other id has no trace
    // identity, so its calls go unrecorded rather than failing.
    tracing::warn!(run_id = routed_run_id, "routed Run id has no trace identity; Runner calls are not recorded");
    strip(headers);
    return Ok(());
  };
  let supplied = auv_tracing::extract(&Headers(headers))
    .map_err(|error| Status::invalid_argument(format!("invalid trace context metadata: {}", error.code())))?;
  match supplied {
    Some(context) if context.run_id() != run_id => {
      Err(Status::invalid_argument("trace context names a different Run than the routed auv-run-id"))
    }
    Some(_) => Ok(()),
    None => {
      auv_tracing::RemoteContext::new(run_id, None).inject(&mut Headers(headers));
      Ok(())
    }
  }
}

fn strip(headers: &mut HeaderMap) {
  for name in auv_tracing::propagation::FIELD_NAMES {
    headers.remove(name);
  }
}

struct Headers<'a>(&'a mut HeaderMap);

impl auv_tracing::TextMapReader for Headers<'_> {
  fn values<'a>(&'a self, name: &str) -> Box<dyn Iterator<Item = &'a str> + 'a> {
    // A non-ASCII value reads as empty, which fails id validation.
    Box::new(self.0.get_all(name).iter().map(|value| value.to_str().unwrap_or_default()))
  }
}

impl auv_tracing::TextMapWriter for Headers<'_> {
  fn set(&mut self, name: &'static str, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
      self.0.insert(name, value);
    }
  }

  fn remove(&mut self, name: &'static str) {
    self.0.remove(name);
  }
}

#[cfg(test)]
#[path = "trace_context_test.rs"]
mod tests;
