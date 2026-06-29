//! Session API service seam (API-P4 boundary).
//!
//! Owns the execute-facing `SessionService` surface separately from the
//! viewer-facing `inspect_server` and the tool-facing `mcp`. This is NOT a
//! transport/gRPC server: API-P4 explicitly defers the tonic/axum/daemon choice
//! and this module impls no tonic service trait.
//!
//! Modules:
//! - `registry`: lightweight in-memory session registry (API-P4 responsibility A).
//! - `mapper`: proto <-> host mapping, isolated from handler code (API-P4 checklist).
//! - `summary`: two-source `GetOperation` read path + join policy (API-P7).
//! - `handler`: transport-agnostic handler skeleton wiring proto RPCs to the
//!   internal seams (API-P8).
//!
//! TODO(api-transport): binding these handlers to a real transport (a tonic
//! `SessionServiceServer` over a chosen async runtime) is an explicit API-P4
//! non-goal and a later owner-named slice. See
//! docs/ai/references/2026-06-30-auv-api-p4-session-proto-server-seam-design.md.

pub mod handler;
pub mod mapper;
pub mod registry;
pub mod summary;

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
  /// Underlying storage open or invoke execution failed (carries the host error).
  Execution(String),
  /// No operation summary was available for the requested run.
  OperationNotFound(String),
  /// A seam this RPC depends on is not wired in the current skeleton.
  NotWired { gate: &'static str },
}

impl fmt::Display for SessionApiError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::MissingField(field) => write!(f, "missing required field: {field}"),
      Self::UnknownSession(id) => write!(f, "unknown session: {id}"),
      Self::PayloadDecode(message) => write!(f, "failed to decode json_payload: {message}"),
      Self::Execution(message) => write!(f, "invoke execution failed: {message}"),
      Self::OperationNotFound(run_id) => write!(f, "no operation summary for run: {run_id}"),
      Self::NotWired { gate } => write!(f, "session API seam not wired: {gate}"),
    }
  }
}

impl std::error::Error for SessionApiError {}
