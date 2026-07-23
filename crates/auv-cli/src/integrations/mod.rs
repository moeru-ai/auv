//! App integration wiring for direct typed CLI, inspect, and MCP calls.
//!
//! Domain logic lives in `crates/auv-game-*`; this tree maps product inputs and
//! uses the current tracing context for optional typed instrumentation.
//!
//! Path convention: new code inside an integration uses `self::` / `super::`
//! for sibling submodules. Cross-integration glue uses
//! `crate::integrations::<app>::...`.

pub mod balatro;
pub mod godot;
pub mod minecraft;
pub mod osu;
pub mod textedit;
