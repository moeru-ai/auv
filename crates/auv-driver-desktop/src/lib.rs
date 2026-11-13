//! Selectable native implementations of the desktop capability.
//!
//! Window and display form one capability. Features compile native backends;
//! they do not select a backend or alter existing session behavior.
pub use auv_driver_desktop_common::*;

pub mod linux;
pub mod macos;
pub mod windows;
