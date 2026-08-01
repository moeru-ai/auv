//! AUV core command frontend and process host for MCP and API servers.
//!
//! Supported crates own app/game behavior and command frontends. This crate
//! owns the root executable frontends over `auv-cli-invoke` and `auv-tracing`.

pub mod cli;
pub mod cli_frontend;
pub mod commands;
mod daemon_discovery;
pub mod mcp;
pub mod plugin;
pub mod runner_child;
pub mod xtask;

#[doc(hidden)]
pub const INTERNAL_RUNNER_SENTINEL: &str = "__auv-internal-runner";
#[doc(hidden)]
pub const LOCAL_RUNNER_ROLE: &str = "local-driver";
#[doc(hidden)]
pub const INFERENCE_RUNNER_ROLE: &str = "inference-ultralytics";
