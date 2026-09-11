//! Protocol adapters for the AUV daemon control interface.
//!
//! Modules:
//! - `control`: transport-independent server contracts implemented by a daemon SDK.
//! - `reflection`: gRPC Reflection that preserves protobuf custom options.
//! - `server`: listener binding, request serving, and control routing.
//! - `runner_transport`: inherited private IPC for daemon-owned Runners.

mod authentication;
pub mod control;
mod middleware;
mod protocol;
pub mod reflection;
mod rest;
pub mod runner_transport;
pub mod server;
