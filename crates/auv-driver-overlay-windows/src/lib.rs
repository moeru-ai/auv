//! Windows layered-window overlay adapter.
//!
//! Enable this crate through `auv-driver-overlay`'s `windows` feature. On
//! non-Windows targets, [`render`] and [`remove`] compile but always return
//! [`Err`], mirroring how `auv-driver-overlay-macos` behaves off its own
//! platform.

mod animator;
#[cfg(target_os = "windows")]
mod canvas;
mod error;
mod overlay;
mod pacing;
mod stats;
#[cfg(target_os = "windows")]
mod svg;
mod window;

pub use animator::Animator;
pub use error::AuvResult;
pub use overlay::{remove, render};
