//! Session API service seam (API-P4 boundary).
//!
//! Owns the execute-facing `SessionService` surface separately from the
//! inspect viewer/server API and the tool-facing `mcp`.
//!
//! Modules:
//! - `registry`: lightweight in-memory session registry (API-P4 responsibility A).
//! - `mapper`: proto <-> host mapping, isolated from handler code (API-P4 checklist).
//! - `handler`: transport-agnostic handler skeleton wiring proto RPCs to the
//!   internal seams (API-P8).
//! - `transport`: loopback-only tonic gRPC adapter (API-P9).
//! - `test_fixtures` (tests only): shared run/artifact staging helpers.
//!
//! TODO(session-live-subscription): A session-scoped live stream is intentionally
//! absent until a consumer requires cursor and gap recovery semantics over
//! `auv_tracing::RunSubscription`; do not add a second generic event schema.

pub mod handler;
pub mod mapper;
pub mod registry;
pub mod transport;

#[cfg(test)]
pub(crate) mod test_fixtures;

use std::fmt;

/// Errors surfaced by the session API handler skeleton (API-P8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionApiError {
  /// A required proto field was absent.
  MissingField(&'static str),
  /// `Invoke` referenced a session that was never created.
  UnknownSession(String),
  /// `json_payload` could not be decoded into a host invoke request.
  PayloadDecode(String),
  /// Local store open or read-side storage I/O failed.
  Storage(String),
  /// Session-aware invoke execution failed after validation.
  InvokeExecution(String),
}

impl fmt::Display for SessionApiError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::MissingField(field) => write!(f, "missing required field: {field}"),
      Self::UnknownSession(id) => write!(f, "unknown session: {id}"),
      Self::PayloadDecode(message) => write!(f, "failed to decode json_payload: {message}"),
      Self::Storage(message) => write!(f, "storage error: {message}"),
      Self::InvokeExecution(message) => write!(f, "invoke execution failed: {message}"),
    }
  }
}

impl std::error::Error for SessionApiError {}
