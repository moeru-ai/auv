//! AUV product assembly: CLI frontends, verticals, donor inspect, and MCP bootstrap.
//!
//! NOTICE(inspect-composition / S4): This crate owns product bins and donor
//! coupling. `auv-cli` stays library-only core without `auv-game-*` deps.

pub mod cli;
pub mod cli_frontend;
pub mod inspect;
pub mod mcp;
pub mod product_inspect;
pub mod projection;
pub mod run_read;
pub mod verticals;
pub mod xtask;

// Compatibility short paths for product-local modules (tests + donor wiring).
pub use verticals::balatro;
pub use verticals::minecraft;
pub use verticals::osu;

pub use projection::ProductInspectReadProjection;
