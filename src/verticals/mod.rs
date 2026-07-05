//! Reference vertical wiring: CLI / inspect / run-record glue for game donors.
//!
//! Domain logic lives in `crates/auv-game-*`; this tree only orchestrates recording,
//! artifact staging, and live-action wiring against those donors.
//!
//! Path convention: new code inside a vertical uses `self::` / `super::` for sibling
//! submodules. Cross-vertical glue uses `crate::verticals::<vertical>::...`. Crate-root
//! names such as `crate::minecraft` remain compatibility re-exports in `lib.rs`.

pub mod balatro;
pub mod help;
pub mod minecraft;
pub mod osu;
pub(crate) mod query_wired_live_action_status;
