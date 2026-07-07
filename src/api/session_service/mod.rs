//! Session API service seam (API-P4 boundary).
//!
//! Owns the execute-facing `SessionService` surface separately from the
//! viewer-facing `inspect_server` and the tool-facing `mcp`.
//!
//! Modules:
//! - `registry`: lightweight in-memory session registry (API-P4 responsibility A).
//! - `mapper`: proto <-> host mapping, isolated from handler code (API-P4 checklist).
//! - `summary`: two-source `GetOperation` read path + join policy (API-P7/P12).
//! - `summary_store`: persisted `operation-summary` write path (API-P11).
//! - `operation_result_store`: persisted `operation-result` write path (API-R2).
//! - `handler`: transport-agnostic handler skeleton wiring proto RPCs to the
//!   internal seams (API-P8).
//! - `transport`: loopback-only tonic gRPC adapter (API-P9).
//! - `test_fixtures` (tests only): shared run/artifact staging helpers.
//!
//! TODO(api-p4-stream-events): `StreamSessionEvents` remains deferred to the
//! event projector (API-P4 responsibility D); the transport returns
//! `UNIMPLEMENTED` until that seam is wired.

pub mod handler;
pub mod mapper;
pub(crate) mod operation_result_store;
pub mod registry;
pub mod summary;
pub mod summary_store;
pub mod transport;

#[cfg(test)]
mod client_smoke;

#[cfg(test)]
pub(crate) mod test_fixtures;

use std::fmt;

/// Errors surfaced by the session API handler skeleton (API-P8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionApiError {
  /// A required proto field was absent.
  MissingField(&'static str),
  /// `Invoke` / `StreamSessionEvents` referenced a session that was never created.
  UnknownSession(String),
  /// `json_payload` could not be decoded into a host invoke request.
  PayloadDecode(String),
  /// Local store open or read-side storage I/O failed.
  Storage(String),
  /// Session-aware invoke execution failed after validation.
  InvokeExecution(String),
  /// `GetOperation` referenced a run that was never recorded in the store.
  RunNotFound(String),
  /// The run exists but recorded no persisted `OperationResult` artifact.
  PersistedOperationRequired(String),
  /// `GetOperation` request `operation_id` does not match the resolved wire id.
  OperationIdMismatch {
    run_id: String,
    requested: String,
    resolved: String,
  },
  /// A seam this RPC depends on is not wired in the current skeleton.
  NotWired { gate: &'static str },
}

impl fmt::Display for SessionApiError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::MissingField(field) => write!(f, "missing required field: {field}"),
      Self::UnknownSession(id) => write!(f, "unknown session: {id}"),
      Self::PayloadDecode(message) => write!(f, "failed to decode json_payload: {message}"),
      Self::Storage(message) => write!(f, "storage error: {message}"),
      Self::InvokeExecution(message) => write!(f, "invoke execution failed: {message}"),
      Self::RunNotFound(run_id) => write!(f, "run not found: {run_id}"),
      Self::PersistedOperationRequired(run_id) => {
        write!(f, "no persisted operation result for run: {run_id}")
      }
      Self::OperationIdMismatch {
        run_id,
        requested,
        resolved,
      } => write!(f, "operation_id mismatch for run {run_id}: requested {requested}, resolved {resolved}"),
      Self::NotWired { gate } => write!(f, "session API seam not wired: {gate}"),
    }
  }
}

impl std::error::Error for SessionApiError {}
