//! App integration wiring: CLI, inspect, and run-record glue for game crates.
//!
//! Domain logic lives in `crates/auv-game-*`; this tree only orchestrates recording,
//! artifact staging, and live-action wiring against those crates.
//!
//! Path convention: new code inside an integration uses `self::` / `super::`
//! for sibling submodules. Cross-integration glue uses
//! `crate::integrations::<app>::...`.

pub mod balatro;
pub mod godot;
pub mod minecraft;
pub mod osu;
pub(crate) mod query_wired_live_action_status;
