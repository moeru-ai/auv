//! Protocol adapters for the AUV daemon control interface.
//!
//! Modules:
//! - `control`: transport-independent server contracts implemented by a daemon SDK.
//! - `reflection`: gRPC Reflection that preserves protobuf custom options.
//! - `method_docs`: long-form method docs and examples, served on request.
//! - `server`: listener binding, request serving, and control routing.
//! - `runner_transport`: inherited private IPC for daemon-owned Runners.

mod authentication;
pub mod control;
pub mod device_local;
pub mod method_docs;
mod middleware;
mod protocol;
pub mod reflection;
mod rest;
pub mod runner_transport;
pub mod server;
