//! AUV core command frontend and MCP server.
//!
//! Supported crates own app/game behavior and command frontends. This crate
//! owns the root executable frontends over `auv-cli-invoke` and `auv-tracing`.

pub mod cli;
pub mod cli_frontend;
pub mod commands;
pub mod mcp;
pub mod plugin;
pub mod xtask;
