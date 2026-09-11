//! Windows layered-window overlay adapter.
//!
//! Enable this crate through `auv-driver-overlay`'s `windows` feature. On
//! non-Windows targets, [`render`] and [`remove`] compile but always return
//! [`Err`], mirroring how `auv-driver-overlay-macos` behaves off its own
//! platform.

mod error;
mod overlay;
mod window;

pub use error::AuvResult;
pub use overlay::{remove, render};
