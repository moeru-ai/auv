//! AUV core command frontend and process host for MCP and API servers.
//!
//! Supported crates own app/game behavior and command frontends. This crate
//! owns the root executable frontends over `auv-cli-invoke` and `auv-tracing`.

pub mod cli;
pub mod commands;
pub mod runner;
pub mod xtask;
