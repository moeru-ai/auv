//! AUV daemon control APIs and transparent capability routing.
//!
//! Modules:
//! - `daemon`: long-lived Device, Run, and Runner ownership.
//! - `server`: listener binding, request serving, and daemon-owned routing.
//! - `runner_transport`: inherited private IPC for daemon-owned Runners.
//! - `test_fixtures` (tests only): shared run/artifact staging helpers.

pub mod auth;
mod daemon;
mod protocol;
mod resource_id;
mod rest;
pub mod runner_transport;
pub mod server;

pub use daemon::runner_provider;

#[cfg(test)]
pub(crate) mod test_fixtures;
