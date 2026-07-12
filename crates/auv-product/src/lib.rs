//! AUV product assembly: CLI frontends, app integrations, inspect composition,
//! and MCP bootstrap.
//!
//! NOTICE(inspect-composition / S4): This crate owns product bins and donor
//! coupling. `auv-cli` stays library-only core without `auv-game-*` deps.

pub mod cli;
pub mod cli_frontend;
pub mod inspect;
pub mod integrations;
pub mod mcp;
pub mod product_inspect;
pub mod projection;
pub mod run_read;
pub mod xtask;

pub use projection::ProductInspectReadProjection;
