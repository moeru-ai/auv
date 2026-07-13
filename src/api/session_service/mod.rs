//! Execute-facing session API boundary.
//!
//! Owns the execute-facing `SessionService` surface separately from the
//! inspect viewer/server API and the tool-facing `mcp`.
//!
//! Modules:
//! - `handler`: protobuf-aware application orchestration.
//! - `grpc`: tonic request adaptation and cancellation.
//! - `server`: loopback listen policy and server lifecycle.
//! - `durability`: post-invoke write order and partial-success policy.
//! - `registry`: lightweight in-memory session registry.
//! - `mapper`: protobuf and host-model mapping.
//! - `summary`: two-source `GetOperation` read path and join policy.
//! - `test_fixtures` (tests only): shared run/artifact staging helpers.
//!
//! TODO: `StreamSessionEvents` remains deferred because no event projector
//! exists. Implement it only after an application event source is approved;
//! until then the gRPC adapter returns `UNIMPLEMENTED`.

pub(crate) mod durability;
pub(crate) mod grpc;
pub(crate) mod handler;
pub(crate) mod mapper;
pub(crate) mod registry;
pub mod server;
pub(crate) mod summary;

#[cfg(test)]
pub(crate) mod test_fixtures;

use std::fmt;

/// Errors surfaced by session application orchestration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SessionApiError {
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
    }
  }
}

impl std::error::Error for SessionApiError {}
