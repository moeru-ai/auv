//! Viewer-facing HTTP/WebSocket inspection server for recorded AUV runs.
//!
//! This crate serves run storage and artifact inspection APIs. It does not
//! execute commands, drive applications, or own runtime semantics.

pub mod read_projection;
pub mod session;

pub use read_projection::{DefaultInspectReadProjection, InspectReadProjection, InspectRunEnrichment};
pub use session::{InspectServerSession, default_session_path, read_inspect_session, write_inspect_session};

// TODO(inspect-server-task-3): server module and exports are deferred until the
// owner-approved server move lands.

pub type InspectResult<T> = Result<T, String>;
