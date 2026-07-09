//! Viewer-facing HTTP/WebSocket inspection server for recorded AUV runs.
//!
//! This crate serves run storage and artifact inspection APIs. It does not
//! execute commands, drive applications, or own runtime semantics.

pub mod read_projection;

pub use read_projection::{DefaultInspectReadProjection, InspectReadProjection, InspectRunEnrichment};

// TODO(inspect-server-task-2): server module and exports are deferred until the
// owner-approved server move lands; Task 1 only establishes the crate shell and
// read projection boundary.
// TODO(inspect-server-task-3): session module and exports are deferred until
// the existing inspect-session code is moved into this crate.

pub type InspectResult<T> = Result<T, String>;
