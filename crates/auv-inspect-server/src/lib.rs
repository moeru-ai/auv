//! Viewer-facing HTTP authority for canonical AUV run data.
//!
//! This crate composes an existing [`auv_tracing::RunStore`]. It does not
//! execute operations, own application control flow, or maintain a second run
//! recording model.

pub mod legacy;
pub mod read_projection;
pub mod session;

mod run_api;
mod server;
mod viewer_assets;

pub use read_projection::{InspectRunExtension, project_snapshot};
pub use server::{
  DEFAULT_INSPECT_HOST, DEFAULT_INSPECT_PORT, InspectServeConfig, router, router_with_artifact_origin, router_with_extension, serve,
};
pub use session::{InspectServerSession, default_session_path, read_inspect_session, write_inspect_session};

pub type InspectResult<T> = Result<T, String>;
