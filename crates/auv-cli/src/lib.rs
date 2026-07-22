//! AUV product assembly: CLI frontends, app integrations, inspect composition,
//! and MCP bootstrap.
//!
//! This crate owns product bins and app-specific coupling. `auv-runtime` stays
//! library-only without `auv-game-*` dependencies.

pub mod cli;
pub mod cli_frontend;
pub mod inspect;
pub mod integrations;
pub mod invoke;
pub mod mcp;
pub mod projection;
pub mod registry;
pub mod run_read;
pub mod xtask;

// Task22 legacy adapter; new frontends call typed domain functions directly.
pub use invoke::{invoke_recorded, invoke_recorded_and_render};
pub use projection::ProductInspectReadProjection;
pub use registry::product_registry;
