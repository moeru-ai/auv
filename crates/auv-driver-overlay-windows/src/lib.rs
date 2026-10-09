//! Windows layered-window overlay adapter.
//!
//! Enable this crate through `auv-driver-overlay`'s `windows` feature. On
//! non-Windows targets, [`render`] and [`remove`] compile but always return
//! [`Err`], mirroring how `auv-driver-overlay-macos` behaves off its own
//! platform.

#[cfg(target_os = "windows")]
mod canvas;
mod error;
mod overlay;
#[cfg(target_os = "windows")]
mod svg;
mod window;

pub use error::AuvResult;
pub use overlay::{remove, render};
