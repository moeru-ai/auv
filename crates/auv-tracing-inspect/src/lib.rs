#![forbid(unsafe_code)]

//! Versioned Inspect protocol DTOs and the remote run-authority client.

mod client;
pub mod protocol;
mod task_spawner;

pub use client::{ConnectError, InspectRunStore};
pub use protocol::ResolvedArtifact;
pub use task_spawner::{NoCurrentRuntime, TokioTaskSpawner};
