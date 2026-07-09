//! Viewer-facing HTTP/WebSocket inspection server for recorded AUV runs.
//!
//! This crate serves run storage and artifact inspection APIs. It does not
//! execute commands, drive applications, or own runtime semantics.

pub mod read_projection;
pub mod session;

mod server;

pub use read_projection::{CommandBoundaryClaim, DefaultInspectReadProjection, InspectReadProjection, InspectRunEnrichment};
pub use server::{
  DEFAULT_INSPECT_HOST, DEFAULT_INSPECT_PORT, InspectServeConfig, InspectWriteConfig, router, router_with_projection, serve,
};
pub use session::{InspectServerSession, default_session_path, read_inspect_session, write_inspect_session};

pub type InspectResult<T> = Result<T, String>;
