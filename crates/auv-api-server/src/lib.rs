//! Transport-independent daemon control and typed capability serving for AUV.
//!
//! Modules:
//! - `handler`: transport-independent control-plane owner.
//! - `transport`: tonic gRPC adapters for loopback TCP and local Unix sockets.
//! - `test_fixtures` (tests only): shared run/artifact staging helpers.

mod aggregated_grpc;
pub mod authority;
mod control_grpc;
mod control_plane;
pub mod handler;
mod rest;
mod runner;
pub mod runner_provider;
pub mod transport;

#[cfg(test)]
pub(crate) mod test_fixtures;
