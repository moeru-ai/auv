//! AUV product assembly: CLI frontends, app integrations, inspect composition,
//! and MCP bootstrap.
//!
//! This crate owns product bins and app-specific coupling. `auv-runtime` stays
//! library-only without `auv-game-*` dependencies.

pub mod cli;
pub mod cli_frontend;
pub mod inspect;
pub mod integrations;
pub mod mcp;
pub mod projection;
pub mod registry;
pub mod xtask;

pub use projection::ProductInspectReadProjection;
pub use registry::product_registry;
