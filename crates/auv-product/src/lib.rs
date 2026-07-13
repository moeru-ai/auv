//! AUV product assembly: CLI frontends, app integrations, inspect composition,
//! and MCP bootstrap.
//!
//! This crate owns product bins and app-specific coupling. `auv-cli` stays
//! library-only core without `auv-game-*` dependencies.

pub mod cli;
pub mod cli_frontend;
pub mod inspect;
pub mod integrations;
pub mod mcp;
pub mod projection;
pub mod run_read;
pub mod xtask;

pub use projection::ProductInspectReadProjection;
